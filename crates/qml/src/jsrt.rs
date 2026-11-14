//! Runtime ABI for the original JS compiled to Rust ahead of time (docs/aot.md).
//!
//! Generated code calls only this module and the value semantics of [`Interp`]
//! (`get`/`put`/`call`/`construct`/`binary`/`to_string`/`iterate`/`entries`/`throw`).
//! Local variables live in [`Env`] slots resolved at codegen time; names that are not
//! locals (QML ids, properties, imports, globals such as `Math`) go through the QML scope
//! exactly like the interpreter's `Ident` lookup.

pub use crate::eval::PropKey;
use crate::runtime::Sym;
use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::{Callable, Ex, Interp, R, Scope, Stored};
use crate::value::Value;

pub type Cx = Interp;

pub use crate::eval::{define_field, define_field_pk};
pub use crate::generator::init_member;

/// A compiled function body. `env` is the environment it closes over (a bare [`Env::new`] root
/// for top-level bodies); a body whose bindings are captured opens its own frame with
/// [`Env::child`]. `this` the receiver, `args` the call arguments.
pub type CompiledFn = fn(cx: &mut Cx, env: &Env, this: &Value, args: &[Value]) -> R<Value>;

/// The syntactic kind of a function (ECMA-262 §10.2): whether it is a constructor and has a
/// `this`/`arguments` of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FnKind {
    /// A `function` declaration or expression (MakeConstructor).
    Normal,
    /// A class constructor: `[[Construct]]` only, `[[Call]]` throws a TypeError (ECMA-262
    /// §10.2.1 step 2, [[IsClassConstructor]]).
    ClassConstructor,
    /// A derived class constructor ([[ConstructorKind]] derived, ECMA-262 §10.2.2 step 6):
    /// its [[Construct]] makes no object, `super(...)` gets it from the parent.
    DerivedConstructor,
    /// An object-literal or class method (DefineMethod, §15.4.4): not a constructor.
    Method,
    /// A getter or setter (§15.4.5): not a constructor.
    Accessor,
    /// An arrow function (§15.3.4): lexical `this`/`arguments`, not a constructor.
    Arrow,
    /// A generator function or generator method (§15.5): calling it returns a generator
    /// object; not a constructor, but it has an own `prototype` (§15.5.4).
    Generator,
}

impl FnKind {
    pub fn is_arrow(self) -> bool {
        self == FnKind::Arrow
    }

    /// IsConstructor: only plain functions and class constructors have `[[Construct]]` and
    /// an own `prototype`.
    pub fn is_constructor(self) -> bool {
        matches!(
            self,
            FnKind::Normal | FnKind::ClassConstructor | FnKind::DerivedConstructor
        )
    }

    /// [[IsClassConstructor]]: its [[Call]] throws a TypeError (ECMA-262 §10.2.1 step 2).
    pub fn is_class_constructor(self) -> bool {
        matches!(self, FnKind::ClassConstructor | FnKind::DerivedConstructor)
    }
}

/// Static facts about a compiled function, needed by `.name`, `.length`, `new`.
#[derive(Debug)]
pub struct FnMeta {
    pub name: &'static str,
    pub kind: FnKind,
    /// ExpectedArgumentCount: the formal parameters before the first default or rest one.
    pub nparams: u32,
    /// Slots of the frame the body makes (informational).
    pub nslots: u32,
    /// Strict mode code (ECMA-262 §11.2.2).
    pub strict: bool,
}

/// Which QML/JS construct a compiled entry implements: the kind [`crate::walk`] gave its site
/// when it was generated, checked against how the runtime runs the site.
pub use crate::walk::BodyKind;

/// One generated function with its key (see docs/aot.md).
#[derive(Debug)]
pub struct Entry {
    pub file: &'static str,
    pub path: &'static str,
    pub member: &'static str,
    pub kind: BodyKind,
    pub f: CompiledFn,
    pub meta: FnMeta,
    /// FNV-1a 64 of the script source the entry was generated from.
    pub hash: u64,
}

/// A declaration's build-time layout: `(objpath, property names in declaration order)`, slot
/// `i` = `names[i]` (rigen `LAYOUTS`).
pub type Layout = (&'static str, &'static [&'static str]);

/// Source of compiled code for a runtime (`ricompiled`, or a filtered view for aotcheck).
/// Paths are as the runtime loads them (absolute, or a `types::module_file` key); the
/// implementation maps them to keys relative to its original root.
pub trait Aot {
    /// Compiled entry of each script of `doc` that has one, by [`crate::Script::id`].
    fn doc_scripts(
        &self,
        file: &std::path::Path,
        doc: &crate::Document,
    ) -> Vec<(crate::NodeId, &'static Entry)>;
    fn library(&self, file: &std::path::Path) -> Option<&'static Entry>;
    /// The compiled code of a JS file `Qt.include` runs in its caller's context (rigen
    /// `SCRIPTS`): its top-level names live in that context, not in a scope of its own.
    fn script(&self, file: &std::path::Path) -> Option<&'static Entry>;
    /// Per object declaration of `doc` (by `Object::decl`), its build-time layout: property
    /// names in declaration order, slot `i` = `names[i]` (rigen `LAYOUTS`).
    fn doc_layouts(
        &self,
        _file: &std::path::Path,
        _doc: &crate::Document,
    ) -> Vec<(crate::NodeId, &'static [&'static str])> {
        Vec::new()
    }
}

thread_local! {
    static LIB_AOT: RefCell<Option<Rc<dyn Aot>>> = const { RefCell::new(None) };
}

/// Run `f` with `aot` as the source of compiled `.js` libraries (used by `load_library`).
pub fn with_lib_aot<T>(aot: Option<Rc<dyn Aot>>, f: impl FnOnce() -> T) -> T {
    let prev = LIB_AOT.with(|a| a.replace(aot));
    let r = f();
    LIB_AOT.with(|a| *a.borrow_mut() = prev);
    r
}

thread_local! {
    static RAN: RefCell<Option<std::collections::HashSet<usize>>> = const { RefCell::new(None) };
}

/// Record which compiled bodies run on this thread (by their [`FnMeta`] address), for
/// coverage checks; `false` stops recording.
pub fn track_runs(on: bool) {
    RAN.with(|r| *r.borrow_mut() = on.then(Default::default));
}

/// [`FnMeta`] addresses of the compiled bodies run since [`track_runs`] (cleared).
pub fn take_runs() -> std::collections::HashSet<usize> {
    RAN.with(|r| {
        r.borrow_mut()
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    })
}

#[inline]
fn note_run(meta: &FnMeta) {
    RAN.with(|r| {
        if let Some(s) = r.borrow_mut().as_mut() {
            s.insert(meta as *const FnMeta as usize);
        }
    });
}

pub fn lib_aot() -> Option<Rc<dyn Aot>> {
    LIB_AOT.with(|a| a.borrow().clone())
}

/// Top-level name `name` of a library body, bound in slot `slot` of the frame of `env`: a
/// property of the library's namespace object (see [`crate::eval::ScriptCtx::export`]).
pub fn export_slot(cx: &mut Cx, name: &str, env: &Env, slot: u32, var_like: bool) {
    let script = cx.global.borrow().script.clone();
    if let (Some(ctx), Some(frame)) = (
        script.as_ref().and_then(|s| s.ctx.as_ref()),
        env.frame.as_ref(),
    ) {
        ctx.export(
            name,
            crate::eval::Binding::Slot(frame.clone(), slot),
            var_like,
        );
    }
}

/// A top-level `var` or function of a script `Qt.include` runs: a name of the context it runs in
/// (`cx.global` meanwhile), keeping the value it has there.
pub fn declare_global(cx: &mut Cx, name: &str) {
    cx.global
        .borrow_mut()
        .vars
        .entry(name.into())
        .or_insert(Value::Undefined);
}

/// Run a compiled entry in `env`: the value of a binding/handler/library body, or the function
/// value of a `Function` entry.
pub fn run_entry(
    cx: &mut Cx,
    e: &'static Entry,
    env: &Env,
    this: Value,
    args: &[Value],
) -> R<Value> {
    match e.kind {
        BodyKind::Function => Ok(closure(e.f, env, &e.meta)),
        _ => {
            note_run(&e.meta);
            let frame = cx.enter()?;
            let code = std::mem::replace(&mut cx.code, env.root.vars.clone());
            let qs = std::mem::replace(&mut cx.qs, env.root.qs);
            let r = crate::eval::on_js_stack(|| (e.f)(cx, env, &this, args));
            cx.code = code;
            cx.qs = qs;
            cx.leave(frame, r)
        }
    }
}

/// What a compiled body resolves non-local names against: its QML scope `(item, ctx)` and the
/// vars the runtime binds by name for it (a handler's signal parameters, a library's own
/// declarations and `.import` aliases). Bindings and functions have no such vars.
#[derive(Clone, Debug, Default)]
pub struct Root {
    pub qs: Option<(usize, usize)>,
    pub vars: Option<Scope>,
}

/// A frame of compiled code that holds the bindings its nested functions capture.
#[derive(Debug)]
pub struct Slots {
    pub slots: RefCell<Vec<Value>>,
    pub parent: Option<Rc<Slots>>,
    pub root: Root,
}

/// The environment of a compiled body: its innermost captured frame (none until a scope with
/// captured bindings opens) and its [`Root`]. Locals resolve to `(hops, slot)` at codegen time,
/// so running a body without captures allocates nothing.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub frame: Option<Rc<Slots>>,
    pub root: Root,
    /// The function object of the call whose body runs in this environment (a function
    /// Environment Record's [[FunctionObject]], ECMA-262 §9.1.1.3).
    pub callee: Option<Value>,
    /// The environment's [[NewTargetValue]] (ECMA-262 §9.1.1.3): the NewTarget of the
    /// [[Construct]] running the body, `undefined` in a [[Call]].
    pub new_target: Value,
}

impl Drop for Slots {
    fn drop(&mut self) {
        let mut o = crate::value::Orphans::default();
        o.values(self.slots.get_mut().iter_mut());
        o.slots(self.parent.take());
        o.scope(self.root.vars.take());
    }
}

impl Env {
    /// Root of a body run in the QML scope `qs` with the by-name `vars` (usually none).
    pub fn new(qs: Option<(usize, usize)>, vars: Option<Scope>) -> Env {
        Env {
            callee: None,
            new_target: Value::Undefined,
            frame: None,
            root: Root { qs, vars },
        }
    }

    /// Root over an interpreter scope chain (its vars and its QML scope, `Frame.qml`).
    pub fn from_scope(scope: Scope) -> Env {
        Env::new(Interp::qml_scope(&scope), Some(scope))
    }

    /// The environment of a closure's body: its captured frame.
    pub fn of(frame: &Rc<Slots>) -> Env {
        Env {
            callee: None,
            new_target: Value::Undefined,
            frame: Some(frame.clone()),
            root: frame.root.clone(),
        }
    }

    /// The environment of a call of the closure `callee` over `frame`.
    pub fn call(frame: &Rc<Slots>, callee: &Value) -> Env {
        Env {
            callee: Some(callee.clone()),
            ..Env::of(frame)
        }
    }

    /// The environment of a [[Construct]] of the closure `callee` over `frame` for the
    /// NewTarget `new_target` (OrdinaryCallEvaluateBody from §10.2.2 step 8).
    pub fn construct(frame: &Rc<Slots>, callee: &Value, new_target: &Value) -> Env {
        Env {
            new_target: new_target.clone(),
            ..Env::call(frame, callee)
        }
    }

    /// A nested scope with `nslots` captured bindings.
    pub fn child(&self, nslots: u32) -> Env {
        let frame = Rc::new(Slots {
            slots: RefCell::new(vec![Value::Undefined; nslots as usize]),
            parent: self.frame.clone(),
            root: self.root.clone(),
        });
        Env {
            callee: None,
            new_target: Value::Undefined,
            frame: Some(frame),
            root: self.root.clone(),
        }
    }

    /// The same scope with a fresh frame holding the current slot values (per-iteration `let`).
    pub fn iteration_copy(&self) -> Env {
        let frame = self.frame.as_ref().map(|f| {
            Rc::new(Slots {
                slots: RefCell::new(f.slots.borrow().clone()),
                parent: f.parent.clone(),
                root: f.root.clone(),
            })
        });
        Env {
            callee: None,
            new_target: Value::Undefined,
            frame,
            root: self.root.clone(),
        }
    }

    /// The enclosing environment `hops` captured frames out.
    pub fn up(&self, hops: u32) -> Env {
        let mut frame = self.frame.clone();
        for _ in 0..hops {
            frame = frame.expect("jsrt: env hops past root").parent.clone();
        }
        Env {
            callee: None,
            new_target: Value::Undefined,
            frame,
            root: self.root.clone(),
        }
    }

    fn frame_at(&self, hops: u32) -> &Slots {
        let mut f = self
            .frame
            .as_deref()
            .expect("jsrt: slot read in a frameless env");
        for _ in 0..hops {
            f = f.parent.as_deref().expect("jsrt: slot hops past root");
        }
        f
    }

    #[inline]
    pub fn get(&self, hops: u32, slot: u32) -> Value {
        self.frame_at(hops).slots.borrow()[slot as usize].clone()
    }

    #[inline]
    pub fn set(&self, hops: u32, slot: u32, v: Value) {
        self.frame_at(hops).slots.borrow_mut()[slot as usize] = v;
    }

    /// Value of a by-name var: the root's vars, then the JS globals.
    pub fn var(&self, cx: &Cx, name: &str) -> Option<Value> {
        if let Some(sc) = &self.root.vars
            && let Some(v) = Interp::find_var(sc, name)
        {
            return Some(v);
        }
        cx.global.borrow().vars.get(name).cloned()
    }

    /// Whether the by-name var [`Self::var`] finds is a property of the global object.
    pub fn is_global_var(&self, name: &str) -> bool {
        match &self.root.vars {
            Some(sc) if Interp::find_var(sc, name).is_some() => Interp::is_global_var(sc, name),
            _ => true,
        }
    }

    /// the frozen builtins of the global object (`Interp::store_var`).
    pub fn store_var(&self, cx: &Cx, name: &str, v: Value) -> Stored {
        let v = match &self.root.vars {
            Some(sc) => match Interp::store_var(sc, name, v) {
                Stored::Undeclared(v) => v,
                done => return done,
            },
            None => v,
        };
        let mut g = cx.global.borrow_mut();
        if g.global && crate::builtins::is_frozen_global(name) {
            return Stored::Undeclared(v);
        }
        match g.vars.get_mut(name) {
            Some(slot) => {
                *slot = v;
                Stored::Done
            }
            None => Stored::Undeclared(v),
        }
    }
}

/// `function`/arrow expression: a callable value closing over `env` (its captured frame, or a
/// slotless one holding just the root).
pub fn closure(f: CompiledFn, env: &Env, meta: &'static FnMeta) -> Value {
    let frame = env.frame.clone().unwrap_or_else(|| {
        Rc::new(Slots {
            slots: RefCell::new(Vec::new()),
            parent: None,
            root: env.root.clone(),
        })
    });
    crate::eval::fn_value(Callable::Compiled(f, frame, meta))
}

/// Entry for `Interp::call`/`construct` on a compiled callable.
pub fn call_compiled(
    cx: &mut Cx,
    f: CompiledFn,
    env: &Env,
    meta: &FnMeta,
    this: Value,
    args: &[Value],
) -> R<Value> {
    note_run(meta);
    let caller = std::mem::replace(&mut cx.strict, meta.strict);
    let code = std::mem::replace(&mut cx.code, env.root.vars.clone());
    let qs = std::mem::replace(&mut cx.qs, env.root.qs);
    let r = f(cx, env, &this, args);
    cx.strict = caller;
    cx.code = code;
    cx.qs = qs;
    let r = r?;
    if meta.kind == FnKind::Generator {
        let callee = env.callee.clone().unwrap_or_default();
        return Ok(crate::generator::make_generator(cx, &callee, r));
    }
    Ok(r)
}

/// A free name that is no by-name var: the QML scope, then the value globals.
fn qml_name(cx: &mut Cx, env: &Env, name: &str) -> R<Value> {
    if let Some(v) = cx.host_lookup(env.root.qs, name) {
        return Ok(v);
    }
    match name {
        "undefined" => Ok(Value::Undefined),
        "NaN" => Ok(Value::Num(f64::NAN)),
        "Infinity" => Ok(Value::Num(f64::INFINITY)),
        _ => match cx.global_binding(name)? {
            Some(v) => Ok(v),
            None => cx.throw("ReferenceError", format!("{name} is not defined")),
        },
    }
}

/// Free name read (not a local): by-name var or global, QML scope, imports,
/// `undefined`/`NaN`/`Infinity`, else `ReferenceError` (`Interp::lookup`).
#[inline]
pub fn lookup(cx: &mut Cx, env: &Env, name: &str) -> R<Value> {
    match env.var(cx, name) {
        Some(v) => Ok(v),
        None => qml_name(cx, env, name),
    }
}

/// `typeof name` for a free name: never throws.
pub fn type_of_name(cx: &mut Cx, env: &Env, name: &str) -> Value {
    match lookup(cx, env, name) {
        Ok(v) => Value::str(v.type_of()),
        Err(_) => Value::str("undefined"),
    }
}

/// The object a primitive `this` of a call of non-strict code binds as
/// ([`Interp::sloppy_this`]).
pub fn sloppy_this(cx: &Cx, this: &Value) -> Option<Value> {
    cx.sloppy_this(this)
}

/// `delete o.k` / `delete o[k]`.
pub fn delete(cx: &mut Cx, o: &Value, k: &str) -> R<Value> {
    match o {
        Value::Obj(ob) => cx.delete_property(ob, k),
        _ => cx.delete_of_primitive(o, &PropKey::Str(k.into()), cx.strict),
    }
}

/// `delete o[k]` in strict mode code whatever `cx.strict` is (the inline parts of a class).
pub fn delete_strict(cx: &mut Cx, o: &Value, k: &str) -> R<Value> {
    match o {
        Value::Obj(ob) => cx.delete_property_or_throw(ob, k, true),
        _ => cx.delete_of_primitive(o, &PropKey::Str(k.into()), true),
    }
}

/// `delete o[k]` for a computed key.
pub fn delete_pk(cx: &mut Cx, o: &Value, k: &PropKey) -> R<Value> {
    match (o, k) {
        (Value::Obj(ob), _) => cx.delete_property_pk(ob, k),
        _ => cx.delete_of_primitive(o, k, cx.strict),
    }
}

/// `delete o[k]` for a computed key in strict mode code (see [`delete_strict`]).
pub fn delete_strict_pk(cx: &mut Cx, o: &Value, k: &PropKey) -> R<Value> {
    match o {
        Value::Obj(ob) => cx.delete_property_or_throw_pk(ob, k, true),
        _ => cx.delete_of_primitive(o, k, true),
    }
}

/// Template literal: `quasis[0] + str(v0) + quasis[1] + ...`.
pub fn template(cx: &mut Cx, quasis: &[&str], vals: &[Value]) -> R<Value> {
    let mut s = String::new();
    for (i, q) in quasis.iter().enumerate() {
        s.push_str(q);
        if let Some(v) = vals.get(i) {
            s.push_str(&cx.to_string(v)?);
        }
    }
    Ok(Value::str(&s))
}

/// GetTemplateObject (ECMA-262 §13.2.8.4) of a compiled tagged template site: its strings are
/// statics of the site, so their address is its key.
pub fn template_object(
    cx: &mut Cx,
    cooked: &'static [Option<&'static str>],
    raw: &'static [&'static str],
) -> Value {
    let key = cooked.as_ptr() as usize;
    match cx.template_object(key) {
        Some(v) => v,
        None => cx.new_template_object(key, None, cooked.iter().copied(), raw.iter().copied()),
    }
}

pub fn regex(pattern: &str, flags: &str) -> Value {
    crate::builtins::regex_value(pattern, flags)
}

/// Object literal builder (keeps insertion order like the interpreter).
pub fn object(n: usize) -> Value {
    let o = crate::eval::new_obj();
    {
        let mut b = o.borrow_mut();
        b.keys.reserve(n);
        b.props.reserve(n);
    }
    Value::Obj(o)
}

pub fn object_set(o: &Value, k: &str, v: Value) {
    if let Value::Obj(ob) = o {
        crate::eval::set_own_rc(ob, k.into(), v);
    }
}

/// `__proto__: v` of object literal `o` (see [`crate::eval::literal_proto`]).
pub fn object_proto(o: &Value, v: &Value) {
    if let Value::Obj(ob) = o {
        crate::eval::literal_proto(ob, v);
    }
}

/// SetFunctionName of anonymous function `f` defined under computed key `k`.
pub fn name_function(f: &Value, prefix: &str, k: &str) {
    crate::eval::set_function_name(f, &format!("{prefix}{k}"));
}

/// [`name_function`] for a computed key: a symbol key names it `[description]`, or the
/// empty string without one (ECMA-262 SetFunctionName step 2).
pub fn name_function_pk(f: &Value, prefix: &str, k: &PropKey) {
    match k {
        PropKey::Str(k) => name_function(f, prefix, k),
        PropKey::Sym(s) => {
            let d = s.description.as_deref().map(|d| format!("[{d}]"));
            crate::eval::set_function_name(f, &format!("{prefix}{}", d.unwrap_or_default()));
        }
    }
}

/// `[k]: v` inside an object literal.
pub fn object_set_pk(o: &Value, k: &PropKey, v: Value) {
    if let Value::Obj(ob) = o {
        match k {
            PropKey::Str(k) => crate::eval::set_own(ob, k, v),
            PropKey::Sym(s) => crate::eval::define_sym(ob, s, v, true),
        }
    }
}

/// `get k() {}` (`getter`) / `set k(v) {}` inside an object literal.
pub fn object_accessor(o: &Value, k: &str, getter: bool, f: Value) {
    if let Value::Obj(ob) = o {
        crate::eval::define_accessor(ob, k, getter, f, true);
    }
}

/// `get [k]() {}` / `set [k](v) {}` inside an object literal.
pub fn object_accessor_pk(o: &Value, k: &PropKey, getter: bool, f: Value) {
    if let Value::Obj(ob) = o {
        match k {
            PropKey::Str(k) => crate::eval::define_accessor(ob, k, getter, f, true),
            PropKey::Sym(s) => crate::eval::define_sym_accessor(ob, s, getter, f, true),
        }
    }
}

/// `{...src}` inside an object literal.
pub fn object_spread(cx: &mut Cx, o: &Value, src: &Value) -> R<()> {
    match o {
        Value::Obj(ob) => cx.copy_data_properties(ob, src, &[]),
        _ => Ok(()),
    }
}

/// QML sequence properties write back on in-place mutation: after `base.p.push(x)` with
/// `base` an item and `p` holding an array, the property is re-set so it notifies.
pub fn write_back_member(cx: &mut Cx, base: &Value, prop: &str, arr: &Value) {
    if !matches!(arr, Value::Obj(x) if x.borrow().array.is_some()) {
        return;
    }
    if let Value::Item(i) = base {
        cx.host.sequence_written(*i, prop, arr);
    }
}

/// Same for a bare free name (`list.push(x)` where `list` is a QML property in scope).
pub fn write_back_name(cx: &mut Cx, env: &Env, name: &str, arr: &Value) {
    if !matches!(arr, Value::Obj(x) if x.borrow().array.is_some()) {
        return;
    }
    if let Some((item, ctx)) = env.root.qs {
        cx.host.write_back(item, ctx, name, arr.clone());
    }
}

/// `base.k = v` when `base` is a color: the changed color ([`crate::valuetypes::color_put`]),
/// which the caller stores where it read `base`. `None` for any other `base`.
pub fn color_put(cx: &mut Cx, base: &Value, k: &str, v: &Value) -> R<Option<Value>> {
    match base {
        Value::Color(c) => Ok(Some(Value::Color(crate::valuetypes::color_put(
            cx, *c, k, v,
        )?))),
        _ => Ok(None),
    }
}

/// [`color_put`] for `base[k] = v`: `None` also when `k` names no field of a color.
pub fn color_put_key(cx: &mut Cx, base: &Value, k: &Value, v: &Value) -> R<Option<Value>> {
    match (base, k) {
        (Value::Color(_), Value::Str(k)) if crate::valuetypes::is_color_field(k) => {
            color_put(cx, base, k, v)
        }
        _ => Ok(None),
    }
}

/// A Date read from `date` property `prop` and changed by a setter called on that read is
/// stored back in the property.
pub fn write_back_date(cx: &mut Cx, date: &Value, prop: &str) {
    let owner = match date {
        Value::Obj(x) if matches!(x.borrow().internal, crate::value::Internal::Date(_)) => {
            x.borrow().owner.clone()
        }
        _ => None,
    };
    if let Some((i, p)) = owner.filter(|(_, p)| **p == *prop) {
        cx.host.set(i, &p, date.clone());
    }
}

/// Any other mutated array: notify the item property that owns it, if any.
pub fn write_back_owner(cx: &mut Cx, arr: &Value) {
    let owner = match arr {
        Value::Obj(x) if x.borrow().array.is_some() => x.borrow().owner.clone(),
        _ => None,
    };
    if let Some((i, p)) = owner {
        cx.host.sequence_changed(i, &p, arr);
    }
}

/// `arguments` object of a non-arrow function.
/// The unmapped arguments object of a call (strict code, non-simple parameters).
pub fn arguments(cx: &Cx, args: &[Value]) -> Value {
    Value::Obj(crate::eval::unmapped_arguments(cx, args))
}

/// The mapped arguments object of a call in `env` whose formal `k` has the binding `ids[k]`
/// (`k`, or `u32::MAX` when a later formal has its name); it holds the formals' bindings.
pub fn mapped_arguments(cx: &Cx, env: &Env, args: &[Value], ids: &[u32]) -> Value {
    let callee = env.callee.clone().unwrap_or_default();
    Value::Obj(crate::eval::mapped_arguments(cx, callee, args, ids.into()))
}

/// Read of formal `k` of a function with the mapped arguments object `a`.
pub fn arg_get(a: &Value, k: usize) -> Value {
    match a {
        Value::Obj(o) => crate::eval::arg_get(o, k),
        _ => Value::Undefined,
    }
}

/// Write of formal `k` of a function with the mapped arguments object `a`.
pub fn arg_set(a: &Value, k: usize, v: Value) {
    if let Value::Obj(o) = a {
        crate::eval::arg_set(o, k, v);
    }
}

/// Coerced `e` of `throw e` (generated code returns `Err(throw(v))`).
pub fn throw(v: Value) -> crate::eval::Ex {
    crate::eval::Ex::Throw(v)
}

/// The exception value of a caught error (`catch (e)`); `Short` (optional chain) is not catchable.
pub fn caught(e: crate::eval::Ex) -> Result<Value, crate::eval::Ex> {
    match e {
        crate::eval::Ex::Throw(v) => Ok(v),
        other => Err(other),
    }
}

/// Array methods that mutate in place: after `o.m(...)` generated code calls a `write_back_*`.
pub fn mutates(name: &str) -> bool {
    matches!(
        name,
        "push"
            | "pop"
            | "shift"
            | "unshift"
            | "splice"
            | "sort"
            | "reverse"
            | "fill"
            | "copyWithin"
    )
}

fn nullish(v: &Value) -> bool {
    matches!(v, Value::Undefined | Value::Null)
}

/// `o.name(args)` / `o?.name(args)` (`opt_base`) / `o.name?.(args)` (`opt_call`), with `o`
/// already evaluated. Builtin methods (arrays, strings, ...) and C++ item methods included.
/// An optional short-circuit returns `Err(Ex::Short)`, caught by the enclosing chain.
pub fn call_member(
    cx: &mut Cx,
    ov: &Value,
    name: &str,
    args: &[Value],
    opt_base: bool,
    opt_call: bool,
) -> R<Value> {
    if opt_base && nullish(ov) {
        return Err(Ex::Short);
    }
    if let Some(r) = crate::builtins::method(cx, ov, name, args)? {
        return Ok(r);
    }
    cx.nullish_call(ov, name)?;
    let f = cx.get(ov, name)?;
    if let (Value::Item(i), Value::Undefined) = (ov, &f)
        && let Some(r) = cx.host_call(*i, name, args)
    {
        return r;
    }
    if opt_call && nullish(&f) {
        return Err(Ex::Short);
    }
    if !matches!(&f, Value::Obj(o) if o.borrow().call.is_some()) {
        return cx.not_a_method(ov, name);
    }
    cx.call(&f, ov.clone(), args)
}

/// `o[k](args)` with `o` and the key already evaluated.
pub fn call_index(
    cx: &mut Cx,
    ov: &Value,
    key: &Value,
    args: &[Value],
    opt_base: bool,
    opt_call: bool,
) -> R<Value> {
    if opt_base && nullish(ov) {
        return Err(Ex::Short);
    }
    if let Value::Symbol(s) = key {
        let f = cx.get_sym(ov, s)?;
        if opt_call && nullish(&f) {
            return Err(Ex::Short);
        }
        return cx.call(&f, ov.clone(), args);
    }
    let name = key.to_js_string();
    if let Some(r) = crate::builtins::method(cx, ov, &name, args)? {
        return Ok(r);
    }
    let f = cx.get(ov, &name)?;
    if opt_call && nullish(&f) {
        return Err(Ex::Short);
    }
    cx.call(&f, ov.clone(), args)
}

/// Call of a local (slot) value: `f(args)`, `this` undefined.
pub fn call_local(cx: &mut Cx, f: &Value, args: &[Value], opt_call: bool) -> R<Value> {
    if opt_call && nullish(f) {
        return Err(Ex::Short);
    }
    if crate::eval::callable(f).is_none() {
        return cx.not_a_function(f);
    }
    cx.call(f, Value::Undefined, args)
}

/// Call of a free name `name(args)` (not a local): by-name var (imports, signal params), C++
/// method of the QML scope object (`stop()` in a Timer), else a QML-scope function.
pub fn call_name(cx: &mut Cx, env: &Env, name: &str, args: &[Value], opt_call: bool) -> R<Value> {
    if let Some(f) = env.var(cx, name) {
        // QV4 names a property of the global object by its name, any other var by its value.
        let named = crate::eval::callable(&f).is_none()
            && !(opt_call && nullish(&f))
            && env.is_global_var(name);
        return match named {
            true => cx.not_a_named_function(&Value::Undefined, name),
            false => call_local(cx, &f, args, opt_call),
        };
    }
    call_scope_name(
        cx,
        env.root.qs,
        crate::runtime::Sym::new(name),
        args,
        opt_call,
    )
}

/// QML scope `(item, ctx)` of the body.
#[inline]
pub fn qml_scope(env: &Env) -> Option<(usize, usize)> {
    env.root.qs
}

/// Free-name read for a name the generator proved is neither a JS variable nor a by-name var
/// (signal param, import alias): QML scope, ids, singletons, types, then globals.
pub fn lookup_qml(cx: &mut Cx, env: &Env, name: Sym) -> R<Value> {
    if let Some(v) = cx.host_lookup_sym(env.root.qs, name) {
        return Ok(v);
    }
    lookup(cx, env, &name)
}

/// `name = v` for such a name: QML property in scope, else declared var, else
/// [`Interp::invalid_global_write`] (QML code cannot create globals).
pub fn assign_qml(cx: &mut Cx, env: &Env, name: Sym, v: Value) -> R<()> {
    if let Some((item, ctx)) = env.root.qs
        && cx.host_assign(item, ctx, &name, v.clone())?
    {
        return Ok(());
    }
    match env.store_var(cx, &name, v) {
        Stored::Done => Ok(()),
        Stored::Uninitialized => cx.uninitialized(&name),
        Stored::Undeclared(_) => cx.invalid_global_write(&name),
    }
}

/// `name(args)` for such a name: native/signal method of the scope item, else the value
/// of `lookup_qml`.
pub fn call_qml(cx: &mut Cx, env: &Env, name: Sym, args: &[Value], opt_call: bool) -> R<Value> {
    call_scope_name(cx, env.root.qs, name, args, opt_call)
}

/// Call of a free name that is no var: native/signal method of the scope item `qs.0`, else the
/// value the QML scope resolves it to. As QV4 (`callQmlScopeObjectProperty`), a function found
/// as a property of the scope object gets that object as `this`; anything else gets `undefined`.
pub fn call_scope_name(
    cx: &mut Cx,
    qs: Option<(usize, usize)>,
    name: Sym,
    args: &[Value],
    opt_call: bool,
) -> R<Value> {
    if let Some((item, _)) = qs
        && let Some(r) = cx.host_call(item, &name, args)
    {
        return r;
    }
    let (f, this) = match cx.host_lookup_scoped(qs, name) {
        Some((f, true)) => (
            f,
            qs.map_or(Value::Undefined, |(item, _)| Value::Item(item)),
        ),
        Some((f, false)) => (f, Value::Undefined),
        None => match &*name {
            "undefined" => (Value::Undefined, Value::Undefined),
            "NaN" => (Value::Num(f64::NAN), Value::Undefined),
            "Infinity" => (Value::Num(f64::INFINITY), Value::Undefined),
            _ => return cx.throw("ReferenceError", format!("{} is not defined", &*name)),
        },
    };
    if opt_call && nullish(&f) {
        return Err(Ex::Short);
    }
    if crate::eval::callable(&f).is_none() {
        return cx.not_a_named_function(&this, &name);
    }
    cx.call(&f, this, args)
}

/// End of an optional chain: `Err(Short)` inside it becomes `undefined`.
pub fn chain(r: R<Value>) -> R<Value> {
    match r {
        Err(Ex::Short) => Ok(Value::Undefined),
        r => r,
    }
}

/// `o?.k` member read with short-circuit.
pub fn get_opt(cx: &mut Cx, ov: &Value, name: &str) -> R<Value> {
    if nullish(ov) {
        return Err(Ex::Short);
    }
    cx.get(ov, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{Frame, Host};

    struct H;
    impl Host for H {}

    // Hand-written stand-in for generated code of:
    //   function outer(k) { return [1, 2, 3].map(x => x * k + base) }   // `base` is free
    static INNER: FnMeta = FnMeta {
        name: "",
        kind: FnKind::Arrow,
        nparams: 1,
        nslots: 1,
        strict: false,
    };
    fn inner(cx: &mut Cx, env: &Env, _this: &Value, args: &[Value]) -> R<Value> {
        let env = &Env::child(env, 1);
        env.set(0, 0, args.first().cloned().unwrap_or_default());
        let x = env.get(0, 0);
        let k = env.get(1, 0);
        let m = cx.binary(crate::eval::BinOp::Mul, x, k)?;
        let base = lookup(cx, env, "base")?;
        cx.binary(crate::eval::BinOp::Add, m, base)
    }
    static OUTER: FnMeta = FnMeta {
        name: "outer",
        kind: FnKind::Normal,
        nparams: 1,
        nslots: 1,
        strict: false,
    };
    fn outer(cx: &mut Cx, env: &Env, _this: &Value, args: &[Value]) -> R<Value> {
        let env = &Env::child(env, 1);
        env.set(0, 0, args.first().cloned().unwrap_or_default());
        let arr = Value::array(vec![Value::Num(1.0), Value::Num(2.0), Value::Num(3.0)]);
        let cb = closure(inner, env, &INNER);
        call_member(cx, &arr, "map", &[cb], false, false)
    }

    // Stand-in for compiled `function rec() { return rec() }`.
    static REC: FnMeta = FnMeta {
        name: "rec",
        kind: FnKind::Normal,
        nparams: 0,
        nslots: 0,
        strict: false,
    };
    fn rec(cx: &mut Cx, env: &Env, _this: &Value, _args: &[Value]) -> R<Value> {
        let f = lookup(cx, env, "rec")?;
        cx.call(&f, Value::Undefined, &[])
    }

    #[test]
    fn bodies_without_captures_run_without_a_frame() {
        let root = Env::new(Some((7, 3)), None);
        assert!(root.frame.is_none());
        let child = root.child(2);
        child.set(0, 1, Value::Num(4.0));
        assert_eq!(child.get(0, 1).to_number(), 4.0);
        assert!(child.up(1).frame.is_none());
        assert_eq!(child.up(1).root.qs, Some((7, 3)));
        // A closure over a frameless env keeps just the root.
        let Value::Obj(f) = closure(rec, &root, &REC) else {
            panic!("no function")
        };
        let Some(Callable::Compiled(_, frame, _)) = &f.borrow().call else {
            panic!("not compiled")
        };
        assert!(frame.slots.borrow().is_empty() && frame.parent.is_none());
        assert_eq!(Env::of(frame).root.qs, Some((7, 3)));
    }

    #[test]
    fn by_name_vars_come_before_globals_and_take_writes() {
        let mut it = Interp::new(Box::new(H));
        it.global
            .borrow_mut()
            .vars
            .insert("g".into(), Value::Num(1.0));
        let vars: Scope = Rc::new(RefCell::new(Frame::default()));
        vars.borrow_mut()
            .vars
            .insert("mouse".into(), Value::Num(2.0));
        vars.borrow_mut().vars.insert("g".into(), Value::Num(3.0));
        let env = Env::new(None, Some(vars.clone()));
        assert_eq!(lookup(&mut it, &env, "g").ok().unwrap().to_number(), 3.0);
        assert_eq!(
            lookup(&mut it, &Env::default(), "g")
                .ok()
                .unwrap()
                .to_number(),
            1.0
        );
        assert!(lookup(&mut it, &env, "nope").is_err());
        assert_eq!(
            type_of_name(&mut it, &env, "nope").to_js_string(),
            "undefined"
        );
        assert!(assign_qml(&mut it, &env, Sym::new("mouse"), Value::Num(5.0)).is_ok());
        // QML code cannot create globals: an undeclared write throws.
        assert!(assign_qml(&mut it, &env, Sym::new("fresh"), Value::Num(6.0)).is_err());
        assert_eq!(vars.borrow().vars.get("mouse").unwrap().to_number(), 5.0);
        assert!(!vars.borrow().vars.contains_key("fresh"));
        assert!(!it.global.borrow().vars.contains_key("fresh"));
    }

    #[test]
    fn builtin_globals_are_frozen() {
        let mut it = Interp::new(Box::new(H));
        let math = it.global.borrow().vars.get("Math").cloned().unwrap();
        // A binding's env, and a library body's (its root vars are the global frame).
        for env in [Env::default(), Env::from_scope(it.global.clone())] {
            let e = assign_qml(&mut it, &env, Sym::new("Math"), Value::Num(1.0));
            let Err(Ex::Throw(e)) = e else {
                panic!("write to Math did not throw")
            };
            assert_eq!(
                it.get(&e, "message").unwrap().to_js_string(),
                "Invalid write to global property \"Math\""
            );
            assert!(matches!(
                env.store_var(&it, "JSON", Value::Num(1.0)),
                crate::eval::Stored::Undeclared(_)
            ));
        }
        let now = it.global.borrow().vars.get("Math").cloned().unwrap();
        assert!(matches!((&math, &now), (Value::Obj(a), Value::Obj(b)) if Rc::ptr_eq(a, b)));
        // A library's top-level declaration in the global frame still takes writes.
        it.global
            .borrow_mut()
            .vars
            .insert("k".into(), Value::Num(1.0));
        let env = Env::from_scope(it.global.clone());
        assert!(assign_qml(&mut it, &env, Sym::new("k"), Value::Num(2.0)).is_ok());
        assert_eq!(it.global.borrow().vars.get("k").unwrap().to_number(), 2.0);
    }

    // `this` of the called function.
    static THIS: FnMeta = FnMeta {
        name: "t",
        kind: FnKind::Normal,
        nparams: 0,
        nslots: 0,
        strict: false,
    };
    fn this_of(_cx: &mut Cx, _env: &Env, this: &Value, _args: &[Value]) -> R<Value> {
        Ok(this.clone())
    }

    /// Resolves `own` as a property of the scope object, `other` elsewhere (an id, the context).
    struct Scoped(Value);
    impl Host for Scoped {
        fn lookup_scoped(&mut self, _: Option<(usize, usize)>, name: Sym) -> Option<(Value, bool)> {
            match &*name {
                "own" => Some((self.0.clone(), true)),
                "other" => Some((self.0.clone(), false)),
                _ => None,
            }
        }
    }

    #[test]
    fn scope_object_functions_get_it_as_this() {
        let f = closure(this_of, &Env::default(), &THIS);
        let mut it = Interp::new(Box::new(Scoped(f)));
        let qs = Some((7, 0));
        let own = call_scope_name(&mut it, qs, Sym::new("own"), &[], false)
            .ok()
            .unwrap();
        assert!(matches!(own, Value::Item(7)));
        let other = call_scope_name(&mut it, qs, Sym::new("other"), &[], false)
            .ok()
            .unwrap();
        assert!(matches!(other, Value::Undefined));
        assert!(call_scope_name(&mut it, qs, Sym::new("missing"), &[], false).is_err());
        assert!(matches!(
            call_scope_name(&mut it, qs, Sym::new("missing"), &[], true),
            Err(Ex::Throw(_))
        ));
    }

    #[test]
    fn deep_closure_chains_drop_without_recursion() {
        let small = std::thread::Builder::new().stack_size(256 << 10);
        small
            .spawn(|| {
                let mut env = Env::new(None, None).child(1);
                for _ in 0..100_000 {
                    let f = closure(inner, &env, &INNER);
                    let child = env.child(1);
                    child.set(0, 0, f);
                    env = child;
                }
                drop(env);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn compiled_recursion_throws_range_error() {
        let mut it = Interp::new(Box::new(H));
        let scope: Scope = Rc::new(RefCell::new(Frame::default()));
        scope.borrow_mut().parent = Some(it.global.clone());
        let f = closure(rec, &Env::from_scope(scope), &REC);
        it.global.borrow_mut().vars.insert("rec".into(), f.clone());
        let Err(crate::eval::Ex::Throw(e)) = it.call(&f, Value::Undefined, &[]) else {
            panic!("no throw")
        };
        assert_eq!(
            it.get(&e, "name").ok().unwrap().to_js_string(),
            "RangeError"
        );
        assert_eq!(
            it.get(&e, "message").ok().unwrap().to_js_string(),
            "Maximum call stack size exceeded"
        );
        assert_eq!(crate::eval::js_depth(), 0);
    }

    #[test]
    fn compiled_closures_through_builtins() {
        let mut it = Interp::new(Box::new(H));
        it.global
            .borrow_mut()
            .vars
            .insert("base".into(), Value::Num(10.0));
        let scope: Scope = Rc::new(RefCell::new(Frame::default()));
        scope.borrow_mut().parent = Some(it.global.clone());
        let root = Env::from_scope(scope);
        let f = closure(outer, &root, &OUTER);
        let r = match it.call(&f, Value::Undefined, &[Value::Num(2.0)]) {
            Ok(v) => v,
            Err(crate::eval::Ex::Throw(e)) => panic!("{}", e.to_js_string()),
            Err(_) => panic!("short"),
        };
        assert_eq!(it.to_string(&r).ok().unwrap(), "12,14,16");
        let o = it.construct(&f, &[Value::Num(1.0)]).ok().unwrap();
        assert_eq!(it.to_string(&o).ok().unwrap(), "11,12,13");
        assert!(it.call(&f, Value::Undefined, &[]).is_ok());
    }
}
