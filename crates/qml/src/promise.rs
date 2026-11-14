//! Promise objects (ECMA-262 §27.2) and the job queue their reactions run on (§9.5).
//!
//! Jobs run in FIFO order once the JavaScript stack is empty: [`Interp::leave`] drains the
//! queue when the outermost frame of a call from the engine (a handler, a binding, a timer or
//! `Qt.callLater` callback, a library body) returns, so they run before the host goes on to
//! its next event. HostPromiseRejectionTracker does nothing: Qt 6.12 reports no unhandled
//! rejection either.
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::builtins::native;
use crate::eval::{
    BoundFn, Callable, Ex, Interp, NativeFn, R, callable, fn_value, link_builtin_prototype,
    new_obj, set_hidden, set_own,
};
use crate::symbol::species_constructor;
use crate::value::{Internal, Obj, Value};

/// [[PromiseState]] with its [[PromiseResult]], and the reactions of a pending promise.
#[derive(Debug, Default)]
pub struct Slot {
    pub state: State,
    pub fulfill: Vec<Reaction>,
    pub reject: Vec<Reaction>,
}

#[derive(Debug, Default)]
pub enum State {
    #[default]
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}

/// A PromiseReaction Record (§27.2.1.2); an undefined `handler` is an empty one.
#[derive(Debug, Clone)]
pub struct Reaction {
    cap: Option<Capability>,
    rejects: bool,
    handler: Value,
}

/// A PromiseCapability Record (§27.2.1.1).
#[derive(Debug, Clone)]
pub struct Capability {
    promise: Value,
    resolve: Value,
    reject: Value,
}

impl Reaction {
    fn values(&self) -> impl Iterator<Item = &Value> {
        std::iter::once(&self.handler).chain(
            self.cap
                .iter()
                .flat_map(|c| [&c.promise, &c.resolve, &c.reject]),
        )
    }

    fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        std::iter::once(&mut self.handler).chain(
            self.cap
                .iter_mut()
                .flat_map(|c| [&mut c.promise, &mut c.resolve, &mut c.reject]),
        )
    }
}

impl Slot {
    /// Every value the promise holds: its result and its reactions' handlers and capabilities.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        let result = match &self.state {
            State::Fulfilled(v) | State::Rejected(v) => Some(v),
            State::Pending => None,
        };
        result.into_iter().chain(
            self.fulfill
                .iter()
                .chain(&self.reject)
                .flat_map(Reaction::values),
        )
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        let result = match &mut self.state {
            State::Fulfilled(v) | State::Rejected(v) => Some(v),
            State::Pending => None,
        };
        result.into_iter().chain(
            self.fulfill
                .iter_mut()
                .chain(&mut self.reject)
                .flat_map(Reaction::values_mut),
        )
    }
}

enum Job {
    /// NewPromiseReactionJob (§27.2.2.1).
    Reaction(Reaction, Value),
    /// NewPromiseResolveThenableJob (§27.2.2.2).
    Thenable {
        promise: Obj,
        thenable: Value,
        then: Value,
    },
}

thread_local! {
    static JOBS: RefCell<VecDeque<Job>> = const { RefCell::new(VecDeque::new()) };
}

/// HostEnqueuePromiseJob (§9.5.5).
fn enqueue(j: Job) {
    JOBS.with(|q| q.borrow_mut().push_back(j));
}

/// Whether promise jobs wait to run.
pub fn pending() -> bool {
    JOBS.with(|q| !q.borrow().is_empty())
}

/// Runs the queued jobs in order, and those they queue, until none is left. A job's own
/// abrupt completion goes to HostReportErrors, as an uncaught error of a callback does.
pub fn run_jobs(it: &mut Interp) {
    while let Some(j) = JOBS.with(|q| q.borrow_mut().pop_front()) {
        let r = match j {
            Job::Reaction(r, v) => reaction_job(it, r, v),
            Job::Thenable {
                promise,
                thenable,
                then,
            } => thenable_job(it, &promise, thenable, &then),
        };
        if let Err(Ex::Throw(e)) = r {
            let m = it
                .get(&e, "message")
                .map(|v| v.to_js_string())
                .unwrap_or_default();
            crate::warning::say(Default::default(), &format!("Promise job: {m}"));
        }
    }
}

fn reaction_job(it: &mut Interp, r: Reaction, arg: Value) -> R<()> {
    let out = if callable(&r.handler).is_none() {
        if r.rejects { Err(arg) } else { Ok(arg) }
    } else {
        match it.call(&r.handler, Value::Undefined, &[arg]) {
            Ok(v) => Ok(v),
            Err(Ex::Throw(e)) => Err(e),
            Err(e) => return Err(e),
        }
    };
    let Some(cap) = r.cap else {
        return Ok(());
    };
    match out {
        Ok(v) => it.call(&cap.resolve, Value::Undefined, &[v]),
        Err(e) => it.call(&cap.reject, Value::Undefined, &[e]),
    }
    .map(drop)
}

fn thenable_job(it: &mut Interp, promise: &Obj, thenable: Value, then: &Value) -> R<()> {
    let (resolve, reject) = resolving_functions(promise);
    match it.call(then, thenable, &[resolve, reject.clone()]) {
        Err(Ex::Throw(e)) => it.call(&reject, Value::Undefined, &[e]).map(drop),
        r => r.map(drop),
    }
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

fn is_object(v: &Value) -> bool {
    matches!(v, Value::Obj(_) | Value::Item(_))
}

/// An anonymous built-in closure (§10.3.4 CreateBuiltinFunction): native `f` called with
/// `captures` before its arguments.
fn closure(f: NativeFn, captures: Vec<Value>, length: f64) -> Value {
    fn_value(Callable::Bound(Rc::new(BoundFn {
        target: fn_value(Callable::Native("", f)),
        this: Value::Undefined,
        args: captures,
        length,
    })))
}

/// A once-only flag shared by closures ([[AlreadyResolved]], [[AlreadyCalled]]).
fn flag() -> Value {
    let o = new_obj();
    o.borrow_mut().native = Some(Rc::new(Cell::new(false)));
    Value::Obj(o)
}

/// Sets the flag; whether this was the first time.
fn first_call(flag: &Value) -> bool {
    let Value::Obj(o) = flag else {
        return false;
    };
    let b = o.borrow();
    b.native
        .as_ref()
        .and_then(|n| n.downcast_ref::<Cell<bool>>())
        .is_some_and(|c| !c.replace(true))
}

/// The promise object `v` is (IsPromise, §27.2.1.6).
fn promise_obj(v: &Value) -> Option<Obj> {
    match v {
        Value::Obj(o) if matches!(o.borrow().internal, Internal::Promise(_)) => Some(o.clone()),
        _ => None,
    }
}

/// Whether `v` is %Promise%.
fn is_promise_ctor(v: &Value) -> bool {
    matches!(callable(v), Some(Callable::Native(_, f)) if std::ptr::fn_addr_eq(f, promise_ctor as NativeFn))
}

/// A pending promise whose prototype is %Promise.prototype%.
fn new_promise(it: &Interp) -> Obj {
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        b.proto = Some(it.promise_proto.clone());
        b.internal = Internal::Promise(Box::default());
    }
    o
}

/// CreateResolvingFunctions (§27.2.1.3).
fn resolving_functions(p: &Obj) -> (Value, Value) {
    let captures = vec![Value::Obj(p.clone()), flag()];
    (
        closure(resolve_function, captures.clone(), 1.0),
        closure(reject_function, captures, 1.0),
    )
}

/// Promise Resolve Functions (§27.2.1.3.2).
fn resolve_function(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    if let Value::Obj(p) = arg(a, 0)
        && first_call(&arg(a, 1))
    {
        resolve_promise(it, &p, arg(a, 2))?;
    }
    Ok(Value::Undefined)
}

/// Promise Reject Functions (§27.2.1.3.1).
fn reject_function(_: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    if let Value::Obj(p) = arg(a, 0)
        && first_call(&arg(a, 1))
    {
        settle(&p, State::Rejected(arg(a, 2)));
    }
    Ok(Value::Undefined)
}

fn resolve_promise(it: &mut Interp, p: &Obj, x: Value) -> R<()> {
    if let Value::Obj(o) = &x
        && Rc::ptr_eq(o, p)
    {
        let e = it.make_error(
            "TypeError",
            Some("Chaining cycle detected for promise".into()),
        );
        settle(p, State::Rejected(e));
        return Ok(());
    }
    if !is_object(&x) {
        settle(p, State::Fulfilled(x));
        return Ok(());
    }
    let then = match it.get(&x, "then") {
        Ok(t) => t,
        Err(Ex::Throw(e)) => {
            settle(p, State::Rejected(e));
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    if callable(&then).is_none() {
        settle(p, State::Fulfilled(x));
    } else {
        enqueue(Job::Thenable {
            promise: p.clone(),
            thenable: x,
            then,
        });
    }
    Ok(())
}

/// FulfillPromise / RejectPromise (§27.2.1.4, §27.2.1.7) with TriggerPromiseReactions.
fn settle(p: &Obj, s: State) {
    let (reactions, v) = {
        let mut b = p.borrow_mut();
        let Internal::Promise(slot) = &mut b.internal else {
            return;
        };
        let fulfill = std::mem::take(&mut slot.fulfill);
        let reject = std::mem::take(&mut slot.reject);
        let r = match &s {
            State::Fulfilled(v) => (fulfill, v.clone()),
            State::Rejected(v) => (reject, v.clone()),
            State::Pending => return,
        };
        slot.state = s;
        r
    };
    for r in reactions {
        enqueue(Job::Reaction(r, v.clone()));
    }
}

/// PerformPromiseThen (§27.2.5.4.1).
fn perform_then(p: &Obj, on_fulfilled: Value, on_rejected: Value, cap: Option<Capability>) {
    let reaction = |h: Value, rejects| Reaction {
        cap: cap.clone(),
        rejects,
        handler: if callable(&h).is_some() {
            h
        } else {
            Value::Undefined
        },
    };
    let (f, r) = (reaction(on_fulfilled, false), reaction(on_rejected, true));
    let mut b = p.borrow_mut();
    let Internal::Promise(slot) = &mut b.internal else {
        return;
    };
    match &slot.state {
        State::Pending => {
            slot.fulfill.push(f);
            slot.reject.push(r);
        }
        State::Fulfilled(v) => enqueue(Job::Reaction(f, v.clone())),
        State::Rejected(v) => enqueue(Job::Reaction(r, v.clone())),
    }
}

/// NewPromiseCapability (§27.2.1.5) of constructor `c`, %Promise% for `None`.
fn new_capability(it: &mut Interp, c: Option<&Value>) -> R<Capability> {
    let Some(c) = c.filter(|c| !is_promise_ctor(c)) else {
        let p = new_promise(it);
        let (resolve, reject) = resolving_functions(&p);
        return Ok(Capability {
            promise: Value::Obj(p),
            resolve,
            reject,
        });
    };
    if callable(c).is_none() {
        return it.type_error();
    }
    let record = new_obj();
    let executor = closure(capability_executor, vec![Value::Obj(record.clone())], 2.0);
    let promise = it.construct(c, &[executor])?;
    let field = |k: &str| record.borrow().props.get(k).cloned().unwrap_or_default();
    let (resolve, reject) = (field("resolve"), field("reject"));
    if callable(&resolve).is_none() || callable(&reject).is_none() {
        return it.throw("TypeError", "Type error");
    }
    Ok(Capability {
        promise,
        resolve,
        reject,
    })
}

/// GetCapabilitiesExecutor Functions (§27.2.1.5 steps 4-5).
fn capability_executor(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let Value::Obj(record) = arg(a, 0) else {
        return Ok(Value::Undefined);
    };
    let set = |k: &str| {
        record
            .borrow()
            .props
            .get(k)
            .is_some_and(|v| !matches!(v, Value::Undefined))
    };
    if set("resolve") || set("reject") {
        return it.throw("TypeError", "Type error");
    }
    set_own(&record, "resolve", arg(a, 1));
    set_own(&record, "reject", arg(a, 2));
    Ok(Value::Undefined)
}

/// PromiseResolve (§27.2.4.7.1) of constructor `c`, %Promise% for `None`.
fn promise_resolve(it: &mut Interp, c: Option<&Value>, x: Value) -> R<Value> {
    if promise_obj(&x).is_some() {
        let xc = it.get(&x, "constructor")?;
        if c.map_or_else(|| is_promise_ctor(&xc), |c| xc.strict_eq(c)) {
            return Ok(x);
        }
    }
    let cap = new_capability(it, c)?;
    it.call(&cap.resolve, Value::Undefined, &[x])?;
    Ok(cap.promise)
}

/// Invoke (§7.3.21).
fn invoke(it: &mut Interp, v: &Value, k: &str, args: &[Value]) -> R<Value> {
    let f = it.get(v, k)?;
    it.call(&f, v.clone(), args)
}

/// `new Promise(executor)` (§27.2.3.1).
fn promise_ctor(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let executor = arg(a, 0);
    if callable(&executor).is_none() {
        return it.throw("TypeError", "Type error");
    }
    let p = new_promise(it);
    let (resolve, reject) = resolving_functions(&p);
    match it.call(&executor, Value::Undefined, &[resolve, reject.clone()]) {
        Ok(_) => {}
        Err(Ex::Throw(e)) => {
            it.call(&reject, Value::Undefined, &[e])?;
        }
        Err(e) => return Err(e),
    }
    Ok(Value::Obj(p))
}

/// `Promise.prototype.then` (§27.2.5.4).
fn then(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let Some(p) = promise_obj(this) else {
        return it.throw("TypeError", "Type error");
    };
    let c = species_constructor(it, this)?;
    let cap = new_capability(it, c.as_ref())?;
    let promise = cap.promise.clone();
    perform_then(&p, arg(a, 0), arg(a, 1), Some(cap));
    Ok(promise)
}

/// `Promise.prototype.catch` (§27.2.5.1).
fn catch(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    invoke(it, this, "then", &[Value::Undefined, arg(a, 0)])
}

/// `Promise.prototype.finally` (§27.2.5.3).
fn finally(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    if !is_object(this) {
        return it.throw("TypeError", "Type error");
    }
    let c = species_constructor(it, this)?.unwrap_or_default();
    let on_finally = arg(a, 0);
    let (then_finally, catch_finally) = if callable(&on_finally).is_none() {
        (on_finally.clone(), on_finally)
    } else {
        (
            closure(then_finally, vec![on_finally.clone(), c.clone()], 1.0),
            closure(catch_finally, vec![on_finally, c], 1.0),
        )
    };
    invoke(it, this, "then", &[then_finally, catch_finally])
}

/// Then Finally Functions (§27.2.5.3.1): `onFinally()`, then the value passes through.
fn then_finally(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let r = it.call(&arg(a, 0), Value::Undefined, &[])?;
    let c = arg(a, 1);
    let p = promise_resolve(it, (!matches!(c, Value::Undefined)).then_some(&c), r)?;
    invoke(
        it,
        &p,
        "then",
        &[closure(return_value, vec![arg(a, 2)], 0.0)],
    )
}

/// Catch Finally Functions (§27.2.5.3.2): `onFinally()`, then the reason is thrown again.
fn catch_finally(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let r = it.call(&arg(a, 0), Value::Undefined, &[])?;
    let c = arg(a, 1);
    let p = promise_resolve(it, (!matches!(c, Value::Undefined)).then_some(&c), r)?;
    invoke(
        it,
        &p,
        "then",
        &[closure(throw_value, vec![arg(a, 2)], 0.0)],
    )
}

fn return_value(_: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    Ok(arg(a, 0))
}

fn throw_value(_: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    Err(Ex::Throw(arg(a, 0)))
}

/// `Promise.resolve` (§27.2.4.7).
fn resolve(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    if !is_object(this) {
        return it.throw("TypeError", "Type error");
    }
    promise_resolve(it, Some(this), arg(a, 0))
}

/// `Promise.reject` (§27.2.4.6).
fn reject(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let cap = new_capability(it, Some(this))?;
    it.call(&cap.reject, Value::Undefined, &[arg(a, 0)])?;
    Ok(cap.promise)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Combinator {
    All,
    AllSettled,
    Any,
    Race,
}

fn all(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    combinator(it, this, &arg(a, 0), Combinator::All)
}

fn all_settled(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    combinator(it, this, &arg(a, 0), Combinator::AllSettled)
}

fn any(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    combinator(it, this, &arg(a, 0), Combinator::Any)
}

fn race(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    combinator(it, this, &arg(a, 0), Combinator::Race)
}

/// `Promise.all`/`allSettled`/`any`/`race` (§27.2.4.1-§27.2.4.5): an abrupt completion
/// while getting `resolve` or iterating rejects the result (IfAbruptRejectPromise).
fn combinator(it: &mut Interp, c: &Value, iterable: &Value, kind: Combinator) -> R<Value> {
    let cap = new_capability(it, Some(c))?;
    match combine(it, c, iterable, &cap, kind) {
        Err(Ex::Throw(e)) => {
            it.call(&cap.reject, Value::Undefined, &[e])?;
            Ok(cap.promise)
        }
        r => r,
    }
}

fn combine(
    it: &mut Interp,
    c: &Value,
    iterable: &Value,
    cap: &Capability,
    kind: Combinator,
) -> R<Value> {
    let promise_resolve = it.get(c, "resolve")?;
    if callable(&promise_resolve).is_none() {
        return it.throw("TypeError", "Type error");
    }
    let items = it.iterate(iterable)?;
    // The shared state of the element functions: the values (or errors) list, the remaining
    // elements counter, and the capability function that completes the result.
    let record = new_obj();
    set_own(
        &record,
        "list",
        Value::array(vec![Value::Undefined; items.len()]),
    );
    set_own(&record, "remaining", Value::Num(1.0));
    let done = match kind {
        Combinator::Any => &cap.reject,
        _ => &cap.resolve,
    };
    set_own(&record, "done", done.clone());
    for (i, v) in items.into_iter().enumerate() {
        let next = it.call(&promise_resolve, c.clone(), &[v])?;
        let element = |f: NativeFn, called: &Value| {
            closure(
                f,
                vec![
                    Value::Obj(record.clone()),
                    Value::Num(i as f64),
                    called.clone(),
                ],
                1.0,
            )
        };
        let called = flag();
        let (on_fulfilled, on_rejected) = match kind {
            Combinator::Race => (cap.resolve.clone(), cap.reject.clone()),
            Combinator::All => (element(all_element, &called), cap.reject.clone()),
            Combinator::AllSettled => (
                element(fulfilled_element, &called),
                element(rejected_element, &called),
            ),
            Combinator::Any => (cap.resolve.clone(), element(any_element, &called)),
        };
        if kind != Combinator::Race {
            count(&record, 1.0);
        }
        invoke(it, &next, "then", &[on_fulfilled, on_rejected])?;
    }
    if kind != Combinator::Race && count(&record, -1.0) {
        complete(it, &record, kind == Combinator::Any)?;
    }
    Ok(cap.promise.clone())
}

/// Adds `d` to the remaining elements count; whether none remains.
fn count(record: &Obj, d: f64) -> bool {
    let mut b = record.borrow_mut();
    let Some(Value::Num(n)) = b.props.get_mut("remaining") else {
        return false;
    };
    *n += d;
    *n == 0.0
}

/// Resolves the result with the values list, or rejects it with an AggregateError of the
/// errors list (`Promise.any`).
fn complete(it: &mut Interp, record: &Obj, any: bool) -> R<()> {
    let (list, done) = {
        let b = record.borrow();
        let field = |k: &str| b.props.get(k).cloned().unwrap_or_default();
        (field("list"), field("done"))
    };
    let v = if any {
        let e = it.make_error("AggregateError", Some("All promises were rejected".into()));
        if let Value::Obj(o) = &e {
            set_hidden(o, "errors", list);
        }
        e
    } else {
        list
    };
    it.call(&done, Value::Undefined, &[v]).map(drop)
}

/// Stores `v` at the element's index, and completes the result after the last one.
fn store(it: &mut Interp, a: &[Value], v: Value, any: bool) -> R<Value> {
    let (Value::Obj(record), Value::Num(i)) = (arg(a, 0), arg(a, 1)) else {
        return Ok(Value::Undefined);
    };
    if !first_call(&arg(a, 2)) {
        return Ok(Value::Undefined);
    }
    let list = record.borrow().props.get("list").cloned();
    if let Some(Value::Obj(l)) = list
        && let Some(slot) = l
            .borrow_mut()
            .array
            .as_mut()
            .and_then(|a| a.get_mut(i as usize))
    {
        *slot = v;
    }
    if count(&record, -1.0) {
        complete(it, &record, any)?;
    }
    Ok(Value::Undefined)
}

/// `Promise.all` Resolve Element Functions (§27.2.4.1.3).
fn all_element(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    store(it, a, arg(a, 3), false)
}

/// `Promise.allSettled` Resolve and Reject Element Functions (§27.2.4.2.2-3).
fn fulfilled_element(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let o = new_obj();
    set_own(&o, "status", Value::str("fulfilled"));
    set_own(&o, "value", arg(a, 3));
    store(it, a, Value::Obj(o), false)
}

fn rejected_element(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let o = new_obj();
    set_own(&o, "status", Value::str("rejected"));
    set_own(&o, "reason", arg(a, 3));
    store(it, a, Value::Obj(o), false)
}

/// `Promise.any` Reject Element Functions (§27.2.4.3.2).
fn any_element(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    store(it, a, arg(a, 3), true)
}

/// `new AggregateError(errors, message)` (§20.5.7.1); calling it builds one too.
pub fn aggregate_error_ctor(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
    let msg = match arg(a, 1) {
        Value::Undefined => None,
        m => Some(it.to_string(&m)?),
    };
    let e = it.make_error("AggregateError", msg);
    let errors = Value::array(it.iterate(&arg(a, 0))?);
    if let Value::Obj(o) = &e {
        set_hidden(o, "errors", errors);
    }
    Ok(e)
}

/// %Promise%, with %Promise.prototype% (`it.promise_proto`) and the statics.
pub fn constructor(it: &Interp) -> Value {
    let ctor = native("Promise", 1, promise_ctor);
    let proto = &it.promise_proto;
    for (name, length, f) in [
        ("then", 2, then as NativeFn),
        ("catch", 1, catch),
        ("finally", 1, finally),
    ] {
        set_hidden(proto, name, native(name, length, f));
    }
    if let Value::Obj(c) = &ctor {
        link_builtin_prototype(c, proto);
        for (name, f) in [
            ("resolve", resolve as NativeFn),
            ("reject", reject),
            ("all", all),
            ("allSettled", all_settled),
            ("any", any),
            ("race", race),
        ] {
            set_hidden(c, name, native(name, 1, f));
        }
    }
    ctor
}

#[cfg(all(test, feature = "interp"))]
mod tests {
    use crate::eval::{Ex, Interp};
    use crate::runtime::Runtime;

    /// Runs `src` as one top-level script and returns its global `result` after the jobs.
    fn run(src: &str) -> String {
        let body = crate::ir::lower_program(src).unwrap();
        let mut it = Interp::default();
        let g = it.global.clone();
        if let Err(Ex::Throw(e)) = it.run(&body, &g) {
            return format!("THROW {}", e.to_js_string());
        }
        assert!(!super::pending());
        let v = g.borrow().vars.get("result").cloned().unwrap_or_default();
        it.to_string(&v).unwrap()
    }

    /// Jobs run once the script returns, not during it, and the reactions interleave in
    /// queue order (PySide6 6.12: `1,2,4,5,adopt,6,7`; adopting a promise takes two jobs).
    #[test]
    fn jobs_run_in_order_after_the_script() {
        assert_eq!(
            run("var log = []; var result;
                 function push(x) { log.push(x); result = log.join(',') }
                 Promise.resolve().then(() => push(1)).then(() => push(4));
                 new Promise(a => a(Promise.resolve())).then(() => push('adopt'));
                 Promise.resolve().then(() => push(2)).then(() => push(5)).then(() => push(6)).then(() => push(7));
                 [0].forEach(() => Promise.resolve().then(() => push('nested')));
                 push('sync:' + log.length)"),
            "sync:0,1,2,nested,4,5,adopt,6,7"
        );
    }

    /// Values pinned from PySide6 6.12.
    #[test]
    fn settles_like_qt() {
        assert_eq!(
            run("var log = []; var result;
                 function push(x) { log.push(x); result = log.join(';') }
                 new Promise(function () { throw new Error('boom') }).catch(e => push('executor ' + e.message));
                 new Promise(function (a) { a(1); throw new Error('ignored') }).then(v => push('resolved ' + v));
                 Promise.resolve(1).then(2).then(v => push('then passes ' + v));
                 Promise.reject(3).then(null, 4).catch(v => push('catch passes ' + v));
                 Promise.all([1, Promise.resolve(2), { then(f) { f(3) } }]).then(v => push('all ' + JSON.stringify(v)));
                 Promise.all([]).then(v => push('all empty ' + JSON.stringify(v)));
                 Promise.all([1, Promise.reject(9), 3]).catch(v => push('all reject ' + v));
                 Promise.race([new Promise(() => {}), Promise.resolve(3), 4]).then(v => push('race ' + v));
                 Promise.all(5).catch(e => push('all non-iterable ' + e.name));
                 Promise.resolve({ then: function (a) { push('thenable ' + typeof this + ' ' + arguments.length); a(10) } }).then(v => push('thenable ' + v));
                 Promise.resolve({ get then() { throw 11 } }).catch(v => push('then getter ' + v));
                 var p = Promise.resolve(42);
                 push([Promise.resolve(p) === p, Object.prototype.toString.call(p), String(p), p instanceof Promise].join(' '))"),
            "true [object Promise] [object Promise] true;executor boom;resolved 1;all empty [];\
             all non-iterable TypeError;thenable object 2;then getter 11;then passes 1;\
             catch passes 3;all reject 9;race 3;thenable 10;all [1,2,3]"
        );
    }

    /// Qt 6.12 throws `TypeError: Type error` for all of these.
    #[test]
    fn type_errors() {
        assert_eq!(
            run(
                "function name(f) { try { f(); return 'none' } catch (e) { return e.name } }
                 var result = [
                     name(() => new Promise(5)),
                     name(() => new Promise()),
                     name(() => Promise.prototype.then.call({}, x => x)),
                     name(() => Promise.resolve.call(undefined, 1)),
                 ].join()"
            ),
            "TypeError,TypeError,TypeError,TypeError"
        );
    }

    /// Job order follows ECMA-262: `finally` adopts its thunk's promise, so it settles two jobs
    /// after a plain reaction.
    /// ECMA-262 parts Qt 6.12 lacks (`finally`, `allSettled`, `any`, AggregateError) or gets
    /// wrong (it never settles a self-resolved promise and throws from `Promise.race(5)`).
    #[test]
    fn spec_beyond_qt() {
        assert_eq!(
            run("var log = []; var result;
                 function push(x) { log.push(x); result = log.join(';') }
                 Promise.resolve(5).finally(() => 99).then(v => push('finally keeps ' + v));
                 Promise.reject(6).finally(() => 99).catch(v => push('finally keeps reject ' + v));
                 Promise.resolve(7).finally(() => { throw 8 }).catch(v => push('finally throws ' + v));
                 Promise.resolve(7).finally(function () { push('finally args ' + arguments.length) });
                 Promise.allSettled([1, Promise.reject(2)]).then(v => push(JSON.stringify(v)));
                 Promise.any([Promise.reject(1), Promise.resolve(2)]).then(v => push('any ' + v));
                 Promise.any([Promise.reject(1), Promise.reject(2)]).catch(e => push([e.name, e.message, e.errors, e instanceof AggregateError, e instanceof Error].join()));
                 Promise.any([]).catch(e => push('any empty ' + e.errors.length));
                 Promise.race(5).catch(e => push('race ' + e.name));
                 var self = new Promise(a => Promise.resolve().then(() => a(self)));
                 self.catch(e => push('self ' + e.name));
                 var agg = new AggregateError([1, 2], 'm');
                 push([agg.name, agg.message, agg.errors].join())"),
            "AggregateError,m,1,2;finally args 0;any empty 0;race TypeError;finally throws 8;\
             [{\"status\":\"fulfilled\",\"value\":1},{\"status\":\"rejected\",\"reason\":2}];\
             any 2;AggregateError,All promises were rejected,1,2,true,true;self TypeError;\
             finally keeps 5;finally keeps reject 6"
        );
    }

    /// Each top-level call from the engine (handler, `Qt.callLater` callback, Timer) runs its
    /// jobs as it returns, before the next callback.
    #[test]
    fn handlers_drain_their_jobs() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem { id: r; property string log: \"\"\n\
                 Timer { id: t; interval: 0; onTriggered: { r.log += \"timer;\"; Promise.resolve().then(() => r.log += \"p-timer;\"); Qt.callLater(() => r.log += \"cl-timer;\") } }\n\
                 Component.onCompleted: { log += \"a;\"; Qt.callLater(() => { log += \"cl1;\"; Promise.resolve().then(() => log += \"p-cl1;\") });\
                 Promise.resolve().then(() => log += \"p1;\").then(() => log += \"p1b;\"); Qt.callLater(() => log += \"cl2;\"); t.start(); log += \"b;\" } }\n",
            )
            .unwrap();
        rt.settle();
        assert_eq!(rt.get(root, "log").to_js_string(), "a;b;p1;p1b;");
        while rt.tick(16.0) {}
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        assert_eq!(
            rt.get(root, "log").to_js_string(),
            "a;b;p1;p1b;cl1;p-cl1;cl2;timer;p-timer;cl-timer;"
        );
    }
}
