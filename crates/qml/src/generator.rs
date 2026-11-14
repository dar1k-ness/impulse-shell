//! Generator objects (ECMA-262 §27.5) and the iteration they take part in.
//!
//! A generator function's body is lowered by [`crate::ir`] into a resumable state machine: a
//! step function `(ctx) => { switch (ctx.at()) { case 0: ... } }` whose cases end by asking
//! for the next action through the `ctx` methods (`yield`, `delegate`, `jump`, `ret`,
//! `endfinally`) and register the try statements they enter (`try`). [`resume`] drives it the
//! way TypeScript's `__generator` helper drives its downlevelled generators: it keeps the
//! stack of active try statements and the completions pending while a `finally` block runs,
//! catches what the step function throws and routes it to the matching `catch` or `finally`
//! label, and forwards to the iterator a `yield*` delegates to. Both the interpreter and
//! compiled code run the same lowered body, so they behave the same by construction.
use std::rc::Rc;

use crate::eval::{
    Callable, Ex, Interp, IterSrc, NativeFn, PropKey, R, fn_value, new_obj, set_hidden,
};
use crate::value::{Internal, JsObject, Obj, PERMANENT, READONLY, Value};

/// [[GeneratorState]] (ECMA-262 §27.5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    SuspendedStart,
    SuspendedYield,
    Executing,
    Completed,
}

/// An action of the step loop: what a resumption or the step function asks for.
#[derive(Debug, Clone)]
enum Op {
    /// Resume with `next(v)`.
    Next(Value),
    /// Resume with `throw(v)`: `ctx.sent()` throws it.
    Throw(Value),
    /// Return `v`, running the enclosing `finally` blocks first.
    Return(Value),
    /// Continue at label `l`, running the `finally` blocks it leaves.
    Jump(u32),
    /// Suspend yielding `v`; resume at label `l`.
    Yield(u32, Value),
    /// `yield* v`: delegate to its iterator, then resume at label `l`.
    Delegate(u32, Value),
    /// An exception thrown in the body: to the innermost `catch` (as `ctx.sent()`).
    Caught(Value),
    /// The end of a `finally` block: carry on with the completion it interrupted.
    EndFinally,
}

impl Op {
    fn values_mut(&mut self) -> Option<&mut Value> {
        match self {
            Op::Next(v)
            | Op::Throw(v)
            | Op::Return(v)
            | Op::Yield(_, v)
            | Op::Delegate(_, v)
            | Op::Caught(v) => Some(v),
            Op::Jump(_) | Op::EndFinally => None,
        }
    }
    fn value(&self) -> Option<&Value> {
        match self {
            Op::Next(v)
            | Op::Throw(v)
            | Op::Return(v)
            | Op::Yield(_, v)
            | Op::Delegate(_, v)
            | Op::Caught(v) => Some(v),
            Op::Jump(_) | Op::EndFinally => None,
        }
    }
}

/// A try statement the body entered: the labels its try block starts at, its `catch` and
/// `finally` blocks start at, and the statement ends at (labels grow through the body, so
/// the current label tells which part runs).
#[derive(Debug, Clone, Copy)]
struct TryEntry {
    start: u32,
    catch: Option<u32>,
    finally: Option<u32>,
    end: u32,
}

/// The state of one generator, held by its `ctx` object.
#[derive(Debug)]
pub struct GenCore {
    state: State,
    label: u32,
    body: Value,
    /// What the running step asked for; a step that falls off its end returns `undefined`.
    op: Op,
    /// What `ctx.sent()` returns (`Next`, `Caught`) or throws (`Throw`).
    sent: Op,
    trys: Vec<TryEntry>,
    /// Completions waiting for the `finally` block that interrupted them.
    pending: Vec<Op>,
    /// The iterator a `yield*` delegates to.
    delegate: Option<IterSrc>,
}

impl GenCore {
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        std::iter::once(&mut self.body)
            .chain(self.op.values_mut())
            .chain(self.sent.values_mut())
            .chain(self.pending.iter_mut().filter_map(Op::values_mut))
            .chain(self.delegate.iter_mut().flat_map(IterSrc::values_mut))
    }
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        std::iter::once(&self.body)
            .chain(self.op.value())
            .chain(self.sent.value())
            .chain(self.pending.iter().filter_map(Op::value))
            .chain(self.delegate.iter().flat_map(IterSrc::values))
    }
}

/// The intrinsics generators need: %GeneratorFunction%, %GeneratorFunction.prototype%,
/// %GeneratorPrototype% and the (script-unreachable) prototype of the step functions' `ctx`
/// objects.
#[derive(Debug, Clone)]
pub struct Protos {
    pub constructor: Obj,
    pub function: Obj,
    pub generator: Obj,
    pub ctx: Obj,
}

/// Makes the generator intrinsics (ECMA-262 §27.3.3, §27.5.1).
pub fn protos(function_proto: &Obj) -> Protos {
    let function = new_obj();
    function.borrow_mut().proto = Some(function_proto.clone());
    let generator = new_obj();
    let Value::Obj(constructor) = crate::builtins::native(
        "GeneratorFunction",
        1,
        crate::builtins::generator_function_ctor,
    ) else {
        unreachable!("a native function is an object")
    };
    // %GeneratorFunction%.prototype is fixed (§27.3.2.2); the other links are read-only but
    // configurable (§27.3.3.1, §27.3.3.2, §27.5.1.1).
    set_hidden(&constructor, "prototype", Value::Obj(function.clone()));
    constructor
        .borrow_mut()
        .set_attr("prototype", READONLY | PERMANENT);
    for (o, k, v) in [
        (&function, "constructor", &constructor),
        (&function, "prototype", &generator),
        (&generator, "constructor", &function),
    ] {
        set_hidden(o, k, Value::Obj(v.clone()));
        o.borrow_mut().set_attr(k, READONLY);
    }
    method(&generator, "next", |it, this, a| {
        result(it, this, Op::Next(a.first().cloned().unwrap_or_default()))
    });
    method(&generator, "return", |it, this, a| {
        result(it, this, Op::Return(a.first().cloned().unwrap_or_default()))
    });
    method(&generator, "throw", |it, this, a| {
        result(it, this, Op::Throw(a.first().cloned().unwrap_or_default()))
    });
    let ctx = new_obj();
    ctx.borrow_mut().null_proto = true;
    nat(&ctx, "at", |it, this, _| {
        core(it, this, |c| Ok(Value::Num(c.label as f64)))
    });
    nat(&ctx, "yield", |it, this, a| {
        let (l, v) = (label(a, 0), arg(a, 1));
        set_op(it, this, Op::Yield(l, v))
    });
    nat(&ctx, "delegate", |it, this, a| {
        let (l, v) = (label(a, 0), arg(a, 1));
        set_op(it, this, Op::Delegate(l, v))
    });
    // The object of a `with` statement that yields (ECMA-262 §14.11.2 step 2, ToObject).
    nat(&ctx, "object", |it, _, a| match arg(a, 0) {
        v @ (Value::Undefined | Value::Null) => it.no_object(&v),
        v => Ok(it.to_object(&v)),
    });
    nat(&ctx, "jump", |it, this, a| {
        set_op(it, this, Op::Jump(label(a, 0)))
    });
    nat(&ctx, "ret", |it, this, a| {
        set_op(it, this, Op::Return(arg(a, 0)))
    });
    nat(&ctx, "endfinally", |it, this, _| {
        set_op(it, this, Op::EndFinally)
    });
    nat(&ctx, "try", |it, this, a| {
        let opt = |i| (arg(a, i).to_number() >= 0.0).then(|| label(a, i));
        let e = TryEntry {
            start: label(a, 0),
            catch: opt(1),
            finally: opt(2),
            end: label(a, 3),
        };
        core(it, this, |c| {
            c.trys.push(e);
            Ok(Value::Undefined)
        })
    });
    nat(&ctx, "sent", |it, this, _| {
        match core(it, this, |c| Ok(c.sent.clone()))? {
            Op::Throw(v) => Err(Ex::Throw(v)),
            o => Ok(o.value().cloned().unwrap_or_default()),
        }
    });
    nat(&ctx, "scope", |_, _, a| {
        let mut o = JsObject::default();
        o.null_proto = true;
        o.internal = Internal::GenScope(Box::default());
        if let Some(Value::Obj(p)) = a.first() {
            let p = p.borrow();
            o.keys = p.keys.clone();
            o.props = p.props.clone();
            if let Internal::GenScope(c) = &p.internal {
                o.internal = Internal::GenScope(c.clone());
            }
        }
        Ok(Value::Obj(crate::value::reg(Rc::new(
            std::cell::RefCell::new(o),
        ))))
    });
    // `call(f, this, ...args)`: a call whose callee was read before its arguments ran.
    nat(&ctx, "call", |it, _, a| {
        it.call(&arg(a, 0), arg(a, 1), a.get(2..).unwrap_or(&[]))
    });
    // The values an array pattern's rest element takes: what the iterator has left.
    nat(&ctx, "rest", |it, _, a| {
        let o = iter_obj(it, a)?;
        let mut i = take_iter(&o);
        let mut out = Vec::new();
        while let Some(v) = it.iter_step(&mut i)? {
            out.push(v);
        }
        Ok(Value::array(out))
    });
    // RequireObjectCoercible of an object pattern's value.
    nat(&ctx, "coercible", |it, _, a| match arg(a, 0) {
        Value::Undefined | Value::Null => it.throw("TypeError", "Type error"),
        _ => Ok(Value::Undefined),
    });
    // An object pattern's rest element: the own enumerable properties not named before it
    // (ECMA-262 CopyDataProperties).
    nat(&ctx, "restObj", |it, _, a| {
        let v = arg(a, 0);
        let keys = match arg(a, 1) {
            Value::Obj(o) => o.borrow().array.clone().unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut used = Vec::with_capacity(keys.len());
        for k in &keys {
            used.push(it.to_property_key(k)?);
        }
        let o = new_obj();
        it.copy_data_properties(&o, &v, &used)?;
        Ok(Value::Obj(o))
    });
    // ToPropertyKey of a computed key in an object pattern, before the property is read.
    nat(&ctx, "propKey", |it, _, a| {
        Ok(match it.to_property_key(&arg(a, 0))? {
            PropKey::Str(k) => Value::str(&k),
            PropKey::Sym(s) => Value::Symbol(s),
        })
    });
    // BlockDeclarationInstantiation of one binding: `declare(scope, name, isConst)` puts it
    // in its temporal dead zone.
    nat(&ctx, "declare", |it, _, a| {
        let Some(Value::Obj(s)) = a.first().filter(|_| a.len() == 3) else {
            return it.throw("TypeError", "not a generator scope");
        };
        let k: Rc<str> = Rc::from(arg(a, 1).to_js_string().as_str());
        let mut b = s.borrow_mut();
        if let Internal::GenScope(c) = &mut b.internal {
            c.retain(|x| *x != k);
            if arg(a, 2).truthy() {
                c.push(k.clone());
            }
        }
        if !b.props.contains_key(&k) {
            b.keys.push(k.clone());
        }
        b.props.insert(k, Value::Empty);
        Ok(Value::Undefined)
    });
    nat(&ctx, "forIn", |it, _, a| {
        let s = IterSrc::ForIn(Box::new(it.for_in(&arg(a, 0))));
        let o = new_obj();
        o.borrow_mut().internal = Internal::IterRecord(Box::new((s, Value::Undefined)));
        Ok(Value::Obj(o))
    });
    nat(&ctx, "iter", |it, _, a| {
        let i = it.get_iterator(&arg(a, 0))?;
        let o = new_obj();
        o.borrow_mut().internal = Internal::IterRecord(Box::new((i, Value::Undefined)));
        Ok(Value::Obj(o))
    });
    nat(&ctx, "step", |it, _, a| {
        let o = iter_obj(it, a)?;
        let mut i = take_iter(&o);
        let r = it.iter_step(&mut i);
        let more = matches!(r, Ok(Some(_)));
        if let Internal::IterRecord(b) = &mut o.borrow_mut().internal {
            b.0 = i;
            if let Ok(Some(v)) = &r {
                b.1 = v.clone();
            }
        }
        r.map(|_| Value::Bool(more))
    });
    nat(&ctx, "value", |it, _, a| {
        let o = iter_obj(it, a)?;
        let b = o.borrow();
        Ok(match &b.internal {
            Internal::IterRecord(b) => b.1.clone(),
            _ => Value::Undefined,
        })
    });
    nat(&ctx, "close", |it, _, a| {
        let o = iter_obj(it, a)?;
        let mut i = take_iter(&o);
        it.iter_close(&mut i).map(|_| Value::Undefined)
    });
    nat(&ctx, "closeQuiet", |it, _, a| {
        let o = iter_obj(it, a)?;
        let mut i = take_iter(&o);
        iter_close_quiet(it, &mut i).map(|_| Value::Undefined)
    });
    Protos {
        constructor,
        function,
        generator,
        ctx,
    }
}

fn nat(o: &Obj, name: &'static str, f: NativeFn) {
    set_hidden(o, name, fn_value(Callable::Native(name, f)));
}

/// A %GeneratorPrototype% method: `next`, `return` and `throw` all take one argument.
fn method(o: &Obj, name: &'static str, f: NativeFn) {
    set_hidden(o, name, crate::builtins::native(name, 1, f));
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

fn label(a: &[Value], i: usize) -> u32 {
    arg(a, i).to_number() as u32
}

fn iter_obj(it: &Interp, a: &[Value]) -> R<Obj> {
    match a.first() {
        Some(Value::Obj(o)) if matches!(o.borrow().internal, Internal::IterRecord(_)) => {
            Ok(o.clone())
        }
        _ => it.throw("TypeError", "not an iterator"),
    }
}

fn take_iter(o: &Obj) -> IterSrc {
    match &mut o.borrow_mut().internal {
        Internal::IterRecord(b) => std::mem::take(&mut b.0),
        _ => IterSrc::default(),
    }
}

/// Runs `f` on the core of `ctx` object `this`.
fn core<T>(it: &Interp, this: &Value, f: impl FnOnce(&mut GenCore) -> R<T>) -> R<T> {
    if let Value::Obj(o) = this
        && let Internal::GenCtx(c) = &mut o.borrow_mut().internal
    {
        return f(c);
    }
    it.throw("TypeError", "not a generator context")
}

fn set_op(it: &Interp, this: &Value, op: Op) -> R<Value> {
    core(it, this, |c| {
        c.op = op;
        Ok(Value::Undefined)
    })
}

/// A new generator object for a call of generator function `callee` whose lowered body
/// returned `step` (ECMA-262 §15.5.2 EvaluateGeneratorBody): its prototype is
/// `callee.prototype`, or %GeneratorPrototype% when that is no object.
pub fn make_generator(it: &mut Interp, callee: &Value, step: Value) -> Value {
    let ctx = new_obj();
    {
        let mut c = ctx.borrow_mut();
        c.proto = Some(it.generator_protos.ctx.clone());
        c.internal = Internal::GenCtx(Box::new(GenCore {
            state: State::SuspendedStart,
            label: 0,
            body: step,
            op: Op::Return(Value::Undefined),
            sent: Op::Next(Value::Undefined),
            trys: Vec::new(),
            pending: Vec::new(),
            delegate: None,
        }));
    }
    let proto = match callee {
        Value::Obj(f) => crate::eval::fn_prototype(f),
        _ => None,
    };
    let g = new_obj();
    {
        let mut b = g.borrow_mut();
        b.proto = Some(proto.unwrap_or_else(|| it.generator_protos.generator.clone()));
        b.internal = Internal::Generator(ctx);
    }
    Value::Obj(g)
}

/// `{ value, done }` (ECMA-262 CreateIterResultObject).
fn iter_result(it: &mut Interp, v: Value, done: bool) -> R<Value> {
    let o = Value::Obj(new_obj());
    it.put(&o, "value", v)?;
    it.put(&o, "done", Value::Bool(done))?;
    Ok(o)
}

fn result(it: &mut Interp, this: &Value, op: Op) -> R<Value> {
    let (v, done) = resume(it, this, op)?;
    iter_result(it, v, done)
}

fn ctx_of(it: &Interp, g: &Value) -> R<Obj> {
    if let Value::Obj(o) = g
        && let Internal::Generator(c) = &o.borrow().internal
    {
        return Ok(c.clone());
    }
    it.throw("TypeError", "Type error")
}

fn with_core<T>(ctx: &Obj, f: impl FnOnce(&mut GenCore) -> T) -> T {
    match &mut ctx.borrow_mut().internal {
        Internal::GenCtx(c) => f(c),
        _ => unreachable!("a generator's ctx holds its core"),
    }
}

/// GeneratorResume / GeneratorResumeAbrupt (ECMA-262 §27.5.3.3–4) with `next(v)`,
/// `throw(v)` or `return(v)`: the yielded or returned value and whether it is done.
fn resume(it: &mut Interp, g: &Value, op: Op) -> R<(Value, bool)> {
    let ctx = ctx_of(it, g)?;
    let state = with_core(&ctx, |c| c.state);
    let op = match (state, op) {
        (State::Executing, _) => return it.throw("TypeError", "Type error"),
        (State::SuspendedStart, op @ (Op::Throw(_) | Op::Return(_))) => {
            with_core(&ctx, |c| c.state = State::Completed);
            op
        }
        (_, op) => op,
    };
    if with_core(&ctx, |c| c.state) == State::Completed {
        return match op {
            Op::Throw(v) => Err(Ex::Throw(v)),
            Op::Return(v) => Ok((v, true)),
            _ => Ok((Value::Undefined, true)),
        };
    }
    with_core(&ctx, |c| c.state = State::Executing);
    let r = run(it, &ctx, op);
    with_core(&ctx, |c| {
        c.state = match &r {
            Ok((_, false)) => State::SuspendedYield,
            _ => State::Completed,
        };
        if c.state == State::Completed {
            c.trys.clear();
            c.pending.clear();
            c.delegate = None;
            c.body = Value::Undefined;
        }
    });
    r
}

/// One step of the delegation of `yield*` to `d` (ECMA-262 §15.5.5 step 7): the inner
/// result, or `None` when `d` has no `return` for a return (the generator then returns).
fn delegate_step(it: &mut Interp, d: &mut IterSrc, op: &Op) -> R<Option<(Value, bool)>> {
    let no_throw = |it: &mut Interp, d: &mut IterSrc| -> R<Option<(Value, bool)>> {
        // No `throw` method: close the iterator, then it is a protocol violation.
        it.iter_close(d)?;
        it.throw("TypeError", "Type error")
    };
    let IterSrc::Js { iter, next, .. } = d else {
        // A built-in iterator with its items taken: `next` only.
        return match op {
            Op::Next(_) => Ok(Some(match it.iter_step(d)? {
                Some(v) => (v, false),
                None => (Value::Undefined, true),
            })),
            Op::Throw(_) => no_throw(it, d),
            _ => Ok(None),
        };
    };
    let (iter, next) = (iter.clone(), next.clone());
    let (f, v) = match op {
        Op::Next(v) => (next, v.clone()),
        Op::Throw(v) => match it.get(&iter, "throw")? {
            Value::Undefined | Value::Null => return no_throw(it, d),
            f => (f, v.clone()),
        },
        Op::Return(v) => match it.get(&iter, "return")? {
            Value::Undefined | Value::Null => return Ok(None),
            f => (f, v.clone()),
        },
        _ => return Ok(None),
    };
    let r = it.call(&f, iter, &[v])?;
    if !matches!(r, Value::Obj(_)) {
        return it.throw("TypeError", "Type error");
    }
    let done = it.get(&r, "done")?.truthy();
    Ok(Some((it.get(&r, "value")?, done)))
}

/// The step loop (TypeScript's `__generator` `step`, with yields resuming at the label the
/// body names): runs the body until it yields or completes.
fn run(it: &mut Interp, ctx: &Obj, mut op: Op) -> R<(Value, bool)> {
    loop {
        if let Some(mut d) = with_core(ctx, |c| c.delegate.take()) {
            match delegate_step(it, &mut d, &op) {
                Ok(Some((v, false))) => {
                    with_core(ctx, |c| c.delegate = Some(d));
                    return Ok((v, false));
                }
                Ok(Some((v, true))) => {
                    op = match op {
                        Op::Return(_) => Op::Return(v),
                        _ => Op::Next(v),
                    }
                }
                Ok(None) => {}
                Err(Ex::Throw(e)) => op = Op::Caught(e),
                Err(e) => return Err(e),
            }
        }
        match op {
            Op::Next(_) | Op::Throw(_) => with_core(ctx, |c| c.sent = op),
            Op::Yield(l, v) => {
                with_core(ctx, |c| c.label = l);
                return Ok((v, false));
            }
            Op::Delegate(l, v) => {
                with_core(ctx, |c| c.label = l);
                op = match it.get_iterator(&v) {
                    Ok(d) => {
                        with_core(ctx, |c| c.delegate = Some(d));
                        Op::Next(Value::Undefined)
                    }
                    Err(Ex::Throw(e)) => Op::Caught(e),
                    Err(e) => return Err(e),
                };
                continue;
            }
            Op::EndFinally => {
                op = with_core(ctx, |c| {
                    c.trys.pop();
                    c.pending.pop()
                })
                .unwrap_or(Op::Return(Value::Undefined));
                continue;
            }
            Op::Return(_) | Op::Jump(_) | Op::Caught(_) => {
                let (label, t) = with_core(ctx, |c| (c.label, c.trys.last().copied()));
                let Some(t) = t else {
                    match op {
                        Op::Jump(l) => with_core(ctx, |c| c.label = l),
                        Op::Return(v) => return Ok((v, true)),
                        Op::Caught(e) => return Err(Ex::Throw(e)),
                        _ => unreachable!(),
                    }
                    op = Op::Next(Value::Undefined);
                    continue;
                };
                match op {
                    Op::Jump(l) if l > t.start && l < t.end => {
                        with_core(ctx, |c| c.label = l);
                    }
                    Op::Caught(e) if t.catch.is_some_and(|c| label < c) => {
                        with_core(ctx, |c| {
                            c.label = t.catch.unwrap();
                            c.sent = Op::Caught(e);
                        });
                    }
                    op2 if t.finally.is_some_and(|f| label < f) => {
                        with_core(ctx, |c| {
                            c.label = t.finally.unwrap();
                            c.pending.push(op2);
                        });
                    }
                    op2 => {
                        with_core(ctx, |c| {
                            if t.finally.is_some() {
                                c.pending.pop();
                            }
                            c.trys.pop();
                        });
                        op = op2;
                        continue;
                    }
                }
            }
        }
        let body = with_core(ctx, |c| {
            c.op = Op::Return(Value::Undefined);
            c.body.clone()
        });
        op = match it.call(&body, Value::Undefined, &[Value::Obj(ctx.clone())]) {
            Ok(_) => with_core(ctx, |c| {
                std::mem::replace(&mut c.op, Op::Return(Value::Undefined))
            }),
            Err(Ex::Throw(e)) => {
                with_core(ctx, |c| c.delegate = None);
                Op::Caught(e)
            }
            Err(e) => return Err(e),
        };
    }
}

/// IteratorClose for a throw completion: the original exception wins over what `return`
/// throws (only uncatchable aborts propagate).
fn iter_close_quiet(it: &mut Interp, i: &mut IterSrc) -> R<()> {
    match it.iter_close(i) {
        Err(Ex::Throw(_)) | Ok(()) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Whether `o` is a scope object of a generator's split block.
pub fn is_scope(o: &Obj) -> bool {
    matches!(o.borrow().internal, Internal::GenScope(_))
}

/// GetBindingValue of binding `k` of scope object `o` (ECMA-262 §9.1.1.1.6): one still in
/// its temporal dead zone throws.
pub fn scope_get(it: &Interp, o: &Obj, k: &str) -> R<Value> {
    match o.borrow().props.get(k) {
        Some(Value::Empty) => it.uninitialized(k),
        v => Ok(v.cloned().unwrap_or_default()),
    }
}

/// SetMutableBinding of binding `k` of scope object `o` (ECMA-262 §9.1.1.1.5): throws in its
/// temporal dead zone, and for a `const`.
pub fn scope_put(it: &Interp, o: &Obj, k: &str, v: Value) -> R<()> {
    let mut b = o.borrow_mut();
    if let Some(Value::Empty) = b.props.get(k) {
        drop(b);
        return it.uninitialized(k);
    }
    if let Internal::GenScope(c) = &b.internal
        && c.iter().any(|x| &**x == k)
    {
        drop(b);
        return it.const_write();
    }
    if let Some(slot) = b.props.get_mut(k) {
        *slot = v;
    } else {
        let k: Rc<str> = Rc::from(k);
        b.keys.push(k.clone());
        b.props.insert(k, v);
    }
    Ok(())
}

/// InitializeReferencedBinding (`ir::Pat::Init`) of binding `k` of scope object `o`.
pub fn init_member(o: &Value, k: &str, v: Value) {
    if let Value::Obj(o) = o {
        let mut b = o.borrow_mut();
        if let Some(slot) = b.props.get_mut(k) {
            *slot = v;
        } else {
            let k: Rc<str> = Rc::from(k);
            b.keys.push(k.clone());
            b.props.insert(k, v);
        }
    }
}
