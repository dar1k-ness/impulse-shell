//! Tree-walking evaluator for the owned JS IR (QML bindings, handlers and `.js` resources).
#[cfg(feature = "interp")]
use crate::ir::{AssignOp, Class, Expr, Func, Key, LogicalOp, Pat, PropKind, Stmt, UnOp, UpdateOp};
use crate::value::{
    ABSENT, Accessor, ArgsMap, Internal, JsObject, JsSymbol, Obj, PERMANENT, READONLY, SymProp,
    Value, WellKnown,
};
use rustc_hash::FxHashMap as HashMap;
use std::{
    cell::{Cell, RefCell},
    fmt,
    rc::Rc,
};

pub type Scope = Rc<RefCell<Frame>>;

#[derive(Debug, Default)]
pub struct Frame {
    pub vars: rustc_hash::FxHashMap<String, Value>,
    pub parent: Option<Scope>,
    pub this: Option<Value>,
    /// A function frame's [[NewTargetValue]] (ECMA-262 §9.1.1.3): what `new.target` reads
    /// and `super(...)` constructs for; `undefined` in a [[Call]]. A derived constructor's
    /// `this` holds `Value::Empty` (uninitialized) until `super(...)` binds it.
    pub new_target: Value,
    /// The frame of a derived class constructor. QV4 copies a function's `this` for its arrow
    /// functions once, on entry (`Context::emitBlockHeader`, qv4compilercontext.cpp:223),
    /// and a derived constructor has none then: `this` in an arrow function of it is never
    /// bound, whatever `super(...)` did.
    pub derived: bool,
    /// The `this` an arrow function was called with. QV4 keeps it in the arrow's own frame:
    /// `super.x` there reads it (`getSuperBase`, qv4runtime.cpp:1089), where the keyword
    /// `this` reads the enclosing function's copy.
    pub arrow_this: Option<Value>,
    /// The object of a `with` statement's object environment (ECMA-262 §9.1.1.2): the frame
    /// binds that object's properties.
    pub with: Option<Value>,
    /// The names in `vars` a direct eval declared (`var` and function declarations): the
    /// deletable bindings (ECMA-262 §19.2.1.3 steps 15.d and 16.b).
    pub eval_vars: Vec<String>,
    /// The call frame of a generator's step function ([`crate::ir::genlower`]), whose code
    /// is its generator function's: a direct eval there declares in that function's frame.
    pub gen_step: bool,
    /// A function call frame: holds the slots `Expr::Local` / `Pat::Local` resolve to.
    pub fun: bool,
    /// The global object: its builtins are frozen ([`crate::builtins::is_frozen_global`]).
    pub global: bool,
    pub slots: Vec<Value>,
    /// The mapped arguments object of a call frame: its formals' slots live there.
    pub args: Option<Obj>,
    /// QML scope of a binding frame (item, context).
    pub qml: Option<(usize, usize)>,
    /// The class whose body this frame is (`super()` resolves here).
    pub class: Option<Rc<ClassScope>>,
    /// The [[HomeObject]] of the methods closed over this frame (ECMA-262 §10.2.7): a class's
    /// prototype or constructor, or an object literal; `super.x` resolves through it.
    pub home: Option<Value>,
    /// The realm's [[TemplateMap]] (ECMA-262 §9.3), on its global frame.
    pub templates: Option<Box<TemplateMap>>,
    /// The generator intrinsics of a global frame, shared by the interpreters over it.
    pub generator_protos: Option<crate::generator::Protos>,
    /// The global object of a global frame (the realm's [[GlobalThis]]), shared likewise.
    pub global_object: Option<Obj>,
    /// The JavaScript file this frame is the top-level scope of.
    pub script: Option<Rc<ScriptFile>>,
    /// The value type prototypes of a global frame, shared by the interpreters over it.
    pub value_type_protos: Option<crate::valuetypes::Protos>,
    /// The prototype of the QML type objects of a global frame, shared likewise.
    pub type_proto: Option<Obj>,
    /// The symbol and iterator intrinsics of a global frame, shared by the interpreters over it.
    pub symbol_realm: Option<Box<crate::symbol::Realm>>,
    pub _n: crate::value::Tally<0>,
}

/// The JavaScript file whose top-level scope a frame is: the base of the relative URLs its code
/// passes to `Qt.include`. `ctx` is set on the frame holding the declarations of a JS file
/// context (Qt's JS-file QQmlContext, a `.js` import), and absent on the frame of a file
/// `Qt.include` ran in another file's context.
#[derive(Debug)]
pub struct ScriptFile {
    pub file: std::path::PathBuf,
    pub ctx: Option<ScriptCtx>,
}

/// A JS file context: the namespace object QML sees (`import "x.js" as X`) and the compiled
/// code it was loaded with, which the files it includes run with too.
pub struct ScriptCtx {
    /// The namespace object. Qt's is the file's context itself, whose [[Get]] and [[Set]] are
    /// the file's bindings; this is a proxy with these traps (see [`ScriptCtx::new`]).
    pub exports: Obj,
    /// The target of `exports`: the file's `var`s and functions as own properties.
    vars: Obj,
    /// The handler of `exports`, which holds the [`Bindings`].
    handler: Obj,
    pub aot: Option<Rc<dyn crate::jsrt::Aot>>,
}

/// Where a top-level name of a JS file lives.
#[derive(Clone)]
pub enum Binding {
    /// By name in a scope: the interpreter's, and compiled code that shares its names with
    /// the files it includes.
    Var(Scope),
    /// A slot of the frame of the file's compiled body.
    Slot(Rc<crate::jsrt::Slots>, u32),
}

impl Binding {
    fn read(&self, name: &str) -> Value {
        let v = match self {
            Binding::Var(sc) => sc.borrow().vars.get(name).cloned(),
            Binding::Slot(frame, k) => frame.slots.borrow().get(*k as usize).cloned(),
        };
        // A `let` the file did not reach has no value yet.
        match v {
            None | Some(Value::Empty) => Value::Undefined,
            Some(v) => v,
        }
    }

    fn write(&self, name: &str, v: Value) {
        match self {
            Binding::Var(sc) => {
                sc.borrow_mut().vars.insert(name.to_string(), v);
            }
            Binding::Slot(frame, k) => {
                if let Some(slot) = frame.slots.borrow_mut().get_mut(*k as usize) {
                    *slot = v;
                }
            }
        }
    }
}

/// The bindings of a JS file's top-level names, by name, and whether each is a `var` or a
/// function rather than a `let`, `const` or class.
#[derive(Default)]
struct Bindings(RefCell<HashMap<Rc<str>, (Binding, bool)>>);

/// Whether `o` is the namespace object of a JS file ([`ScriptCtx::exports`]): a proxy of the
/// engine, which JS sees as a plain object.
pub(crate) fn is_script_namespace(o: &JsObject) -> bool {
    match &o.internal {
        Internal::Proxy(Some(th)) => {
            (th.1.borrow().native.as_ref()).is_some_and(|n| n.is::<Bindings>())
        }
        _ => false,
    }
}

impl ScriptCtx {
    /// As Qt 6.12 has it, the namespace object reads every top-level name of the file, but
    /// only its `var`s and functions are properties of it (enumerable, not configurable)
    /// and can be written; writing any other name is an error.
    pub fn new(aot: Option<Rc<dyn crate::jsrt::Aot>>) -> Self {
        let (vars, handler) = (new_obj(), new_obj());
        handler.borrow_mut().native = Some(Rc::new(Bindings::default()));
        let traps: [(&'static str, NativeFn); 4] = [
            ("get", namespace_get),
            ("set", namespace_set),
            ("getOwnPropertyDescriptor", namespace_own_desc),
            ("defineProperty", namespace_define),
        ];
        for (name, f) in traps {
            set_hidden(&handler, name, fn_value(Callable::Native(name, f)));
        }
        ScriptCtx {
            exports: crate::proxy::new(&vars, &handler),
            vars,
            handler,
            aot,
        }
    }

    /// Shows top-level name `name`, bound in `binding`, on the namespace object, unless it
    /// is there already; `var_like` is a `var` or a function.
    pub fn export(&self, name: &str, binding: Binding, var_like: bool) {
        let handler = self.handler.borrow();
        let Some(bindings) = handler
            .native
            .as_ref()
            .and_then(|n| n.downcast_ref::<Bindings>())
        else {
            return;
        };
        let key: Rc<str> = name.into();
        if bindings.0.borrow().contains_key(&key) {
            return;
        }
        bindings.0.borrow_mut().insert(key, (binding, var_like));
        if var_like {
            // The value is the binding's, read by the traps.
            set_own(&self.vars, name, Value::Undefined);
            let mut vars = self.vars.borrow_mut();
            vars.set_attr(name, PERMANENT);
            // Qt lists the names in order.
            vars.keys.sort_unstable();
        }
    }

    /// Shows the names declared in `decls`, the scope of the file's `var`s and functions.
    pub fn export_vars(&self, decls: &Scope) {
        let names: Vec<String> = decls.borrow().vars.keys().cloned().collect();
        for name in names {
            self.export(&name, Binding::Var(decls.clone()), true);
        }
    }
}

/// The binding of the name a trap of a namespace object is called for: `this` is the
/// handler, `key` the property key.
fn namespace_binding(this: &Value, key: Option<&Value>) -> Option<(Rc<str>, Binding, bool)> {
    let (Value::Obj(handler), Some(Value::Str(name))) = (this, key) else {
        return None;
    };
    let handler = handler.borrow();
    let bindings = handler.native.as_ref()?.downcast_ref::<Bindings>()?;
    let (binding, var_like) = bindings.0.borrow().get(name)?.clone();
    Some((name.clone(), binding, var_like))
}

/// The target and the key a trap is called with.
fn namespace_target(it: &mut Interp, a: &[Value]) -> R<(Obj, PropKey)> {
    let Some(Value::Obj(target)) = a.first() else {
        return crate::proxy::type_error(it);
    };
    let key = it.to_property_key(a.get(1).unwrap_or(&Value::Undefined))?;
    Ok((target.clone(), key))
}

fn namespace_get(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    if let Some((name, binding, _)) = namespace_binding(this, a.get(1)) {
        return Ok(binding.read(&name));
    }
    let (target, key) = namespace_target(it, a)?;
    let receiver = a.get(2).cloned().unwrap_or_default();
    match &key {
        PropKey::Str(k) => it.get_from(&target, k, &receiver),
        PropKey::Sym(s) => it.get_sym_from(Some(target), s, &receiver),
    }
}

fn namespace_set(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let v = a.get(2).cloned().unwrap_or_default();
    match namespace_binding(this, a.get(1)) {
        Some((name, binding, true)) => {
            binding.write(&name, v);
            return Ok(Value::Bool(true));
        }
        Some((name, _, false)) => {
            return it.throw(
                "Error",
                format!("Invalid write to global property \"{name}\""),
            );
        }
        None => {}
    }
    let (target, key) = namespace_target(it, a)?;
    let target = Value::Obj(target);
    if crate::props::own_desc_pk(it, &target, &key)?.is_none() {
        let name = a.get(1).map(Value::to_js_string).unwrap_or_default();
        return it.throw(
            "Error",
            format!("Invalid write to global property \"{name}\""),
        );
    }
    let receiver = a.get(3).cloned().unwrap_or_default();
    crate::props::set_with_receiver_pk(it, &target, &key, v, &receiver).map(Value::Bool)
}

fn namespace_own_desc(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let (target, key) = namespace_target(it, a)?;
    if let Some((name, binding, true)) = namespace_binding(this, a.get(1)) {
        set_own(&target, &name, binding.read(&name));
    }
    let desc = crate::props::own_desc_pk(it, &Value::Obj(target), &key)?;
    Ok(desc.map_or(Value::Undefined, |d| crate::props::from_desc(&d)))
}

fn namespace_define(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let (target, key) = namespace_target(it, a)?;
    let desc = crate::props::to_desc(it, a.get(2).unwrap_or(&Value::Undefined))?;
    if let (Some((name, binding, true)), Some(v)) = (namespace_binding(this, a.get(1)), &desc.value)
    {
        binding.write(&name, v.clone());
    }
    crate::props::define_pk(it, &target, &key, &desc).map(Value::Bool)
}

/// The namespace object of a QML module a JS file imports (`.import QtQuick as QQ`): the
/// module's `names`, none enumerable, each resolved when it is read.
fn module_namespace(names: &[String]) -> Value {
    fn member(it: &mut Interp, _: &Value, a: &[Value]) -> R<Value> {
        let name = a.first().map(Value::to_js_string).unwrap_or_default();
        if let Some(v) = it.host.singleton(&name) {
            return Ok(v);
        }
        if let Some(load) = it.host.take_lazy() {
            load(it);
            if let Some(v) = it.host.singleton(&name) {
                return Ok(v);
            }
        }
        Ok(it.host.type_value(&name).unwrap_or_default())
    }
    let o = new_obj();
    for name in names {
        let get = fn_value(Callable::Bound(Rc::new(BoundFn {
            target: fn_value(Callable::Native("", member)),
            this: Value::Undefined,
            args: vec![Value::str(name)],
            length: 0.0,
        })));
        define_accessor(&o, name, true, get, false);
    }
    Value::Obj(o)
}

impl fmt::Debug for ScriptCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptCtx").finish_non_exhaustive()
    }
}

/// Template objects by tagged template site (see [`Interp::template_object`]). A heap site
/// (parsed code) is held weakly: its allocation, and so its address, stays a unique key for as
/// long as the entry exists, and the entry goes once the code holding the site is dropped
/// (each `new Function` parses its sites anew).
#[derive(Default)]
pub struct TemplateMap {
    objects: HashMap<usize, (Value, Option<std::rc::Weak<dyn std::any::Any>>)>,
    /// Number of heap sites at which the entries of dropped ones are next removed.
    sweep_at: usize,
    heap_sites: usize,
}

impl TemplateMap {
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    fn insert(&mut self, key: usize, object: Value, site: Option<&Rc<dyn std::any::Any>>) {
        if site.is_some() {
            if self.heap_sites >= self.sweep_at {
                self.objects
                    .retain(|_, (_, s)| s.as_ref().is_none_or(|s| s.strong_count() > 0));
                self.heap_sites = self.objects.values().filter(|(_, s)| s.is_some()).count();
                self.sweep_at = (self.heap_sites * 2).max(16);
            }
            self.heap_sites += 1;
        }
        self.objects.insert(key, (object, site.map(Rc::downgrade)));
    }
}

impl fmt::Debug for TemplateMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TemplateMap").field(&self.len()).finish()
    }
}

/// A class body's [[HomeObject]] data: the class value and its `extends` heritage.
#[derive(Debug)]
pub struct ClassScope {
    pub class: Value,
    pub parent: Option<Value>,
    /// Property keys of the class's fields, evaluated once by ClassDefinitionEvaluation.
    pub field_keys: std::cell::OnceCell<Vec<PropKey>>,
}

impl Drop for Frame {
    fn drop(&mut self) {
        let mut o = crate::value::Orphans::default();
        o.values(self.vars.values_mut());
        o.values(self.slots.iter_mut());
        o.values(self.this.iter_mut());
        o.values(std::iter::once(&mut self.new_target));
        o.values(self.home.iter_mut());
        o.obj(self.args.take());
        o.scope(self.parent.take());
        if let Some(p) = self.generator_protos.take() {
            o.obj(Some(p.function));
            o.obj(Some(p.generator));
            o.obj(Some(p.ctx));
        }
        if let Some(p) = self.value_type_protos.take() {
            p.all().for_each(|x| o.obj(Some(x.clone())));
        }
        o.obj(self.type_proto.take());
        if let Some(r) = self.symbol_realm.take() {
            r.objs().for_each(|x| o.obj(Some(x)));
        }
        if let Some(ClassScope {
            mut class,
            mut parent,
            ..
        }) = self.class.take().and_then(|c| Rc::try_unwrap(c).ok())
        {
            o.value(&mut class);
            o.values(parent.iter_mut());
        }
    }
}

pub type NativeFn = fn(&mut Interp, &Value, &[Value]) -> R<Value>;

/// A binary operator, resolved when the source is lowered (ECMA-262 §13.6–§13.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Exp,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Gt,
    Le,
    Ge,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    In,
    InstanceOf,
}

impl BinOp {
    pub fn as_str(self) -> &'static str {
        use BinOp::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Rem => "%",
            Exp => "**",
            Eq => "==",
            Ne => "!=",
            StrictEq => "===",
            StrictNe => "!==",
            Lt => "<",
            Gt => ">",
            Le => "<=",
            Ge => ">=",
            BitAnd => "&",
            BitOr => "|",
            BitXor => "^",
            Shl => "<<",
            Shr => ">>",
            UShr => ">>>",
            In => "in",
            InstanceOf => "instanceof",
        }
    }
}

/// The preferred type of [`Interp::to_primitive`] (ECMA-262 §7.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    Default,
    Number,
    String,
}

#[derive(Clone)]
pub enum Callable {
    #[cfg(feature = "interp")]
    Closure(Rc<Func>, Scope),
    #[cfg(feature = "interp")]
    Class(Rc<Class>, Scope),
    Native(&'static str, NativeFn),
    /// C++ method of a QML item taken as a value (`Qt.callLater(positionViewAtEnd)`).
    Item(usize, &'static str),
    /// A method Qt declares (class, method) that the engine has not written, on an object
    /// that is no QML item: calling it throws [`crate::unwritten::message`].
    Unwritten(&'static str, &'static str),
    /// Ahead-of-time compiled function (see `jsrt`).
    Compiled(
        crate::jsrt::CompiledFn,
        Rc<crate::jsrt::Slots>,
        &'static crate::jsrt::FnMeta,
    ),
    /// `Function.prototype.bind` result (ECMA-262 10.4.1).
    Bound(Rc<BoundFn>),
    /// A function of the Proxy built-in (ECMA-262 §10.5, §28.2), whose state is in the
    /// function object's `internal` slots.
    Proxy(ProxyFn),
}

/// The kinds of [`Callable::Proxy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyFn {
    /// A proxy whose target is callable: [[Call]] (ECMA-262 §10.5.12) and, when `construct`
    /// (the target is a constructor), [[Construct]] (§10.5.13).
    Object { construct: bool },
    /// %Proxy% (§28.2.1.1): a constructor that throws when called.
    Constructor,
    /// A `Proxy.revocable` revocation function (§28.2.2.1.1).
    Revoke,
}

pub struct BoundFn {
    pub target: Value,
    pub this: Value,
    pub args: Vec<Value>,
    /// SetFunctionLength at creation: the target's `length` less the bound arguments.
    pub length: f64,
}

impl Callable {
    /// The function's `length` (ECMA-262 SetFunctionLength).
    pub fn length(&self) -> Option<f64> {
        match self {
            #[cfg(feature = "interp")]
            Callable::Closure(f, _) => Some(f.length() as f64),
            #[cfg(feature = "interp")]
            Callable::Class(c, _) => Some(c.ctor.as_ref().map_or(0, |f| f.length()) as f64),
            Callable::Compiled(_, _, m) => Some(m.nparams as f64),
            Callable::Bound(b) => Some(b.length),
            // A built-in with a `length` of its own has it as a property.
            Callable::Native(..) | Callable::Item(..) | Callable::Unwritten(..) => Some(0.0),
            // A proxy's own properties are its target's (ECMA-262 §10.5.5).
            Callable::Proxy(ProxyFn::Object { .. }) => None,
            Callable::Proxy(_) => Some(0.0),
        }
    }

    /// The function's `name` (ECMA-262 SetFunctionName): its binding or inferred name, `""`
    /// when it has none; a bound function's is `bound ` and its target's.
    pub fn name(&self) -> Rc<str> {
        match self {
            #[cfg(feature = "interp")]
            Callable::Closure(f, _) => f.display_name().into(),
            #[cfg(feature = "interp")]
            Callable::Class(c, _) => c.display_name().into(),
            Callable::Native(n, _) | Callable::Item(_, n) | Callable::Unwritten(_, n) => {
                (*n).into()
            }
            Callable::Compiled(_, _, m) => m.name.into(),
            // V4 names every bound function so.
            Callable::Bound(_) => "__bound function__".into(),
            Callable::Proxy(ProxyFn::Constructor) => "Proxy".into(),
            Callable::Proxy(_) => "".into(),
        }
    }

    /// Whether `new` may call it and it owns a `prototype` (ECMA-262 MakeConstructor): a plain
    /// function or a class, not a method, accessor, arrow, bound or host function (built-in
    /// constructors set theirs up front).
    /// The kind of a JS function (`None` for host, native and bound functions).
    pub fn kind(&self) -> Option<crate::jsrt::FnKind> {
        match self {
            #[cfg(feature = "interp")]
            Callable::Closure(f, _) => Some(f.kind),
            #[cfg(feature = "interp")]
            Callable::Class(..) => Some(crate::jsrt::FnKind::ClassConstructor),
            Callable::Compiled(_, _, m) => Some(m.kind),
            Callable::Native(..)
            | Callable::Item(..)
            | Callable::Unwritten(..)
            | Callable::Bound(_)
            | Callable::Proxy(_) => None,
        }
    }

    /// Whether it has an own `prototype` made with it (a constructor's, or a generator
    /// function's, ECMA-262 §15.5.4), which [`fn_prototype`] makes on first use.
    pub fn owns_prototype(&self) -> bool {
        self.is_constructor() || self.kind() == Some(crate::jsrt::FnKind::Generator)
    }

    /// Whether a script or compiled function is a constructor; IsConstructor of any function
    /// value, built-ins included, is [`crate::proxy::is_constructor`].
    pub fn is_constructor(&self) -> bool {
        match self {
            #[cfg(feature = "interp")]
            Callable::Closure(f, _) => f.kind.is_constructor(),
            #[cfg(feature = "interp")]
            Callable::Class(..) => true,
            Callable::Compiled(_, _, m) => m.kind.is_constructor(),
            Callable::Native(..)
            | Callable::Item(..)
            | Callable::Unwritten(..)
            | Callable::Bound(_)
            | Callable::Proxy(_) => false,
        }
    }
}

impl fmt::Debug for Callable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            #[cfg(feature = "interp")]
            Callable::Closure(func, _) => {
                write!(f, "function {}", func.name.as_deref().unwrap_or(""))
            }
            #[cfg(feature = "interp")]
            Callable::Class(c, _) => write!(f, "class {}", c.name.as_deref().unwrap_or("")),
            Callable::Native(n, _) => write!(f, "native {n}"),
            Callable::Item(i, n) => write!(f, "method {n} of item {i}"),
            Callable::Unwritten(c, n) => write!(f, "unwritten {c}.{n}"),
            Callable::Compiled(_, _, m) => write!(f, "function {}", m.name),
            Callable::Bound(b) => write!(f, "bound {:?}", callable(&b.target)),
            Callable::Proxy(p) => write!(f, "proxy {p:?}"),
        }
    }
}

/// Deferred load of a lazy QML singleton (`Host::take_lazy`).
pub type LazyLoad = Box<dyn FnOnce(&mut Interp)>;

/// What a write to a member of a QML object meets (QV4 QObjectWrapper::virtualPut).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteTarget {
    /// A property: it takes the write, or refuses it as a read-only one.
    Property,
    /// A method, a signal or a signal's handler: never writable. The name is the member's as
    /// QV4's error gives it (the signal's for a handler).
    Function(String),
    /// No member, on an object a QML document created: such an object takes no new members.
    Missing,
    /// No member, on another object: the value is kept as a JavaScript property of it
    /// ([`Host::extras`]).
    Extra,
    /// No member, on a value type: the write is dropped.
    Dropped,
    /// No member, on a type's attached object: the write is dropped, and strict mode code
    /// gets a TypeError.
    Refused,
}

/// Bridge to QML items / context; `Value::Item(id)` is resolved through it.
pub trait Host {
    /// Queues `Qt.callLater(f, args...)` on the engine's delayed-call queue
    /// (QQmlDelayedCallQueue); false without an engine.
    fn call_later(&mut self, _f: Value, _args: Vec<Value>) -> bool {
        false
    }
    /// The text of QML object `item` in JS, `QQuickItem_QML_0(0x55d0c8a3f2b0)`.
    fn object_text(&mut self, _item: usize) -> Option<String> {
        None
    }
    fn lookup(&mut self, _scope: Option<(usize, usize)>, _name: &str) -> Option<Value> {
        None
    }
    /// `lookup` with an interned name (compiled code that proved the name is not a JS var).
    fn lookup_sym(
        &mut self,
        scope: Option<(usize, usize)>,
        name: crate::runtime::Sym,
    ) -> Option<Value> {
        self.lookup(scope, &name)
    }
    /// [`Host::lookup_sym`], also telling whether `name` resolved to a property of the scope
    /// object `scope.0` (QV4 calls such a function with the scope object as `this`).
    fn lookup_scoped(
        &mut self,
        scope: Option<(usize, usize)>,
        name: crate::runtime::Sym,
    ) -> Option<(Value, bool)> {
        let v = self.lookup_sym(scope, name)?;
        let own = scope
            .is_some_and(|(item, _)| self.get_sym(item, name).is_some_and(|p| p.strict_eq(&v)));
        Some((v, own))
    }
    /// Unqualified assignment `name = v` resolved against the QML scope (item, context):
    /// `Ok(false)` if no object in scope has the property, `Err` if it refuses the write.
    fn assign(
        &mut self,
        _item: usize,
        _ctx: usize,
        _name: &str,
        _v: Value,
    ) -> Result<bool, HostError> {
        Ok(false)
    }
    /// Re-sets the scope property `name` after its value was mutated in place (`list.push(x)`
    /// with `list` a QML property in scope), so it notifies: no write, so never refused.
    fn write_back(&mut self, item: usize, ctx: usize, name: &str, v: Value) {
        let _ = self.assign(item, ctx, name, v);
    }
    /// Whether item member `name` refuses JavaScript writes: a read-only property, a method
    /// declared in QML or a signal.
    fn read_only(&mut self, _item: usize, _name: &str) -> bool {
        false
    }
    /// Why a write to member `name` of `item` throws, for the cases [`Host::read_only`] does
    /// not cover: a signal handler (`onX`, read-only like its signal) and a name that is no
    /// member of a QML object whose members are all known (QV4::QObjectWrapper::virtualPut).
    fn write_refusal(&mut self, _item: usize, _name: &str) -> Option<HostError> {
        None
    }
    /// The C++ type of item property `name` if it takes no `undefined`: neither a `var` nor
    /// resettable (QV4::QObjectWrapper::setProperty).
    fn no_undefined(&mut self, _item: usize, _name: &str) -> Option<String> {
        None
    }
    /// A script's write takes the binding of item property `name` off before the value is
    /// looked at (QV4::QObjectWrapper::setProperty: removeBinding, then the conversion), so a
    /// write the property refuses leaves it with no binding too. `set` and `assign` do it for
    /// a value they take; whoever refuses one calls this first.
    fn unbind(&mut self, _item: usize, _name: &str) {}
    /// What `undefined` written by a script to `name` of `item` is, where `item` is a value
    /// type property of a Qt type and `name` its field: the field's type made of it.
    fn undefined_field(&mut self, _item: usize, _name: &str) -> Option<Value> {
        None
    }
    fn get(&mut self, _item: usize, _name: &str) -> Option<Value> {
        None
    }
    /// Whether `name` can be a member of the QML object: false only for a string that is no
    /// name yet and that nothing of the object answers to. Never interns `name`.
    fn may_have(&mut self, _item: usize, _name: &str) -> bool {
        true
    }
    /// What a write to member `name` of QML object `item` meets. Never interns `name`.
    fn write_target(&mut self, _item: usize, _name: &str) -> WriteTarget {
        WriteTarget::Property
    }
    /// A write to role `k` of `row`, a row of a ListModel (`model.get(i).role = v`): the
    /// model's `setProperty` when delegates show the row. False when the host did not write.
    fn row_write(&mut self, _row: &Obj, _k: &str, _v: &Value) -> bool {
        false
    }
    /// The object holding the properties script gave QML object `item` beyond its members
    /// (`item.anchors.mine = 1`, `Object.defineProperty`), made on demand when `make`. It goes
    /// with `item`. V4's QObjectWrapper defines and deletes like any object, in the wrapper's
    /// own storage, and reads the QObject first: a property defined under the name of one the
    /// QObject has is never seen.
    fn extras(&mut self, _item: usize, _make: bool) -> Option<Obj> {
        None
    }
    /// A QML object's enumerable properties (V4 QObjectWrapper: `Object.keys`, `for in`,
    /// `Object.assign`, spread).
    fn keys(&mut self, _item: usize) -> Vec<String> {
        Vec::new()
    }
    /// The namespace object of the import qualifier `name` of the document of `ctx`, read as
    /// a member of `item` (`item.QQ.Keys`): QObjectWrapper looks a name the object does not
    /// have up in the imports of the calling context.
    fn qualifier(&mut self, _item: usize, _ctx: usize, _name: &str) -> Option<Value> {
        None
    }
    /// `get` with an interned name (member expressions).
    fn get_sym(&mut self, item: usize, name: crate::runtime::Sym) -> Option<Value> {
        self.get(item, &name)
    }
    /// Native (C++-side) method of a QML item, e.g. `FileView.reload()`; `None` if the item
    /// has no such native method and the call should go through normal property lookup.
    /// `args: None` only asks whether it has one, without effects: one dispatch answers both
    /// [`Host::call`] and [`Host::has_method`], so the two cannot disagree.
    /// `Some(Err(..))` when the method throws (QV4: calling a signal with too few arguments).
    fn native(&mut self, _item: usize, _name: &str, _args: Option<&[Value]>) -> Option<HostR> {
        None
    }
    fn call(&mut self, item: usize, name: &str, args: &[Value]) -> Option<HostR> {
        self.native(item, name, Some(args))
    }
    /// Whether `item`'s native method `name` resolves a url it is given in the document of
    /// the code that calls it (Qt's `callingQmlContext()`), not in the item's:
    /// [`Interp::host_call`] then tells that document with [`Host::called_from`] first.
    fn resolves_for_caller(&mut self, _item: usize, _name: &str) -> bool {
        false
    }
    /// The document of the code about to call a method [`Host::resolves_for_caller`] named:
    /// `Some(None)` in a context no document is behind, `None` in code no context runs.
    fn called_from(&mut self, _doc: Option<Option<crate::qurl::QUrl>>) {}
    /// The declared parameter types of `item`'s signal `name`, when it is declared in QML
    /// with a type other than `var`: its arguments are converted to them on emission.
    fn signal_types(&mut self, _item: usize, _name: &str) -> Option<Vec<crate::runtime::Sym>> {
        None
    }
    /// Whether object `item` is of the QML type `ty` or of one derived from it.
    fn item_is(&mut self, _item: usize, _ty: &str) -> bool {
        true
    }
    /// Whether `call(item, name, ..)` handles `name` (a signal or native method).
    fn has_method(&mut self, item: usize, name: &str) -> bool {
        self.native(item, name, None).is_some()
    }
    /// The value of QML type name `name` in an expression: a singleton's object, else the
    /// type's object.
    fn type_value(&mut self, _name: &str) -> Option<Value> {
        None
    }
    /// Whether QML object `item` is of QML type `ty` or of one derived from it.
    fn inherits(&mut self, _item: usize, _ty: crate::runtime::Sym) -> bool {
        false
    }
    /// Whether QML object `item` has member `name` as V4's QObjectWrapper resolves one: a
    /// property, a method, a signal or a signal's handler (`onClicked`).
    fn has_member(&mut self, item: usize, name: &str) -> bool {
        self.get(item, name).is_some() || self.has_method(item, name)
    }
    /// `item.sig.connect(this, f)` / `disconnect` (QV4 QObjectMethod::method_connect): `f` is
    /// called with `this` on each emission until disconnected. `false` if `sig` is no signal.
    fn connect(&mut self, _item: usize, _sig: &str, _this: Value, _f: Value, _on: bool) -> bool {
        false
    }
    /// Re-notify `name` after its sequence value was mutated in place (keeps bindings).
    fn notify(&mut self, _item: usize, _name: &str) {}
    /// [`Self::notify`] of `name` after `seq`, the sequence read from it, was mutated in place.
    fn sequence_changed(&mut self, item: usize, name: &str, _seq: &Value) {
        self.notify(item, name)
    }
    /// Stores `seq`, read from `name` and mutated in place, back to it (`item.list.push(x)`).
    /// That is no assignment: a property that holds `seq` keeps its binding.
    fn sequence_written(&mut self, item: usize, name: &str, seq: &Value) {
        self.set(item, name, seq.clone());
    }
    fn set(&mut self, _item: usize, _name: &str, _v: Value) -> bool {
        false
    }
    /// The declared type of property `name` of `item`.
    fn prop_type(&mut self, _item: usize, _name: &str) -> Option<crate::runtime::Sym> {
        None
    }
    /// Property `name` of `item` as a write sees it (`valuetypes::Slot`), `None` for one that
    /// takes any value as it is.
    fn write_slot(&mut self, _item: usize, _name: &str) -> Option<Rc<crate::valuetypes::Slot>> {
        None
    }
    /// [`Self::write_slot`] of the property a free-name write `name = v` in scope
    /// `(item, ctx)` targets.
    fn assign_slot(
        &mut self,
        _item: usize,
        _ctx: usize,
        _name: &str,
    ) -> Option<Rc<crate::valuetypes::Slot>> {
        None
    }
    /// Whether object `item` is of the QML type, or the C++ class, `class`.
    fn is_instance(&mut self, _item: usize, _class: &str) -> bool {
        false
    }
    fn as_any(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
    /// Loader for a singleton the last `lookup` miss named but that is not created yet (Qt
    /// creates a QML singleton on first use).
    fn take_lazy(&mut self) -> Option<LazyLoad> {
        None
    }
    /// Whether the last `lookup` miss was of `name` and it names a singleton that is not
    /// created yet; the miss is forgotten and the singleton stays uncreated.
    fn lazy_missed(&mut self, _name: &str) -> bool {
        false
    }
    /// QML singleton `name` if created; one not created yet is left for [`Host::take_lazy`].
    fn singleton(&mut self, _name: &str) -> Option<Value> {
        None
    }
}

/// An exception a [`Host`] method throws: an error built from the engine's `kind` constructor
/// (`Error`, `TypeError`, ...) with `message` (V4's `ExecutionEngine::throwError`).
#[derive(Clone, Debug, PartialEq)]
pub struct HostError {
    pub kind: &'static str,
    pub message: String,
}

impl HostError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> HostError {
        HostError {
            kind,
            message: message.into(),
        }
    }

    /// QV4's error for a write to a property that is not writable (QObjectWrapper::setProperty),
    /// thrown whatever the strictness.
    pub fn read_only(name: &str) -> HostError {
        HostError::new(
            "TypeError",
            format!("Cannot assign to read-only property \"{name}\""),
        )
    }

    /// QV4's error for `undefined` written to a property that neither holds nor resets on it.
    pub fn no_undefined(ty: &str) -> HostError {
        HostError::new("Error", format!("Cannot assign [undefined] to {ty}"))
    }
}

/// What a [`Host`] method returns or throws.
pub type HostR = Result<Value, HostError>;

pub struct NoHost;
impl Host for NoHost {}

#[derive(Debug)]
pub enum Ex {
    Throw(Value),
    Short,
}

pub type R<T> = Result<T, Ex>;

pub enum Flow {
    Normal,
    Return(Value),
    Break(Option<String>),
    Continue(Option<String>),
}

/// A member assignment target with its base (and key) evaluated (`Interp::target`).
#[cfg(feature = "interp")]
enum Target {
    Member(Value, crate::runtime::Sym),
    /// `this.n` in a derived constructor that has not called `super(...)`: QV4 reads that
    /// `this` when it stores, after the right-hand side (`Interp::unbound_this`).
    This(crate::runtime::Sym),
    /// A field of a color: the color, the field and where the color was read. A color is a
    /// value: the changed one is stored back there (QQmlValueTypeWrapper's write-back).
    ColorField(crate::color::Color, crate::runtime::Sym, ColorAt),
    Index(Value, Value),
    /// `super.n` / `super[k]` (MakeSuperPropertyReference, ECMA-262 §13.3.7.3): the super
    /// base, the `this` the write lands on, and the key value.
    Super(Value, Value, Value),
    /// A name bound by a `with` object: the object and the name.
    With(Value, String),
    /// A name bound in a frame's `vars`.
    Var(Scope, String),
}

/// Where [`Interp::dyn_ref`] finds a name.
#[cfg(feature = "interp")]
enum DynRef {
    /// A property of this `with` object.
    With(Value),
    /// A named binding of this frame.
    Frame(Scope),
    /// The binding the code shows.
    Static,
}

/// The call frames between a use and the slot the static binding `e` of an
/// [`Expr::Dynamic`] reads; `None` when it is looked up by name.
#[cfg(feature = "interp")]
fn static_hops(e: &Expr) -> Option<u32> {
    match e {
        Expr::Local(h, ..) => Some(*h),
        Expr::Member(o, ..) => static_hops(o),
        Expr::Seq(v) => v.last().and_then(static_hops),
        _ => None,
    }
}

/// [`static_hops`] of the static target of a [`Pat::Dynamic`].
#[cfg(feature = "interp")]
fn static_pat_hops(p: &Pat) -> Option<u32> {
    match p {
        Pat::Local(h, ..) => Some(*h),
        Pat::Const(p, _) => static_pat_hops(p),
        Pat::Member(e) => static_hops(e),
        _ => None,
    }
}

/// Where a color whose field is assigned was read.
#[cfg(feature = "interp")]
enum ColorAt {
    Member(Value, crate::runtime::Sym),
    Index(Value, Value),
    Name(Pat),
    /// A temporary: the change is dropped.
    Nowhere,
}

pub fn new_obj() -> Obj {
    crate::value::reg(Rc::new(RefCell::new(JsObject::default())))
}

/// Own symbol-keyed property `s` of `b`.
pub fn sym_own<'a>(b: &'a JsObject, s: &Rc<JsSymbol>) -> Option<&'a SymProp> {
    b.ext
        .as_deref()?
        .syms
        .iter()
        .find(|p| Rc::ptr_eq(&p.key, s))
}

fn sym_own_mut<'a>(b: &'a mut JsObject, s: &Rc<JsSymbol>) -> Option<&'a mut SymProp> {
    b.ext
        .as_deref_mut()?
        .syms
        .iter_mut()
        .find(|p| Rc::ptr_eq(&p.key, s))
}

/// CreateDataProperty with a symbol key: replaces an own property `s`, keeping its place.
pub fn define_sym(o: &Obj, s: &Rc<JsSymbol>, v: Value, enumerable: bool) {
    write_sym(o, s, v, None, enumerable, 0);
}

/// Stores own symbol-keyed property `s` of `o` with attributes `attrs` ([`READONLY`],
/// [`PERMANENT`]), replacing one in its place.
pub fn write_sym(
    o: &Obj,
    s: &Rc<JsSymbol>,
    value: Value,
    accessor: Option<Box<Accessor>>,
    enumerable: bool,
    attrs: u8,
) {
    let mut b = o.borrow_mut();
    if let Some(p) = sym_own_mut(&mut b, s) {
        p.value = value;
        p.accessor = accessor;
        p.enumerable = enumerable;
        p.attrs = attrs;
        return;
    }
    b.ext.get_or_insert_default().syms.push(SymProp {
        key: s.clone(),
        value,
        accessor,
        enumerable,
        attrs,
    });
}

/// Defines the getter or setter `f` of symbol-keyed accessor property `s` (keeping the other).
pub fn define_sym_accessor(o: &Obj, s: &Rc<JsSymbol>, getter: bool, f: Value, enumerable: bool) {
    let mut b = o.borrow_mut();
    if sym_own(&b, s).is_none() {
        b.ext.get_or_insert_default().syms.push(SymProp {
            key: s.clone(),
            value: Value::Undefined,
            accessor: None,
            enumerable,
            attrs: 0,
        });
    }
    let p = sym_own_mut(&mut b, s).unwrap();
    p.value = Value::Undefined;
    p.enumerable = enumerable;
    p.attrs = 0;
    let a = p.accessor.get_or_insert_default();
    if getter {
        a.get = Some(f);
    } else {
        a.set = Some(f);
    }
}

/// Removes own symbol-keyed property `s` of `o`.
pub fn delete_sym(o: &Obj, s: &Rc<JsSymbol>) {
    let removed = {
        let mut b = o.borrow_mut();
        b.ext.as_deref_mut().and_then(|e| {
            let i = e.syms.iter().position(|p| Rc::ptr_eq(&p.key, s))?;
            Some(e.syms.remove(i))
        })
    };
    drop(removed);
}

/// An iterator being consumed (the Iterator Record of ECMA-262 §7.4.1); a built-in
/// iterable with its built-in `@@iterator` gives its items directly.
#[derive(Debug)]
pub enum IterSrc {
    List(std::vec::IntoIter<Value>),
    /// An Array or arguments object with its built-in `@@iterator`: `length` and the element
    /// are read at each step (%ArrayIteratorPrototype%.next, ECMA-262 §23.1.5.2.1);
    /// `Undefined` once done.
    Array {
        src: Value,
        pos: usize,
    },
    /// A Map or Set with its built-in `@@iterator` (§24.1.5.2.1, §24.2.5.2.1), walked live
    /// from entry id `cursor`; `Undefined` once done.
    Coll {
        store: Value,
        map: bool,
        kind: crate::value::IterKind,
        cursor: u64,
    },
    /// A URLSearchParams object with its built-in `@@iterator`, its list read at each step
    /// (WebIDL §3.7.10.2); `Undefined` once done.
    Params {
        src: Value,
        pos: usize,
    },
    Js {
        iter: Value,
        next: Value,
        done: bool,
    },
    /// A `for-in` enumeration (in a lowered generator body).
    ForIn(Box<ForIn>),
}

/// A `for-in` enumeration in progress: the state of the iterator CreateForInIterator returns
/// (ECMA-262 §14.7.5.10), made by [`Interp::for_in`] and stepped by [`Interp::for_in_next`].
#[derive(Debug, Default)]
pub struct ForIn {
    /// The object whose own keys are being visited; `None` once done.
    object: Option<Value>,
    object_was_visited: bool,
    visited: rustc_hash::FxHashSet<String>,
    remaining: std::collections::VecDeque<String>,
    /// The QML object being enumerated.
    item: Option<usize>,
}

impl Default for IterSrc {
    /// An exhausted iterator.
    fn default() -> Self {
        IterSrc::List(Vec::new().into_iter())
    }
}

impl IterSrc {
    pub fn values_mut(&mut self) -> Box<dyn Iterator<Item = &mut Value> + '_> {
        match self {
            IterSrc::List(l) => Box::new(l.as_mut_slice().iter_mut()),
            IterSrc::Array { src: v, .. }
            | IterSrc::Coll { store: v, .. }
            | IterSrc::Params { src: v, .. } => Box::new(std::iter::once(v)),
            IterSrc::Js { iter, next, .. } => Box::new([iter, next].into_iter()),
            IterSrc::ForIn(f) => Box::new(f.object.iter_mut()),
        }
    }
    pub fn values(&self) -> Box<dyn Iterator<Item = &Value> + '_> {
        match self {
            IterSrc::List(l) => Box::new(l.as_slice().iter()),
            IterSrc::Array { src: v, .. }
            | IterSrc::Coll { store: v, .. }
            | IterSrc::Params { src: v, .. } => Box::new(std::iter::once(v)),
            IterSrc::Js { iter, next, .. } => Box::new([iter, next].into_iter()),
            IterSrc::ForIn(f) => Box::new(f.object.iter()),
        }
    }
}

/// A property key (ECMA-262 §6.1.7): a string or a symbol.
#[derive(Clone, Debug)]
pub enum PropKey {
    Str(String),
    Sym(Rc<JsSymbol>),
}

impl PartialEq for PropKey {
    fn eq(&self, other: &PropKey) -> bool {
        match (self, other) {
            (PropKey::Str(a), PropKey::Str(b)) => a == b,
            (PropKey::Sym(a), PropKey::Sym(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for PropKey {}

impl std::hash::Hash for PropKey {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        match self {
            PropKey::Str(k) => k.hash(h),
            PropKey::Sym(s) => Rc::as_ptr(s).hash(h),
        }
    }
}

impl PropKey {
    /// The key in an error message: the string, or `Symbol(description)`.
    pub fn describe(&self) -> String {
        match self {
            PropKey::Str(k) => k.clone(),
            PropKey::Sym(s) => s.descriptive(),
        }
    }

    /// The key as a value: a Symbol or a String.
    pub fn to_value(&self) -> Value {
        match self {
            PropKey::Str(k) => Value::str(k),
            PropKey::Sym(s) => Value::Symbol(s.clone()),
        }
    }

    /// The key a primitive converts to, without calling JS. An object key needs
    /// [`Interp::to_property_key`] (ToPrimitive first); here it only gets a description.
    pub fn of(v: &Value) -> PropKey {
        match v {
            Value::Symbol(s) => PropKey::Sym(s.clone()),
            v => PropKey::Str(v.to_js_string()),
        }
    }
}

/// Old-generation graph nodes one `collect_step` examines while a sweep runs, on top of the
/// young generation (always examined whole) and of the closure of the objects it takes.
pub const GC_SLICE: usize = 4096;

/// What one cycle-collector increment did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GcStep {
    pub freed: usize,
    /// Graph nodes examined: JS objects, scope frames and compiled-closure environments.
    pub examined: usize,
    /// This increment finished an old-generation sweep.
    pub swept: bool,
}

/// Start an old-generation sweep unless one is running: everything old becomes unexamined.
pub fn request_sweep() {
    crate::value::HEAP.with(|h| {
        if !h.borrow().sweeping {
            begin_sweep(&mut h.borrow_mut());
        }
    })
}

fn begin_sweep(h: &mut crate::value::Heap) {
    h.epoch = h.epoch.checked_add(1).unwrap_or(1);
    h.cursor = 0;
    h.kept = 0;
    h.sweep_end = h.young;
    h.sweeping = true;
    h.promoted = 0;
    h.sweep_live = 0;
}

/// An old-generation sweep is in progress (the host keeps calling `collect_step`).
pub fn sweep_running() -> bool {
    crate::value::HEAP.with(|h| h.borrow().sweeping)
}

/// Full collection in one go: every JS object is examined. Must run with no evaluation on
/// the stack. Returns the number of freed objects.
pub fn collect_cycles() -> usize {
    crate::value::HEAP.with(|h| begin_sweep(&mut h.borrow_mut()));
    increment(usize::MAX).freed
}

/// One bounded increment of the cycle collector; the host calls it once per loop turn,
/// animating or not. Must run with no evaluation on the stack.
///
/// Generational and incremental like CPython's collector: each increment examines the
/// objects allocated since the last one (most garbage cycles die young) and, while a sweep
/// runs, about `old_budget` more nodes of the old generation. A sweep starts when the old
/// generation has grown by a quarter since the last one (CPython's long-lived threshold) or
/// on `request_sweep` (the host's teardowns drop references held by old objects).
pub fn collect_step(old_budget: usize) -> GcStep {
    crate::value::HEAP.with(|h| {
        let mut h = h.borrow_mut();
        if !h.sweeping && h.promoted > h.old_live / 4 {
            begin_sweep(&mut h);
        }
    });
    increment(old_budget)
}

/// The next unexamined object to start an increment's subgraph from: the young generation
/// first, then the running sweep's old objects while `more_old` allows.
fn next_seed(epoch: u32, more_old: bool) -> Option<(Obj, bool)> {
    crate::value::HEAP.with(|h| {
        let h = &mut *h.borrow_mut();
        while h.young < h.objs.len() {
            h.young += 1;
            if let Some(o) = h.objs[h.young - 1]
                .upgrade()
                .filter(|o| !examined(o, epoch))
            {
                return Some((o, true));
            }
        }
        while more_old && h.sweeping && h.cursor < h.sweep_end {
            let w = std::mem::take(&mut h.objs[h.cursor]);
            h.cursor += 1;
            let Some(o) = w.upgrade() else { continue };
            let kept = h.kept;
            h.objs[kept] = w;
            h.kept += 1;
            if !examined(&o, epoch) {
                return Some((o, false));
            }
        }
        None
    })
}

fn examined(o: &Obj, epoch: u32) -> bool {
    o.try_borrow().is_ok_and(|b| b.gc.get() == epoch)
}

/// Free JS objects (and closure frames) kept alive only by reference cycles — Qt's JS
/// engine garbage-collects these (e.g. The original's MeasuredPolygon <-> MeasuredCubic back-links).
///
/// Trial deletion over a subgraph S: a node whose strong count is fully explained by
/// references from other nodes of S, and which is unreachable from any node of S held from
/// outside S, is garbage. Any S is sound: an edge from outside S (a live object, a QML
/// property, a Rust holder, or an edge the walk does not enumerate) only makes its target
/// look externally held. S is a set of seeds plus everything reachable from them that was
/// not examined yet in this epoch, so a garbage cycle with a member among the seeds lies in
/// S whole. Survivors are stamped with the epoch.
fn increment(old_budget: usize) -> GcStep {
    enum N {
        O(Obj),
        F(Scope),
        E(Rc<crate::jsrt::Slots>),
    }
    let epoch = crate::value::HEAP.with(|h| h.borrow().epoch);
    let mut idx: HashMap<usize, usize> = HashMap::default();
    let mut nodes: Vec<N> = Vec::new();
    // Per node: a young-generation seed (counts as promoted when it survives).
    let mut young_seed: Vec<bool> = Vec::new();
    let mut edges: Vec<Vec<usize>> = Vec::new();
    let mut opaque: Vec<bool> = Vec::new();
    // Node count when the first old seed was taken: the old part's budget starts there.
    let mut old_from: Option<usize> = None;
    let mut i = 0;
    loop {
        if i == nodes.len() {
            // The subgraph reachable from the seeds so far is complete: take another seed.
            let more_old = old_from.is_none_or(|b| nodes.len() - b < old_budget);
            let Some((o, young)) = next_seed(epoch, more_old) else {
                break;
            };
            if !young && old_from.is_none() {
                old_from = Some(nodes.len());
            }
            let k = Rc::as_ptr(&o) as usize;
            if idx.contains_key(&k) {
                continue;
            }
            idx.insert(k, nodes.len());
            nodes.push(N::O(o));
            young_seed.push(young);
        }
        let mut out = Vec::new();
        let mut objs: Vec<Obj> = Vec::new();
        let mut scopes: Vec<Scope> = Vec::new();
        let mut envs: Vec<Rc<crate::jsrt::Slots>> = Vec::new();
        let val = |v: &Value, objs: &mut Vec<Obj>| {
            if let Value::Obj(o) = v {
                objs.push(o.clone());
            }
        };
        let ok = match &nodes[i] {
            N::O(o) => match o.try_borrow() {
                Ok(b) => {
                    b.props.values().for_each(|v| val(v, &mut objs));
                    b.array.iter().flatten().for_each(|v| val(v, &mut objs));
                    objs.extend(b.proto.clone());
                    for a in b.accessors.iter().flat_map(|m| m.values()) {
                        a.get.iter().chain(&a.set).for_each(|v| val(v, &mut objs));
                    }
                    objs.extend(b.internal.objs().cloned());
                    match &b.internal {
                        Internal::Arguments(Some(m)) => {
                            m.cells.iter().flatten().for_each(|v| val(v, &mut objs))
                        }
                        Internal::WeakMap(c) | Internal::WeakSet(c) => {
                            for (k, v) in &c.others {
                                val(k, &mut objs);
                                val(v, &mut objs);
                            }
                        }
                        Internal::Generator(c) => objs.push(c.clone()),
                        Internal::GenCtx(c) => c.values().for_each(|v| val(v, &mut objs)),
                        Internal::IterRecord(i) => {
                            i.0.values().chain([&i.1]).for_each(|v| val(v, &mut objs))
                        }
                        Internal::TypedArray(v) | Internal::DataView(v) => {
                            objs.push(v.buffer.clone())
                        }
                        Internal::Promise(p) => p.values().for_each(|v| val(v, &mut objs)),
                        Internal::CollIter(c) => objs.extend(c.store.clone()),
                        Internal::ParamsIter(p) => objs.push(p.params.clone()),
                        Internal::RegExpStringIter(r) => val(&r.matcher, &mut objs),
                        Internal::Iter(l) => {
                            val(&l.src, &mut objs);
                            l.items.iter().for_each(|v| val(v, &mut objs));
                        }
                        _ => {}
                    }
                    // A weak entry is an edge from its key only (`Ext::weak`).
                    if let Some(e) = &b.ext {
                        for p in &e.syms {
                            val(&p.value, &mut objs);
                            if let Some(a) = &p.accessor {
                                a.get.iter().chain(&a.set).for_each(|v| val(v, &mut objs));
                            }
                        }
                        e.weak.iter().for_each(|(_, v)| val(v, &mut objs));
                        objs.extend(e.members.clone());
                    }
                    match &b.call {
                        #[cfg(feature = "interp")]
                        Some(Callable::Closure(_, sc) | Callable::Class(_, sc)) => {
                            scopes.push(sc.clone())
                        }
                        Some(Callable::Compiled(_, env, _)) => envs.push(env.clone()),
                        Some(Callable::Bound(bf)) => [&bf.target, &bf.this]
                            .into_iter()
                            .chain(&bf.args)
                            .for_each(|v| val(v, &mut objs)),
                        _ => {}
                    }
                    true
                }
                Err(_) => false,
            },
            N::F(f) => match f.try_borrow() {
                Ok(b) => {
                    b.vars.values().for_each(|v| val(v, &mut objs));
                    b.slots.iter().for_each(|v| val(v, &mut objs));
                    b.this.iter().for_each(|v| val(v, &mut objs));
                    b.home.iter().for_each(|v| val(v, &mut objs));
                    objs.extend(b.args.clone());
                    if let Some(c) = &b.class {
                        val(&c.class, &mut objs);
                        c.parent.iter().for_each(|v| val(v, &mut objs));
                    }
                    scopes.extend(b.parent.clone());
                    if let Some(p) = &b.generator_protos {
                        objs.extend([p.function.clone(), p.generator.clone(), p.ctx.clone()]);
                    }
                    if let Some(p) = &b.value_type_protos {
                        objs.extend(p.all().cloned());
                    }
                    objs.extend(b.type_proto.clone());
                    if let Some(r) = &b.symbol_realm {
                        objs.extend(r.objs());
                    }
                    true
                }
                Err(_) => false,
            },
            N::E(e) => match e.slots.try_borrow() {
                Ok(slots) => {
                    slots.iter().for_each(|v| val(v, &mut objs));
                    envs.extend(e.parent.clone());
                    scopes.extend(e.root.vars.clone());
                    true
                }
                Err(_) => false,
            },
        };
        for o in objs {
            let k = Rc::as_ptr(&o) as usize;
            if let Some(&j) = idx.get(&k) {
                out.push(j);
            } else if !examined(&o, epoch) {
                idx.insert(k, nodes.len());
                out.push(nodes.len());
                nodes.push(N::O(o));
                young_seed.push(false);
            }
        }
        for sc in scopes {
            let k = Rc::as_ptr(&sc) as usize;
            let j = *idx.entry(k).or_insert_with(|| {
                nodes.push(N::F(sc.clone()));
                young_seed.push(false);
                nodes.len() - 1
            });
            out.push(j);
        }
        for e in envs {
            let k = Rc::as_ptr(&e) as usize;
            let j = *idx.entry(k).or_insert_with(|| {
                nodes.push(N::E(e.clone()));
                young_seed.push(false);
                nodes.len() - 1
            });
            out.push(j);
        }
        edges.push(out);
        opaque.push(!ok);
        i += 1;
    }
    let mut internal = vec![0usize; nodes.len()];
    for out in &edges {
        for &j in out {
            internal[j] += 1;
        }
    }
    // `nodes` holds one strong reference to each node.
    let mut live: Vec<bool> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let sc = match n {
                N::O(o) => Rc::strong_count(o),
                N::F(f) => Rc::strong_count(f),
                N::E(e) => Rc::strong_count(e),
            };
            opaque[i] || sc > 1 + internal[i]
        })
        .collect();
    let mut stack: Vec<usize> = (0..nodes.len()).filter(|&i| live[i]).collect();
    while let Some(i) = stack.pop() {
        for &j in &edges[i] {
            if !live[j] {
                live[j] = true;
                stack.push(j);
            }
        }
    }
    let mut trash: Vec<Box<dyn std::any::Any>> = Vec::new();
    let (mut freed, mut promoted, mut old_live) = (0, 0, 0);
    for (i, n) in nodes.iter().enumerate() {
        if live[i] {
            if let N::O(o) = n
                && let Ok(b) = o.try_borrow()
            {
                b.gc.set(epoch);
                if young_seed[i] {
                    promoted += 1;
                } else {
                    old_live += 1;
                }
            }
            continue;
        }
        match n {
            N::O(o) => {
                let mut b = o.borrow_mut();
                freed += 1;
                trash.push(Box::new((
                    std::mem::take(&mut b.props),
                    b.array.take(),
                    b.proto.take(),
                    b.call.take(),
                    std::mem::take(&mut b.keys),
                    b.accessors.take(),
                    std::mem::take(&mut b.internal),
                    b.ext.take(),
                )));
            }
            N::F(f) => {
                let mut b = f.borrow_mut();
                trash.push(Box::new((
                    std::mem::take(&mut b.vars),
                    std::mem::take(&mut b.slots),
                    b.parent.take(),
                    b.this.take(),
                    b.class.take(),
                    b.args.take(),
                )));
            }
            // `parent` and `scope` are not cleared: a dead parent env or scope frame is a
            // node of its own and is cleared there.
            N::E(e) => trash.push(Box::new(std::mem::take(&mut *e.slots.borrow_mut()))),
        }
    }
    let examined = nodes.len();
    drop(nodes);
    // Dropping the garbage runs no JS, but it frees objects whose `Weak` entries `next_seed`
    // skips from now on.
    drop(trash);
    let swept = crate::value::HEAP.with(|h| {
        let h = &mut *h.borrow_mut();
        if !h.sweeping {
            h.promoted += promoted;
            return false;
        }
        // Young survivors examined during a sweep are stamped with its epoch: covered by it.
        h.sweep_live += old_live + promoted;
        if h.cursor < h.sweep_end {
            return false;
        }
        h.sweeping = false;
        h.old_live = h.sweep_live;
        let (kept, end) = (h.kept, h.sweep_end);
        h.objs.drain(kept..end);
        h.young -= end - kept;
        h.cursor = kept;
        h.sweep_end = kept;
        true
    });
    GcStep {
        freed,
        examined,
        swept,
    }
}

/// Own property `k` of `b` is about to change: a built-in iterator prototype's `next` stops
/// being the built-in one (see [`JsObject::builtin_next`]).
#[inline]
pub(crate) fn touch(b: &mut JsObject, k: &str) {
    if b.builtin_next && k == "next" {
        b.builtin_next = false;
    }
}

pub fn set_own(o: &Obj, k: &str, v: Value) {
    let mut b = o.borrow_mut();
    touch(&mut b, k);
    if let Some(slot) = b.props.get_mut(k) {
        *slot = v;
        return;
    }
    let b = &mut *b;
    let k: Rc<str> = k.into();
    if !drop_accessor(b, &k) {
        push_key(&mut b.keys, k.clone());
    }
    b.props.insert(k, v);
}

/// `set_own` with a shared key (no allocation for a new key).
pub fn set_own_rc(o: &Obj, k: Rc<str>, v: Value) {
    let b = &mut *o.borrow_mut();
    touch(b, &k);
    if let Some(slot) = b.props.get_mut(&k) {
        *slot = v;
        return;
    }
    if !drop_accessor(b, &k) {
        push_key(&mut b.keys, k.clone());
    }
    b.props.insert(k, v);
}

/// A data property `k` replaces own accessor `k`: removes it; whether `k` stays in `keys`.
fn drop_accessor(b: &mut JsObject, k: &str) -> bool {
    let removed = b.accessors.as_mut().is_some_and(|m| m.remove(k).is_some());
    removed && b.keys.iter().any(|x| **x == *k)
}

/// `__proto__: v` of object literal `o` (ECMA-262 Annex B §B.3.1): [[SetPrototypeOf]] when
/// `v` is an object or null, else nothing. `o` is a fresh ordinary object, which no value of
/// its literal can reach yet, so OrdinarySetPrototypeOf always succeeds.
pub fn literal_proto(o: &Obj, v: &Value) {
    let p = match v {
        Value::Obj(p) => Some(p.clone()),
        Value::Null => None,
        _ => return,
    };
    let mut b = o.borrow_mut();
    b.null_proto = p.is_none();
    b.proto = p;
}

/// DefineField (ECMA-262 §7.3.33): CreateDataPropertyOrThrow of field `key` on `target`,
/// naming an anonymous function initializer under a computed key first (`name`).
/// [`define_field`] for a computed key: a symbol-keyed field is named `[description]`.
pub fn define_field_pk(
    it: &mut Interp,
    target: &Value,
    key: &PropKey,
    v: Value,
    name: bool,
) -> R<()> {
    match key {
        PropKey::Str(k) => define_field(it, target, k, v, name),
        PropKey::Sym(s) => {
            if name {
                crate::jsrt::name_function_pk(&v, "", key);
            }
            if let Value::Obj(o) = target {
                define_sym(o, s, v, true);
            }
            Ok(())
        }
    }
}

pub fn define_field(it: &mut Interp, target: &Value, key: &str, v: Value, name: bool) -> R<()> {
    if name {
        set_function_name(&v, key);
    }
    let Value::Obj(o) = target else {
        return Ok(());
    };
    let d = crate::props::Desc {
        value: Some(v),
        writable: Some(true),
        enumerable: Some(true),
        configurable: Some(true),
        ..Default::default()
    };
    if crate::props::define(it, o, key, &d)? {
        return Ok(());
    }
    it.throw("TypeError", format!("Cannot define property {key}"))
}

/// SetFunctionName (ECMA-262 §10.2.9) at run time, for a function defined under a computed
/// key: an own `name` that is non-writable, non-enumerable and configurable.
pub fn set_function_name(f: &Value, name: &str) {
    if let Value::Obj(o) = f {
        set_hidden(o, "name", Value::str(name));
        o.borrow_mut().set_attr("name", crate::value::READONLY);
    }
}

/// An own property that `Object.keys`/`JSON.stringify` skip (V4 non-enumerable).
pub fn set_hidden(o: &Obj, k: &str, v: Value) {
    let mut b = o.borrow_mut();
    touch(&mut b, k);
    if let Some(slot) = b.props.get_mut(k) {
        *slot = v;
        return;
    }
    if drop_accessor(&mut b, k) {
        b.keys.retain(|x| **x != *k);
    }
    b.props.insert(k.into(), v);
}

/// Links constructor `f` and its prototype object `p` (non-enumerable `f.prototype` and
/// `p.constructor`, as ECMA-262 MakeConstructor and the built-in constructors); a built-in
/// `f` becomes a constructor.
pub fn link_prototype(f: &Obj, p: &Obj) {
    set_hidden(p, "constructor", Value::Obj(f.clone()));
    set_hidden(f, "prototype", Value::Obj(p.clone()));
    let mut b = f.borrow_mut();
    b.builtin_ctor = true;
    // A class's `prototype` is also read-only (ClassDefinitionEvaluation: MakeConstructor with
    // writablePrototype false, ECMA-262 §15.7.14).
    let class =
        b.call.as_ref().and_then(Callable::kind) == Some(crate::jsrt::FnKind::ClassConstructor);
    let bits = b.attr("prototype") | PERMANENT | if class { READONLY } else { 0 };
    b.set_attr("prototype", bits);
}

/// [`link_prototype`] for a built-in constructor, whose `prototype` is also read-only
/// (ECMA-262 §20.1.2.21 and the like).
pub fn link_builtin_prototype(f: &Obj, p: &Obj) {
    link_prototype(f, p);
    f.borrow_mut()
        .set_attr("prototype", crate::value::READONLY | PERMANENT);
}

/// The `@@toStringTag` of a built-in object: read-only, non-enumerable and configurable
/// (ECMA-262 §20.4.3.6 and the like).
pub fn define_tag(o: &Obj, tag: &str) {
    let s = crate::value::WellKnown::ToStringTag.sym();
    write_sym(o, &s, Value::str(tag), None, false, crate::value::READONLY);
}

/// The `prototype` object of function object `f`, made on first use as MakeConstructor makes
/// it at creation (no script can tell the difference, and most functions never need one);
/// `None` when it has no object there and is no constructor, so never gets one.
pub fn fn_prototype(f: &Obj) -> Option<Obj> {
    match f.borrow().props.get("prototype") {
        Some(Value::Obj(p)) => return Some(p.clone()),
        // Set to a non-object: no prototype object (GetPrototypeFromConstructor falls back).
        Some(_) => return None,
        None => {}
    }
    // A generator function's `prototype` inherits %GeneratorPrototype% and has no
    // `constructor` (ECMA-262 §15.5.4, §27.3.4.3).
    if f.borrow().call.as_ref().and_then(Callable::kind) == Some(crate::jsrt::FnKind::Generator) {
        let p = new_obj();
        p.borrow_mut().internal = Internal::GeneratorPrototype;
        set_hidden(f, "prototype", Value::Obj(p.clone()));
        f.borrow_mut().record_order_first(&"prototype".into());
        return Some(p);
    }
    if !f
        .borrow()
        .call
        .as_ref()
        .is_some_and(Callable::is_constructor)
    {
        return None;
    }
    let p = new_obj();
    link_prototype(f, &p);
    // MakeConstructor (ECMA-262 §10.2.5) makes both with the function, before any key a
    // script adds.
    p.borrow_mut().record_order_first(&"constructor".into());
    f.borrow_mut().record_order_first(&"prototype".into());
    Some(p)
}

/// Defines the getter (`getter`) or setter of accessor property `k` on `o`, keeping the other
/// half of an existing accessor and replacing a data property `k`. Object-literal accessors
/// are enumerable; class members and native getters are not.
pub fn define_accessor(o: &Obj, k: &str, getter: bool, f: Value, enumerable: bool) {
    let b = &mut *o.borrow_mut();
    touch(b, k);
    b.props.remove(k);
    let m = b.accessors.get_or_insert_with(Default::default);
    let key: Rc<str> = match m.get_key_value(k) {
        Some((key, _)) => key.clone(),
        None => k.into(),
    };
    let a = m.entry(key.clone()).or_default();
    if getter {
        a.get = Some(f);
    } else {
        a.set = Some(f);
    }
    b.set_enumerable(&key, enumerable);
}

/// A class element: method `name` (`kind` "method"), or a "get"/"set" accessor.
pub fn define_method(target: &Obj, name: &str, kind: &str, f: Value) {
    match kind {
        "get" | "set" => define_accessor(target, name, kind == "get", f, false),
        _ => {
            set_hidden(target, name, f);
            target.borrow_mut().record_order(&name.into());
        }
    }
}

/// `define_method` for a computed class element key.
pub fn define_method_pk(target: &Obj, name: &PropKey, kind: &str, f: Value) {
    match name {
        PropKey::Str(n) => define_method(target, n, kind, f),
        PropKey::Sym(s) => match kind {
            "get" | "set" => define_sym_accessor(target, s, kind == "get", f, false),
            _ => define_sym(target, s, f, false),
        },
    }
}

/// The own accessor property `k` of `b`.
pub fn own_accessor<'a>(b: &'a JsObject, k: &str) -> Option<&'a Accessor> {
    b.accessors.as_deref()?.get(k)
}

/// Records a new own key in OrdinaryOwnPropertyKeys order (ECMA-262 10.1.11.1): array
/// indices first, ascending, then the other strings in insertion order.
pub(crate) fn push_key(keys: &mut Vec<Rc<str>>, k: Rc<str>) {
    match array_index(&k) {
        Some(i) => {
            let at = keys.partition_point(|x| array_index(x).is_some_and(|j| j < i));
            keys.insert(at, k);
        }
        None => keys.push(k),
    }
}

/// The binding of each formal of a parameter list whose formals bind `slots`: the last formal
/// of a name owns its binding (ECMA-262 §10.4.4.7 step 17), earlier ones get `u32::MAX`.
pub fn owner_ids(slots: &[u32]) -> Box<[u32]> {
    slots
        .iter()
        .enumerate()
        .map(|(k, s)| match slots[k + 1..].contains(s) {
            true => u32::MAX,
            false => *s,
        })
        .collect()
}

fn arguments_object(it: &Interp, args: &[Value], internal: Internal) -> (Obj, Vec<Rc<str>>) {
    let o = new_obj();
    let keys: Vec<Rc<str>> = (0..args.len()).map(|i| Rc::from(i.to_string())).collect();
    for (k, v) in keys.iter().zip(args) {
        set_own_rc(&o, k.clone(), v.clone());
    }
    set_hidden(&o, "length", Value::Num(args.len() as f64));
    // %Array.prototype.values% (§10.4.4.6 step 8, §10.4.4.7 step 20).
    define_sym(
        &o,
        &WellKnown::Iterator.sym(),
        it.sym.array_values.clone(),
        false,
    );
    o.borrow_mut().internal = internal;
    (o, keys)
}

/// CreateUnmappedArgumentsObject (ECMA-262 §10.4.4.6): strict code and non-simple parameter
/// lists. `callee` throws a TypeError.
pub fn unmapped_arguments(it: &Interp, args: &[Value]) -> Obj {
    let (o, _) = arguments_object(it, args, Internal::Arguments(None));
    let thrower = fn_value(Callable::Native("", |it, _, _| {
        it.throw("TypeError", "Type error")
    }));
    define_accessor(&o, "callee", true, thrower.clone(), false);
    define_accessor(&o, "callee", false, thrower, false);
    o
}

/// CreateMappedArgumentsObject (ECMA-262 §10.4.4.7) of a call of `callee`, whose formal `k`
/// has the binding `ids[k]` (see [`owner_ids`]). The formals' bindings live in the object
/// (see [`ArgsMap`]): read and write them with [`arg_get`]/[`arg_set`].
pub fn mapped_arguments(it: &Interp, callee: Value, args: &[Value], ids: Box<[u32]>) -> Obj {
    let (o, mut keys) = arguments_object(it, args, Internal::Ordinary);
    keys.extend((args.len()..ids.len()).map(|i| Rc::from(i.to_string())));
    keys.truncate(ids.len());
    let cells = (0..ids.len())
        .map(|k| (ids[k] == u32::MAX || k >= args.len()).then_some(Value::Undefined))
        .collect();
    o.borrow_mut().internal = Internal::Arguments(Some(Box::new(ArgsMap {
        ids,
        keys: keys.into(),
        cells,
    })));
    set_hidden(&o, "callee", callee);
    o
}

/// The formal of a mapped arguments object `a` whose binding is `id`.
pub fn arg_of_slot(a: &Obj, id: u32) -> Option<usize> {
    match &a.borrow().internal {
        Internal::Arguments(Some(m)) => m.ids.iter().position(|&x| x == id),
        _ => None,
    }
}

/// Value of formal `k` of the mapped arguments object `a`.
pub fn arg_get(a: &Obj, k: usize) -> Value {
    let b = a.borrow();
    let Internal::Arguments(Some(m)) = &b.internal else {
        return Value::Undefined;
    };
    match &m.cells[k] {
        Some(v) => v.clone(),
        None => b.props.get(&m.keys[k]).cloned().unwrap_or_default(),
    }
}

/// Assigns formal `k` of the mapped arguments object `a`: `arguments[k]` too while mapped.
pub fn arg_set(a: &Obj, k: usize, v: Value) {
    let b = &mut *a.borrow_mut();
    let Internal::Arguments(Some(m)) = &mut b.internal else {
        return;
    };
    match &mut m.cells[k] {
        Some(c) => *c = v,
        None => match b.props.get_mut(&m.keys[k]) {
            Some(p) => *p = v,
            None => {
                let key = m.keys[k].clone();
                push_key(&mut b.keys, key.clone());
                b.props.insert(key, v);
            }
        },
    }
}

/// Whether `k` is `length` or a code unit index of String object `b` (ECMA-262 §10.4.3.5): a
/// read-only, non-configurable own property that a sloppy write or `delete` leaves as it is.
fn string_object_fixed(b: &JsObject, k: &str) -> bool {
    match &b.internal {
        Internal::Primitive(p) if let Some(s) = p.js_str() => {
            k == "length" || idx(k).is_some_and(|i| i < s.len())
        }
        _ => false,
    }
}

/// Removes own property `k`: `delete o.k` / `delete o[k]`. An array element becomes a hole
/// (see [`JsObject::holes`]); a data or accessor property is removed.
pub fn delete_own(o: &Obj, k: &str) -> bool {
    if k == "prototype" {
        fn_prototype(o);
    }
    let b = &mut *o.borrow_mut();
    // A method or constant property of the `Qt` object or `Qt.platform` stays.
    if b.native.is_some() && crate::qtobj::constant_member(b, k) {
        return false;
    }
    // A value type's properties are its type's: deleting one leaves it. What script
    // defined on it is deleted where it is kept.
    if let Internal::ValueType(vt) = b.internal {
        if vt.fields().contains(&k) {
            return true;
        }
        if let Some(m) = b.ext.as_ref().and_then(|e| e.members.clone()) {
            return delete_own(&m, k);
        }
    }
    // QV4: `delete` of a typed array's element leaves it and answers true (ECMA-262
    // §10.4.5.6 answers false for an element in range).
    if let Internal::TypedArray(t) = &b.internal
        && crate::typedarray::has_index(t, k).is_some()
    {
        return true;
    }
    if string_object_fixed(b, k)
        || b.attr(k) & PERMANENT != 0
        || (k == "length" && b.array.is_some())
    {
        return false;
    }
    // [[Delete]] of a mapped arguments object (ECMA-262 §10.4.4.5): the formal keeps its value.
    if let Internal::Arguments(Some(m)) = &mut b.internal
        && let Some(i) = m.keys.iter().position(|x| **x == *k)
        && m.ids[i] != u32::MAX
        && m.cells[i].is_none()
    {
        m.cells[i] = Some(b.props.get(k).cloned().unwrap_or_default());
    }
    if b.array.is_some()
        && let Some(i) = array_index(k)
    {
        // QV4 (`SimpleArrayData::del`): a position without a cell has nothing to delete, and
        // its attribute byte stays; an element with a zero byte is not configurable.
        if i >= b.slots() && !b.props.contains_key(k) && own_accessor(b, k).is_none() {
            return true;
        }
        if b.attr(k) & crate::value::HIDDEN != 0 && !b.is_hole(i) {
            return false;
        }
        let cleared = b.cleared_attr(i);
        if let Some(e) = b.array.as_mut().and_then(|a| a.get_mut(i)) {
            *e = Value::Undefined;
            b.add_holes([i]);
        }
        b.set_attr(k, cleared);
        let data = b.props.remove(k).is_some();
        let acc = b.accessors.as_mut().is_some_and(|m| m.remove(k).is_some());
        if data || acc {
            b.keys.retain(|x| **x != *k);
        }
        return true;
    }
    touch(b, k);
    let data = b.props.remove(k).is_some();
    let acc = b.accessors.as_mut().is_some_and(|m| m.remove(k).is_some());
    if data || acc {
        b.keys.retain(|x| **x != *k);
        b.forget_order(k);
        b.set_attr(k, 0);
    }
    if b.call.is_some() && matches!(k, "length" | "name") {
        b.set_attr(k, ABSENT);
    }
    true
}

/// A function's built-in own `length` or `name` (ECMA-262 §10.2.9, §10.2.10), unless it was
/// deleted or redefined as an ordinary property or an accessor.
pub fn intrinsic(b: &JsObject, k: &str) -> Option<Value> {
    let c = b.call.as_ref()?;
    if b.props.contains_key(k)
        || b.attr(k) & ABSENT != 0
        || b.accessors.as_ref().is_some_and(|a| a.contains_key(k))
    {
        return None;
    }
    match k {
        "length" => c.length().map(Value::Num),
        // V4 gives a signal or a C++ method of an object no `name`.
        "name" if matches!(c, Callable::Item(..)) => None,
        "name" => Some(Value::Str(c.name())),
        _ => None,
    }
}

/// `o` has own property `k` (a value or an accessor).
pub fn has_own(o: &Obj, k: &str) -> bool {
    // A constructor's `prototype` is made on first use, but is an own property from the start.
    if k == "prototype" && o.borrow().call.is_some() && !o.borrow().props.contains_key(k) {
        fn_prototype(o);
    }
    let b = o.borrow();
    if let Internal::TypedArray(t) = &b.internal
        && let Some(has) = crate::typedarray::has_index(t, k)
    {
        return has;
    }
    // An index written to a value type is its own; a name defined on it is not.
    if let Internal::ValueType(_) = b.internal
        && idx(k).is_some()
    {
        let members = b.ext.as_ref().and_then(|e| e.members.as_ref());
        return members.is_some_and(|m| has_own(m, k));
    }
    b.props.contains_key(k)
        || own_accessor(&b, k).is_some()
        || b.array.as_ref().is_some_and(|a| {
            idx(k).is_some_and(|i| i < a.len() && !b.is_hole(i) && !b.hidden(k)) || k == "length"
        })
        || intrinsic(&b, k).is_some()
}

/// The QML property a mutated object read from it writes back to; a value type writes back
/// itself ([`crate::valuetypes::notify_owner`]).
#[cfg(feature = "interp")]
fn sequence_owner(v: &Value) -> Option<(usize, Rc<str>)> {
    match v {
        Value::Obj(x) => {
            let b = x.borrow();
            (!matches!(b.internal, Internal::ValueType(_)))
                .then(|| b.owner.clone())
                .flatten()
        }
        _ => None,
    }
}

pub fn fn_value(c: Callable) -> Value {
    let mut o = JsObject::default();
    o.call = Some(c);
    Value::Obj(crate::value::reg(Rc::new(RefCell::new(o))))
}

pub fn callable(v: &Value) -> Option<Callable> {
    match v {
        Value::Obj(o) => o.borrow().call.clone(),
        _ => None,
    }
}

/// Longest string a builtin may build, in UTF-16 units (V8's limit on 64-bit).
pub const MAX_STRING_LEN: usize = (1 << 29) - 24;
/// The most elements an array stores (256 MB of values, the image decoder's allocation
/// cap): a builtin that would store more throws instead of aborting the shell. A length
/// alone stores nothing ([`JsObject::tail`]), so it goes up to `2^32 - 1`
/// ([`array_length`]).
pub const MAX_DENSE_ARRAY: usize = 1 << 24;
/// `2^32 - 1`: array indices are below it, larger numeric keys are plain properties.
const MAX_ARRAY_INDEX: usize = u32::MAX as usize;

/// A requested array length (`length = n`, `new Array(n)`): QV4's `RangeError: <n> out of
/// range` unless `n` is a uint32 (ECMA-262 §10.4.2.4 step 5, §23.1.1.1 step 6.d).
pub fn array_length(it: &Interp, n: f64) -> R<usize> {
    if !(n >= 0.0 && n <= u32::MAX as f64 && n.fract() == 0.0) {
        let n = Value::Num(n).to_js_string();
        return it.throw("RangeError", format!("{n} out of range"));
    }
    Ok(n as usize)
}

/// The count of elements a builtin is about to store (the elements of an array-like it
/// copies, the result it builds): [`array_length`], and a RangeError past
/// [`MAX_DENSE_ARRAY`].
pub fn array_len(it: &Interp, n: f64) -> R<usize> {
    array_length(it, n)?;
    if n > MAX_DENSE_ARRAY as f64 {
        return it.throw(
            "RangeError",
            format!("Invalid array length: {n} exceeds the dense array limit of {MAX_DENSE_ARRAY}"),
        );
    }
    Ok(n as usize)
}

/// The integer `k` is the canonical decimal string of, if any.
pub(crate) fn idx(k: &str) -> Option<usize> {
    let b = k.as_bytes();
    if b.is_empty() || (b.len() > 1 && b[0] == b'0') || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    k.parse().ok()
}

/// ECMA-262 array index: a canonical numeric string below `2^32 - 1`.
pub(crate) fn array_index(k: &str) -> Option<usize> {
    if !k.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    idx(k).filter(|&i| i < MAX_ARRAY_INDEX)
}

/// `Math.round` (ECMA-262 §21.3.2.28): ties go up, and -0.5 <= x < 0 gives -0.
pub fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 {
        (f + 1.0).copysign(x)
    } else {
        f
    }
}

thread_local! {
    static JS_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Deepest JS call nesting before `RangeError` (V8 allows about this many simple frames). The
/// limit is a frame count, not the size of whatever native stack the caller runs on: JS frames
/// move to a fresh stack segment whenever the current one runs low ([`on_js_stack`]), so the
/// same script reaches the same depth in debug and release, on the main thread or a 2 MiB one.
pub const MAX_JS_DEPTH: u32 = 10_000;
/// Native stack kept free at every JS frame entry: room for the deepest native recursion
/// between two frames (expression nesting, builtins, the host) and for the error path.
const JS_RED_ZONE: usize = 512 << 10;
/// Size of each stack segment JS frames continue on once the current one is down to the red zone.
const JS_STACK_SEGMENT: usize = 4 << 20;

/// Runs `f` (the body of one JS frame) with at least [`JS_RED_ZONE`] of native stack, on a
/// new segment if the current stack is lower than that.
#[inline]
pub fn on_js_stack<T>(f: impl FnOnce() -> T) -> T {
    stacker::maybe_grow(JS_RED_ZONE, JS_STACK_SEGMENT, f)
}

/// One entered JS frame ([`Interp::enter`]); leaves it when dropped, also on unwinding.
#[must_use]
pub struct Frames(/** Whether it is the outermost frame. */ bool);

impl Drop for Frames {
    fn drop(&mut self) {
        let d = JS_DEPTH.with(|c| {
            let d = c.get().saturating_sub(1);
            c.set(d);
            d
        });
        if d == 0 {
            crate::watchdog::end();
        }
    }
}

/// JS frames active on this thread.
pub fn js_depth() -> u32 {
    JS_DEPTH.with(Cell::get)
}

/// What `Object.preventExtensions`, `seal` and `freeze` leave on a QML object in Qt 6.12
/// (V4 keeps it on the object's wrapper). A QObject wrapper has no own JS properties, so
/// `Object.isFrozen` is true at every level and `isSealed` from `Sealed` on; only `Frozen`
/// refuses writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemIntegrity {
    NonExtensible,
    Sealed,
    Frozen,
}

pub struct Interp {
    pub global: Scope,
    /// The running code is strict mode code (ECMA-262 §11.2.2): a refused [[Set]] or
    /// [[Delete]] throws a TypeError there.
    pub strict: bool,
    pub host: Box<dyn Host>,
    /// This thread's watchdog slot: whether the running evaluation was interrupted.
    watchdog: std::sync::Arc<crate::watchdog::Slot>,
    /// The QML objects `Object.preventExtensions`, `seal` or `freeze` was applied to, with
    /// the level reached ([`ItemIntegrity`]).
    pub item_integrity: rustc_hash::FxHashMap<usize, ItemIntegrity>,
    /// The counters and timers of the `console` object.
    pub console: crate::builtins::Console,
    #[cfg(feature = "interp")]
    /// Call frames no closure kept, for reuse.
    frames: Vec<Scope>,
    /// Argument vectors for reuse.
    #[cfg(feature = "interp")]
    argvs: Vec<Vec<Value>>,
    /// %Object.prototype%: the `[[Prototype]]` of every object whose `proto` is `None`
    /// (unless `null_proto`), so ordinary objects need no link to it.
    pub object_proto: Obj,
    /// %Array.prototype%: the implicit `[[Prototype]]` of every array, as `object_proto`.
    pub array_proto: Obj,
    /// %Function.prototype%: the implicit `[[Prototype]]` of every function object.
    pub function_proto: Obj,
    /// The `[[Prototype]]` of every QML type and import qualifier object and of `Qt`.
    pub type_proto: Obj,
    /// %String.prototype%, %Number.prototype%, %Boolean.prototype%: what primitives inherit.
    pub string_proto: Obj,
    pub number_proto: Obj,
    pub boolean_proto: Obj,
    /// %RegExp.prototype%, %Map.prototype%, %Set.prototype%: the implicit `[[Prototype]]` of
    /// every RegExp, Map and Set object.
    pub regexp_proto: Obj,
    pub map_proto: Obj,
    pub set_proto: Obj,
    /// The last successful RegExp match, which the legacy `RegExp.$1`-style statics read.
    pub(crate) regexp_last: Option<crate::builtins::LegacyMatch>,
    /// %GeneratorFunction.prototype%, %GeneratorPrototype% and the generator `ctx` prototype.
    pub generator_protos: crate::generator::Protos,
    /// %Promise.prototype%.
    pub promise_proto: Obj,
    /// The realm's global object: what `this` of sloppy code called without a receiver binds
    /// ([`Self::sloppy_this`]), and where an otherwise unresolved free name is looked up.
    pub global_object: Obj,
    /// The Symbol, built-in iterator and weak collection intrinsics.
    pub sym: crate::symbol::Realm,
    /// Whether the native function now entered runs as [[Construct]] (NewTarget defined).
    pub constructing: bool,
    /// The prototype a native constructor now entered makes its object with when NewTarget is
    /// another constructor (GetPrototypeFromConstructor, ECMA-262 §10.1.14), for built-ins that
    /// need the object's prototype while they run (Map and Set read their adder from it).
    pub(crate) new_target_proto: Option<Obj>,
    /// The scope of the running JavaScript code (its function's closure scope, or the root
    /// scope of a top-level body), from which `Qt.include` finds its calling file.
    pub code: Option<Scope>,
    /// The QML scope `(item, ctx)` of the running compiled code, which has no scope chain to
    /// find it in; `None` while interpreted code runs ([`Interp::calling_qml_scope`]).
    pub qs: Option<(usize, usize)>,
    /// The realm's value type prototypes ([`crate::valuetypes::Protos`]).
    pub vt_protos: crate::valuetypes::Protos,
}

impl Default for Interp {
    fn default() -> Self {
        Self::new(Box::new(NoHost))
    }
}

impl Interp {
    pub fn new(host: Box<dyn Host>) -> Self {
        let mut global = Frame::default();
        global.global = true;
        let mut it = Self::with_global(host, Rc::new(RefCell::new(global)));
        crate::builtins::install(&mut it);
        it
    }

    /// An interpreter sharing `global`, which already holds the builtins: no reinstall.
    pub fn with_global(host: Box<dyn Host>, global: Scope) -> Self {
        let proto = |ctor: &str| match global.borrow().vars.get(ctor) {
            Some(Value::Obj(o)) => match o.borrow().props.get("prototype") {
                Some(Value::Obj(p)) => p.clone(),
                _ => new_obj(),
            },
            _ => new_obj(),
        };
        let function_proto = proto("Function");
        let generator_protos = global
            .borrow_mut()
            .generator_protos
            .get_or_insert_with(|| crate::generator::protos(&function_proto))
            .clone();
        let global_object = global
            .borrow_mut()
            .global_object
            .get_or_insert_with(new_obj)
            .clone();
        let vt_protos = global
            .borrow_mut()
            .value_type_protos
            .get_or_insert_with(crate::valuetypes::Protos::new)
            .clone();
        let sym = global
            .borrow()
            .symbol_realm
            .as_deref()
            .cloned()
            .unwrap_or_default();
        let type_proto = global
            .borrow_mut()
            .type_proto
            .get_or_insert_with(|| crate::types::wrapper_proto(&function_proto))
            .clone();
        Interp {
            strict: false,
            object_proto: proto("Object"),
            array_proto: proto("Array"),
            function_proto,
            type_proto,
            generator_protos,
            vt_protos,
            string_proto: proto("String"),
            number_proto: proto("Number"),
            boolean_proto: proto("Boolean"),
            regexp_proto: proto("RegExp"),
            map_proto: proto("Map"),
            set_proto: proto("Set"),
            regexp_last: None,
            promise_proto: proto("Promise"),
            global_object,
            sym,
            constructing: false,
            new_target_proto: None,
            global,
            host,
            watchdog: crate::watchdog::slot(),
            item_integrity: Default::default(),
            console: Default::default(),
            #[cfg(feature = "interp")]
            frames: Vec::new(),
            #[cfg(feature = "interp")]
            argvs: Vec::new(),
            code: None,
            qs: None,
        }
    }

    /// An interpreter for a `.pragma library` body in the realm whose global scope is `realm`:
    /// it shares the realm's built-in constructors and prototypes (Qt runs every script of an
    /// engine against one set of intrinsics), and its own `global` is a scope on top of
    /// `realm` holding only the library's declarations.
    pub fn for_library(realm: &Scope) -> Self {
        let mut it = Self::with_global(Box::new(NoHost), realm.clone());
        it.global = Self::child(realm);
        it
    }

    /// The realm's global scope (the builtins): the root of `global`'s chain.
    pub fn realm(&self) -> Scope {
        let mut sc = self.global.clone();
        while let Some(p) = sc.try_borrow().ok().and_then(|f| f.parent.clone()) {
            sc = p;
        }
        sc
    }

    /// GetTemplateObject (ECMA-262 §13.2.8.4): the template object this realm made for the
    /// tagged template site `key`, if any.
    pub fn template_object(&self, key: usize) -> Option<Value> {
        let realm = self.realm();
        let g = realm.borrow();
        let (v, _) = g.templates.as_ref()?.objects.get(&key)?;
        Some(v.clone())
    }

    /// GetTemplateObject's new object for site `key`: the array of `cooked` strings (`undefined`
    /// for an invalid escape) with a non-enumerable `raw` array of the raw strings, cached in
    /// the realm. `site` is the heap site whose address is `key`.
    pub fn new_template_object<'a>(
        &mut self,
        key: usize,
        site: Option<Rc<dyn std::any::Any>>,
        cooked: impl Iterator<Item = Option<&'a str>>,
        raw: impl Iterator<Item = &'a str>,
    ) -> Value {
        let raw = Value::array(raw.map(Value::str).collect());
        let t = Value::array(
            cooked
                .map(|c| c.map_or(Value::Undefined, Value::str))
                .collect(),
        );
        if let (Value::Obj(o), Value::Obj(r)) = (&t, &raw) {
            crate::props::freeze_array(r);
            set_hidden(o, "raw", raw.clone());
            crate::props::freeze_array(o);
        }
        let realm = self.realm();
        let mut g = realm.borrow_mut();
        let map = g.templates.get_or_insert_with(Default::default);
        map.insert(key, t.clone(), site.as_ref());
        t
    }

    /// `o.[[Prototype]]`: its `proto`, else its built-in prototype ([`Self::builtin_proto`])
    /// or %Object.prototype%, unless that is `o` itself or `o` was made with a null prototype.
    pub fn proto_of(&self, o: &Obj) -> Option<Obj> {
        let b = o.borrow();
        match &b.proto {
            Some(p) => Some(p.clone()),
            None if b.null_proto || Rc::ptr_eq(o, &self.object_proto) => None,
            // `Qt` has the prototype of a QML type, yet reads its names as a QObject does
            // (QQmlTypeWrapper::virtualGet): `"call" in Qt`, and `Qt.call` is undefined.
            None if crate::types::is_qt_global(&b) => Some(self.type_proto.clone()),
            None => Some(
                self.builtin_proto(o, &b)
                    .unwrap_or_else(|| self.object_proto.clone()),
            ),
        }
    }

    /// The implicit prototype of an array, a function, a RegExp, a Map or a Set `o` (with no
    /// `proto` of its own), whose own prototype is %Object.prototype%.
    fn builtin_proto(&self, o: &Obj, b: &JsObject) -> Option<Obj> {
        // A Date made outside script (a `date` property's): the realm's %Date.prototype%.
        if matches!(b.internal, Internal::Date(_)) {
            return match self.realm().borrow().vars.get("Date") {
                Some(Value::Obj(d)) => match d.borrow().props.get("prototype") {
                    Some(Value::Obj(p)) if !Rc::ptr_eq(o, p) => Some(p.clone()),
                    _ => None,
                },
                _ => None,
            };
        }
        let p = if Rc::ptr_eq(o, &self.vt_protos.sequence) {
            &self.array_proto
        } else if crate::valuetypes::is_readonly_sequence(b) {
            &self.vt_protos.sequence
        } else if b.array.is_some() {
            &self.array_proto
        } else if crate::types::inherits_function(b) {
            &self.type_proto
        } else if let Some(c) = &b.call {
            if c.kind() == Some(crate::jsrt::FnKind::Generator) {
                &self.generator_protos.function
            } else {
                &self.function_proto
            }
        } else {
            match b.internal {
                Internal::RegExp { .. } => &self.regexp_proto,
                Internal::Map(_) => &self.map_proto,
                Internal::Set(_) => &self.set_proto,
                Internal::GeneratorPrototype => &self.generator_protos.generator,
                Internal::ValueType(_)
                | Internal::Url(..)
                | Internal::AnchorLine(_)
                | Internal::ScriptString(_)
                | Internal::Locale(..) => self.vt_protos.of(b)?,
                _ => return None,
            }
        };
        (!Rc::ptr_eq(o, p)).then(|| p.clone())
    }

    /// Property `k` primitive `v` inherits: from %String.prototype%, %Number.prototype% or
    /// %Boolean.prototype%, then %Object.prototype%.
    /// ToObject (ECMA-262 §7.1.18) of a primitive: a new Number, Boolean, String or Symbol
    /// object with it in its internal slot; any other value as it is (undefined and null, whose
    /// ToObject throws, are left to the caller).
    pub fn to_object(&self, v: &Value) -> Value {
        if let Value::Symbol(s) = v {
            let o = new_obj();
            let mut b = o.borrow_mut();
            b.proto = Some(self.sym.symbol_proto.clone());
            b.internal = Internal::Symbol(s.clone());
            drop(b);
            return Value::Obj(o);
        }
        let proto = match v {
            Value::Num(_) => &self.number_proto,
            Value::Bool(_) => &self.boolean_proto,
            Value::Str(_) | Value::Str16(_) => &self.string_proto,
            _ => return v.clone(),
        };
        let o = new_obj();
        // A String object's code units are its own enumerable, read-only, non-configurable
        // index properties and `length` a non-enumerable one (ECMA-262 §10.4.3).
        if let Some(s) = v.js_str() {
            let units = s.units();
            for (i, u) in units.iter().enumerate() {
                set_own(&o, &i.to_string(), Value::from_utf16(&[*u]));
            }
            set_hidden(&o, "length", Value::Num(units.len() as f64));
            let mut b = o.borrow_mut();
            for k in (0..units.len())
                .map(|i| i.to_string())
                .chain(["length".into()])
            {
                b.set_attr(&k, READONLY | PERMANENT);
            }
        }
        let mut b = o.borrow_mut();
        b.proto = Some(proto.clone());
        b.internal = Internal::Primitive(v.clone());
        drop(b);
        Value::Obj(o)
    }

    /// OrdinaryCallBindThis (ECMA-262 §10.2.1.2) of non-strict code: the global object for
    /// an undefined or null `this`, the wrapper object of a primitive one, `None` for an
    /// object bound as it is.
    pub fn sloppy_this(&self, this: &Value) -> Option<Value> {
        match this {
            Value::Undefined | Value::Null => Some(Value::Obj(self.global_object.clone())),
            Value::Num(_) | Value::Bool(_) | Value::Str(_) | Value::Str16(_) | Value::Symbol(_) => {
                Some(self.to_object(this))
            }
            _ => None,
        }
    }

    /// A free name no declaration binds, as a property of the global object (own or
    /// inherited: GetBindingValue of the global Object Environment Record, ECMA-262
    /// §9.1.1.2.6); `None` when it has none.
    pub fn global_binding(&mut self, name: &str) -> R<Option<Value>> {
        let g = self.global_object.clone();
        if !self.has_property(&g, name)? {
            return Ok(None);
        }
        self.get(&Value::Obj(g), name).map(Some)
    }

    /// GetValue on a primitive base (ECMA-262 §6.2.5.5): [[Get]] of `k` from its wrapper's
    /// prototype on, with the primitive as the receiver (getters see it as `this`).
    fn primitive_inherited(&mut self, v: &Value, k: &str) -> R<Value> {
        let proto = match v {
            Value::Str(_) | Value::Str16(_) => self.string_proto.clone(),
            Value::Num(_) => self.number_proto.clone(),
            _ => self.boolean_proto.clone(),
        };
        self.get_from(&proto, k, v)
    }

    /// Property `k` a symbol inherits from %Symbol.prototype% (with its accessors).
    fn symbol_inherited(&mut self, v: &Value, k: &str) -> R<Value> {
        let mut cur = Some(self.sym.symbol_proto.clone());
        while let Some(o) = cur {
            let (g, x) = {
                let b = o.borrow();
                (
                    own_accessor(&b, k).map(|a| a.get.clone()),
                    b.props.get(k).cloned(),
                )
            };
            match (g, x) {
                (Some(Some(g)), _) => return self.call_getter(&g, v),
                (Some(None), _) => return Ok(Value::Undefined),
                (None, Some(x)) => return Ok(x),
                (None, None) => cur = self.proto_of(&o),
            }
        }
        Ok(Value::Undefined)
    }

    /// [`Host::call`] with what the host throws raised as a JS exception.
    pub fn host_call(&mut self, item: usize, name: &str, args: &[Value]) -> Option<R<Value>> {
        // A signal declared in QML takes its arguments converted to the declared types
        // (QV4::QObjectMethod::callInternal -> CallArgument::fromValue).
        let converted: Vec<Value>;
        let args = match self.host.signal_types(item, name) {
            Some(tys) => {
                let mut out = Vec::with_capacity(args.len());
                for (i, a) in args.iter().enumerate() {
                    out.push(match tys.get(i) {
                        Some(ty) => match crate::valuetypes::signal_arg(self, ty, a.clone()) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        },
                        None => a.clone(),
                    });
                }
                converted = out;
                &converted[..]
            }
            None => args,
        };
        if self.host.resolves_for_caller(item, name) {
            let doc = crate::runtime::calling_url(self);
            self.host.called_from(doc);
        }
        Some(
            self.host
                .call(item, name, args)?
                .or_else(|e| self.throw(e.kind, e.message)),
        )
    }

    /// `delete` of a Super Reference (ECMA-262 §13.5.1.2 step 5.b).
    pub fn delete_super(&self) -> R<Value> {
        self.throw("ReferenceError", "Unsupported reference to 'super'")
    }

    pub fn throw<T>(&self, kind: &str, msg: impl Into<String>) -> R<T> {
        Err(Ex::Throw(self.make_error(kind, Some(msg.into()))))
    }

    /// A `kind` error object, like `new TypeError(msg)`: `name` comes from the global
    /// constructor's prototype, `message` is own and non-enumerable.
    pub fn make_error(&self, kind: &str, msg: Option<String>) -> Value {
        let e = new_obj();
        let proto = self
            .realm()
            .try_borrow()
            .ok()
            .and_then(|g| match g.vars.get(kind) {
                Some(Value::Obj(c)) => match c.borrow().props.get("prototype") {
                    Some(Value::Obj(p)) => Some(p.clone()),
                    _ => None,
                },
                _ => None,
            });
        match proto {
            Some(p) => e.borrow_mut().proto = Some(p),
            None => set_hidden(&e, "name", Value::str(kind)),
        }
        // QV4 makes every error with `stack`, `fileName` and `lineNumber`, then `message` if
        // it has one. No frames are kept here (compiled code has none), so the error is the
        // one QV4 makes with an empty stack trace: an empty `stack`, the other two undefined.
        set_hidden(&e, "stack", Value::str(""));
        set_hidden(&e, "fileName", Value::Undefined);
        set_hidden(&e, "lineNumber", Value::Undefined);
        if let Some(m) = msg {
            set_hidden(&e, "message", Value::str(&m));
        }
        {
            let mut b = e.borrow_mut();
            b.internal = Internal::Error;
            thread_local! {
                static KEYS: [Rc<str>; 4] =
                    ["stack", "fileName", "lineNumber", "message"].map(Rc::from);
            }
            KEYS.with(|ks| b.ext.get_or_insert_default().order = Some(ks.to_vec()));
        }
        Value::Obj(e)
    }

    /// Out of line: inlined, the `Frame` is built on the caller's stack, which grows the frames of
    /// the recursive `exec`/`eval` paths and so lowers the JS recursion depth.
    #[inline(never)]
    pub fn child(parent: &Scope) -> Scope {
        let mut f = Frame::default();
        f.parent = Some(parent.clone());
        Rc::new(RefCell::new(f))
    }

    #[cfg(feature = "interp")]
    fn iteration_copy(fr: &Scope) -> Scope {
        let f = fr.borrow();
        let mut copy = Frame::default();
        copy.vars = f.vars.clone();
        copy.parent = f.parent.clone();
        copy.this = f.this.clone();
        Rc::new(RefCell::new(copy))
    }

    /// Slot `slot` of the call frame `hops` call frames up from `sc`.
    #[cfg(feature = "interp")]
    fn slot_get(sc: &Scope, hops: u32, slot: u32) -> Value {
        let f = sc.borrow();
        if f.fun {
            if hops == 0 {
                if let Some(a) = &f.args
                    && let Some(k) = arg_of_slot(a, slot)
                {
                    return arg_get(a, k);
                }
                return f.slots[slot as usize].clone();
            }
            return Self::slot_get(f.parent.as_ref().unwrap(), hops - 1, slot);
        }
        Self::slot_get(f.parent.as_ref().unwrap(), hops, slot)
    }

    /// Value of the local `name` in slot `slot`, `hops` call frames up: a binding still in
    /// its temporal dead zone throws.
    #[cfg(feature = "interp")]
    fn local(&self, sc: &Scope, hops: u32, slot: u32, name: &str) -> R<Value> {
        match Self::slot_get(sc, hops, slot) {
            Value::Empty => self.uninitialized(name),
            v => Ok(v),
        }
    }

    #[cfg(feature = "interp")]
    fn slot_set(sc: &Scope, hops: u32, slot: u32, v: Value) {
        let mut f = sc.borrow_mut();
        if f.fun {
            if hops == 0 {
                if let Some(a) = &f.args
                    && let Some(k) = arg_of_slot(a, slot)
                {
                    return arg_set(a, k, v);
                }
                f.slots[slot as usize] = v;
                return;
            }
            let p = f.parent.clone().unwrap();
            drop(f);
            return Self::slot_set(&p, hops - 1, slot, v);
        }
        let p = f.parent.clone().unwrap();
        drop(f);
        Self::slot_set(&p, hops, slot, v)
    }

    /// Enters one JS frame (a call, construction or top-level body), the one entry point of
    /// every interpreter and compiled entry on this thread. The outermost frame starts an
    /// evaluation for the watchdog ([`crate::watchdog`]) and the guard ends it when dropped,
    /// whichever path (binding, handler, synchronous update inside a write) entered JS; nesting
    /// deeper than [`MAX_JS_DEPTH`] throws `RangeError` like V4. Run the frame's body in
    /// [`on_js_stack`].
    pub fn enter(&mut self) -> R<Frames> {
        if JS_DEPTH.with(Cell::get) >= MAX_JS_DEPTH {
            return self.stack_overflow();
        }
        let d = JS_DEPTH.with(|c| {
            let d = c.get();
            c.set(d + 1);
            d
        });
        if d == 0 {
            crate::watchdog::begin();
        }
        Ok(Frames(d == 0))
    }

    /// Leaves a frame of [`Self::enter`] with its result `r`. Leaving the outermost one first
    /// runs the promise jobs queued meanwhile (ECMA-262 §9.5: jobs run once the execution
    /// context stack is empty), under the same watchdog turn.
    pub fn leave<T>(&mut self, frame: Frames, r: R<T>) -> R<T> {
        if frame.0 {
            crate::promise::run_jobs(self);
        }
        drop(frame);
        r
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn stack_overflow<T>(&self) -> R<T> {
        self.throw("RangeError", "Maximum call stack size exceeded")
    }

    /// The watchdog interrupted the running evaluation (checked at loop back-edges and calls).
    #[inline]
    pub fn interrupted(&self) -> bool {
        self.watchdog.interrupted()
    }

    pub(crate) fn tick(&mut self) -> R<()> {
        if self.interrupted() {
            return self.throw("Error", "Interrupted");
        }
        Ok(())
    }

    pub fn define(&self, sc: &Scope, name: &str, v: Value) {
        sc.borrow_mut().vars.insert(name.to_string(), v);
    }

    /// `name` resolves to a binding of a function call frame or a scope inside one.
    #[cfg(feature = "interp")]
    fn declared_in_function(sc: &Scope, name: &str) -> bool {
        let mut found = false;
        let mut cur = Some(sc.clone());
        while let Some(s) = cur {
            let f = s.borrow();
            found |= f.vars.contains_key(name);
            if found && f.fun {
                return true;
            }
            cur = f.parent.clone();
        }
        false
    }

    /// Deletes the binding `name` resolves to if a direct eval declared it; whether it did.
    #[cfg(feature = "interp")]
    fn delete_eval_var(sc: &Scope, name: &str) -> bool {
        let mut cur = Some(sc.clone());
        while let Some(s) = cur {
            let mut f = s.borrow_mut();
            if f.vars.contains_key(name) {
                let Some(i) = f.eval_vars.iter().position(|n| n == name) else {
                    return false;
                };
                f.eval_vars.swap_remove(i);
                f.vars.remove(name);
                return true;
            }
            cur = f.parent.clone();
        }
        false
    }

    /// HasBinding of an object environment (ECMA-262 §9.1.1.2.1): `o` has the property
    /// `name` and its `@@unscopables` does not block it.
    #[cfg(feature = "interp")]
    fn with_binds(&mut self, o: &Value, name: &str) -> R<bool> {
        let found = match o {
            Value::Obj(ob) => {
                self.has_property(ob, name)? || crate::builtins::has_method(self, o, name)
            }
            Value::Item(id) => {
                self.host.get(*id, name).is_some() || self.host.has_method(*id, name)
            }
            _ => false,
        };
        if !found {
            return Ok(false);
        }
        let blocked = self.get_sym(o, &WellKnown::Unscopables.sym())?;
        if matches!(blocked, Value::Obj(_) | Value::Item(_)) {
            return Ok(!self.get(&blocked, name)?.truthy());
        }
        Ok(true)
    }

    /// GetIdentifierReference (ECMA-262 §9.1.2.1) for `name` over the environments its code
    /// cannot see into: the innermost `withs` object environments, and the named bindings
    /// (a direct eval's `var`s) of the frames up to the call frame `hops` call frames up,
    /// where the code shows the binding (`None`: it is a name, looked up to the top).
    #[cfg(feature = "interp")]
    fn dyn_ref(&mut self, sc: &Scope, name: &str, withs: u32, hops: Option<u32>) -> R<DynRef> {
        let (mut withs, mut hops) = (withs, hops);
        let mut cur = sc.clone();
        loop {
            let (with, fun, named, parent) = {
                let f = cur.borrow();
                let named = !f.vars.is_empty() && f.vars.contains_key(name);
                (f.with.clone(), f.fun, named, f.parent.clone())
            };
            if let Some(o) = with {
                if withs > 0 {
                    withs -= 1;
                    if self.with_binds(&o, name)? {
                        return Ok(DynRef::With(o));
                    }
                }
            } else {
                if fun && hops == Some(0) {
                    return Ok(DynRef::Static);
                }
                if named {
                    // A name the code looks up by name is found there the usual way.
                    return Ok(match hops {
                        Some(_) => DynRef::Frame(cur),
                        None => DynRef::Static,
                    });
                }
                if fun && let Some(h) = &mut hops {
                    *h -= 1;
                }
            }
            match parent {
                Some(p) => cur = p,
                None => return Ok(DynRef::Static),
            }
        }
    }

    /// The value of the name of an [`Expr::Dynamic`] whose static binding is `x`.
    #[cfg(feature = "interp")]
    fn dyn_value(&mut self, name: &str, withs: u32, x: &Expr, sc: &Scope) -> R<Value> {
        match self.dyn_ref(sc, name, withs, static_hops(x))? {
            DynRef::With(o) => self.get(&o, name),
            DynRef::Frame(fr) => {
                let v = fr.borrow().vars.get(name).cloned();
                match v {
                    Some(Value::Empty) => self.uninitialized(name),
                    Some(v) => Ok(v),
                    None => self.eval(x, sc),
                }
            }
            DynRef::Static => self.eval(x, sc),
        }
    }

    /// The variable environment of the code running in `sc` (ECMA-262 §9.4): the frame of
    /// the function called, or that of the top-level code. A generator's step function
    /// runs its generator function's code: that function's frame is the one.
    #[cfg(feature = "interp")]
    fn var_env(&self, sc: &Scope) -> Scope {
        let mut cur = sc.clone();
        loop {
            let parent = {
                let f = cur.borrow();
                let top = self.code.as_ref().is_some_and(|c| Rc::ptr_eq(c, &cur));
                let step = f.fun && f.gen_step;
                if (f.fun && !step) || top {
                    None
                } else {
                    f.parent.clone()
                }
            };
            match parent {
                Some(p) => cur = p,
                None => return cur,
            }
        }
    }

    /// A direct eval (PerformEval, ECMA-262 §19.2.1.1 with `direct` true) of `src`, called
    /// in `sc` by code whose scopes are `scope`: the code sees the caller's bindings; the
    /// `var` and function declarations of non-strict code go to the caller's variable
    /// environment (EvalDeclarationInstantiation, §19.2.1.3), its lexical ones, and all
    /// those of strict code, to a new scope.
    #[cfg(feature = "interp")]
    fn direct_eval(&mut self, src: &str, scope: &crate::ir::EvalScope, sc: &Scope) -> R<Value> {
        let code = match crate::ir::lower_direct_eval(src, scope) {
            Ok(c) => c,
            Err(e) => return self.throw("SyntaxError", e),
        };
        let frame = self.enter()?;
        let lex = Self::child(sc);
        // QV4's eval code reads the `this` of the frame that called it, and an arrow
        // function's frame holds the `this` of its own call ([`Frame::arrow_this`]), the
        // global object for none.
        if let Some(t) = Self::arrow_this(sc) {
            lex.borrow_mut().this = Some(self.sloppy_this(&t).unwrap_or(t));
        }
        let caller = std::mem::replace(&mut self.strict, code.strict);
        let r = on_js_stack(|| {
            let body = &code.body[..];
            if code.strict {
                self.hoist_vars(body, &lex);
                self.hoist_funcs(body, &lex);
            } else {
                let vars = self.var_env(sc);
                for n in &code.vars {
                    let mut f = vars.borrow_mut();
                    if !f.vars.contains_key(n) {
                        f.vars.insert(n.clone(), Value::Undefined);
                        f.eval_vars.push(n.clone());
                    }
                }
                self.hoist_lexical(body, &lex);
                for s in body {
                    if let Stmt::Func(f) = s
                        && let Some(n) = &f.name
                    {
                        let fv = fn_value(Callable::Closure(f.clone(), lex.clone()));
                        match code.slots.get(n) {
                            Some((h, i)) => Self::slot_set(&lex, *h, *i, fv),
                            None => {
                                vars.borrow_mut().vars.insert(n.clone(), fv);
                            }
                        }
                    }
                }
            }
            match self.exec_stmts(body, &lex)? {
                Flow::Return(v) => Ok(v),
                _ => Ok(Value::Undefined),
            }
        });
        self.strict = caller;
        self.leave(frame, r)
    }

    pub fn find_var(sc: &Scope, name: &str) -> Option<Value> {
        let f = sc.borrow();
        if !f.vars.is_empty()
            && let Some(v) = f.vars.get(name)
        {
            return Some(v.clone());
        }
        Self::find_var(f.parent.as_ref()?, name)
    }

    /// Whether the binding [`Self::find_var`] finds for `name` is a property of the global
    /// object rather than a variable of a function or a block.
    pub fn is_global_var(sc: &Scope, name: &str) -> bool {
        let f = sc.borrow();
        if f.vars.contains_key(name) {
            return f.global;
        }
        f.parent
            .as_ref()
            .is_some_and(|p| Self::is_global_var(p, name))
    }

    /// `host.lookup`, first creating the lazy singleton a miss names (Qt creates singletons
    /// on first use). Every free-name read (interpreted or compiled) goes through this.
    pub fn host_lookup(&mut self, scope: Option<(usize, usize)>, name: &str) -> Option<Value> {
        if let Some(v) = self.host.lookup(scope, name) {
            return Some(v);
        }
        let load = self.host.take_lazy()?;
        load(self);
        self.host.lookup(scope, name)
    }

    /// Whether the free name `name` is a singleton nothing has created yet. `typeof Name`
    /// asks this instead of reading the name: in Qt the name is the type's wrapper, an object,
    /// and the singleton is created by the first read of a member.
    pub fn names_uncreated_singleton(&mut self, scope: Option<(usize, usize)>, name: &str) -> bool {
        self.host.lookup(scope, name).is_none() && self.host.lazy_missed(name)
    }

    /// [`Self::host_lookup`] for an interned name.
    pub fn host_lookup_sym(
        &mut self,
        scope: Option<(usize, usize)>,
        name: crate::runtime::Sym,
    ) -> Option<Value> {
        if let Some(v) = self.host.lookup_sym(scope, name) {
            return Some(v);
        }
        let load = self.host.take_lazy()?;
        load(self);
        self.host.lookup_sym(scope, name)
    }

    /// [`Self::host_lookup_sym`] through [`Host::lookup_scoped`].
    pub fn host_lookup_scoped(
        &mut self,
        scope: Option<(usize, usize)>,
        name: crate::runtime::Sym,
    ) -> Option<(Value, bool)> {
        if let Some(r) = self.host.lookup_scoped(scope, name) {
            return Some(r);
        }
        let load = self.host.take_lazy()?;
        load(self);
        self.host.lookup_scoped(scope, name)
    }

    pub fn lookup(&mut self, sc: &Scope, name: &str) -> R<Value> {
        if let Some(v) = Self::find_var(sc, name) {
            if let Value::Empty = v {
                return self.uninitialized(name);
            }
            return Ok(v);
        }
        if let Some(v) = self.host_lookup(Self::qml_scope(sc), name) {
            return Ok(v);
        }
        match name {
            "undefined" => Ok(Value::Undefined),
            "NaN" => Ok(Value::Num(f64::NAN)),
            "Infinity" => Ok(Value::Num(f64::INFINITY)),
            _ => match self.global_binding(name)? {
                Some(v) => Ok(v),
                None => self.throw("ReferenceError", format!("{name} is not defined")),
            },
        }
    }

    /// QML scope (item, context) of the nearest binding/function frame up the chain.
    pub fn qml_scope(sc: &Scope) -> Option<(usize, usize)> {
        let f = sc.borrow();
        match (f.qml, &f.parent) {
            (Some(q), _) => Some(q),
            (None, Some(p)) => Self::qml_scope(p),
            (None, None) => None,
        }
    }

    /// The class body enclosing `sc` (for `super`).
    #[cfg(feature = "interp")]
    fn class_scope(sc: &Scope) -> Option<Rc<ClassScope>> {
        let f = sc.borrow();
        match (&f.class, &f.parent) {
            (Some(c), _) => Some(c.clone()),
            (None, Some(p)) => Self::class_scope(p),
            (None, None) => None,
        }
    }

    /// The [[HomeObject]] of the method whose code `sc` is in (undefined outside one).
    #[cfg(feature = "interp")]
    fn home_object(sc: &Scope) -> Value {
        let f = sc.borrow();
        match (&f.home, &f.parent) {
            (Some(h), _) => h.clone(),
            (None, Some(p)) => Self::home_object(p),
            (None, None) => Value::Undefined,
        }
    }

    /// GetSuperBase (ECMA-262 §9.1.1.3.5): the prototype `home` has now, which `super.x`
    /// reads from; undefined without a home object.
    pub fn super_base(&self, home: &Value) -> Value {
        match home {
            Value::Obj(o) => self.proto_of(o).map_or(Value::Null, Value::Obj),
            _ => Value::Undefined,
        }
    }

    /// The call of the method `f` read as `super[k]` (an optional call ends the chain on a
    /// nullish `f`).
    pub fn call_super_method(
        &mut self,
        f: &Value,
        this: &Value,
        k: &PropKey,
        argv: &[Value],
        optional: bool,
    ) -> R<Value> {
        if optional && matches!(f, Value::Undefined | Value::Null) {
            return Err(Ex::Short);
        }
        if !matches!(f, Value::Obj(o) if o.borrow().call.is_some()) {
            let (k, o) = (k.describe(), self.shown(this));
            return self.throw(
                "TypeError",
                format!("Property '{k}' of object {o} is not a function"),
            );
        }
        self.call(f, this.clone(), argv)
    }

    /// `super[k]` (MakeSuperPropertyReference, §13.3.7.3): [[Get]] of `k` on the super base
    /// with `this` as the receiver of a getter.
    pub fn super_get(&mut self, home: &Value, this: &Value, k: &PropKey) -> R<Value> {
        let base = self.super_base(home);
        self.super_get_from(base, this, k)
    }

    /// GetValue of a Super Reference with base `base` (ECMA-262 §6.2.5.5).
    pub fn super_get_from(&mut self, base: Value, this: &Value, k: &PropKey) -> R<Value> {
        match (base, k) {
            (Value::Obj(o), PropKey::Str(n)) => self.get_from(&o, n, this),
            (Value::Obj(o), PropKey::Sym(sy)) => self.get_sym_from(Some(o), sy, this),
            (b, k) => self.get_key(&b, &k.to_value()),
        }
    }

    /// PutValue of a Super Reference (ECMA-262 §6.2.5.6): [[Set]] of key value `k` on the super
    /// base `base` with `this` as the receiver; a refused write throws in strict code.
    pub fn super_put(
        &mut self,
        base: &Value,
        this: &Value,
        k: &Value,
        v: Value,
        strict: bool,
    ) -> R<()> {
        if let Value::Undefined | Value::Null = base {
            return self.nullish_write(base);
        }
        let k = self.reference_key(base, k)?;
        if crate::props::set_with_receiver_pk(self, base, &k, v, this)? {
            return Ok(());
        }
        self.refuse_set(strict).map(drop)
    }

    /// Overwrite `name` where it is declared in the chain; hands `v` back when it is not, and
    /// for a frozen builtin of the global object.
    pub fn store_var(sc: &Scope, name: &str, v: Value) -> Stored {
        let mut f = sc.borrow_mut();
        if !(f.global && crate::builtins::is_frozen_global(name))
            && let Some(slot) = f.vars.get_mut(name)
        {
            if let Value::Empty = slot {
                return Stored::Uninitialized;
            }
            *slot = v;
            return Stored::Done;
        }
        match &f.parent {
            Some(p) => Self::store_var(p, name, v),
            None => Stored::Undeclared(v),
        }
    }

    /// `name = v` for a free name: its declaration in the chain, else the QML scope's property.
    /// QML code cannot create globals nor overwrite the frozen global object (Qt docs,
    /// "JavaScript Environment Restrictions"), so any other name throws like QV4's
    /// `QQmlContextWrapper::virtualPut`.
    pub fn assign_var(&mut self, sc: &Scope, name: &str, v: Value) -> R<()> {
        let v = match Self::store_var(sc, name, v) {
            Stored::Done => return Ok(()),
            Stored::Uninitialized => return self.uninitialized(name),
            Stored::Undeclared(v) => v,
        };
        if let Some((item, ctx)) = Self::qml_scope(sc)
            && self.host_assign(item, ctx, name, v)?
        {
            return Ok(());
        }
        self.invalid_global_write(name)
    }

    /// `host.assign` of a free-name write, first converting `v` to the target property's type
    /// (QQmlPropertyPrivate::write: a value type is copied, an incompatible value throws).
    pub fn host_assign(&mut self, item: usize, ctx: usize, name: &str, v: Value) -> R<bool> {
        let v = match self.host.assign_slot(item, ctx, name) {
            Some(slot) if !matches!(v, Value::Undefined) => {
                crate::valuetypes::qobject_wrapper_set_property(self, &slot, v)?
            }
            _ => v,
        };
        match self.host.assign(item, ctx, name, v) {
            Ok(done) => Ok(done),
            Err(e) => self.throw(e.kind, e.message),
        }
    }

    /// Access to a `let`/`const`/`class` binding before its declaration ran (ECMA-262
    /// GetBindingValue / SetMutableBinding on an uninitialized binding); QV4's message.
    #[cold]
    #[inline(never)]
    pub fn uninitialized<T>(&self, name: &str) -> R<T> {
        self.throw("ReferenceError", format!("{name} is not defined"))
    }

    /// A write to an initialized immutable binding (ECMA-262 SetMutableBinding: `TypeError`, in
    /// sloppy code too). QV4 gives it no message.
    #[cold]
    #[inline(never)]
    pub fn const_write<T>(&self) -> R<T> {
        self.throw("TypeError", "")
    }

    /// QV4's error for a write to a name declared nowhere.
    #[cold]
    #[inline(never)]
    pub fn invalid_global_write<T>(&self, name: &str) -> R<T> {
        self.throw(
            "Error",
            format!("Invalid write to global property \"{name}\""),
        )
    }

    /// QML sequence properties write back on in-place mutation (`root.list.push(x)` notifies `list`).
    #[cfg(feature = "interp")]
    fn write_back(&mut self, o: &Expr, ov: &Value, sc: &Scope) -> R<()> {
        if !matches!(ov, Value::Obj(x) if x.borrow().array.is_some()) {
            return Ok(());
        }
        match o {
            Expr::Member(b, p, _)
                if matches!(**b, Expr::Ident(_) | Expr::Local(..) | Expr::This) =>
            {
                if let Value::Item(i) = self.eval(b, sc)? {
                    self.host.sequence_written(i, p, ov);
                }
            }
            Expr::Ident(n) if Self::find_var(sc, n).is_none() => {
                if let Some((item, ctx)) = Self::qml_scope(sc) {
                    self.host.write_back(item, ctx, n, ov.clone());
                }
            }
            _ => {
                let owner = sequence_owner(ov);
                if let Some((i, p)) = owner {
                    self.host.sequence_changed(i, &p, ov);
                }
            }
        }
        Ok(())
    }

    /// The frame holding the `this` binding `sc` sees (GetThisEnvironment).
    #[cfg(feature = "interp")]
    fn this_frame(sc: &Scope) -> Option<Scope> {
        if sc.borrow().this.is_some() {
            return Some(sc.clone());
        }
        let p = sc.borrow().parent.clone();
        p.as_ref().and_then(Self::this_frame)
    }

    /// ResolveThisBinding: a derived constructor's `this` before `super(...)` is a
    /// ReferenceError (ECMA-262 §9.1.1.3.4 GetThisBinding), which QV4 words with an empty
    /// name. In an arrow function of a derived constructor QV4's `this` is a ReferenceError
    /// named `this`, before `super(...)` and after it ([`Frame::derived`]).
    #[cfg(feature = "interp")]
    fn this_binding(&mut self, sc: &Scope) -> R<Value> {
        if Self::this_is_own(sc) {
            return match Self::this(sc) {
                Value::Empty => self.uninitialized(""),
                v => Ok(v),
            };
        }
        match Self::this_frame(sc) {
            Some(f) if f.borrow().derived => self.uninitialized("this"),
            _ => self.this_bound(Self::this(sc)),
        }
    }

    /// The `this` the arrow function `sc` is in was called with ([`Frame::arrow_this`]);
    /// `None` in a function with its own `this`.
    #[cfg(feature = "interp")]
    fn arrow_this(sc: &Scope) -> Option<Value> {
        let f = sc.borrow();
        match (&f.arrow_this, &f.this, &f.parent) {
            (Some(t), ..) => Some(t.clone()),
            (None, Some(_), _) | (None, None, None) => None,
            (None, None, Some(p)) => Self::arrow_this(p),
        }
    }

    /// Whether the function `sc` is in has its own `this` binding (it is no arrow function).
    #[cfg(feature = "interp")]
    fn this_is_own(sc: &Scope) -> bool {
        let f = sc.borrow();
        match (&f.this, &f.parent) {
            (Some(_), _) | (None, None) => true,
            (None, Some(_)) if f.fun => false,
            (None, Some(p)) => Self::this_is_own(p),
        }
    }

    /// Whether `e` is the `this` of a derived constructor that has not called `super(...)`,
    /// read by the constructor itself (an arrow function there reads it checked). QV4 reads
    /// that `this` unchecked where it is the base of a store, a call or a delete.
    #[cfg(feature = "interp")]
    fn unbound_this(e: &Expr, sc: &Scope) -> bool {
        matches!(e, Expr::This) && matches!(Self::this(sc), Value::Empty) && Self::this_is_own(sc)
    }

    /// The `this` binding for a `super.x` reference: QV4's ReferenceError in a derived
    /// constructor before `super(...)`. In an arrow function QV4 takes the `this` of the
    /// arrow's own call ([`Frame::arrow_this`]).
    #[cfg(feature = "interp")]
    fn super_this(&mut self, sc: &Scope) -> R<Value> {
        if let Some(t) = Self::arrow_this(sc) {
            return Ok(t);
        }
        match Self::this(sc) {
            Value::Empty => self.throw("ReferenceError", "Missing call to super()."),
            v => Ok(v),
        }
    }

    /// `this` read from a binding that may be uninitialized (`Value::Empty`).
    pub fn this_bound(&mut self, this: Value) -> R<Value> {
        match this {
            Value::Empty => self.uninitialized("this"),
            v => Ok(v),
        }
    }

    /// BindThisValue for `super(...)`: the binding (`bound`) must still be uninitialized.
    pub fn bind_this(&mut self, bound: &Value) -> R<()> {
        match bound {
            Value::Empty => Ok(()),
            _ => self.throw("ReferenceError", "super() already called."),
        }
    }

    /// Result of a derived class constructor's [[Construct]] (ECMA-262 §10.2.2 steps 10-12):
    /// its returned object, else its `this` binding; a returned non-undefined primitive is a
    /// TypeError and an uninitialized `this` (no `super(...)` ran) a ReferenceError.
    pub fn derived_result(&mut self, r: Value, this: Value) -> R<Value> {
        match r {
            Value::Obj(_) => Ok(r),
            Value::Undefined => self.this_bound(this),
            _ => self.throw("TypeError", "Type error"),
        }
    }

    pub fn this(sc: &Scope) -> Value {
        let f = sc.borrow();
        if let Some(t) = &f.this {
            return t.clone();
        }
        match &f.parent {
            Some(p) => Self::this(p),
            None => Value::Undefined,
        }
    }

    // ---------- property access ----------

    /// Where a symbol-keyed lookup on `v` starts: the object, or the prototype a primitive
    /// inherits from. `None` for QML objects and colours, which have no symbol properties.
    fn sym_holder(&mut self, v: &Value) -> Option<Obj> {
        Some(match v {
            Value::Obj(o) => o.clone(),
            Value::Item(i) => return self.host.extras(*i, false),
            Value::Str(_) | Value::Str16(_) => self.string_proto.clone(),
            Value::Num(_) => self.number_proto.clone(),
            Value::Bool(_) => self.boolean_proto.clone(),
            Value::Symbol(_) => self.sym.symbol_proto.clone(),
            _ => return None,
        })
    }

    /// `v[s]` for a symbol `s` (OrdinaryGet, ECMA-262 §10.1.8.1).
    pub fn get_sym(&mut self, v: &Value, s: &Rc<JsSymbol>) -> R<Value> {
        if matches!(v, Value::Undefined | Value::Null) {
            return self.throw(
                "TypeError",
                format!(
                    "Cannot read property '{}' of {}",
                    s.descriptive(),
                    v.to_js_string()
                ),
            );
        }
        let holder = self.sym_holder(v);
        self.get_sym_from(holder, s, v)
    }

    /// [[Get]] of symbol `s` from `start` on, with `receiver` as the `this` of a getter; a
    /// proxy on the chain takes over with its [[Get]] (§10.5.8).
    pub fn get_sym_from(
        &mut self,
        start: Option<Obj>,
        s: &Rc<JsSymbol>,
        receiver: &Value,
    ) -> R<Value> {
        let mut cur = start;
        while let Some(o) = cur {
            if o.borrow().is_proxy() {
                return crate::proxy::get(self, &o, &PropKey::Sym(s.clone()), receiver);
            }
            let found = sym_own(&o.borrow(), s).map(|p| (p.value.clone(), p.accessor.clone()));
            match found {
                Some((_, Some(a))) => {
                    return match a.get {
                        Some(g) => self.call_getter(&g, receiver),
                        None => Ok(Value::Undefined),
                    };
                }
                Some((x, None)) => return Ok(x),
                None => cur = self.proto_of(&o),
            }
        }
        Ok(Value::Undefined)
    }

    /// `v[s] = val` for a symbol `s`, throwing where it is refused in strict mode code.
    /// ToObject of `undefined` or `null` (ECMA-262 §7.1.18), in V4's words.
    pub fn not_an_object<T>(&mut self, v: &Value) -> R<T> {
        let v = v.to_js_string();
        self.throw(
            "TypeError",
            format!("Value is {v} and could not be converted to an object"),
        )
    }

    /// A write to a property of `undefined` or `null`: V4 names the object it could not make
    /// of the base, and in strict mode code only that the write was refused.
    fn nullish_write<T>(&mut self, base: &Value) -> R<T> {
        if self.strict {
            return self.throw("TypeError", "Type error");
        }
        self.not_an_object(base)
    }

    /// `base.name(...)` on `undefined` or `null`, which V4 words by the call outside strict
    /// mode code and by the read of the method inside it ([`Interp::get`]).
    pub fn nullish_call(&mut self, base: &Value, name: &str) -> R<()> {
        if self.strict || !matches!(base, Value::Undefined | Value::Null) {
            return Ok(());
        }
        let b = base.to_js_string();
        self.throw("TypeError", format!("Cannot call method '{name}' of {b}"))
    }

    pub fn put_sym(&mut self, v: &Value, s: &Rc<JsSymbol>, val: Value) -> R<()> {
        self.set_sym(v, s, val, self.strict).map(drop)
    }

    /// [[Set]] of symbol `s` (OrdinarySet, ECMA-262 §10.1.9.2): an inherited setter runs, a
    /// read-only property or a non-extensible receiver refuses (a TypeError if `throw`),
    /// otherwise the receiver gets an own data property.
    pub fn set_sym(&mut self, v: &Value, s: &Rc<JsSymbol>, val: Value, throw: bool) -> R<bool> {
        if matches!(v, Value::Undefined | Value::Null) {
            return self.nullish_write(v);
        }
        let k = PropKey::Sym(s.clone());
        if crate::props::set_with_receiver_pk(self, v, &k, val, v)? {
            return Ok(true);
        }
        if !throw {
            return Ok(false);
        }
        self.type_error()
    }

    /// HasProperty with a symbol key (the `in` operator); a proxy on the chain answers with
    /// its [[HasProperty]] (§10.5.7).
    pub fn has_sym(&mut self, o: &Obj, s: &Rc<JsSymbol>) -> R<bool> {
        let mut cur = Some(o.clone());
        while let Some(o) = cur {
            if o.borrow().is_proxy() {
                return crate::proxy::has(self, &o, &PropKey::Sym(s.clone()));
            }
            if sym_own(&o.borrow(), s).is_some() {
                return Ok(true);
            }
            cur = self.proto_of(&o);
        }
        Ok(false)
    }

    /// The enumerable own symbol-keyed properties of `v` with their values, in creation
    /// order (the symbol part of CopyDataProperties and Object.assign).
    pub fn own_symbol_entries(&mut self, v: &Value) -> R<Vec<(Rc<JsSymbol>, Value)>> {
        let Value::Obj(o) = v else {
            return Ok(Vec::new());
        };
        if o.borrow().is_proxy() {
            let mut out = Vec::new();
            for k in crate::props::own_property_keys(self, v)? {
                if let PropKey::Sym(s) = &k
                    && crate::props::own_desc_pk(self, v, &k)?
                        .is_some_and(|d| d.enumerable == Some(true))
                {
                    let x = self.get_sym(v, s)?;
                    out.push((s.clone(), x));
                }
            }
            return Ok(out);
        }
        let keys: Vec<Rc<JsSymbol>> = o
            .borrow()
            .ext
            .iter()
            .flat_map(|e| &e.syms)
            .filter(|p| p.enumerable)
            .map(|p| p.key.clone())
            .collect();
        let mut out = Vec::with_capacity(keys.len());
        for s in keys {
            let x = self.get_sym(v, &s)?;
            out.push((s, x));
        }
        Ok(out)
    }

    /// The key of reference `base[k]` as GetValue/PutValue/`delete` use it (ECMA-262
    /// §6.2.5.5): ToPropertyKey of `k`, except that a nullish `base` throws first (ToObject)
    /// in the operation the caller runs next, so `k`'s conversion must not run for it.
    pub fn reference_key(&mut self, base: &Value, k: &Value) -> R<PropKey> {
        match (base, k) {
            (Value::Undefined | Value::Null, _)
            | (_, Value::Symbol(_) | Value::Str(_) | Value::Str16(_)) => Ok(PropKey::of(k)),
            _ => self.to_property_key(k),
        }
    }

    /// Converts the key of reference `base[k]` in place ([`Interp::reference_key`]):
    /// GetValue stores the property key back into the reference, so `o[k] += v` runs
    /// ToPropertyKey once, before `v`.
    pub fn convert_key(&mut self, base: &Value, k: &mut Value) -> R<()> {
        if let Value::Obj(_) = k {
            *k = self.reference_key(base, k)?.to_value();
        }
        Ok(())
    }

    /// The callee key of `base[k](..)`: GetValue of the callee runs before the arguments are
    /// evaluated, so a nullish `base` throws and `k` converts at this point.
    pub fn callee_key(&mut self, base: &Value, mut k: Value) -> R<Value> {
        if let Value::Undefined | Value::Null = base {
            self.get_key(base, &k)?;
        }
        self.convert_key(base, &mut k)?;
        Ok(k)
    }

    /// `v[k]` for a property key value `k` (GetValue: ToPropertyKey of `k`).
    pub fn get_key(&mut self, v: &Value, k: &Value) -> R<Value> {
        match k {
            Value::Symbol(s) => self.get_sym(v, s),
            Value::Obj(_) => {
                let k = self.reference_key(v, k)?;
                self.get_pk(v, &k)
            }
            k => self.get(v, &k.to_js_string()),
        }
    }

    /// QV4's `o[k] = v` (`Runtime::StoreElement`, qv4runtime.cpp) for an array index `k`:
    /// a cell of an array's storage is written without a look at the element's attributes
    /// or at whether the array can be extended, so an element of a frozen array is
    /// overwritten and a hole inside the storage is filled; a read-only indexed data
    /// property of another object is overwritten too. `Reflect.set`, `defineProperty` and
    /// the array methods take the checked path. Whether it stored.
    fn store_element(o: &Value, k: &Value, v: &Value) -> bool {
        let Value::Obj(o) = o else { return false };
        let b = &mut *o.borrow_mut();
        if !b.restricted() || !matches!(b.internal, Internal::Ordinary) || b.is_proxy() {
            return false;
        }
        // A QV4::Sequence is no ArrayObject: its `virtualPut` answers.
        if crate::valuetypes::is_readonly_sequence(b) {
            return false;
        }
        let i = match k {
            Value::Num(n) if *n >= 0.0 && n.fract() == 0.0 && *n < MAX_ARRAY_INDEX as f64 => {
                *n as usize
            }
            Value::Str(s) => match array_index(s) {
                Some(i) => i,
                None => return false,
            },
            _ => return false,
        };
        let key = i.to_string();
        if b.accessors.as_ref().is_some_and(|m| m.contains_key(&*key)) {
            return false;
        }
        if b.array.is_some() {
            if b.sparse() || b.owner.is_some() || i >= b.slots() {
                return false;
            }
            // An element held as an ordinary property (not enumerable).
            if let Some(e) = b.props.get_mut(&*key) {
                *e = v.clone();
                return true;
            }
            if let Some(e) = b.array.as_mut().and_then(|a| a.get_mut(i)) {
                *e = v.clone();
            }
            if b.is_hole(i) {
                b.fill_hole(i);
                b.stored_without_byte(i);
            }
            return true;
        }
        if b.attr(&key) & crate::value::READONLY == 0 {
            return false;
        }
        match b.props.get_mut(&*key) {
            Some(e) => {
                *e = v.clone();
                true
            }
            None => false,
        }
    }

    /// `v[k] = val` for a property key value `k` (PutValue: ToPropertyKey of `k`).
    pub fn put_key(&mut self, v: &Value, k: &Value, val: Value) -> R<()> {
        if Self::store_element(v, k, &val) {
            return Ok(());
        }
        match k {
            Value::Symbol(s) => self.put_sym(v, s, val),
            Value::Obj(_) => {
                let k = self.reference_key(v, k)?;
                self.put_pk(v, &k, val)
            }
            k => self.put(v, &k.to_js_string(), val),
        }
    }

    pub fn get(&mut self, v: &Value, k: &str) -> R<Value> {
        match v {
            Value::Obj(o) if crate::generator::is_scope(o) => {
                crate::generator::scope_get(self, o, k)
            }
            Value::Obj(o) => self.get_from(o, k, v),
            Value::Str(s) => {
                if k == "length" {
                    return Ok(Value::Num(crate::utf16::len(s) as f64));
                }
                if let Some(i) = idx(k) {
                    return Ok(if crate::utf16::is_ascii(s) {
                        s.get(i..i + 1).map(Value::str).unwrap_or_default()
                    } else {
                        crate::utf16::unit(s, i)
                            .map(|c| Value::from_utf16(&[c]))
                            .unwrap_or_default()
                    });
                }
                self.primitive_inherited(v, k)
            }
            Value::Str16(u) => {
                if k == "length" {
                    return Ok(Value::Num(u.len() as f64));
                }
                if let Some(i) = idx(k) {
                    return Ok(u
                        .get(i)
                        .map(|c| Value::from_utf16(&[*c]))
                        .unwrap_or_default());
                }
                self.primitive_inherited(v, k)
            }
            Value::Num(_) | Value::Bool(_) => self.primitive_inherited(v, k),
            Value::Symbol(_) => self.symbol_inherited(v, k),
            Value::Color(c) if crate::valuetypes::is_color_own(k) => {
                Ok(crate::builtins::color_prop(c, k))
            }
            // What a color inherits from the value types' prototype and Object.prototype.
            Value::Color(_) => {
                let proto = self.vt_protos.types[0].clone();
                self.get_from(&proto, k, v)
            }
            // A key that is no name yet is not interned for the lookup ([`Host::get`]).
            Value::Item(id) => match self.host.get(*id, k) {
                Some(v) => Ok(v),
                None => self.item_inherited(*id, k),
            },
            Value::Undefined | Value::Null => self.throw(
                "TypeError",
                format!("Cannot read property '{k}' of {}", v.to_js_string()),
            ),
            _ => Ok(Value::Undefined),
        }
    }

    /// [[Get]] of `name` on QML object `id`: its property or signal, else its method, else
    /// what %Object.prototype% gives it, which V4's QObjectWrapper inherits.
    #[inline]
    pub fn item_get(&mut self, id: usize, name: crate::runtime::Sym) -> R<Value> {
        match self.host.get_sym(id, name) {
            Some(v) => Ok(v),
            None => self.item_inherited(id, &name),
        }
    }

    #[inline(never)]
    fn item_inherited(&mut self, id: usize, k: &str) -> R<Value> {
        if k.starts_with(char::is_uppercase)
            && let Some((_, ctx)) = self.calling_qml_scope()
            && let Some(v) = self.host.qualifier(id, ctx, k)
        {
            return Ok(v);
        }
        if self.host.has_method(id, k) {
            return Ok(fn_value(Callable::Item(
                id,
                crate::runtime::Sym::new(k).as_str(),
            )));
        }
        let holder = self.item_holder(&Value::Item(id));
        self.get_from(&holder, k, &Value::Item(id))
    }

    /// [[HasProperty]] of `k` on QML object `id`: a member of its own
    /// ([`Host::has_member`]) or a property of %Object.prototype%.
    pub fn item_has(&mut self, id: usize, k: &str) -> R<bool> {
        if self.host.has_member(id, k) {
            return Ok(true);
        }
        let holder = self.item_holder(&Value::Item(id));
        self.has_property(&holder, k)
    }

    /// OrdinaryGet (ECMA-262 §10.1.8.1) of `k` from `o` on, with `receiver` as the `this` of a
    /// getter; a proxy on the chain takes over with its [[Get]] (§10.5.8).
    pub fn get_from(&mut self, o: &Obj, k: &str, receiver: &Value) -> R<Value> {
        let mut cur = Some(o.clone());
        let mut first = true;
        while let Some(ob) = cur {
            let acc;
            {
                let b = ob.borrow();
                if b.is_proxy() {
                    drop(b);
                    return crate::proxy::get(self, &ob, &PropKey::Str(k.into()), receiver);
                }
                // An array anywhere on the chain (%Array.prototype% is one) holds its own
                // `length` and elements.
                if let Some(a) = &b.array {
                    if k == "length" {
                        return Ok(Value::Num(b.array_length() as f64));
                    }
                    // Only a present element is read here: a hole or an index past the end
                    // goes on to the own properties and the prototype chain (§10.1.8.1).
                    if let Some(i) = idx(k).filter(|&i| i < a.len() && !b.is_hole(i)) {
                        let e = a[i].clone();
                        // Elements of a QML sequence property write back through it.
                        if let (true, Some(own), Value::Obj(eo)) = (first, &b.owner, &e)
                            && let Ok(mut eb) = eo.try_borrow_mut()
                            && eb.array.is_none()
                            && eb.call.is_none()
                        {
                            eb.owner = Some(own.clone());
                        }
                        return Ok(e);
                    }
                }
                if first
                    && let Internal::TypedArray(t) = &b.internal
                    && let Some(e) = crate::typedarray::get_index(t, k)
                {
                    return Ok(e);
                }
                // The Easing type reads its own names, then %Object.prototype%: what lies
                // between them answers `in` only.
                if first && b.native.is_some() && crate::builtins::is_easing(&b) {
                    if k == "toString" {
                        return Ok(crate::valuetypes::bound_to(
                            &ob,
                            crate::builtins::easing_to_string(),
                        ));
                    }
                    if let Some(p) = b.props.get(k) {
                        return Ok(p.clone());
                    }
                    cur = Some(self.object_proto.clone());
                    first = false;
                    continue;
                }
                if let Some(p) = b.props.get(k) {
                    return Ok(p.clone());
                }
                if let Some(v) = intrinsic(&b, k) {
                    return Ok(v);
                }
                if first && let Internal::ValueType(vt) = b.internal {
                    if let Some(m) = crate::valuetypes::method(&ob, vt, k) {
                        return Ok(m);
                    }
                    let members = b.ext.as_ref().and_then(|e| e.members.clone());
                    if let Some(m) = members.filter(|m| has_own(m, k)) {
                        drop(b);
                        return self.get(&Value::Obj(m), k);
                    }
                }
                if first
                    && matches!(b.internal, Internal::Locale(..))
                    && let Some(m) = self.vt_protos.locale_method(&ob, k)
                {
                    return Ok(m);
                }
                if first
                    && k == "prototype"
                    && b.call.as_ref().is_some_and(Callable::owns_prototype)
                {
                    drop(b);
                    return Ok(fn_prototype(&ob).map_or(Value::Undefined, Value::Obj));
                }
                acc = own_accessor(&b, k).map(|a| a.get.clone());
                // Past the receiver's own prototype chain: its built-in prototype
                // (%Array.prototype%, %Function.prototype%, a Map's, Set's or RegExp's), then
                // %Object.prototype%, which also has what was deleted from the former.
                cur = match &b.proto {
                    Some(p) => Some(p.clone()),
                    None if b.null_proto || Rc::ptr_eq(&ob, &self.object_proto) => None,
                    None => Some(
                        self.builtin_proto(&ob, &b)
                            .unwrap_or_else(|| self.object_proto.clone()),
                    ),
                };
            }
            match acc {
                Some(Some(g)) => return self.call_getter(&g, receiver),
                Some(None) => return Ok(Value::Undefined),
                None => {}
            }
            first = false;
        }
        Ok(Value::Undefined)
    }

    /// [[HasProperty]] (OrdinaryHasProperty, ECMA-262 §10.1.7.1) of `k` on `o`: own, then up
    /// the prototype chain; a proxy on the chain answers with its own (§10.5.7).
    pub fn has_property(&mut self, o: &Obj, k: &str) -> R<bool> {
        let mut cur = Some(o.clone());
        while let Some(ob) = cur {
            if ob.borrow().is_proxy() {
                return crate::proxy::has(self, &ob, &PropKey::Str(k.into()));
            }
            if has_own(&ob, k) {
                return Ok(true);
            }
            cur = self.proto_of(&ob);
        }
        Ok(false)
    }

    /// Call getter `g` on `this`; a `this_only` body is evaluated without a call frame.
    pub(crate) fn call_getter(&mut self, g: &Value, this: &Value) -> R<Value> {
        #[cfg(feature = "interp")]
        {
            static PROF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            let func = match g {
                Value::Obj(o) => match &o.borrow().call {
                    #[cfg(feature = "interp")]
                    Some(Callable::Closure(f, _)) if f.this_only => Some(f.clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(func) = func
                && !*PROF.get_or_init(|| crate::log_on!("ri::jsprof"))
                && let [Stmt::Return(Some(e))] = &func.body[..]
            {
                // A JS frame like `call`'s (depth limit, stack segment), minus the scope.
                let frame = self.enter()?;
                // The ticks `call` and the `return` would count.
                let r = self
                    .tick()
                    .and_then(|()| self.tick())
                    .and_then(|()| on_js_stack(|| self.eval_this(e, this)));
                return self.leave(frame, r);
            }
        }
        self.call(g, this.clone(), &[])
    }

    /// Evaluate a [`crate::ir::this_only`] expression with `this` bound.
    #[cfg(feature = "interp")]
    fn eval_this(&mut self, e: &Expr, this: &Value) -> R<Value> {
        Ok(match e {
            Expr::This => this.clone(),
            Expr::Num(n) => Value::Num(*n),
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Str16(u) => Value::Str16(u.clone()),
            Expr::Member(o, n, _) => {
                let ov = self.eval_this(o, this)?;
                self.get(&ov, n)?
            }
            Expr::Index(o, k, _) => {
                let ov = self.eval_this(o, this)?;
                let kv = self.eval_this(k, this)?;
                if let (Value::Obj(ob), Value::Num(n)) = (&ov, &kv)
                    && *n >= 0.0
                    && *n < MAX_ARRAY_INDEX as f64
                    && n.fract() == 0.0
                    && let Some(e) = ob.borrow().dense_element(*n as usize)
                {
                    return Ok(e);
                }
                self.get_key(&ov, &kv)?
            }
            Expr::Binary(op, a, b) => {
                let x = self.eval_this(a, this)?;
                let y = self.eval_this(b, this)?;
                if let (Value::Num(p), Value::Num(q)) = (&x, &y)
                    && let Some(r) = num_binary(*op, *p, *q)
                {
                    r
                } else {
                    self.binary(*op, x, y)?
                }
            }
            _ => unreachable!("not this_only"),
        })
    }

    pub fn put(&mut self, v: &Value, k: &str, val: Value) -> R<()> {
        self.set(v, k, val, self.strict).map(drop)
    }

    /// [[Set]] (ECMA-262 §7.3.4 with `Throw` false): whether the write was done.
    pub fn try_put(&mut self, v: &Value, k: &str, val: Value) -> R<bool> {
        self.set(v, k, val, false)
    }

    /// `Set(v, k, val, true)` (ECMA-262 §7.3.4): [`Interp::put`] that throws a TypeError
    /// where the write is refused, as the built-ins that write through it do.
    pub fn put_or_throw(&mut self, v: &Value, k: &str, val: Value) -> R<()> {
        if self.set(v, k, val, false)? {
            return Ok(());
        }
        self.throw(
            "TypeError",
            format!("Cannot assign to read-only property \"{k}\""),
        )
    }

    fn set(&mut self, v: &Value, k: &str, val: Value, throw: bool) -> R<bool> {
        match v {
            Value::Obj(o) => {
                let vt = match o.borrow().internal {
                    Internal::ValueType(vt) => Some(vt),
                    _ => None,
                };
                if let Some(vt) = vt {
                    let index = idx(k).is_some();
                    // An index is an element of the wrapper, lost with it when the value
                    // was read from a property.
                    if index && o.borrow().owner.is_some() {
                        return Ok(true);
                    }
                    // An index, and a name script defined on the wrapper, is written where
                    // the wrapper keeps them.
                    let members = match index {
                        true => Some(crate::valuetypes::members_mut(o)),
                        false => crate::valuetypes::members(o).filter(|m| {
                            has_own(m, k)
                                && !vt.fields().contains(&k)
                                && !crate::valuetypes::has_method(vt, k)
                        }),
                    };
                    if let Some(m) = members {
                        if !self.set(&Value::Obj(m), k, val, false)? && throw {
                            return self.throw("TypeError", "Type error");
                        }
                        return Ok(true);
                    }
                    // A name the type has no property for is refused.
                    if !crate::valuetypes::put(self, o, vt, k, val)? && throw {
                        return self.throw("TypeError", "Type error");
                    }
                    return Ok(true);
                }
                // A gadget without properties (QQuickAnchorLine, QQmlScriptString) takes none.
                if matches!(
                    o.borrow().internal,
                    Internal::AnchorLine(_) | Internal::ScriptString(_)
                ) {
                    return Ok(true);
                }
                if matches!(o.borrow().internal, Internal::Locale(..)) {
                    return crate::builtins::locale_put(self, o, k, val, throw);
                }
                if crate::generator::is_scope(o) {
                    return crate::generator::scope_put(self, o, k, val).map(|()| true);
                }
                let host = match o.borrow().native.is_some() {
                    true => crate::types::host_write(&o.borrow(), k),
                    false => None,
                };
                if host.is_none()
                    && crate::types::is_list_row(&o.borrow())
                    && self.host.row_write(o, k, &val)
                {
                    return Ok(true);
                }
                if let Some(host) = host {
                    use crate::types::HostWrite;
                    return match host {
                        HostWrite::Silent => Ok(true),
                        HostWrite::Refused if throw => self.throw("TypeError", "Type error"),
                        HostWrite::Refused => Ok(false),
                        HostWrite::ReadOnly(name) => {
                            let e = HostError::read_only(&name);
                            self.throw(e.kind, e.message)
                        }
                    };
                }
                if o.borrow().is_proxy() {
                    if crate::proxy::set(self, o, &PropKey::Str(k.into()), val, v)? {
                        return Ok(true);
                    }
                    return self.refuse_set(throw);
                }
                if crate::valuetypes::is_readonly_sequence(&o.borrow())
                    && let Some(why) = crate::valuetypes::refused_sequence_write(k)
                {
                    return self.throw("TypeError", why);
                }
                if o.borrow().array.is_some() {
                    if k == "length" {
                        let n = self.to_numeric(&val)?;
                        let n = array_length(self, n)?;
                        if o.borrow().restricted() {
                            return self.set_restricted_length(o, n, throw);
                        }
                        let mut b = o.borrow_mut();
                        if n > b.array_length() {
                            b.keep_slots();
                        }
                        b.cells = true;
                        b.set_array_length(n);
                        return Ok(true);
                    }
                    if let Some(i) = idx(k).filter(|&i| i < MAX_ARRAY_INDEX) {
                        let present = {
                            let b = o.borrow();
                            i < b.array.as_ref().map_or(0, Vec::len) && !b.is_hole(i)
                        };
                        if present || !self.index_shadowed(o, k) {
                            return self.set_element(o, k, i, val, throw);
                        }
                    }
                }
                if string_object_fixed(&o.borrow(), k) {
                    return self.refuse_set(throw);
                }
                // A typed array's numeric keys are its elements (ECMA-262 §10.4.5.5).
                if matches!(o.borrow().internal, Internal::TypedArray(_))
                    && crate::typedarray::put_index(self, o, k, &val)?
                {
                    return Ok(true);
                }
                // A constructor's or generator function's `prototype` is a non-enumerable own
                // data property.
                if k == "prototype"
                    && o.borrow()
                        .call
                        .as_ref()
                        .is_some_and(Callable::owns_prototype)
                {
                    // A class's `prototype` is read-only (ECMA-262 §15.7.14).
                    if o.borrow().call.as_ref().and_then(Callable::kind)
                        == Some(crate::jsrt::FnKind::ClassConstructor)
                    {
                        fn_prototype(o);
                        if o.borrow().attr(k) & READONLY != 0 {
                            return self.refuse_set(throw);
                        }
                    }
                    set_hidden(o, k, val);
                    return Ok(true);
                }
                // The `Qt` object and `Qt.platform` are QObjects in Qt: a write to a method or
                // a constant property throws, and `Qt` takes no new property.
                if o.borrow().native.is_some() {
                    if crate::qtobj::constant_member(&o.borrow(), k) {
                        return self.throw(
                            "TypeError",
                            format!("Cannot assign to read-only property \"{k}\""),
                        );
                    }
                    if crate::qtobj::is_qt_global(&o.borrow()) && !has_own(o, k) {
                        return match throw {
                            true => self.throw(
                                "TypeError",
                                format!("Cannot assign to read-only property \"{k}\""),
                            ),
                            false => Ok(false),
                        };
                    }
                }
                // OrdinarySet: the nearest own-or-inherited property `k` decides; only an
                // accessor runs code, and one without a setter refuses the write.
                let mut setter = None;
                let mut cur = Some(o.clone());
                while let Some(p) = cur {
                    {
                        let b = p.borrow();
                        if b.is_proxy() {
                            drop(b);
                            if crate::proxy::set(self, &p, &PropKey::Str(k.into()), val, v)? {
                                return Ok(true);
                            }
                            return self.refuse_set(throw);
                        }
                        // A function's built-in `length` and `name` are read-only.
                        if intrinsic(&b, k).is_some() {
                            drop(b);
                            return self.refuse_set(throw);
                        }
                        if b.props.contains_key(k) {
                            if b.attr(k) & READONLY != 0 {
                                drop(b);
                                return self.refuse_set(throw);
                            }
                            break;
                        }
                        if let Some(a) = own_accessor(&b, k) {
                            setter = Some(a.set.clone());
                            break;
                        }
                    }
                    cur = self.proto_of(&p);
                }
                if let Some(s) = setter {
                    match s {
                        Some(s) => drop(self.call(&s, v.clone(), &[val])?),
                        None => return self.refuse_set(throw),
                    }
                } else if o.borrow().non_extensible && !has_own(o, k) {
                    return self.refuse_set(throw);
                } else if let Some(i) = idx(k).filter(|&i| {
                    let b = o.borrow();
                    i < MAX_ARRAY_INDEX && b.array.is_some() && !b.props.contains_key(k)
                }) {
                    return self.set_element(o, k, i, val, throw);
                } else {
                    set_own(o, k, val);
                }
                Ok(true)
            }
            Value::Item(id) => {
                if self.item_integrity.get(id) == Some(&ItemIntegrity::Frozen) {
                    // QV4's error (QObjectWrapper::virtualPut), thrown whatever the strictness.
                    return self.throw(
                        "Error",
                        format!("Cannot assign to property \"{k}\" of read-only object"),
                    );
                }
                // An array index is no QObject member (QV4: not a string key): kept by the
                // object like any other JavaScript property.
                let target = match array_index(k) {
                    Some(_) => WriteTarget::Extra,
                    None => self.host.write_target(*id, k),
                };
                match target {
                    WriteTarget::Property => {}
                    WriteTarget::Function(name) => {
                        let e = HostError::read_only(&name);
                        return self.throw(e.kind, e.message);
                    }
                    WriteTarget::Missing => {
                        return self.throw(
                            "Error",
                            format!("Cannot assign to non-existent property \"{k}\""),
                        );
                    }
                    WriteTarget::Extra => {
                        if let Some(o) = self.host.extras(*id, true) {
                            set_own(&o, k, val);
                        }
                        return Ok(true);
                    }
                    WriteTarget::Dropped => return Ok(true),
                    WriteTarget::Refused if throw => return self.throw("TypeError", "Type error"),
                    WriteTarget::Refused => return Ok(false),
                }
                if self.host.read_only(*id, k) {
                    let e = HostError::read_only(k);
                    return self.throw(e.kind, e.message);
                }
                if let Some(e) = self.host.write_refusal(*id, k) {
                    return self.throw(e.kind, e.message);
                }
                let val = match val {
                    Value::Undefined => match self.host.undefined_field(*id, k) {
                        Some(made) => made,
                        None => match self.host.no_undefined(*id, k) {
                            Some(ty) => {
                                self.host.unbind(*id, k);
                                let e = HostError::no_undefined(&ty);
                                return self.throw(e.kind, e.message);
                            }
                            None => val,
                        },
                    },
                    val => val,
                };
                // `undefined` that got here resets the property or is stored as it is.
                let val = match self.host.write_slot(*id, k) {
                    Some(slot) if !matches!(val, Value::Undefined) => {
                        crate::valuetypes::qobject_wrapper_set_property(self, &slot, val)?
                    }
                    _ => val,
                };
                self.host.set(*id, k, val);
                Ok(true)
            }
            Value::Undefined | Value::Null => self.nullish_write(v),
            Value::Str(_) | Value::Str16(_) | Value::Num(_) | Value::Bool(_) => {
                self.set_primitive(v, k, val, throw)
            }
            Value::Symbol(_) => self.refuse_set(throw),
            // A color's `rgba` is read-only; a name it has no property for is refused.
            Value::Color(_) if k == "rgba" => {
                let from = crate::valuetypes::type_name(&val);
                self.throw("Error", format!("Cannot assign {from} to int"))
            }
            Value::Color(_) if throw && !crate::valuetypes::is_color_own(k) && idx(k).is_none() => {
                self.throw("TypeError", "Type error")
            }
            _ => Ok(true),
        }
    }

    /// Whether array `o`'s dense elements and holes are all its indexed properties along its
    /// prototype chain: none is held as an ordinary property and, when it has holes, no
    /// prototype has an indexed property a hole would expose. The array methods' fast paths
    /// read the dense storage only then.
    pub fn elements_are_dense(&self, o: &Obj) -> bool {
        {
            let b = o.borrow();
            if b.tail == 0 && b.holes.is_none() {
                return true;
            }
            if b.tail > 0 || b.has_slow_elements() {
                return false;
            }
        }
        let mut cur = self.proto_of(o);
        while let Some(p) = cur {
            {
                let b = p.borrow();
                if b.is_proxy()
                    || !matches!(b.internal, Internal::Ordinary)
                    || b.array.as_ref().is_some_and(|a| !a.is_empty())
                    || (b.props.keys())
                        .chain(b.accessors.iter().flat_map(|m| m.keys()))
                        .any(|k| array_index(k).is_some())
                {
                    return false;
                }
            }
            cur = self.proto_of(&p);
        }
        true
    }

    /// Whether [[Set]] of index `k` on array `o`, which has no element `k`, meets another
    /// property `k` first: an own one held as an ordinary property, or an inherited one.
    fn index_shadowed(&self, o: &Obj, k: &str) -> bool {
        {
            let b = o.borrow();
            if b.props.contains_key(k) || own_accessor(&b, k).is_some() {
                return true;
            }
        }
        let mut cur = self.proto_of(o);
        while let Some(p) = cur {
            if p.borrow().is_proxy() || has_own(&p, k) {
                return true;
            }
            cur = self.proto_of(&p);
        }
        false
    }

    /// Writes element `i` (key `k`) of array `o` (the receiver `v`), growing it as
    /// ArraySetLength does (ECMA-262 §10.4.2.1).
    fn set_element(&mut self, o: &Obj, k: &str, i: usize, val: Value, throw: bool) -> R<bool> {
        let (stored, len) = {
            let b = o.borrow();
            (b.array.as_ref().map_or(0, Vec::len), b.array_length())
        };
        let absent = i >= stored || o.borrow().is_hole(i);
        {
            let b = o.borrow();
            // QV4 (`Object::internalPut`): an element with a zero attribute byte is not
            // writable; a position that holds nothing is a new property, whatever byte an
            // earlier element left there.
            if b.restricted()
                && match absent {
                    false => b.attr(k) & (READONLY | crate::value::HIDDEN) != 0,
                    true => b.non_extensible || (i >= len && b.attr("length") & READONLY != 0),
                }
            {
                drop(b);
                return self.refuse_set(throw);
            }
        }
        // The next stored element: the array grows by it.
        if i == stored {
            array_len(self, i as f64 + 1.0)?;
        }
        let mut b = o.borrow_mut();
        b.store_element(i, val);
        if absent {
            b.stored_without_byte(i);
        }
        Ok(true)
    }

    /// OrdinarySet with a primitive receiver (ECMA-262 §10.1.9.2): only an inherited setter
    /// accepts the write, since nothing can be created on a primitive.
    fn set_primitive(&mut self, v: &Value, k: &str, val: Value, throw: bool) -> R<bool> {
        if let Some(s) = v.js_str()
            && (k == "length" || idx(k).is_some_and(|i| i < s.len()))
        {
            return self.refuse_set(throw);
        }
        let mut cur = Some(match v {
            Value::Str(_) | Value::Str16(_) => self.string_proto.clone(),
            Value::Num(_) => self.number_proto.clone(),
            _ => self.boolean_proto.clone(),
        });
        while let Some(p) = cur {
            let setter = {
                let b = p.borrow();
                if b.props.contains_key(k) {
                    break;
                }
                own_accessor(&b, k).map(|a| a.set.clone())
            };
            match setter {
                Some(Some(s)) => {
                    self.call(&s, v.clone(), &[val])?;
                    return Ok(true);
                }
                Some(None) => break,
                None => cur = self.proto_of(&p),
            }
        }
        self.refuse_set(throw)
    }

    /// A [[Set]] that returned false: a read-only property, or a new one on a non-extensible
    /// object (ECMA-262 §10.1.9.2). QV4 throws its plain TypeError for both.
    pub fn refuse_set(&self, throw: bool) -> R<bool> {
        if throw { self.type_error() } else { Ok(false) }
    }

    /// QV4's TypeError without a message of its own (`ExecutionEngine::throwTypeError()`).
    pub fn type_error<T>(&self) -> R<T> {
        self.throw("TypeError", "Type error")
    }

    /// The text QV4 names a value by in its `… is not a function` messages
    /// (`Value::toQStringNoThrow`): ToPrimitive with the string hint; where that throws, the
    /// text of what it threw; nothing where that throws as well.
    pub fn shown(&mut self, v: &Value) -> String {
        if let Value::Item(i) = v
            && let Some(s) = self.host.object_text(*i)
        {
            return s;
        }
        if !matches!(v, Value::Obj(_)) {
            return v.to_js_string();
        }
        let thrown = match self.to_primitive(v, Hint::String) {
            Ok(p) => return p.to_js_string(),
            Err(Ex::Throw(e)) => e,
            Err(Ex::Short) => return String::new(),
        };
        match self.to_primitive(&thrown, Hint::String) {
            Ok(p) => p.to_js_string(),
            Err(_) => String::new(),
        }
    }

    /// QV4's TypeError for calling `f`, which is not callable.
    pub fn not_a_function<T>(&mut self, f: &Value) -> R<T> {
        let f = self.shown(f);
        self.throw("TypeError", format!("{f} is not a function"))
    }

    /// QV4's TypeError for `name(…)` where the free name `name` is not callable: it names the
    /// scope object the name is a property of (`this`), or `[null]` for any other name.
    pub fn not_a_named_function<T>(&mut self, this: &Value, name: &str) -> R<T> {
        match this {
            Value::Undefined => self.throw(
                "TypeError",
                format!("Property '{name}' of object [null] is not a function"),
            ),
            o => self.not_a_method(o, name),
        }
    }

    pub fn not_a_method<T>(&mut self, o: &Value, name: &str) -> R<T> {
        let o = self.shown(o);
        self.throw(
            "TypeError",
            format!("Property '{name}' of object {o} is not a function"),
        )
    }

    /// QV4's TypeError of ToObject for undefined or null.
    pub fn no_object<T>(&self, v: &Value) -> R<T> {
        self.throw(
            "TypeError",
            format!(
                "Value is {} and could not be converted to an object",
                v.to_js_string()
            ),
        )
    }

    /// `a.length = n` on array `o` with restricted properties (ArraySetLength, ECMA-262
    /// §10.4.2.4): stops above the last non-configurable element it would delete.
    fn set_restricted_length(&mut self, o: &Obj, n: usize, throw: bool) -> R<bool> {
        let (len, ro) = {
            let b = o.borrow();
            (b.array_length(), b.attr("length") & READONLY != 0)
        };
        // OrdinarySet refuses a non-writable `length` whatever the value (§10.1.9.2 2.a).
        if ro {
            return self.refuse_set(throw);
        }
        if n == len {
            return Ok(true);
        }
        let keep = {
            let b = o.borrow();
            // A byte an earlier element left at a position that holds nothing does not count.
            let held = |i: usize, x: &str| {
                !b.is_hole(i) || b.props.contains_key(x) || own_accessor(&b, x).is_some()
            };
            (b.attrs.iter().flat_map(|m| m.iter()))
                .filter(|(_, a)| **a & (PERMANENT | crate::value::HIDDEN) != 0)
                .filter_map(|(x, _)| idx(x).filter(|&i| i >= n && i < len && held(i, x)))
                .max()
                .map_or(n, |i| i + 1)
        };
        {
            let mut b = o.borrow_mut();
            if keep > len {
                b.keep_slots();
            }
            b.cells = true;
            let stay = b.has_bytes() && !b.sparse();
            b.set_array_length(keep);
            if let Some(m) = &mut b.attrs
                && !stay
            {
                m.retain(|x, _| idx(x).is_none_or(|i| i < keep));
            }
        }
        if keep != n {
            return self.refuse_set(throw);
        }
        Ok(true)
    }

    #[cfg(feature = "interp")]
    fn key(&mut self, k: &Key, sc: &Scope) -> R<PropKey> {
        match k {
            Key::Name(n) => Ok(PropKey::Str(n.to_string())),
            Key::Computed(e) => {
                let v = self.eval(e, sc)?;
                self.to_property_key(&v)
            }
        }
    }

    /// `v[k]` for a converted property key.
    pub fn get_pk(&mut self, v: &Value, k: &PropKey) -> R<Value> {
        match k {
            PropKey::Str(n) => self.get(v, n),
            PropKey::Sym(s) => self.get_sym(v, s),
        }
    }

    /// `v[k] = val` for a converted property key.
    pub fn put_pk(&mut self, v: &Value, k: &PropKey, val: Value) -> R<()> {
        match k {
            PropKey::Str(n) => self.put(v, n, val),
            PropKey::Sym(s) => self.put_sym(v, s, val),
        }
    }

    /// [`Interp::put_pk`] in strict mode code: a refused string-keyed write throws.
    pub fn put_or_throw_pk(&mut self, v: &Value, k: &PropKey, val: Value) -> R<()> {
        match k {
            PropKey::Str(n) => self.put_or_throw(v, n, val),
            PropKey::Sym(s) => self.put_sym(v, s, val),
        }
    }

    // ---------- calls ----------

    pub fn call(&mut self, f: &Value, this: Value, args: &[Value]) -> R<Value> {
        let frame = self.enter()?;
        let r = on_js_stack(|| self.call_entered(f, this, args));
        self.leave(frame, r)
    }

    fn call_entered(&mut self, f: &Value, this: Value, args: &[Value]) -> R<Value> {
        self.tick()?;
        match callable(f) {
            Some(Callable::Native(_, nf)) => {
                self.constructing = false;
                nf(self, &this, args)
            }
            Some(Callable::Item(i, n)) => {
                self.host_call(i, n, args).unwrap_or(Ok(Value::Undefined))
            }
            Some(Callable::Unwritten(c, n)) => self.throw("Error", crate::unwritten::message(c, n)),
            #[cfg(feature = "interp")]
            Some(Callable::Closure(func, sc)) => self.invoke(&func, &sc, f, Some(this), args),
            Some(Callable::Compiled(_, _, m)) if m.kind.is_class_constructor() => self.class_call(),
            Some(Callable::Compiled(cf, env, m)) => crate::jsrt::call_compiled(
                self,
                cf,
                &crate::jsrt::Env::call(&env, f),
                m,
                this,
                args,
            ),
            Some(Callable::Bound(b)) => self.call_bound(&b, args),
            Some(Callable::Proxy(ProxyFn::Object { .. })) => {
                crate::proxy::call(self, f, this, args)
            }
            Some(Callable::Proxy(ProxyFn::Constructor)) => crate::proxy::type_error(self),
            Some(Callable::Proxy(ProxyFn::Revoke)) => {
                crate::proxy::revoke(f);
                Ok(Value::Undefined)
            }
            #[cfg(feature = "interp")]
            Some(Callable::Class(..)) => self.class_call(),
            None if Self::is_type_wrapper(f) => self.type_error(),
            None => self.not_a_function(f),
        }
    }

    /// Whether `v` is the JS object of a QML type or of an import qualifier, or `Qt`.
    fn is_type_wrapper(v: &Value) -> bool {
        matches!(v, Value::Obj(o) if crate::types::is_type_wrapper(&o.borrow()))
    }

    /// `[[Call]]` of a class constructor (ECMA-262 §10.2.1 step 2).
    #[cold]
    fn class_call<T>(&mut self) -> R<T> {
        self.throw("TypeError", "Cannot call a class constructor without |new|")
    }

    #[cfg(feature = "interp")]
    fn invoke(
        &mut self,
        func: &Func,
        sc: &Scope,
        callee: &Value,
        this: Option<Value>,
        args: &[Value],
    ) -> R<Value> {
        self.invoke_with(func, sc, callee, this, &Value::Undefined, args)
    }

    /// [`Interp::invoke`] for the NewTarget `nt` (`undefined` in a [[Call]]). A derived class
    /// constructor (only ever constructed) returns the object `super(...)` bound (ECMA-262
    /// §10.2.2 steps 10-12).
    #[cfg(feature = "interp")]
    fn invoke_with(
        &mut self,
        func: &Func,
        sc: &Scope,
        callee: &Value,
        this: Option<Value>,
        nt: &Value,
        args: &[Value],
    ) -> R<Value> {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let caller = std::mem::replace(&mut self.strict, func.strict);
        let code = self.code.replace(sc.clone());
        let qs = self.qs.take();
        let r = if !*ON.get_or_init(|| crate::log_on!("ri::jsprof")) {
            self.invoke_inner(func, sc, callee, this, nt, args)
        } else {
            jsprof::enter();
            let r = self.invoke_inner(func, sc, callee, this, nt, args);
            jsprof::leave(func);
            r
        };
        self.strict = caller;
        self.code = code;
        self.qs = qs;
        if func.kind == crate::jsrt::FnKind::Generator {
            return r.map(|step| crate::generator::make_generator(self, callee, step));
        }
        r
    }

    #[cfg(feature = "interp")]
    fn invoke_inner(
        &mut self,
        func: &Func,
        sc: &Scope,
        callee: &Value,
        this: Option<Value>,
        nt: &Value,
        args: &[Value],
    ) -> R<Value> {
        if let Some(c) = func.coroutine.filter(|c| !c.runs()) {
            return self.throw("SyntaxError", format!("unsupported construct {}", c.name()));
        }
        let fr = match self.frames.pop() {
            Some(fr) => {
                let mut f = fr.borrow_mut();
                f.parent = Some(sc.clone());
                f.slots.resize(func.nslots as usize, Value::Undefined);
                drop(f);
                fr
            }
            None => {
                let mut f = Frame::default();
                f.parent = Some(sc.clone());
                f.fun = true;
                f.slots = vec![Value::Undefined; func.nslots as usize];
                Rc::new(RefCell::new(f))
            }
        };
        if func.direct_eval && func.is_gen_step() {
            fr.borrow_mut().gen_step = true;
        }
        if !func.is_arrow() {
            let this = this.unwrap_or_default();
            fr.borrow_mut().this = Some(match func.strict {
                true => this,
                false => self.sloppy_this(&this).unwrap_or(this),
            });
            fr.borrow_mut().new_target = nt.clone();
            fr.borrow_mut().derived = func.kind == crate::jsrt::FnKind::DerivedConstructor;
            if func.uses_arguments {
                let ids: Option<Box<[u32]>> = func
                    .mapped_arguments()
                    .then(|| {
                        func.params
                            .iter()
                            .map(|p| match p {
                                Pat::Local(0, j, _) => Some(*j),
                                _ => None,
                            })
                            .collect()
                    })
                    .flatten()
                    .map(|slots: Vec<u32>| owner_ids(&slots));
                let a = match ids {
                    Some(ids) => {
                        let a = mapped_arguments(self, callee.clone(), args, ids);
                        fr.borrow_mut().args = Some(a.clone());
                        a
                    }
                    None => unmapped_arguments(self, args),
                };
                self.define(&fr, "arguments", Value::Obj(a));
            }
        } else if !func.is_gen_step() {
            fr.borrow_mut().arrow_this = Some(this.unwrap_or_default());
        }
        let mapped = fr.borrow().args.is_some();
        if !func.simple_params() {
            func.params
                .iter()
                .chain(func.rest.iter())
                .for_each(|p| self.uninit(p, &fr));
        }
        for (i, p) in func.params.iter().enumerate() {
            if mapped {
                break;
            }
            let v = args.get(i).cloned().unwrap_or_default();
            match p {
                Pat::Local(0, j, _) => fr.borrow_mut().slots[*j as usize] = v,
                p => self.bind(p, v, &fr, true)?,
            }
        }
        if let Some(r) = &func.rest {
            let rest = args.get(func.params.len()..).unwrap_or(&[]).to_vec();
            self.bind(r, Value::array(rest), &fr, true)?;
        }
        for (e, slot) in &func.var_init {
            let v = self.eval(e, &fr)?;
            fr.borrow_mut().slots[*slot as usize] = v;
        }
        let names = func.hoisted.get_or_init(|| Self::var_names(&func.body));
        if !names.is_empty() {
            let mut f = fr.borrow_mut();
            for n in names {
                f.vars.entry(n.clone()).or_insert(Value::Undefined);
            }
        }
        let r = if let [Stmt::Return(Some(e))] = &func.body[..] {
            self.tick()?;
            self.eval(e, &fr)?
        } else {
            match self.exec_block(&func.body, &fr)? {
                Flow::Return(v) => v,
                _ => Value::Undefined,
            }
        };
        let r = match func.kind {
            crate::jsrt::FnKind::DerivedConstructor => {
                let this = fr.borrow().this.clone().unwrap_or_default();
                self.derived_result(r, this)?
            }
            _ => r,
        };
        // Nothing captured the frame: reset it for the next call.
        if Rc::strong_count(&fr) == 1 && self.frames.len() < 64 {
            let mut f = fr.borrow_mut();
            f.slots.clear();
            f.vars.clear();
            f.this = None;
            f.new_target = Value::Undefined;
            f.derived = false;
            f.arrow_this = None;
            f.parent = None;
            f.args = None;
            f.gen_step = false;
            f.eval_vars.clear();
            drop(f);
            self.frames.push(fr);
        }
        Ok(r)
    }

    /// `var` declarations are function-scoped: predeclare them in the function frame.
    #[cfg(feature = "interp")]
    fn hoist_vars(&self, body: &[Stmt], fr: &Scope) {
        let mut f = fr.borrow_mut();
        for n in Self::var_names(body) {
            f.vars.entry(n).or_insert(Value::Undefined);
        }
    }

    #[cfg(feature = "interp")]
    fn var_names(body: &[Stmt]) -> Vec<String> {
        fn pat_ids(p: &Pat, out: &mut Vec<String>) {
            match p {
                Pat::Id(n) => out.push(n.clone()),
                Pat::Dynamic(_, _, p) => pat_ids(p, out),
                Pat::Local(..) | Pat::Const(..) => {}
                Pat::Object(ps, rest) => {
                    ps.iter().for_each(|(_, p)| pat_ids(p, out));
                    rest.iter().for_each(|p| pat_ids(p, out));
                }
                Pat::Array(ps, rest) => {
                    ps.iter().flatten().for_each(|p| pat_ids(p, out));
                    rest.iter().for_each(|p| pat_ids(p, out));
                }
                Pat::Default(p, _) => pat_ids(p, out),
                Pat::Member(_) | Pat::Init(_) => {}
            }
        }
        fn walk(s: &Stmt, out: &mut Vec<String>) {
            match s {
                Stmt::Var("var", ds) => ds.iter().for_each(|(p, _)| pat_ids(p, out)),
                Stmt::If(_, a, b) => {
                    walk(a, out);
                    b.iter().for_each(|b| walk(b, out));
                }
                Stmt::Block(b) => b.iter().for_each(|s| walk(s, out)),
                Stmt::For(i, _, _, b) => {
                    i.iter().for_each(|s| walk(s, out));
                    walk(b, out);
                }
                Stmt::ForOf(k, p, _, b) | Stmt::ForIn(k, p, _, b) => {
                    if *k == Some("var") {
                        pat_ids(p, out);
                    }
                    walk(b, out);
                }
                Stmt::While(_, b)
                | Stmt::DoWhile(b, _)
                | Stmt::Labeled(_, b)
                | Stmt::With(_, b) => walk(b, out),
                Stmt::Switch(_, cs) => cs.iter().flat_map(|c| &c.1).for_each(|s| walk(s, out)),
                Stmt::Try(t, c, f) => {
                    t.iter().for_each(|s| walk(s, out));
                    c.iter().flat_map(|c| &c.1).for_each(|s| walk(s, out));
                    f.iter().flatten().for_each(|s| walk(s, out));
                }
                _ => {}
            }
        }
        let mut names = Vec::new();
        body.iter().for_each(|s| walk(s, &mut names));
        names
    }

    /// Links class `cv` and its prototype object `proto` to the `extends` heritage `parent`
    /// (ClassDefinitionEvaluation steps 8.e-h, ECMA-262 §15.7.14): `extends null` leaves a
    /// null-prototype `proto`; anything else not a constructor, or whose `prototype` is
    /// neither an object nor null, is a TypeError.
    pub fn class_heritage(&mut self, cv: &Value, proto: &Obj, parent: &Value) -> R<()> {
        if matches!(parent, Value::Null) {
            let mut b = proto.borrow_mut();
            b.proto = None;
            b.null_proto = true;
            return Ok(());
        }
        if !crate::proxy::is_constructor(parent) {
            return self.throw("TypeError", "The superclass is not a function object.");
        }
        let pp = match self.get(parent, "prototype")? {
            Value::Obj(p) => Some(p),
            Value::Null => None,
            _ => {
                return self.throw(
                    "TypeError",
                    "The value of the superclass's prototype property is not an object.",
                );
            }
        };
        {
            let mut b = proto.borrow_mut();
            b.null_proto = pp.is_none();
            b.proto = pp;
        }
        if let (Value::Obj(c), Value::Obj(po)) = (cv, parent) {
            c.borrow_mut().proto = Some(po.clone());
        }
        Ok(())
    }

    /// `new f(...args)`: Construct(f, args) (ECMA-262 §7.3.15).
    pub fn construct(&mut self, f: &Value, args: &[Value]) -> R<Value> {
        self.construct_new(f, args, f)
    }

    /// Construct(f, args, newTarget) (ECMA-262 §7.3.15): the object is made with
    /// `newTarget`'s `prototype` (OrdinaryCreateFromConstructor, §10.1.13).
    pub fn construct_new(&mut self, f: &Value, args: &[Value], nt: &Value) -> R<Value> {
        let frame = self.enter()?;
        let r = on_js_stack(|| self.construct_entered(f, args, nt));
        self.leave(frame, r)
    }

    /// GetPrototypeFromConstructor (ECMA-262 §10.1.14) of NewTarget `nt` constructing `f`:
    /// `None` when `nt.prototype` is no object (the intrinsic default prototype applies).
    fn proto_from_ctor(&mut self, f: &Value, nt: &Value) -> R<Option<Obj>> {
        if nt.strict_eq(f) {
            return Ok(self.prototype_of(f));
        }
        Ok(match self.get(nt, "prototype")? {
            Value::Obj(p) => Some(p),
            _ => None,
        })
    }

    /// `new f` for an `f` that has no [[Construct]]: QV4 names no function, and has its
    /// plain TypeError for a value that is not callable either.
    #[cold]
    fn not_a_constructor<T>(&mut self, f: &Value) -> R<T> {
        match callable(f) {
            Some(_) => self.throw("TypeError", "Function is not a constructor."),
            None => self.type_error(),
        }
    }

    fn construct_entered(&mut self, f: &Value, args: &[Value], nt: &Value) -> R<Value> {
        self.tick()?;
        match callable(f) {
            Some(Callable::Native(..)) if !crate::proxy::is_constructor(f) => {
                self.not_a_constructor(f)
            }
            Some(Callable::Native(_, nf)) => {
                // A built-in makes its object with its intrinsic prototype; another NewTarget's
                // `prototype` (read first, as the built-ins do) replaces it.
                let lent = match nt.strict_eq(f) {
                    true => None,
                    false => Some(self.proto_from_ctor(f, nt)?),
                };
                if lent.is_some() && crate::builtins::is_object_ctor(nf) {
                    // Object (ECMA-262 §20.1.1.1 step 1): another NewTarget ignores the value.
                    let o = new_obj();
                    o.borrow_mut().proto = lent.flatten();
                    return Ok(Value::Obj(o));
                }
                self.constructing = true;
                self.new_target_proto = lent.clone().flatten();
                let r = nf(self, &Value::Undefined, args);
                self.new_target_proto = None;
                let r = r?;
                let wrapper = Self::is_wrapper_constructor(f);
                let r = self.native_constructed(f, r);
                let proto = match lent {
                    Some(Some(p)) => Some(p),
                    _ if wrapper => self.prototype_of(f),
                    _ => None,
                };
                if let (Value::Obj(o), Some(p)) = (&r, proto) {
                    o.borrow_mut().proto = Some(p);
                }
                Ok(r)
            }
            Some(Callable::Item(..) | Callable::Unwritten(..)) => self.not_a_constructor(f),
            #[cfg(feature = "interp")]
            Some(Callable::Closure(func, _)) if !func.kind.is_constructor() => {
                self.not_a_constructor(f)
            }
            #[cfg(feature = "interp")]
            Some(Callable::Closure(func, sc)) => {
                let o = new_obj();
                o.borrow_mut().proto = self.proto_from_ctor(f, nt)?;
                let this = Some(Value::Obj(o.clone()));
                let r = self.invoke_with(&func, &sc, f, this, nt, args)?;
                Ok(if matches!(r, Value::Obj(_)) {
                    r
                } else {
                    Value::Obj(o)
                })
            }
            // A derived constructor's object comes from its `super(...)` call.
            Some(Callable::Compiled(cf, env, m))
                if m.kind == crate::jsrt::FnKind::DerivedConstructor =>
            {
                crate::jsrt::call_compiled(
                    self,
                    cf,
                    &crate::jsrt::Env::construct(&env, f, nt),
                    m,
                    Value::Empty,
                    args,
                )
            }
            Some(Callable::Compiled(cf, env, m)) if m.kind.is_constructor() => {
                let o = new_obj();
                o.borrow_mut().proto = self.proto_from_ctor(f, nt)?;
                let r = crate::jsrt::call_compiled(
                    self,
                    cf,
                    &crate::jsrt::Env::construct(&env, f, nt),
                    m,
                    Value::Obj(o.clone()),
                    args,
                )?;
                Ok(if matches!(r, Value::Obj(_)) {
                    r
                } else {
                    Value::Obj(o)
                })
            }
            Some(Callable::Compiled(..)) => self.not_a_constructor(f),
            Some(Callable::Bound(b)) => self.construct_bound(f, &b, args, nt),
            Some(Callable::Proxy(ProxyFn::Object { construct: true })) => {
                crate::proxy::construct(self, f, args, nt)
            }
            Some(Callable::Proxy(ProxyFn::Constructor)) => Ok(Value::Obj(crate::proxy::create(
                self,
                args.first().unwrap_or(&Value::Undefined),
                args.get(1).unwrap_or(&Value::Undefined),
            )?)),
            Some(Callable::Proxy(_)) => self.not_a_constructor(f),
            #[cfg(feature = "interp")]
            Some(Callable::Class(cls, csc)) => self.construct_class(f, &cls, &csc, args, nt),
            None => self.not_a_constructor(f),
        }
    }

    fn is_wrapper_constructor(f: &Value) -> bool {
        matches!(f, Value::Obj(c) if matches!(c.borrow().internal, Internal::WrapperConstructor))
    }

    /// The object a built-in constructor's [[Construct]] makes from what its function
    /// returned: `Number`, `String` and `Boolean` return the primitive, which becomes the
    /// wrapper's [[NumberData]], [[StringData]] or [[BooleanData]] (§21.1.1.1, §22.1.1.1,
    /// §20.3.1.1).
    fn native_constructed(&mut self, f: &Value, r: Value) -> Value {
        match Self::is_wrapper_constructor(f) {
            true => self.to_object(&r),
            false => r,
        }
    }

    fn prototype_of(&mut self, f: &Value) -> Option<Obj> {
        let Value::Obj(fo) = f else { return None };
        fn_prototype(fo)
    }

    /// InitializeInstanceElements: field initializers are strict mode code like the whole class.
    #[cfg(feature = "interp")]
    fn init_fields(&mut self, cls: &Class, sc: &Scope, this: &Value) -> R<()> {
        if cls.fields.iter().all(|f| f.2) {
            return Ok(());
        }
        let caller = std::mem::replace(&mut self.strict, true);
        let r = self.instance_fields(cls, sc, this);
        self.strict = caller;
        r
    }

    #[cfg(feature = "interp")]
    fn instance_fields(&mut self, cls: &Class, sc: &Scope, this: &Value) -> R<()> {
        let scope = Self::class_scope(sc);
        let Some(keys) = scope.as_ref().and_then(|c| c.field_keys.get()) else {
            return Ok(());
        };
        let fr = Self::child(sc);
        fr.borrow_mut().this = Some(this.clone());
        for ((k, e, st), name) in cls.fields.iter().zip(keys) {
            if *st {
                continue;
            }
            let v = match e {
                Some(e) => self.eval(e, &fr)?,
                None => Value::Undefined,
            };
            define_field_pk(self, this, name, v, crate::ir::names_field(k, e.as_ref()))?;
        }
        Ok(())
    }

    /// SuperCall steps 3-6 (ECMA-262 §13.3.7.1) in the derived constructor `active`:
    /// Construct(func, args, NewTarget) of its [[GetPrototypeOf]] (GetSuperConstructor,
    /// §13.3.7.2), a TypeError when that is no constructor (`extends null`).
    pub fn super_construct(&mut self, active: &Value, args: &[Value], nt: &Value) -> R<Value> {
        let func = &crate::builtins::prototype_of(self, active)?;
        if !crate::proxy::is_constructor(func) {
            return self.not_a_constructor(func);
        }
        self.construct_new(func, args, nt)
    }

    /// [[Construct]] of the class `f` for the NewTarget `nt` (ECMA-262 §10.2.2): a base class
    /// makes its object from NewTarget and initializes its fields before the constructor
    /// body; a derived one gets its object from `super(...)`.
    #[cfg(feature = "interp")]
    fn construct_class(
        &mut self,
        f: &Value,
        cls: &Class,
        sc: &Scope,
        args: &[Value],
        nt: &Value,
    ) -> R<Value> {
        // `sc` is the class scope from `make_class`.
        if cls.extends.is_some() {
            return match &cls.ctor {
                Some(ctor) => self.invoke_with(ctor, sc, f, Some(Value::Empty), nt, args),
                // `constructor(...args) { super(...args) }` (§15.7.14 step 14.a).
                None => {
                    let this = self.super_construct(f, args, nt)?;
                    self.init_fields(cls, sc, &this)?;
                    Ok(this)
                }
            };
        }
        let o = new_obj();
        {
            let proto = self.proto_from_ctor(f, nt)?;
            let mut b = o.borrow_mut();
            b.proto = proto;
            let n = cls.size_hint.get() as usize;
            b.keys.reserve_exact(n);
            b.props.reserve(n);
        }
        let this = Value::Obj(o.clone());
        self.init_fields(cls, sc, &this)?;
        let r = match &cls.ctor {
            Some(ctor) => self.invoke_with(ctor, sc, f, Some(this.clone()), nt, args)?,
            None => Value::Undefined,
        };
        cls.size_hint.set(o.borrow().props.len() as u32);
        Ok(if matches!(r, Value::Obj(_)) { r } else { this })
    }

    /// ClassDefinitionEvaluation: all parts of a class are strict mode code (ECMA-262 §11.2.2).
    #[cfg(feature = "interp")]
    fn make_class(&mut self, cls: &Class, sc: &Scope) -> R<Value> {
        let caller = std::mem::replace(&mut self.strict, true);
        let r = self.class_definition(cls, sc);
        self.strict = caller;
        r
    }

    #[cfg(feature = "interp")]
    fn class_definition(&mut self, cls: &Class, sc: &Scope) -> R<Value> {
        let csc = Self::child(sc);
        let parent = match &cls.extends {
            Some(e) => Some(self.eval(e, sc)?),
            None => None,
        };
        let cls_rc = Rc::new(cls.clone());
        // The [[HomeObject]] of the constructor, instance methods and field initializers is
        // the prototype; that of static methods, static fields and static blocks the class.
        let psc = Self::child(&csc);
        let ssc = Self::child(&csc);
        let cv = fn_value(Callable::Class(cls_rc, psc.clone()));
        ssc.borrow_mut().home = Some(cv.clone());
        let scope = Rc::new(ClassScope {
            class: cv.clone(),
            parent: parent.clone(),
            field_keys: Default::default(),
        });
        csc.borrow_mut().class = Some(scope.clone());
        if let Some(n) = &cls.name {
            self.define(&csc, n, cv.clone());
        }
        let proto = self.prototype_of(&cv).unwrap();
        psc.borrow_mut().home = Some(Value::Obj(proto.clone()));
        if let Some(p) = &parent {
            self.class_heritage(&cv, &proto, p)?;
        }
        let mut keys: Vec<PropKey> = Vec::with_capacity(cls.fields.len());
        for (j, m) in cls.methods.iter().enumerate() {
            while let Some((k, _, _)) = cls.fields.get(keys.len())
                && cls.field_after[keys.len()] as usize <= j
            {
                keys.push(self.key(k, &csc)?);
            }
            let name = self.key(&m.key, &csc)?;
            let home = if m.is_static { &ssc } else { &psc };
            let fv = fn_value(Callable::Closure(Rc::new(m.func.clone()), home.clone()));
            if matches!(m.key, Key::Computed(_)) && m.func.display_name().is_empty() {
                let prefix = match m.kind {
                    "get" => "get ",
                    "set" => "set ",
                    _ => "",
                };
                crate::jsrt::name_function_pk(&fv, prefix, &name);
            }
            let target = if m.is_static {
                let Value::Obj(c) = &cv else { unreachable!() };
                c.clone()
            } else {
                proto.clone()
            };
            define_method_pk(&target, &name, m.kind, fv);
        }
        while let Some((k, _, _)) = cls.fields.get(keys.len()) {
            keys.push(self.key(k, &csc)?);
        }
        let keys = scope.field_keys.get_or_init(|| keys);
        // Static field initializers and static blocks run in order as methods of the class:
        // `this` is the class (ClassDefinitionEvaluation step 31).
        let fsc = Self::child(&ssc);
        fsc.borrow_mut().this = Some(cv.clone());
        let mut blocks = cls.blocks.iter().peekable();
        for (i, ((k, e, st), name)) in cls.fields.iter().zip(keys).enumerate() {
            while let Some((_, b)) = blocks.next_if(|(at, _)| *at as usize <= i) {
                self.eval(b, &fsc)?;
            }
            if *st {
                let v = match e {
                    Some(e) => self.eval(e, &fsc)?,
                    None => Value::Undefined,
                };
                define_field_pk(self, &cv, name, v, crate::ir::names_field(k, e.as_ref()))?;
            }
        }
        for (_, b) in blocks {
            self.eval(b, &fsc)?;
        }
        Ok(cv)
    }

    /// The `this`, key and method of a `super.m(...)` / `super[k](...)` callee: the reference
    /// and its [[Get]] come before the arguments (EvaluateCall, ECMA-262 §13.3.6.2).
    #[cfg(feature = "interp")]
    fn super_callee(&mut self, callee: &Expr, sc: &Scope) -> R<Option<(Value, PropKey, Value)>> {
        let (this, key) = match callee {
            Expr::Member(o, n, _) if matches!(**o, Expr::Super) => {
                (self.super_this(sc)?, PropKey::Str(n.to_string()))
            }
            Expr::Index(o, k, _) if matches!(**o, Expr::Super) => {
                let this = self.super_this(sc)?;
                (this, PropKey::of(&self.eval(k, sc)?))
            }
            _ => return Ok(None),
        };
        let f = self.super_get(&Self::home_object(sc), &this, &key)?;
        Ok(Some((this, key, f)))
    }

    #[cfg(feature = "interp")]
    fn call_expr(&mut self, callee: &Expr, args: &[Expr], optional: bool, sc: &Scope) -> R<Value> {
        // The reference is made before the arguments run. A function found in a `with`
        // object is called as its method: `this` is that object (ECMA-262 §13.3.6.1 step
        // 4.b.ii, WithBaseObject).
        if let Expr::Dynamic(n, withs, x) = callee {
            let (callee, base);
            let member;
            match self.dyn_ref(sc, n, *withs, static_hops(x))? {
                DynRef::With(o) => {
                    member = Expr::Member(Box::new(Expr::Null), crate::runtime::Sym::new(n), false);
                    (callee, base) = (&member, Some((o, Value::Undefined)));
                }
                DynRef::Frame(fr) => {
                    let f = match fr.borrow().vars.get(n.as_str()).cloned() {
                        Some(Value::Empty) | None => Value::Empty,
                        Some(f) => f,
                    };
                    if let Value::Empty = f {
                        return self.uninitialized(n);
                    }
                    if optional && matches!(f, Value::Undefined | Value::Null) {
                        return Err(Ex::Short);
                    }
                    let argv = self.args(args, sc)?;
                    let r = match callable(&f) {
                        Some(_) => self.call(&f, Value::Undefined, &argv),
                        None if fr.borrow().global => {
                            self.not_a_named_function(&Value::Undefined, n)
                        }
                        None => self.not_a_function(&f),
                    };
                    self.recycle(argv);
                    return r;
                }
                DynRef::Static => return self.call_expr(x, args, optional, sc),
            }
            let pre = if optional {
                self.optional_callee(callee, &base, sc)?
            } else {
                None
            };
            let argv = self.args(args, sc)?;
            let r = self.call_expr_with(callee, base, pre, &argv, optional, sc);
            self.recycle(argv);
            return r;
        }
        // QV4's `super(...)` asks the running frame whether `this` is bound, and an arrow
        // function's frame always has one (`Runtime::LoadSuperConstructor`,
        // qv4runtime.cpp:1227): there it is "already called" before the arguments run.
        if matches!(callee, Expr::Super) && Self::arrow_this(sc).is_some() {
            return self.throw("ReferenceError", "super() already called.");
        }
        if let Some((this, key, f)) = self.super_callee(callee, sc)? {
            let argv = self.args(args, sc)?;
            let r = self.call_super_method(&f, &this, &key, &argv, optional);
            self.recycle(argv);
            return r;
        }
        // QV4's `this.name(...)` in a derived constructor before `super(...)`: the arguments
        // run first, and the call has no function if `this` is unbound after them.
        if !optional
            && let Expr::Member(o, n, false) = callee
            && Self::unbound_this(o, sc)
        {
            let argv = self.args(args, sc)?;
            if Self::unbound_this(o, sc) {
                self.recycle(argv);
                // QV4 prints the unbound `this` as the number it holds there.
                return self.not_a_method(&Value::Num(0.0), n);
            }
            let base = Some((Self::this(sc), Value::Undefined));
            let r = self.call_expr_with(callee, base, None, &argv, optional, sc);
            self.recycle(argv);
            return r;
        }
        // `(o?.f)()`: a parenthesized optional chain is still a property reference, so the
        // call's `this` is its base (ECMA-262 §13.3.9.1); a short-circuit makes the whole
        // chain, and so the callee, `undefined`.
        let (callee, base) = match callee {
            Expr::Chain(inner) if is_member_callee(inner) => match self.callee_base(inner, sc) {
                Err(Ex::Short) if optional => return Err(Ex::Short),
                Err(Ex::Short) => {
                    let argv = self.args(args, sc)?;
                    let r = self.call(&Value::Undefined, Value::Undefined, &argv);
                    self.recycle(argv);
                    return r;
                }
                b => (&**inner, b?),
            },
            _ => (callee, self.callee_base(callee, sc)?),
        };
        let pre = if optional {
            self.optional_callee(callee, &base, sc)?
        } else {
            None
        };
        let argv = self.args(args, sc)?;
        let r = self.call_expr_with(callee, base, pre, &argv, optional, sc);
        self.recycle(argv);
        r
    }

    /// V4 order: the callee's base object (and computed key) before the arguments.
    #[cfg(feature = "interp")]
    fn callee_base(&mut self, callee: &Expr, sc: &Scope) -> R<Option<(Value, Value)>> {
        Ok(match callee {
            Expr::Member(o, n, opt) if !matches!(**o, Expr::Super) => {
                if !*opt && Self::unbound_this(o, sc) {
                    // QV4 prints the unbound `this` as the number it holds there.
                    return self.not_a_method(&Value::Num(0.0), n);
                }
                let ov = self.eval_fast(o, sc)?;
                if *opt && matches!(ov, Value::Undefined | Value::Null) {
                    return Err(Ex::Short);
                }
                Some((ov, Value::Undefined))
            }
            Expr::Index(o, k, opt) if !matches!(**o, Expr::Super) => {
                let ov = self.eval_fast(o, sc)?;
                if *opt && matches!(ov, Value::Undefined | Value::Null) {
                    return Err(Ex::Short);
                }
                let kv = self.eval(k, sc)?;
                let kv = self.callee_key(&ov, kv)?;
                Some((ov, kv))
            }
            _ => None,
        })
    }

    /// ECMA-262 OptionalChain: an optional call resolves its callee before its arguments,
    /// and a nullish callee ends the chain without evaluating them. `Some(f)`: the callee's
    /// value; `None`: a builtin or native method the call dispatches itself.
    #[cfg(feature = "interp")]
    fn optional_callee(
        &mut self,
        callee: &Expr,
        base: &Option<(Value, Value)>,
        sc: &Scope,
    ) -> R<Option<Value>> {
        let f = match (callee, base) {
            (Expr::Member(_, name, _), Some((ov, _))) => {
                if crate::builtins::has_method(self, ov, name) {
                    return Ok(None);
                }
                self.get(ov, name)?
            }
            (Expr::Index(..), Some((ov, Value::Symbol(s)))) => self.get_sym(ov, s)?,
            (Expr::Index(..), Some((ov, key))) => {
                let name = key.to_js_string();
                if crate::builtins::has_method(self, ov, &name) {
                    return Ok(None);
                }
                self.get(ov, &name)?
            }
            (Expr::Super | Expr::Member(..) | Expr::Index(..), _) => return Ok(None),
            (Expr::Local(h, i, n), _) => self.local(sc, *h, *i, n)?,
            (Expr::Ident(n), _) => match Self::find_var(sc, n) {
                Some(Value::Empty) => return self.uninitialized(n),
                Some(f) => f,
                None if Self::qml_scope(sc)
                    .is_some_and(|(item, _)| self.host.has_method(item, n)) =>
                {
                    return Ok(None);
                }
                None => self.eval(callee, sc)?,
            },
            _ => self.eval(callee, sc)?,
        };
        if matches!(f, Value::Undefined | Value::Null) {
            return Err(Ex::Short);
        }
        Ok(Some(f))
    }

    #[cfg(feature = "interp")]
    fn recycle(&mut self, mut v: Vec<Value>) {
        v.clear();
        if self.argvs.len() < 32 {
            self.argvs.push(v);
        }
    }

    #[cfg(feature = "interp")]
    fn call_expr_with(
        &mut self,
        callee: &Expr,
        base: Option<(Value, Value)>,
        pre: Option<Value>,
        argv: &[Value],
        optional: bool,
        sc: &Scope,
    ) -> R<Value> {
        match callee {
            // SuperCall (ECMA-262 §13.3.7.1): construct the parent for NewTarget, bind
            // `this` to the result, then initialize this class's fields on it.
            Expr::Super => {
                let home = Self::class_scope(sc);
                let active = home.as_ref().map(|c| c.class.clone()).unwrap_or_default();
                let fr = Self::this_frame(sc);
                let nt = fr.as_ref().map(|f| f.borrow().new_target.clone());
                let this = self.super_construct(&active, argv, &nt.unwrap_or_default())?;
                self.bind_this(&Self::this(sc))?;
                if let Some(f) = &fr {
                    f.borrow_mut().this = Some(this.clone());
                }
                if let Some(Value::Obj(c)) = home.map(|c| c.class.clone())
                    && let Some(Callable::Class(cls, csc)) = c.borrow().call.clone()
                {
                    self.init_fields(&cls, &csc, &this)?;
                }
                Ok(this)
            }
            Expr::Member(o, name, opt) => {
                // A Date read from a `date` property and changed where it was read is stored
                // back (QV4::ReferenceObject, which a Date kept in a variable has left): only
                // a read of the property itself, not `o.d` or `f()` holding that Date.
                let read_as = match o.shown() {
                    Expr::Member(_, p, _) => Some(p.as_str()),
                    Expr::Index(_, k, _) => match &**k {
                        Expr::Str(p) => Some(&**p),
                        _ => None,
                    },
                    Expr::Ident(n) if Self::find_var(sc, n).is_none() => Some(n.as_str()),
                    _ => None,
                };
                let read_as = read_as.filter(|_| crate::builtins::is_date_setter(name));
                let (this, f) = {
                    let ov = match base {
                        Some((ov, _)) => ov,
                        None => {
                            let ov = self.eval_fast(o, sc)?;
                            if *opt && matches!(ov, Value::Undefined | Value::Null) {
                                return Err(Ex::Short);
                            }
                            ov
                        }
                    };
                    if let Some(r) = crate::builtins::method(self, &ov, name, argv)? {
                        if let Some(p) = read_as {
                            crate::jsrt::write_back_date(self, &ov, p);
                        }
                        if matches!(
                            name.as_str(),
                            "push"
                                | "pop"
                                | "shift"
                                | "unshift"
                                | "splice"
                                | "sort"
                                | "reverse"
                                | "fill"
                                | "copyWithin"
                        ) {
                            self.write_back(o, &ov, sc)?;
                        }
                        return Ok(r);
                    }
                    let f = match pre {
                        Some(f) => f,
                        None => {
                            self.nullish_call(&ov, name)?;
                            self.get(&ov, name)?
                        }
                    };
                    if let (Value::Item(i), Value::Undefined) = (&ov, &f)
                        && let Some(r) = self.host_call(*i, name, argv)
                    {
                        return r;
                    }
                    (ov, f)
                };
                if optional && matches!(f, Value::Undefined | Value::Null) {
                    return Err(Ex::Short);
                }
                if !matches!(&f, Value::Obj(o) if o.borrow().call.is_some()) {
                    return self.not_a_method(&this, name);
                }
                match read_as {
                    Some(p) => {
                        let r = self.call(&f, this.clone(), argv)?;
                        crate::jsrt::write_back_date(self, &this, p);
                        Ok(r)
                    }
                    None => self.call(&f, this, argv),
                }
            }
            Expr::Index(o, k, opt) => {
                let (ov, key) = match base {
                    Some(b) => b,
                    None => {
                        let ov = self.eval_fast(o, sc)?;
                        if *opt && matches!(ov, Value::Undefined | Value::Null) {
                            return Err(Ex::Short);
                        }
                        let kv = self.eval(k, sc)?;
                        let kv = self.callee_key(&ov, kv)?;
                        (ov, kv)
                    }
                };
                let name = match &key {
                    Value::Symbol(s) => {
                        let f = match pre {
                            Some(f) => f,
                            None => self.get_sym(&ov, s)?,
                        };
                        return self.call(&f, ov, argv);
                    }
                    key => key.to_js_string(),
                };
                if let Some(r) = crate::builtins::method(self, &ov, &name, argv)? {
                    return Ok(r);
                }
                let f = match pre {
                    Some(f) => f,
                    None => self.get(&ov, &name)?,
                };
                if optional && matches!(f, Value::Undefined | Value::Null) {
                    return Err(Ex::Short);
                }
                self.call(&f, ov, argv)
            }
            Expr::Local(h, i, n) => {
                let f = match pre {
                    Some(f) => f,
                    None => self.local(sc, *h, *i, n)?,
                };
                if optional && matches!(f, Value::Undefined | Value::Null) {
                    return Err(Ex::Short);
                }
                if callable(&f).is_none() {
                    return self.not_a_function(&f);
                }
                self.call(&f, Value::Undefined, argv)
            }
            _ => {
                // A function from the scope chain: called with `this` undefined.
                if let Expr::Ident(n) = callee
                    && let Some(v) = Self::find_var(sc, n)
                {
                    let f = match (pre, v) {
                        (Some(f), _) => f,
                        (None, Value::Empty) => return self.uninitialized(n),
                        (None, v) => v,
                    };
                    if optional && matches!(f, Value::Undefined | Value::Null) {
                        return Err(Ex::Short);
                    }
                    if callable(&f).is_none() {
                        return match Self::is_global_var(sc, n) {
                            true => self.not_a_named_function(&Value::Undefined, n),
                            false => self.not_a_function(&f),
                        };
                    }
                    return self.call(&f, Value::Undefined, argv);
                }
                // Unqualified C++ method of the QML scope object (`stop()` inside a Timer).
                if let Expr::Ident(n) = callee
                    && Self::find_var(sc, n).is_none()
                    && let Some((item, _)) = Self::qml_scope(sc)
                    && let Some(r) = self.host_call(item, n, argv)
                {
                    return r;
                }
                let f = match pre {
                    Some(f) => f,
                    None => self.eval(callee, sc)?,
                };
                if optional && matches!(f, Value::Undefined | Value::Null) {
                    return Err(Ex::Short);
                }
                // QV4: an unqualified call of the scope object's own method binds `this` to it.
                // The name of a type reads from any object and is a member of none.
                let mut this = Value::Undefined;
                if let Expr::Ident(n) = callee
                    && !matches!(&f, Value::Undefined)
                    && Self::find_var(sc, n).is_none()
                    && let Some((item, _)) = Self::qml_scope(sc)
                    && self
                        .get(&Value::Item(item), n)
                        .is_ok_and(|g| g.strict_eq(&f))
                    && self.host.has_member(item, n)
                {
                    this = Value::Item(item);
                }
                if let Expr::Ident(n) = callee
                    && callable(&f).is_none()
                {
                    return self.not_a_named_function(&this, n);
                }
                self.call(&f, this, argv)
            }
        }
    }

    #[cfg(feature = "interp")]
    fn args(&mut self, args: &[Expr], sc: &Scope) -> R<Vec<Value>> {
        let mut out = self.argvs.pop().unwrap_or_default();
        for a in args {
            if let Expr::Spread(e) = a {
                let v = self.eval(e, sc)?;
                out.extend(self.iterate(&v)?);
            } else {
                out.push(self.eval_fast(a, sc)?);
            }
        }
        Ok(out)
    }

    /// IteratorBindingInitialization of an array pattern (ECMA-262 §8.6.3): each element
    /// binds as the iterator steps.
    #[cfg(feature = "interp")]
    fn bind_items(
        &mut self,
        items: &[Option<Pat>],
        rest: Option<&Pat>,
        src: &mut IterSrc,
        sc: &Scope,
        decl: bool,
    ) -> R<()> {
        for it in items {
            match it {
                Some(p) => {
                    self.bind_elem(p, sc, decl, |me| Ok(me.iter_step(src)?.unwrap_or_default()))?
                }
                None => {
                    self.iter_step(src)?;
                }
            }
        }
        if let Some(r) = rest {
            self.bind_elem(r, sc, decl, |me| Ok(Value::array(me.iter_rest(src)?)))?;
        }
        Ok(())
    }

    /// One element, property or rest of a destructuring pattern, its value read by `value`:
    /// in an assignment a member target's reference is evaluated first, then the value, the
    /// default and PutValue (ECMA-262 §13.15.5.5 AssignmentElement, §13.15.5.6
    /// KeyedDestructuringAssignmentEvaluation, AssignmentRestElement/RestProperty).
    #[cfg(feature = "interp")]
    fn bind_elem(
        &mut self,
        p: &Pat,
        sc: &Scope,
        decl: bool,
        value: impl FnOnce(&mut Self) -> R<Value>,
    ) -> R<()> {
        let (q, d) = match p {
            Pat::Default(q, d) => (&**q, Some(d)),
            q => (q, None),
        };
        let t = if decl { None } else { self.target(q, sc)? };
        if t.is_none() {
            let v = value(self)?;
            return self.bind(p, v, sc, decl);
        }
        let mut v = value(self)?;
        if let Some(d) = d
            && matches!(v, Value::Undefined)
        {
            v = self.eval(d, sc)?;
        }
        self.target_put(q, t, v, sc)
    }

    /// A built-in iterable `v` whose `@@iterator` method `f` is the built-in one, while the
    /// `next` of the iterators that method makes is too (a protector, see
    /// [`JsObject::builtin_next`]): iterated without the protocol's per-step objects (the
    /// steps are the same: live for Arrays, arguments objects, Maps and Sets).
    fn builtin_iter(&self, v: &Value, f: &Value) -> Option<IterSrc> {
        let native = |own: &Value, proto: &Obj| f.strict_eq(own) && proto.borrow().builtin_next;
        let sym = &self.sym;
        match v {
            Value::Obj(o) => {
                let (list, coll, params) = {
                    let b = o.borrow();
                    let coll = match &b.internal {
                        Internal::Map(s) => Some((s.clone(), true)),
                        Internal::Set(s) => Some((s.clone(), false)),
                        _ => None,
                    };
                    let list = b.array.is_some() || matches!(b.internal, Internal::Arguments(_));
                    (list, coll, crate::url::is_params(&b))
                };
                if params {
                    return native(&sym.params_iterator, &sym.params_iter_proto).then(|| {
                        IterSrc::Params {
                            src: v.clone(),
                            pos: 0,
                        }
                    });
                }
                let array = || IterSrc::Array {
                    src: v.clone(),
                    pos: 0,
                };
                if crate::typedarray::typed(o).is_some() {
                    return native(&sym.typed_values, &sym.array_iter_proto).then(array);
                }
                if list {
                    return native(&sym.array_values, &sym.array_iter_proto).then(array);
                }
                let (store, map) = coll?;
                let (own, proto, kind) = match map {
                    true => (
                        &sym.map_entries,
                        &sym.map_iter_proto,
                        crate::value::IterKind::Entries,
                    ),
                    false => (
                        &sym.set_values,
                        &sym.set_iter_proto,
                        crate::value::IterKind::Values,
                    ),
                };
                native(own, proto).then(|| IterSrc::Coll {
                    store: Value::Obj(store),
                    map,
                    kind,
                    cursor: 0,
                })
            }
            Value::Str(s) => native(&sym.string_iterator, &sym.string_iter_proto).then(|| {
                let items: Vec<Value> = s
                    .chars()
                    .map(|c| Value::str(c.encode_utf8(&mut [0; 4])))
                    .collect();
                IterSrc::List(items.into_iter())
            }),
            // %StringIteratorPrototype%.next (ECMA-262 §22.1.5.1): code points, an unpaired
            // surrogate one of its own.
            Value::Str16(u) => native(&sym.string_iterator, &sym.string_iter_proto).then(|| {
                let items: Vec<Value> = char::decode_utf16(u.iter().copied())
                    .map(|c| match c {
                        Ok(c) => Value::str(c.encode_utf8(&mut [0; 4])),
                        Err(e) => Value::from_utf16(&[e.unpaired_surrogate()]),
                    })
                    .collect();
                IterSrc::List(items.into_iter())
            }),
            _ => None,
        }
    }

    /// GetIterator(v, sync) (ECMA-262 §7.4.3).
    pub fn get_iterator(&mut self, v: &Value) -> R<IterSrc> {
        let f = match v {
            Value::Undefined | Value::Null => Value::Undefined,
            _ => self.get_sym(v, &WellKnown::Iterator.sym())?,
        };
        self.iterator_from_method(v, &f)
    }

    /// GetIteratorFromMethod(v, f) (ECMA-262 §7.4.2), with `f` the `@@iterator` of `v`.
    pub fn iterator_from_method(&mut self, v: &Value, f: &Value) -> R<IterSrc> {
        if let Some(s) = self.builtin_iter(v, f) {
            return Ok(s);
        }
        if callable(f).is_none() {
            return match v {
                Value::Obj(_) => self.throw("TypeError", "Type error"),
                _ => self.throw("TypeError", "Type error"),
            };
        }
        let iter = self.call(f, v.clone(), &[])?;
        if !matches!(iter, Value::Obj(_)) {
            return self.throw("TypeError", "Type error");
        }
        let next = self.get(&iter, "next")?;
        Ok(IterSrc::Js {
            iter,
            next,
            done: false,
        })
    }

    /// IteratorStepValue (ECMA-262 §7.4.8): the next value, `None` once done. An abrupt
    /// step leaves the iterator done (not to be closed).
    pub fn iter_step(&mut self, s: &mut IterSrc) -> R<Option<Value>> {
        let (iter, next, done) = match s {
            IterSrc::List(i) => return Ok(i.next()),
            IterSrc::ForIn(f) => return Ok(self.for_in_next(f)?.map(|k| Value::str(&k))),
            IterSrc::Array { src, pos } => {
                let Value::Obj(o) = src.clone() else {
                    return Ok(None);
                };
                // A real array's element is read directly unless it is a hole.
                let direct = {
                    let b = o.borrow();
                    b.array.as_ref().map(|a| {
                        (
                            b.array_length(),
                            a.get(*pos).filter(|_| !b.is_hole(*pos)).cloned(),
                        )
                    })
                };
                let item = match direct {
                    Some((n, _)) if *pos >= n => None,
                    Some((_, Some(x))) => Some(x),
                    Some((_, None)) => Some(self.get(src, &pos.to_string())?),
                    None => {
                        let n = self.get(src, "length")?;
                        if (*pos as f64) < self.to_numeric(&n)? {
                            Some(self.get(src, &pos.to_string())?)
                        } else {
                            None
                        }
                    }
                };
                match item {
                    Some(_) => *pos += 1,
                    None => *src = Value::Undefined,
                }
                return Ok(item);
            }
            IterSrc::Coll {
                store,
                map,
                kind,
                cursor,
            } => {
                let Value::Obj(st) = store.clone() else {
                    return Ok(None);
                };
                let v = crate::builtins::coll_step(&st, *map, *kind, cursor);
                if v.is_none() {
                    *store = Value::Undefined;
                }
                return Ok(v);
            }
            IterSrc::Params { src, pos } => {
                let Value::Obj(o) = src.clone() else {
                    return Ok(None);
                };
                let v = crate::url::params_step(&o, crate::value::IterKind::Entries, pos);
                if v.is_none() {
                    *src = Value::Undefined;
                }
                return Ok(v);
            }
            IterSrc::Js { iter, next, done } => (iter, next, done),
        };
        if *done {
            return Ok(None);
        }
        *done = true;
        let (iter, next) = (iter.clone(), next.clone());
        let r = self.call(&next, iter, &[])?;
        if !matches!(r, Value::Obj(_)) {
            return self.throw("TypeError", "Type error");
        }
        if self.get(&r, "done")?.truthy() {
            return Ok(None);
        }
        let v = self.get(&r, "value")?;
        if let IterSrc::Js { done, .. } = s {
            *done = false;
        }
        Ok(Some(v))
    }

    /// The values left in `s` (an array pattern's rest element).
    pub fn iter_rest(&mut self, s: &mut IterSrc) -> R<Vec<Value>> {
        let mut out = Vec::new();
        while let Some(x) = self.iter_step(s)? {
            out.push(x);
        }
        Ok(out)
    }

    /// IteratorClose (ECMA-262 §7.4.11) for a normal completion: calls `return` of an
    /// iterator not yet done.
    pub fn iter_close(&mut self, s: &mut IterSrc) -> R<()> {
        let IterSrc::Js { iter, done, .. } = s else {
            return Ok(());
        };
        if std::mem::replace(done, true) {
            return Ok(());
        }
        let iter = iter.clone();
        let f = self.get(&iter, "return")?;
        if matches!(f, Value::Undefined | Value::Null) {
            return Ok(());
        }
        if callable(&f).is_none() {
            return self.throw("TypeError", "Type error");
        }
        let r = self.call(&f, iter, &[])?;
        if !matches!(r, Value::Obj(_)) {
            return self.throw("TypeError", "Type error");
        }
        Ok(())
    }

    /// IteratorClose for the throw completion `e`, which wins over what closing throws.
    pub fn iter_close_throw(&mut self, s: &mut IterSrc, e: Ex) -> Ex {
        let _ = self.iter_close(s);
        e
    }

    /// The values `v` iterates to (Array.from, spread, collection constructors).
    pub fn iterate(&mut self, v: &Value) -> R<Vec<Value>> {
        let src = self.get_iterator(v)?;
        self.collect_iter(src)
    }

    /// The items of iterator `src` (IteratorToList, ECMA-262 §7.4.12).
    pub fn collect_iter(&mut self, mut src: IterSrc) -> R<Vec<Value>> {
        if let IterSrc::Array {
            src: Value::Obj(o),
            pos: 0,
        } = &src
            && let b = o.borrow()
            && let Some(a) = &b.array
            && b.holes.is_none()
        {
            return Ok(a.clone());
        }
        self.iter_rest(&mut src)
    }

    // ---------- expressions ----------

    /// `eval` with the leaf kinds inlined, for operands.
    #[inline(always)]
    #[cfg(feature = "interp")]
    fn eval_fast(&mut self, e: &Expr, sc: &Scope) -> R<Value> {
        match e {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Local(h, i, n) => self.local(sc, *h, *i, n),
            Expr::This => self.this_binding(sc),
            _ => self.eval(e, sc),
        }
    }

    #[cfg(feature = "interp")]
    pub fn eval(&mut self, e: &Expr, sc: &Scope) -> R<Value> {
        Ok(match e {
            Expr::Num(n) => Value::Num(*n),
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Str16(u) => Value::Str16(u.clone()),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Null => Value::Null,
            Expr::Hole => Value::Undefined,
            Expr::Local(h, i, n) => self.local(sc, *h, *i, n)?,
            Expr::Ident(n) => self.lookup(sc, n)?,
            Expr::This => self.this_binding(sc)?,
            Expr::Member(o, n, opt) => {
                if matches!(**o, Expr::Super) {
                    let this = self.super_this(sc)?;
                    let key = PropKey::Str(n.to_string());
                    return self.super_get(&Self::home_object(sc), &this, &key);
                }
                let ov = self.eval_fast(o, sc)?;
                if *opt && matches!(ov, Value::Undefined | Value::Null) {
                    return Err(Ex::Short);
                }
                match ov {
                    Value::Item(i) => self.item_get(i, *n)?,
                    _ => self.get(&ov, n)?,
                }
            }
            Expr::Call(c, a, opt) => self.call_expr(c, a, *opt, sc)?,
            Expr::Binary(op, a, b) => {
                let x = self.eval_fast(a, sc)?;
                let y = self.eval_fast(b, sc)?;
                if let (Value::Num(p), Value::Num(q)) = (&x, &y)
                    && let Some(r) = num_binary(*op, *p, *q)
                {
                    r
                } else {
                    self.binary(*op, x, y)?
                }
            }
            _ => return self.eval_warm(e, sc),
        })
    }

    /// Less frequent expression kinds, out of line to keep `eval`'s frame small.
    #[inline(never)]
    #[cfg(feature = "interp")]
    fn eval_warm(&mut self, e: &Expr, sc: &Scope) -> R<Value> {
        Ok(match e {
            Expr::Index(o, k, _) if matches!(**o, Expr::Super) => {
                let this = self.super_this(sc)?;
                let key = PropKey::of(&self.eval(k, sc)?);
                return self.super_get(&Self::home_object(sc), &this, &key);
            }
            Expr::Index(o, k, opt) => {
                let ov = self.eval_fast(o, sc)?;
                if *opt && matches!(ov, Value::Undefined | Value::Null) {
                    return Err(Ex::Short);
                }
                let kv = self.eval_fast(k, sc)?;
                if let (Value::Obj(ob), Value::Num(n)) = (&ov, &kv)
                    && *n >= 0.0
                    && *n < MAX_ARRAY_INDEX as f64
                    && n.fract() == 0.0
                    && let Some(e) = ob.borrow().dense_element(*n as usize)
                {
                    return Ok(e);
                }
                self.get_key(&ov, &kv)?
            }
            Expr::New(c, a) => {
                let f = self.eval(c, sc)?;
                let argv = self.args(a, sc)?;
                let r = self.construct(&f, &argv);
                self.recycle(argv);
                r?
            }
            Expr::Chain(inner) => match self.eval(inner, sc) {
                Err(Ex::Short) => Value::Undefined,
                r => r?,
            },
            Expr::Unary(op, a) => self.unary(*op, a, sc)?,
            Expr::Logical(op, a, b) => {
                let x = self.eval_fast(a, sc)?;
                let take_left = match *op {
                    LogicalOp::And => !x.truthy(),
                    LogicalOp::Or => x.truthy(),
                    LogicalOp::Nullish => !matches!(x, Value::Undefined | Value::Null),
                };
                if take_left { x } else { self.eval(b, sc)? }
            }
            Expr::Cond(c, a, b) => {
                if self.eval(c, sc)?.truthy() {
                    self.eval(a, sc)?
                } else {
                    self.eval(b, sc)?
                }
            }
            Expr::Array(items) if items.iter().any(|e| matches!(e, Expr::Hole)) => {
                return self.eval_cold(e, sc);
            }
            Expr::Array(items) => Value::array(self.args(items, sc)?),
            Expr::Spread(e) => self.eval(e, sc)?,
            Expr::Func(f) => fn_value(Callable::Closure(f.clone(), sc.clone())),
            _ => return self.eval_cold(e, sc),
        })
    }

    /// The rarer expression kinds, out of line to keep `eval`'s frame small.
    #[inline(never)]
    #[cfg(feature = "interp")]
    fn eval_cold(&mut self, e: &Expr, sc: &Scope) -> R<Value> {
        Ok(match e {
            Expr::Dynamic(n, withs, x) => self.dyn_value(n, *withs, x, sc)?,
            // ECMA-262 §13.3.6.1 step 6: the callee is the name `eval` and its value is
            // %eval%; the argument list is evaluated first, and a non-string is the result.
            Expr::DirectEval(scope, call) => {
                let Expr::Call(callee, args, _) = &**call else {
                    unreachable!("Expr::DirectEval wraps a call")
                };
                let f = self.eval(callee, sc)?;
                let intrinsic = matches!(
                    callable(&f),
                    Some(Callable::Native(n, _)) if n == crate::builtins::EVAL
                );
                if !intrinsic {
                    return self.call_expr(callee, args, false, sc);
                }
                let argv = self.args(args, sc)?;
                let src = argv.first().cloned().unwrap_or_default();
                self.recycle(argv);
                match src {
                    Value::Str(src) => self.direct_eval(&src, scope, sc)?,
                    v => v,
                }
            }
            // An array literal with elisions: the holes are absent elements (ECMA-262 §13.2.4.1).
            Expr::Array(items) => {
                let (mut v, mut holes) = (Vec::new(), Vec::new());
                for e in items {
                    match e {
                        Expr::Hole => {
                            holes.push(v.len());
                            v.push(Value::Undefined);
                        }
                        Expr::Spread(x) => {
                            let x = self.eval(x, sc)?;
                            v.extend(self.iterate(&x)?);
                        }
                        _ => v.push(self.eval_fast(e, sc)?),
                    }
                }
                Value::array_holes(v, holes)
            }
            Expr::Regex(p, f) => crate::builtins::regex_value(p, f),
            Expr::TemplateObject(t) => {
                let key = Rc::as_ptr(t) as usize;
                match self.template_object(key) {
                    Some(v) => v,
                    None => self.new_template_object(
                        key,
                        Some(t.clone()),
                        t.cooked.iter().map(Option::as_deref),
                        t.raw.iter().map(String::as_str),
                    ),
                }
            }
            Expr::Template(q, es) => {
                let mut s = crate::value::JsBuf::new();
                for (i, part) in q.iter().enumerate() {
                    s.push_str(part);
                    if let Some(e) = es.get(i) {
                        let v = self.eval(e, sc)?;
                        self.push_substitution(&mut s, &v)?;
                    }
                }
                s.finish()
            }
            Expr::Update(op, prefix, p) => {
                let mut t = self.target(p, sc)?;
                let old = self.target_get(p, &mut t, sc)?;
                let old = self.to_numeric(&old)?;
                let new = if *op == UpdateOp::Inc {
                    old + 1.0
                } else {
                    old - 1.0
                };
                self.target_put(p, t, Value::Num(new), sc)?;
                Value::Num(if *prefix { new } else { old })
            }
            Expr::Assign(op, p, rhs) => {
                let mut t = self.target(p, sc)?;
                let v = match *op {
                    AssignOp::Assign => self.eval(rhs, sc)?,
                    AssignOp::Logical(l) => {
                        let cur = self.target_get(p, &mut t, sc)?;
                        let keep = match l {
                            LogicalOp::Or => cur.truthy(),
                            LogicalOp::And => !cur.truthy(),
                            LogicalOp::Nullish => !matches!(cur, Value::Undefined | Value::Null),
                        };
                        if keep {
                            return Ok(cur);
                        }
                        self.eval(rhs, sc)?
                    }
                    AssignOp::Bin(b) => {
                        let cur = self.target_get(p, &mut t, sc)?;
                        let r = self.eval(rhs, sc)?;
                        self.binary(b, cur, r)?
                    }
                };
                self.target_put(p, t, v.clone(), sc)?;
                v
            }
            Expr::Object(props) => {
                let o = new_obj();
                {
                    let mut b = o.borrow_mut();
                    b.keys.reserve(props.len());
                    b.props.reserve(props.len());
                }
                let ov = Value::Obj(o.clone());
                // MakeMethod (§10.2.7): the literal is the [[HomeObject]] of its methods.
                let msc = props
                    .iter()
                    .any(|(_, v, _)| crate::ir::is_super_method(v))
                    .then(|| {
                        let m = Self::child(sc);
                        m.borrow_mut().home = Some(ov.clone());
                        m
                    });
                for (k, v, kind) in props {
                    let vsc = match &msc {
                        Some(m) if crate::ir::is_super_method(v) => m,
                        _ => sc,
                    };
                    if let Expr::Spread(src) = v {
                        let s = self.eval(src, sc)?;
                        self.copy_data_properties(&o, &s, &[])?;
                        continue;
                    }
                    if *kind == PropKind::Proto {
                        let val = self.eval(v, vsc)?;
                        literal_proto(&o, &val);
                        continue;
                    }
                    let name: Rc<str> = match k {
                        Key::Name(n) => n.clone(),
                        _ => match self.key(k, sc)? {
                            PropKey::Str(n) => n.into(),
                            PropKey::Sym(sy) => {
                                let val = self.eval(v, vsc)?;
                                if crate::ir::is_anonymous_fn(v) {
                                    let prefix = match kind {
                                        PropKind::Init | PropKind::Proto => "",
                                        PropKind::Get => "get ",
                                        PropKind::Set => "set ",
                                    };
                                    crate::jsrt::name_function_pk(
                                        &val,
                                        prefix,
                                        &PropKey::Sym(sy.clone()),
                                    );
                                }
                                match kind {
                                    PropKind::Init | PropKind::Proto => {
                                        define_sym(&o, &sy, val, true)
                                    }
                                    PropKind::Get | PropKind::Set => define_sym_accessor(
                                        &o,
                                        &sy,
                                        *kind == PropKind::Get,
                                        val,
                                        true,
                                    ),
                                }
                                continue;
                            }
                        },
                    };
                    let val = self.eval(v, vsc)?;
                    if matches!(k, Key::Computed(_)) && crate::ir::is_anonymous_fn(v) {
                        let prefix = match kind {
                            PropKind::Init | PropKind::Proto => "",
                            PropKind::Get => "get ",
                            PropKind::Set => "set ",
                        };
                        set_function_name(&val, &format!("{prefix}{name}"));
                    }
                    match kind {
                        PropKind::Init | PropKind::Proto => set_own_rc(&o, name, val),
                        PropKind::Get | PropKind::Set => {
                            define_accessor(&o, &name, *kind == PropKind::Get, val, true)
                        }
                    }
                }
                ov
            }
            Expr::DefineField(k, v, name) => {
                let key = self.eval(k, sc)?;
                let key = self.to_property_key(&key)?;
                let val = self.eval(v, sc)?;
                define_field_pk(self, &Self::this(sc), &key, val, *name)?;
                Value::Undefined
            }
            Expr::Seq(es) => {
                let mut last = Value::Undefined;
                for e in es {
                    last = self.eval(e, sc)?;
                }
                last
            }
            Expr::Class(c) => self.make_class(c, sc)?,
            Expr::Super => self.super_base(&Self::home_object(sc)),
            // GetNewTarget (ECMA-262 §9.4.5): the [[NewTargetValue]] of GetThisEnvironment.
            Expr::NewTarget => Self::this_frame(sc)
                .map(|f| f.borrow().new_target.clone())
                .unwrap_or_default(),
            Expr::Unsupported(s) => {
                return self.throw("SyntaxError", format!("unsupported construct {s}"));
            }
            // A generator body's yields are lowered away (`ir` genlower).
            Expr::Yield(..) => return self.throw("SyntaxError", "unsupported construct Yield"),
            _ => unreachable!(),
        })
    }

    /// Own enumerable (key, value) pairs.
    pub fn entries(&self, v: &Value) -> Vec<(String, Value)> {
        match v {
            Value::Obj(o) => {
                let b = o.borrow();
                // OrdinaryOwnPropertyKeys: the present elements, then the other own keys.
                if let Some(a) = &b.array {
                    // An element QV4 has no attribute byte for is no property (`HIDDEN`).
                    let bytes = b.has_bytes();
                    // QV4::Sequence lists its elements, not what else was written to it.
                    let named = match crate::valuetypes::is_readonly_sequence(&b) {
                        true => &[][..],
                        false => &b.keys[..],
                    };
                    let mut es: Vec<(String, Value)> =
                        a.iter()
                            .enumerate()
                            .filter(|(i, _)| !b.is_hole(*i))
                            .map(|(i, v)| (i.to_string(), v.clone()))
                            .filter(|(k, _)| !bytes || b.attr(k) & crate::value::HIDDEN == 0)
                            .chain(named.iter().map(|k| {
                                (k.to_string(), b.props.get(k).cloned().unwrap_or_default())
                            }))
                            .collect();
                    // An element held as an ordinary property (an accessor one) is listed
                    // at its index.
                    if b.keys.iter().any(|k| array_index(k).is_some()) {
                        es.sort_by_key(|(k, _)| array_index(k).unwrap_or(usize::MAX));
                    }
                    return es;
                }
                // A typed array's integer-indexed elements come first (ECMA-262 §10.4.5.7).
                if let Internal::TypedArray(t) = &b.internal {
                    return t
                        .values()
                        .into_iter()
                        .enumerate()
                        .map(|(i, v)| (i.to_string(), v))
                        .chain(
                            b.keys.iter().map(|k| {
                                (k.to_string(), b.props.get(k).cloned().unwrap_or_default())
                            }),
                        )
                        .collect();
                }
                // A key without a value is an object-literal accessor (see `own_entries`).
                b.keys
                    .iter()
                    .map(|k| (k.to_string(), b.props.get(k).cloned().unwrap_or_default()))
                    .collect()
            }
            // A String's own keys are its code unit indices (ECMA-262 §10.4.3.3): each half
            // of a surrogate pair is an element.
            Value::Str(_) | Value::Str16(_) => v
                .js_str()
                .into_iter()
                .flat_map(crate::value::JsStr::code_units)
                .enumerate()
                .map(|(i, u)| (i.to_string(), Value::from_utf16(&[u])))
                .collect(),
            Value::Color(c) => (crate::valuetypes::COLOR_PROPERTIES.iter())
                .map(|k| (k.to_string(), crate::builtins::color_prop(c, k)))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Where the properties a QML object `item` does not have itself are looked up: what
    /// `Object.defineProperty` put on it ([`Host::extras`]), else Object.prototype.
    pub fn item_holder(&mut self, item: &Value) -> Obj {
        let own = match item {
            Value::Item(i) => self.host.extras(*i, false),
            _ => None,
        };
        own.unwrap_or_else(|| self.object_proto.clone())
    }

    /// Own enumerable keys, a QML object's properties included.
    pub fn keys(&mut self, v: &Value) -> R<Vec<String>> {
        Ok(match v {
            Value::Item(i) => self.host.keys(*i),
            Value::Obj(o) if o.borrow().is_proxy() => {
                return crate::props::enumerable_own_keys(self, v);
            }
            Value::Obj(o) if crate::valuetypes::members(o).is_some() => {
                self.own_entries(v)?.into_iter().map(|(k, _)| k).collect()
            }
            _ => self.entries(v).into_iter().map(|(k, _)| k).collect(),
        })
    }

    /// The enumeration `for (k in v)` runs (ECMA-262 §14.7.5.6 ForIn/OfHeadEvaluation): over
    /// ToObject(`v`), none for undefined and null. Step it with [`Self::for_in_next`].
    pub fn for_in(&self, v: &Value) -> ForIn {
        let mut s = ForIn {
            object: (!matches!(v, Value::Undefined | Value::Null)).then(|| v.clone()),
            item: match v {
                Value::Item(i) => Some(*i),
                _ => None,
            },
            ..ForIn::default()
        };
        // A built-in function's own non-enumerable `length` and `name`, which
        // [[OwnPropertyKeys]] does not list for it.
        if let Value::Native(_) = v {
            s.visited.extend(["length".into(), "name".into()]);
        }
        s
    }

    /// The next key of a `for-in` enumeration (%ForInIteratorPrototype%.next, ECMA-262
    /// §14.7.5.10.2.1): each object's own keys are read when the walk up the prototype chain
    /// reaches it, and each key's [[GetOwnProperty]] when it is visited, so a property deleted
    /// before it is reached is skipped and one already visited shadows the same name further up.
    pub fn for_in_next(&mut self, s: &mut ForIn) -> R<Option<String>> {
        while let Some(o) = s.object.clone() {
            if !s.object_was_visited {
                s.remaining = crate::props::own_keys(self, &o)?.into();
                s.object_was_visited = true;
            }
            while let Some(k) = s.remaining.pop_front() {
                if s.visited.contains(&k) {
                    continue;
                }
                // A QML object's own signals and methods shadow inherited keys too.
                if let Some(i) = s.item
                    && !matches!(o, Value::Item(_))
                    && (self.host.get(i, &k).is_some() || self.host.has_method(i, &k))
                {
                    s.visited.insert(k);
                    continue;
                }
                let d = match crate::props::own_desc(self, &o, &k)? {
                    Some(d) => d,
                    // A name script defined on a value type is listed, though not its own.
                    None => match &o {
                        Value::Obj(vt) => match crate::valuetypes::members(vt) {
                            Some(m) => match crate::props::own_desc(self, &Value::Obj(m), &k)? {
                                Some(d) => d,
                                None => continue,
                            },
                            None => continue,
                        },
                        _ => continue,
                    },
                };
                // V4 lists no enum value among the keys of `Qt` and of a QML type, though
                // their descriptors say enumerable (see `props::own_desc`).
                let listed = match &o {
                    Value::Obj(ob) => {
                        let b = ob.borrow();
                        !(crate::qtobj::is_qt(&b) || crate::types::is_type_wrapper(&b))
                            || b.keys.iter().any(|x| **x == *k)
                    }
                    _ => true,
                };
                let enumerable = listed && d.enumerable == Some(true);
                if enumerable {
                    s.visited.insert(k.clone());
                    return Ok(Some(k));
                }
                s.visited.insert(k);
            }
            s.object = crate::props::get_prototype(self, &o)?.map(Value::Obj);
            s.object_was_visited = false;
        }
        Ok(None)
    }

    /// CopyDataProperties (ECMA-262 §7.3.25): defines on `o` the own enumerable properties of
    /// `src` whose keys are not `excluded`, in [[OwnPropertyKeys]] order. Each key's
    /// [[GetOwnProperty]] and [[Get]] run as it is copied; an excluded key is never read.
    pub fn copy_data_properties(&mut self, o: &Obj, src: &Value, excluded: &[PropKey]) -> R<()> {
        let s = match src {
            Value::Item(i) => {
                for k in self.host.keys(*i) {
                    if !excluded
                        .iter()
                        .any(|e| matches!(e, PropKey::Str(e) if *e == k))
                    {
                        let x = self.get(src, &k)?;
                        set_own(o, &k, x);
                    }
                }
                return Ok(());
            }
            Value::Obj(s) => Some(s),
            Value::Str(_) => None,
            _ => return Ok(()),
        };
        let keys = if let Some(s) = s.filter(|s| {
            let b = s.borrow();
            !b.is_proxy() && b.accessors.is_none()
        }) {
            // No string-keyed accessor: copying the string keys runs no code.
            for (k, x) in self.entries(src) {
                if !excluded
                    .iter()
                    .any(|e| matches!(e, PropKey::Str(e) if *e == k))
                {
                    set_own(o, &k, x);
                }
            }
            let b = s.borrow();
            b.ext
                .iter()
                .flat_map(|e| &e.syms)
                .map(|p| PropKey::Sym(p.key.clone()))
                .collect()
        } else {
            crate::props::own_property_keys(self, src)?
        };
        for k in keys {
            if excluded.contains(&k)
                || !crate::props::own_desc_pk(self, src, &k)?
                    .is_some_and(|d| d.enumerable == Some(true))
            {
                continue;
            }
            let x = self.get_pk(src, &k)?;
            match &k {
                PropKey::Str(k) => set_own(o, k, x),
                PropKey::Sym(sy) => define_sym(o, sy, x, true),
            }
        }
        Ok(())
    }

    /// [`Interp::entries`] with accessor values read through their getters.
    pub fn own_entries(&mut self, v: &Value) -> R<Vec<(String, Value)>> {
        // A value type lists what script defined on it after its own properties.
        if let Value::Obj(o) = v
            && let Some(m) = crate::valuetypes::members(o)
        {
            let mut es = self.entries(v);
            es.extend(self.own_entries(&Value::Obj(m))?);
            return Ok(es);
        }
        if let Value::Item(i) = v {
            let mut es = Vec::new();
            for k in self.host.keys(*i) {
                let x = self.get(v, &k)?;
                es.push((k, x));
            }
            return Ok(es);
        }
        if let Value::Obj(o) = v
            && o.borrow().is_proxy()
        {
            // EnumerableOwnProperties (ECMA-262 §7.3.23): each key's descriptor, then its value.
            let mut es = Vec::new();
            for k in crate::props::own_keys(self, v)? {
                if crate::props::own_desc(self, v, &k)?.is_some_and(|d| d.enumerable == Some(true))
                {
                    let x = self.get(v, &k)?;
                    es.push((k, x));
                }
            }
            return Ok(es);
        }
        let mut es = self.entries(v);
        if let Value::Obj(o) = v
            && o.borrow().accessors.is_some()
            && o.borrow().array.is_none()
        {
            for (k, x) in es.iter_mut() {
                if !o.borrow().props.contains_key(k.as_str()) {
                    *x = self.get(v, k)?;
                }
            }
        }
        Ok(es)
    }

    /// ToPropertyKey (ECMA-262 §7.1.19).
    pub fn to_property_key(&mut self, v: &Value) -> R<PropKey> {
        let p;
        let v = match v {
            Value::Obj(_) => {
                p = self.to_primitive(v, Hint::String)?;
                &p
            }
            v => v,
        };
        Ok(match v {
            Value::Symbol(s) => PropKey::Sym(s.clone()),
            v => PropKey::Str(self.to_string(v)?),
        })
    }

    pub fn to_string(&mut self, v: &Value) -> R<String> {
        if let Value::Symbol(_) = v {
            return self.type_error();
        }
        if let Value::Obj(_) | Value::Item(_) = v {
            let p = self.to_primitive(v, Hint::String)?;
            return self.to_string(&p);
        }
        // A QML object converts by its `toString`: `QQuickItem(0x...)`.
        if let Value::Item(i) = v
            && let Some(r) = self.host_call(*i, "toString", &[])
        {
            let r = r?;
            if !matches!(r, Value::Obj(_) | Value::Item(_) | Value::Undefined) {
                return self.to_string(&r);
            }
        }
        if let Value::Item(i) = v
            && let Some(s) = self.host.object_text(*i)
        {
            return Ok(s);
        }
        v.try_js_string().or_else(|_| self.stack_overflow())
    }

    /// ToString (ECMA-262 §7.1.17) as a String value, keeping an unpaired surrogate that
    /// [`Self::to_string`]'s UTF-8 replaces.
    pub fn to_str_value(&mut self, v: &Value) -> R<Value> {
        match v {
            Value::Str(_) | Value::Str16(_) => Ok(v.clone()),
            Value::Obj(_) | Value::Item(_) => {
                let p = self.to_primitive(v, Hint::String)?;
                self.to_str_value(&p)
            }
            _ => self.to_string(v).map(|s| Value::str(&s)),
        }
    }

    /// ToString as QV4's `Value::toString(engine)` does it, for `+` with a string, a template's
    /// substitution, `String`, `Symbol`, `String.prototype.concat` and RegExp's `exec` and
    /// `test`: its TypeError for a symbol says what was converted, where [`Self::to_string`]
    /// throws the plain one.
    pub fn to_str_named(&mut self, v: &Value) -> R<Value> {
        let p = match v {
            Value::Obj(o) if o.borrow().call.is_none() => self.to_primitive(v, Hint::String)?,
            v => v.clone(),
        };
        if let Value::Symbol(_) = p {
            return self.throw("TypeError", "Cannot convert a symbol to a string.");
        }
        self.to_str_value(&p)
    }

    /// Appends a template's substitution `v` to `out`.
    pub fn push_substitution(&mut self, out: &mut crate::value::JsBuf, v: &Value) -> R<()> {
        match v {
            Value::Symbol(_) | Value::Obj(_) => {
                // V4 adds a substitution as `+` does: ToPrimitive without a hint.
                let p = self.to_primitive(v, Hint::Default)?;
                let p = self.to_str_named(&p)?;
                out.push_value(&p);
                Ok(())
            }
            v => self.push_string(out, v),
        }
    }

    /// Appends ToString(`v`) to `out` (a join's element) without
    /// first making a String value of a number or boolean.
    pub fn push_string(&mut self, out: &mut crate::value::JsBuf, v: &Value) -> R<()> {
        match v {
            Value::Str(s) => out.push_str(s),
            Value::Str16(u) => out.push(crate::value::JsStr::Utf16(u)),
            Value::Obj(_) | Value::Item(_) => {
                let p = self.to_str_value(v)?;
                out.push_value(&p);
            }
            v => out.push_str(&self.to_string(v)?),
        }
        Ok(())
    }

    /// `Array.prototype.join` on array `o` (ECMA-262 §23.1.3.18): each element, read as the
    /// join reaches it, through ToString, so an object's own `toString`, `valueOf` or
    /// `Symbol.toPrimitive` runs; `undefined`/`null` join as empty and an array joined further
    /// up (a cycle) as `""` ([`crate::value::joining`]).
    pub fn join(&mut self, o: &Obj, sep: &Value) -> R<Value> {
        let len = o.borrow().array.as_ref().map_or(0, Vec::len);
        let joined = crate::value::joining(o, || {
            let mut out = crate::value::JsBuf::new();
            for k in 0..len {
                if k > 0 {
                    out.push_value(sep);
                }
                let v = o.borrow().array.as_ref().and_then(|a| a.get(k).cloned());
                if let Some(v) = v.filter(|v| !matches!(v, Value::Undefined | Value::Null)) {
                    self.push_string(&mut out, &v)?;
                }
            }
            Ok(out.finish())
        });
        match joined {
            Ok(Some(r)) => r,
            Ok(None) => Ok(Value::str("")),
            Err(_) => self.stack_overflow(),
        }
    }

    #[cfg(feature = "interp")]
    #[inline(never)]
    fn delete(&mut self, a: &Expr, sc: &Scope) -> R<Value> {
        match a {
            // A Super Reference is made (the this binding, then the key) and cannot be
            // deleted (ECMA-262 §13.5.1.2 step 5.b).
            Expr::Member(o, _, _) if matches!(**o, Expr::Super) => {
                self.super_this(sc)?;
                return self.delete_super();
            }
            Expr::Index(o, k, _) if matches!(**o, Expr::Super) => {
                self.super_this(sc)?;
                let k = self.eval(k, sc)?;
                self.to_property_key(&k)?;
                return self.delete_super();
            }
            // `delete a?.b`: a short-circuit anywhere in the chain deletes nothing.
            Expr::Chain(inner) => {
                return match self.delete(inner, sc) {
                    Err(Ex::Short) => Ok(Value::Bool(true)),
                    r => r,
                };
            }
            // QV4: nothing is deleted from the unbound `this` of a derived constructor.
            Expr::Member(o, _, _) if Self::unbound_this(o, sc) => return Ok(Value::Bool(true)),
            Expr::Member(o, n, opt) => {
                let ov = self.eval(o, sc)?;
                if *opt && matches!(ov, Value::Undefined | Value::Null) {
                    return Err(Ex::Short);
                }
                return match ov {
                    Value::Obj(ob) => self.delete_property(&ob, n),
                    ov => self.delete_of_primitive(&ov, &PropKey::Str(n.to_string()), self.strict),
                };
            }
            Expr::Index(o, k, opt) => {
                let ov = self.eval(o, sc)?;
                if *opt && matches!(ov, Value::Undefined | Value::Null) {
                    return Err(Ex::Short);
                }
                let k = self.eval(k, sc)?;
                if matches!(ov, Value::Undefined | Value::Null) {
                    return self.not_an_object(&ov);
                }
                let k = self.reference_key(&ov, &k)?;
                return match ov {
                    Value::Obj(ob) => self.delete_property_pk(&ob, &k),
                    ov => self.delete_of_primitive(&ov, &k, self.strict),
                };
            }
            // A binding declared in a function is not deletable (ECMA-262 §9.1.1.1.7). Any
            // other name deletes nothing and gives true, as V4 does for an unresolvable name
            // and for a library's top-level `var` and function.
            Expr::Local(..) => return Ok(Value::Bool(false)),
            Expr::Ident(n) => {
                return Ok(Value::Bool(
                    Self::delete_eval_var(sc, n) || !Self::declared_in_function(sc, n),
                ));
            }
            // DeleteBinding: of a `with` object's property, [[Delete]] (ECMA-262 §9.1.1.2.7);
            // a `var` a direct eval declared is deletable (§19.2.1.3 step 16.b).
            Expr::Dynamic(n, withs, x) => {
                return match self.dyn_ref(sc, n, *withs, static_hops(x))? {
                    DynRef::With(Value::Obj(o)) => self.delete_property(&o, n),
                    DynRef::With(_) => Ok(Value::Bool(false)),
                    DynRef::Frame(fr) => Ok(Value::Bool(Self::delete_eval_var(&fr, n))),
                    // A generator's block binding (a scope-object member) is no property.
                    DynRef::Static if matches!(**x, Expr::Member(..)) => Ok(Value::Bool(false)),
                    DynRef::Static => self.delete(x, sc),
                };
            }
            // Not a reference: the operand still runs (ECMA-262 §13.5.1.2 step 2).
            e => {
                self.eval(e, sc)?;
            }
        }
        Ok(Value::Bool(true))
    }

    /// `delete v[k]` for a `v` that is no object: QV4's ToObject TypeError for undefined and
    /// null, and a string's own `length` and index properties refuse (a TypeError if
    /// `strict`). Of a QML object a property script gave it goes, as [[Delete]] has it; a
    /// member stays.
    pub fn delete_of_primitive(&mut self, v: &Value, k: &PropKey, strict: bool) -> R<Value> {
        if matches!(v, Value::Undefined | Value::Null) {
            return self.no_object(v);
        }
        if let Value::Item(i) = v
            && let Some(own) = self.host.extras(*i, false)
        {
            return self.delete_property_or_throw_pk(&own, k, strict);
        }
        if let (true, PropKey::Str(k)) = (v.is_str(), k) {
            let own = k == "length"
                || match idx(k) {
                    Some(i) => (i as f64) < self.get(v, "length")?.to_number(),
                    None => false,
                };
            if own && strict {
                return self.type_error();
            }
            return Ok(Value::Bool(!own));
        }
        Ok(Value::Bool(true))
    }

    /// `delete o[k]` (ECMA-262 §13.5.1.2): false where [[Delete]] refuses, a TypeError in
    /// strict mode code.
    pub fn delete_property(&mut self, o: &Obj, k: &str) -> R<Value> {
        self.delete_property_or_throw(o, k, self.strict)
    }

    /// [`Interp::delete_property`] for a property key.
    pub fn delete_property_pk(&mut self, o: &Obj, k: &PropKey) -> R<Value> {
        self.delete_property_or_throw_pk(o, k, self.strict)
    }

    /// [`Interp::delete_property_or_throw`] for a property key.
    pub fn delete_property_or_throw_pk(&mut self, o: &Obj, k: &PropKey, strict: bool) -> R<Value> {
        match k {
            PropKey::Str(k) => self.delete_property_or_throw(o, k, strict),
            PropKey::Sym(_) => {
                if crate::props::delete_pk(self, o, k)? {
                    return Ok(Value::Bool(true));
                }
                if strict {
                    return self.type_error();
                }
                Ok(Value::Bool(false))
            }
        }
    }

    /// `delete o[k]` that throws a TypeError where [[Delete]] refuses if `strict`.
    pub fn delete_property_or_throw(&mut self, o: &Obj, k: &str, strict: bool) -> R<Value> {
        if idx(k).is_some() && crate::valuetypes::is_readonly_sequence(&o.borrow()) {
            return self.throw("TypeError", "Cannot delete from a readonly container");
        }
        if crate::builtins::is_easing(&o.borrow()) {
            return Ok(Value::Bool(true));
        }
        if crate::props::delete(self, o, k)? {
            return Ok(Value::Bool(true));
        }
        if strict {
            return self.type_error();
        }
        Ok(Value::Bool(false))
    }

    /// [[Call]] of a bound function (ECMA-262 10.4.1.1).
    #[inline(never)]
    fn call_bound(&mut self, b: &BoundFn, args: &[Value]) -> R<Value> {
        let argv = [&b.args[..], args].concat();
        self.call(&b.target, b.this.clone(), &argv)
    }

    /// [[Construct]] of a bound function (ECMA-262 10.4.1.2).
    #[inline(never)]
    /// [[Construct]] of bound function `f` (ECMA-262 §10.4.1.2).
    fn construct_bound(&mut self, f: &Value, b: &BoundFn, args: &[Value], nt: &Value) -> R<Value> {
        let argv = [&b.args[..], args].concat();
        let nt = if nt.strict_eq(f) { &b.target } else { nt };
        self.construct_new(&b.target, &argv, nt)
    }

    #[cfg(feature = "interp")]
    fn unary(&mut self, op: UnOp, a: &Expr, sc: &Scope) -> R<Value> {
        if op == UnOp::Typeof
            && let Expr::Local(h, i, n) = a
        {
            return Ok(Value::str(self.local(sc, *h, *i, n)?.type_of()));
        }
        if op == UnOp::Typeof
            && let Expr::Dynamic(n, withs, x) = a
        {
            return match self.dyn_ref(sc, n, *withs, static_hops(x))? {
                DynRef::Static => self.unary(op, x, sc),
                _ => Ok(Value::str(self.dyn_value(n, *withs, x, sc)?.type_of())),
            };
        }
        if op == UnOp::Typeof
            && let Expr::Ident(n) = a
        {
            // `typeof` of an undeclared name is "undefined"; of one in its TDZ, an error.
            match Self::find_var(sc, n) {
                Some(Value::Empty) => return self.uninitialized(n),
                Some(_) => {}
                None if self.names_uncreated_singleton(Self::qml_scope(sc), n) => {
                    return Ok(Value::str("object"));
                }
                None => {}
            }
            return Ok(match self.lookup(sc, n) {
                Ok(v) => Value::str(v.type_of()),
                Err(_) => Value::str("undefined"),
            });
        }
        if op == UnOp::Delete {
            return self.delete(a, sc);
        }
        let v = self.eval(a, sc)?;
        Ok(match op {
            UnOp::Not => Value::Bool(!v.truthy()),
            UnOp::Neg => Value::Num(-self.to_numeric(&v)?),
            UnOp::Pos => Value::Num(self.to_numeric(&v)?),
            UnOp::BitNot => Value::Num(!to_i32(self.to_numeric(&v)?) as f64),
            UnOp::Typeof => Value::str(v.type_of()),
            UnOp::Void | UnOp::Delete => Value::Undefined,
        })
    }

    /// `x op y` (ECMA-262 §13.15.3 ApplyStringOrNumericBinaryOperator, §7.2.13 IsLessThan,
    /// §7.2.14 IsLooselyEqual): objects go through [`Self::to_primitive`], which may throw.
    #[inline(never)]
    pub fn binary(&mut self, op: BinOp, x: Value, y: Value) -> R<Value> {
        use BinOp::*;
        match op {
            Add => {
                let a = self.to_primitive(&x, Hint::Default)?;
                let b = self.to_primitive(&y, Hint::Default)?;
                if matches!(a, Value::Symbol(_)) || matches!(b, Value::Symbol(_)) {
                    return if a.is_str() || b.is_str() {
                        self.throw("TypeError", "Cannot convert a symbol to a string.")
                    } else {
                        self.type_error()
                    };
                }
                if let (Value::Str(a), Value::Str(b)) = (&a, &b) {
                    Ok(Value::str(&(a.to_string() + b)))
                } else if a.is_str() || b.is_str() {
                    let mut s = crate::value::JsBuf::new();
                    s.push_value(&a);
                    s.push_value(&b);
                    Ok(s.finish())
                } else {
                    Ok(Value::Num(a.to_number() + b.to_number()))
                }
            }
            Sub | Mul | Div | Rem | Exp | BitAnd | BitOr | BitXor | Shl | Shr | UShr => {
                let a = self.to_numeric(&x)?;
                let b = self.to_numeric(&y)?;
                Ok(Value::Num(num_arith(op, a, b)))
            }
            Lt | Gt | Le | Ge => {
                let a = self.to_primitive(&x, Hint::Number)?;
                let b = self.to_primitive(&y, Hint::Number)?;
                // IsLessThan (ECMA-262 §7.2.13): ToNumeric of a non-string operand throws for
                // a symbol.
                if matches!(a, Value::Symbol(_)) || matches!(b, Value::Symbol(_)) {
                    return self.type_error();
                }
                Ok(Value::Bool(rel_prim(op, &a, &b)))
            }
            Eq => Ok(Value::Bool(self.loose_eq(&x, &y)?)),
            Ne => Ok(Value::Bool(!self.loose_eq(&x, &y)?)),
            StrictEq => Ok(Value::Bool(x.strict_eq(&y))),
            StrictNe => Ok(Value::Bool(!x.strict_eq(&y))),
            In => {
                // RelationalExpression `in` (ECMA-262 §13.10.1): a TypeError unless the right
                // operand is an object, then ToPropertyKey and HasProperty.
                let o = match &y {
                    Value::Obj(o) => Some(o.clone()),
                    Value::Item(_) => None,
                    Value::Native(_) => return Ok(Value::Bool(false)),
                    Value::Color(_) => {
                        let k = self.to_property_key(&x)?;
                        return Ok(Value::Bool(crate::props::has_pk(self, &y, &k)?));
                    }
                    Value::Undefined
                    | Value::Empty
                    | Value::Null
                    | Value::Bool(_)
                    | Value::Num(_)
                    | Value::Str(_)
                    | Value::Str16(_)
                    | Value::Symbol(_) => {
                        return self.throw("TypeError", "Type error");
                    }
                };
                let k = self.to_property_key(&x)?;
                Ok(Value::Bool(match (&o, &k, &y) {
                    (Some(o), PropKey::Sym(s), _) => self.has_sym(o, s)?,
                    // HasProperty: own, then up the prototype chain, ending at the built-in
                    // prototype methods.
                    (Some(o), PropKey::Str(k), _) if o.borrow().is_proxy() => {
                        self.has_property(o, k)?
                    }
                    // QV4::Sequence has no element and no `length` of its own to `in`: its
                    // prototype answers.
                    (Some(o), PropKey::Str(k), _)
                        if (idx(k).is_some() || &**k == "length")
                            && crate::valuetypes::is_readonly_sequence(&o.borrow()) =>
                    {
                        match self.proto_of(o) {
                            Some(p) => self.has_property(&p, k)?,
                            None => false,
                        }
                    }
                    (Some(o), PropKey::Str(k), _) => {
                        self.has_property(o, k)? || crate::builtins::has_method(self, &y, k)
                    }
                    (None, PropKey::Str(k), Value::Item(id)) => self.item_has(*id, k)?,
                    (None, ..) => false,
                }))
            }
            InstanceOf => {
                // InstanceofOperator (ECMA-262 §13.10.2): the target's `@@hasInstance` (the
                // built-in one is OrdinaryHasInstance), else a TypeError unless callable.
                if let Value::Obj(_) = &y {
                    let h = self.get_sym(&y, &WellKnown::HasInstance.sym())?;
                    if !matches!(h, Value::Undefined | Value::Null)
                        && !h.strict_eq(&self.sym.has_instance)
                    {
                        if callable(&h).is_none() {
                            return self.throw("TypeError", "Type error");
                        }
                        return Ok(Value::Bool(self.call(&h, y.clone(), &[x])?.truthy()));
                    }
                }
                // A QML type (V4's QQmlTypeWrapper::virtualInstanceOf): whether the QML
                // object is of it or of a type derived from it.
                if let Value::Obj(o) = &y
                    && let Some(ty) = o.borrow().native.as_ref()
                    && let Some(ty) = ty.downcast_ref::<crate::types::QmlType>()
                {
                    // `QtQuick/Item` names the type `Item`.
                    let name = ty.name.rsplit('/').next().unwrap_or_default();
                    let name = crate::runtime::Sym::new(name);
                    return Ok(Value::Bool(match x {
                        Value::Item(i) => self.host.inherits(i, name),
                        _ => false,
                    }));
                }
                // QQmlTypeWrapper::virtualInstanceOf: only a QML object is an instance of a
                // QML type.
                if let Value::Obj(t) = &y
                    && !matches!(x, Value::Item(_))
                    && crate::types::is_type_wrapper(&t.borrow())
                {
                    return Ok(Value::Bool(false));
                }
                if callable(&y).is_none() {
                    return self.throw("TypeError", "Type error");
                }
                Ok(Value::Bool(self.ordinary_has_instance(&y, &x)?))
            }
        }
    }

    /// OrdinaryHasInstance (ECMA-262 §7.3.21): a bound function tests its target; a
    /// primitive is no instance of anything.
    pub fn ordinary_has_instance(&mut self, c: &Value, o: &Value) -> R<bool> {
        if callable(c).is_none() {
            return Ok(false);
        }
        let mut y = c.clone();
        while let Some(Callable::Bound(b)) = callable(&y) {
            y = b.target.clone();
        }
        // A QML object, a colour and a native function are objects too.
        if !matches!(
            o,
            Value::Obj(_) | Value::Item(_) | Value::Color(_) | Value::Native(_)
        ) {
            return Ok(false);
        }
        let Value::Obj(target) = self.get(&y, "prototype")? else {
            return self.throw("TypeError", "Type error");
        };
        let mut cur = crate::props::get_prototype(self, o)?;
        while let Some(p) = cur {
            if Rc::ptr_eq(&p, &target) {
                return Ok(true);
            }
            cur = crate::props::get_prototype(self, &Value::Obj(p))?;
        }
        Ok(false)
    }

    /// ToPrimitive (ECMA-262 §7.1.1): an object's `@@toPrimitive` (a Date's prefers the string
    /// for `Default`), else its `valueOf`/`toString` in `hint` order; a function is an object
    /// like any other here, and so is a QML object, whose `toString` gives `Class(0x...)`.
    /// Colours and native functions, which hold no properties of their own, convert through
    /// their string form.
    #[inline(never)]
    pub fn to_primitive(&mut self, v: &Value, hint: Hint) -> R<Value> {
        if let Value::Obj(_) = v {
            let f = self.get_sym(v, &WellKnown::ToPrimitive.sym())?;
            // V4 passes over an `@@toPrimitive` that is not a function.
            if callable(&f).is_some() {
                let h = match hint {
                    Hint::String => "string",
                    Hint::Number => "number",
                    Hint::Default => "default",
                };
                let r = self.call(&f, v.clone(), &[Value::str(h)])?;
                if matches!(r, Value::Obj(_) | Value::Item(_) | Value::Native(_)) {
                    return self.throw("TypeError", "Type error");
                }
                return Ok(r);
            }
        }
        match v {
            Value::Obj(_) | Value::Item(_) => {}
            Value::Color(_) | Value::Native(_) => {
                return Ok(Value::str(&self.to_string(v)?));
            }
            _ => return Ok(v.clone()),
        }
        self.ordinary_to_primitive(v, hint == Hint::String)
    }

    /// OrdinaryToPrimitive (ECMA-262 §7.1.1.1): `toString` then `valueOf` when `string_first`,
    /// else the other way round; a `TypeError` when both return objects.
    pub fn ordinary_to_primitive(&mut self, v: &Value, string_first: bool) -> R<Value> {
        let order = if string_first {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for m in order {
            let own = matches!(v, Value::Obj(o) if o.borrow().call.is_some() && o.borrow().props.contains_key(m));
            let built_in = if own {
                None
            } else {
                crate::builtins::method(self, v, m, &[])?
            };
            let r = match built_in {
                Some(r) => r,
                None => {
                    let f = self.get(v, m)?;
                    if callable(&f).is_none() {
                        continue;
                    }
                    self.call(&f, v.clone(), &[])?
                }
            };
            if !matches!(
                r,
                Value::Obj(_) | Value::Item(_) | Value::Color(_) | Value::Native(_)
            ) {
                return Ok(r);
            }
        }
        self.throw("TypeError", "Type error")
    }

    /// ToNumber (ECMA-262 §7.1.4) with ToPrimitive(hint Number) for objects.
    pub fn to_numeric(&mut self, v: &Value) -> R<f64> {
        let p = match v {
            Value::Obj(_) => self.to_primitive(v, Hint::Number)?,
            Value::Symbol(_) => v.clone(),
            _ => return Ok(v.to_number()),
        };
        if let Value::Symbol(_) = p {
            return self.type_error();
        }
        Ok(p.to_number())
    }

    /// `x == y` (IsLooselyEqual, ECMA-262 §7.2.14): an object against a primitive compares
    /// the object's ToPrimitive. A QML object is an object here too: against a string it
    /// compares its string form, as in QV4.
    #[inline(never)]
    pub fn loose_eq(&mut self, x: &Value, y: &Value) -> R<bool> {
        // IsLooselyEqual steps 11-12 (ECMA-262 §7.2.14).
        let prim = |v: &Value| {
            matches!(
                v,
                Value::Num(_) | Value::Str(_) | Value::Str16(_) | Value::Bool(_) | Value::Symbol(_)
            )
        };
        let object = |v: &Value| matches!(v, Value::Obj(_) | Value::Item(_));
        if object(x) && prim(y) {
            let p = self.to_primitive(x, Hint::Default)?;
            return Ok(p.loose_eq(y));
        }
        if prim(x) && object(y) {
            let p = self.to_primitive(y, Hint::Default)?;
            return Ok(x.loose_eq(&p));
        }
        Ok(x.loose_eq(y))
    }

    #[cfg(feature = "interp")]
    fn read_pat(&mut self, p: &Pat, sc: &Scope) -> R<Value> {
        match p {
            Pat::Id(n) => self.lookup(sc, n),
            Pat::Local(h, i, n) => self.local(sc, *h, *i, n),
            Pat::Member(e) => self.eval(e, sc),
            Pat::Const(p, _) | Pat::Dynamic(_, _, p) => self.read_pat(p, sc),
            _ => self.throw("SyntaxError", "invalid compound assignment target"),
        }
    }

    /// The base (and key) of a member assignment target, evaluated once and before the
    /// right-hand side as in JS: `o()[k()] += v()` runs `o`, `k`, then `v`.
    #[cfg(feature = "interp")]
    fn target(&mut self, p: &Pat, sc: &Scope) -> R<Option<Target>> {
        if let Pat::Dynamic(n, withs, q) = p {
            return Ok(match self.dyn_ref(sc, n, *withs, static_pat_hops(q))? {
                DynRef::With(o) => Some(Target::With(o, n.clone())),
                DynRef::Frame(fr) => Some(Target::Var(fr, n.clone())),
                DynRef::Static => self.target(q, sc)?,
            });
        }
        let Pat::Member(e) = p else { return Ok(None) };
        Ok(match &**e {
            // GetThisBinding, the key, then GetSuperBase (ECMA-262 §13.3.7.1).
            Expr::Member(o, n, _) if matches!(**o, Expr::Super) => {
                let this = self.super_this(sc)?;
                let base = self.super_base(&Self::home_object(sc));
                Some(Target::Super(base, this, Value::Str(n.as_str().into())))
            }
            Expr::Index(o, k, _) if matches!(**o, Expr::Super) => {
                let this = self.super_this(sc)?;
                let kv = self.eval(k, sc)?;
                let base = self.super_base(&Self::home_object(sc));
                Some(Target::Super(base, this, kv))
            }
            Expr::Member(o, n, _) if Self::unbound_this(o, sc) => Some(Target::This(*n)),
            Expr::Member(o, n, _) if crate::valuetypes::is_color_field(n) => {
                let (ov, at) = self.color_base(o, sc)?;
                Some(match ov {
                    Value::Color(c) => Target::ColorField(c, *n, at),
                    ov => Target::Member(ov, *n),
                })
            }
            Expr::Member(o, n, _) => Some(Target::Member(self.eval_fast(o, sc)?, *n)),
            Expr::Index(o, k, _) => {
                let ov = self.eval_fast(o, sc)?;
                let kv = self.eval(k, sc)?;
                match self.color_key(o, &ov, &kv, sc)? {
                    Some((c, n, at)) => Some(Target::ColorField(c, n, at)),
                    None => Some(Target::Index(ov, kv)),
                }
            }
            _ => None,
        })
    }

    /// The value of `o`, the base of a member whose name a color has, and where a color
    /// read there is stored back.
    #[cfg(feature = "interp")]
    fn color_base(&mut self, o: &Expr, sc: &Scope) -> R<(Value, ColorAt)> {
        Ok(match o {
            Expr::Member(b, p, _) if !matches!(**b, Expr::Super) => {
                let bv = self.eval_fast(b, sc)?;
                let ov = match bv {
                    Value::Item(i) => self.host.get_sym(i, *p).unwrap_or_default(),
                    _ => self.get(&bv, p)?,
                };
                (ov, ColorAt::Member(bv, *p))
            }
            Expr::Index(b, k, _) if !matches!(**b, Expr::Super) => {
                let bv = self.eval_fast(b, sc)?;
                let mut kv = self.eval(k, sc)?;
                self.convert_key(&bv, &mut kv)?;
                let ov = self.get_key(&bv, &kv)?;
                (ov, ColorAt::Index(bv, kv))
            }
            Expr::Ident(name) => (
                self.eval_fast(o, sc)?,
                ColorAt::Name(Pat::Id(name.to_string())),
            ),
            Expr::Local(h, i, name) => (
                self.eval_fast(o, sc)?,
                ColorAt::Name(Pat::Local(*h, *i, name.to_string())),
            ),
            Expr::Dynamic(name, withs, x) => {
                match self.dyn_ref(sc, name, *withs, static_hops(x))? {
                    DynRef::With(w) => {
                        let ov = self.get(&w, name)?;
                        (ov, ColorAt::Member(w, crate::runtime::Sym::new(name)))
                    }
                    DynRef::Frame(_) => (self.eval_fast(o, sc)?, ColorAt::Nowhere),
                    DynRef::Static => return self.color_base(x, sc),
                }
            }
            _ => (self.eval_fast(o, sc)?, ColorAt::Nowhere),
        })
    }

    /// `o[kv]` as an assignment target when `ov`, the value of `o`, is a color and `kv`
    /// names one of its fields: the color, the field and where `o` was read. `o` is not
    /// evaluated again; the base of a member is, when reading it has no effect.
    #[cfg(feature = "interp")]
    fn color_key(
        &mut self,
        o: &Expr,
        ov: &Value,
        kv: &Value,
        sc: &Scope,
    ) -> R<Option<(crate::color::Color, crate::runtime::Sym, ColorAt)>> {
        let (Value::Color(c), Value::Str(k)) = (ov, kv) else {
            return Ok(None);
        };
        if !crate::valuetypes::is_color_field(k) {
            return Ok(None);
        }
        let pure = |b: &Expr| matches!(b, Expr::Ident(_) | Expr::Local(..) | Expr::This);
        let at = match o {
            Expr::Member(b, p, _) if pure(b) => ColorAt::Member(self.eval_fast(b, sc)?, *p),
            Expr::Ident(name) => ColorAt::Name(Pat::Id(name.to_string())),
            Expr::Local(h, i, name) => ColorAt::Name(Pat::Local(*h, *i, name.to_string())),
            Expr::Dynamic(name, withs, x) => {
                match self.dyn_ref(sc, name, *withs, static_hops(x))? {
                    DynRef::With(w) => ColorAt::Member(w, crate::runtime::Sym::new(name)),
                    DynRef::Frame(_) => ColorAt::Nowhere,
                    DynRef::Static => match &**x {
                        Expr::Ident(name) => ColorAt::Name(Pat::Id(name.to_string())),
                        Expr::Local(h, i, name) => {
                            ColorAt::Name(Pat::Local(*h, *i, name.to_string()))
                        }
                        _ => ColorAt::Nowhere,
                    },
                }
            }
            _ => ColorAt::Nowhere,
        };
        Ok(Some((*c, crate::runtime::Sym::new(k), at)))
    }

    #[cfg(feature = "interp")]
    fn target_get(&mut self, p: &Pat, t: &mut Option<Target>, sc: &Scope) -> R<Value> {
        match t {
            Some(Target::Member(Value::Item(i), n)) => self.item_get(*i, *n),
            Some(Target::Member(ov, n)) => self.get(ov, n),
            Some(Target::This(n)) => {
                let this = self.this_binding(sc)?;
                self.get(&this, n)
            }
            Some(Target::ColorField(c, n, _)) => Ok(crate::builtins::color_prop(c, n)),
            Some(Target::Index(ov, k)) => {
                self.convert_key(ov, k)?;
                self.get_key(ov, k)
            }
            Some(Target::Super(base, this, k)) => {
                self.convert_key(base, k)?;
                self.super_get_from(base.clone(), this, &PropKey::of(k))
            }
            Some(Target::With(o, n)) => self.get(o, n),
            Some(Target::Var(fr, n)) => {
                let v = fr.borrow().vars.get(n.as_str()).cloned();
                match v {
                    Some(Value::Empty) | None => self.uninitialized(n),
                    Some(v) => Ok(v),
                }
            }
            None => self.read_pat(p, sc),
        }
    }

    /// `ov.n = v`, then notifies the QML property that owns the sequence `ov`, unless the
    /// write was of the length it already had.
    #[cfg(feature = "interp")]
    fn put_notify(&mut self, ov: &Value, n: &str, v: Value) -> R<()> {
        let len = |ov: &Value| match ov {
            Value::Obj(x) if n == "length" => x.borrow().array.as_ref().map(Vec::len),
            _ => None,
        };
        let was = len(ov);
        self.put(ov, n, v)?;
        if n == "length" && len(ov) == was {
            return Ok(());
        }
        if let Some((i, p)) = sequence_owner(ov) {
            self.host.sequence_changed(i, &p, ov);
        }
        Ok(())
    }

    #[cfg(feature = "interp")]
    fn target_put(&mut self, p: &Pat, t: Option<Target>, v: Value, sc: &Scope) -> R<()> {
        // The static target of a name no dynamic scope binds.
        let p = match p {
            Pat::Dynamic(_, _, q) => &**q,
            p => p,
        };
        match t {
            Some(Target::With(o, n)) => {
                self.put(&o, &n, v)?;
                if let Some((i, p)) = sequence_owner(&o) {
                    self.host.notify(i, &p);
                }
                Ok(())
            }
            Some(Target::Var(fr, n)) => {
                let mut f = fr.borrow_mut();
                match f.vars.get_mut(n.as_str()) {
                    Some(Value::Empty) => {
                        drop(f);
                        self.uninitialized(&n)
                    }
                    Some(slot) => {
                        *slot = v;
                        Ok(())
                    }
                    None => {
                        drop(f);
                        self.bind(p, v, sc, false)
                    }
                }
            }
            Some(Target::Member(ov, n)) => self.put_notify(&ov, &n, v),
            Some(Target::ColorField(c, n, at)) => {
                let c = Value::Color(crate::valuetypes::color_put(self, c, &n, &v)?);
                match at {
                    ColorAt::Member(bv, p) => self.put(&bv, &p, c),
                    ColorAt::Index(bv, k) => self.put_key(&bv, &k, c),
                    ColorAt::Name(p) => self.bind(&p, c, sc, false),
                    ColorAt::Nowhere => Ok(()),
                }
            }
            // QV4's store on the unbound `this` is a TypeError.
            Some(Target::This(n)) => match Self::this(sc) {
                Value::Empty => self.throw("TypeError", "Type error"),
                this => self.put_notify(&this, &n, v),
            },
            Some(Target::Index(ov, k)) => {
                self.put_key(&ov, &k, v)?;
                match p {
                    Pat::Member(e) => match &**e {
                        Expr::Index(o, _, _) => self.write_back(o, &ov, sc),
                        _ => Ok(()),
                    },
                    _ => Ok(()),
                }
            }
            Some(Target::Super(base, this, k)) => self.super_put(&base, &this, &k, v, self.strict),
            None => self.bind(p, v, sc, false),
        }
    }

    #[cfg(feature = "interp")]
    pub fn bind(&mut self, p: &Pat, v: Value, sc: &Scope, decl: bool) -> R<()> {
        match p {
            Pat::Id(n) => {
                if decl {
                    self.define(sc, n, v);
                } else {
                    self.assign_var(sc, n, v)?;
                }
            }
            Pat::Local(h, i, n) => {
                if !decl && let Value::Empty = Self::slot_get(sc, *h, *i) {
                    return self.uninitialized(n);
                }
                Self::slot_set(sc, *h, *i, v)
            }
            Pat::Dynamic(..) => {
                let t = self.target(p, sc)?;
                self.target_put(p, t, v, sc)?;
            }
            Pat::Const(p, throws) => {
                if *throws {
                    // SetMutableBinding (ECMA-262 §9.1.1.1.5) checks that the binding is
                    // initialized (ReferenceError) before that it is mutable (TypeError).
                    self.read_pat(p, sc)?;
                    return self.const_write();
                }
            }
            Pat::Init(e) => match &**e {
                Expr::Member(o, n, _) => {
                    let ov = self.eval_fast(o, sc)?;
                    crate::generator::init_member(&ov, n, v);
                }
                _ => unreachable!("Pat::Init initializes a scope-object member"),
            },
            Pat::Member(e) => match &**e {
                Expr::Member(o, ..) | Expr::Index(o, ..) if matches!(**o, Expr::Super) => {
                    let t = self.target(p, sc)?;
                    self.target_put(p, t, v, sc)?;
                }
                Expr::Member(_, n, _) if crate::valuetypes::is_color_field(n) => {
                    let t = self.target(p, sc)?;
                    self.target_put(p, t, v, sc)?;
                }
                Expr::Member(o, n, _) => {
                    if Self::unbound_this(o, sc) {
                        return self.throw("TypeError", "Type error");
                    }
                    let ov = self.eval_fast(o, sc)?;
                    self.put_notify(&ov, n, v)?;
                }
                Expr::Index(o, k, _) => {
                    let ov = self.eval_fast(o, sc)?;
                    let kv = self.eval(k, sc)?;
                    if let Some((c, n, at)) = self.color_key(o, &ov, &kv, sc)? {
                        let t = Some(Target::ColorField(c, n, at));
                        return self.target_put(p, t, v, sc);
                    }
                    self.put_key(&ov, &kv, v)?;
                    self.write_back(o, &ov, sc)?;
                }
                _ => return self.throw("SyntaxError", "invalid assignment target"),
            },
            Pat::Default(inner, d) => {
                let v = if matches!(v, Value::Undefined) {
                    self.eval(d, sc)?
                } else {
                    v
                };
                self.bind(inner, v, sc, decl)?;
            }
            Pat::Array(items, rest) => {
                let mut src = self.get_iterator(&v)?;
                match self.bind_items(items, rest.as_deref(), &mut src, sc, decl) {
                    Ok(()) => self.iter_close(&mut src)?,
                    Err(e) => return Err(self.iter_close_throw(&mut src, e)),
                }
            }
            Pat::Object(props, rest) => {
                // RequireObjectCoercible, before any computed key is evaluated.
                if matches!(v, Value::Undefined | Value::Null) {
                    return self.throw("TypeError", "Type error");
                }
                let mut used = Vec::new();
                for (k, p) in props {
                    let name = self.key(k, sc)?;
                    self.bind_elem(p, sc, decl, |me| me.get_pk(&v, &name))?;
                    if rest.is_some() {
                        used.push(name);
                    }
                }
                if let Some(r) = rest {
                    self.bind_elem(r, sc, decl, |me| {
                        let o = new_obj();
                        me.copy_data_properties(&o, &v, &used)?;
                        Ok(Value::Obj(o))
                    })?;
                }
            }
        }
        Ok(())
    }

    // ---------- statements ----------

    #[cfg(feature = "interp")]
    pub fn exec_block(&mut self, body: &[Stmt], sc: &Scope) -> R<Flow> {
        self.hoist_funcs(body, sc);
        self.exec_stmts(body, sc)
    }

    /// BlockDeclarationInstantiation (ECMA-262 §14.2.3) on entry to the scope `sc` of `body`:
    /// its `let`/`const`/`class` bindings start uninitialized and its function declarations
    /// are created.
    #[cfg(feature = "interp")]
    fn hoist_funcs(&mut self, body: &[Stmt], sc: &Scope) {
        self.hoist_lexical(body, sc);
        self.hoist_func_decls(body, sc);
    }

    /// The `let`/`const`/`class` bindings of `body`, uninitialized in `sc`.
    #[cfg(feature = "interp")]
    fn hoist_lexical(&mut self, body: &[Stmt], sc: &Scope) {
        for s in body {
            match s {
                Stmt::Var(k, ds) if *k != "var" => {
                    ds.iter().for_each(|(p, _)| self.uninit(p, sc));
                }
                Stmt::Class(c) => match (c.slot, &c.name) {
                    (Some(i), _) => Self::slot_set(sc, 0, i, Value::Empty),
                    (None, Some(n)) => self.define(sc, n, Value::Empty),
                    (None, None) => {}
                },
                _ => {}
            }
        }
    }

    /// The function declarations of `body`, created in `sc`.
    #[cfg(feature = "interp")]
    fn hoist_func_decls(&mut self, body: &[Stmt], sc: &Scope) {
        for s in body {
            if let Stmt::Func(f) = s
                && let Some(n) = &f.name
            {
                let fv = fn_value(Callable::Closure(f.clone(), sc.clone()));
                match f.slot {
                    Some(i) => Self::slot_set(sc, 0, i, fv),
                    None => self.define(sc, n, fv),
                }
            }
        }
    }

    /// Every binding the declaration pattern `p` makes, in its temporal dead zone.
    #[cfg(feature = "interp")]
    fn uninit(&self, p: &Pat, sc: &Scope) {
        match p {
            Pat::Id(n) => self.define(sc, n, Value::Empty),
            Pat::Local(h, i, _) => Self::slot_set(sc, *h, *i, Value::Empty),
            Pat::Object(ps, rest) => {
                ps.iter().for_each(|(_, p)| self.uninit(p, sc));
                rest.iter().for_each(|p| self.uninit(p, sc));
            }
            Pat::Array(ps, rest) => {
                ps.iter().flatten().for_each(|p| self.uninit(p, sc));
                rest.iter().for_each(|p| self.uninit(p, sc));
            }
            Pat::Default(p, _) => self.uninit(p, sc),
            Pat::Member(_) | Pat::Const(..) | Pat::Init(_) | Pat::Dynamic(..) => {}
        }
    }

    #[cfg(feature = "interp")]
    fn exec_stmts(&mut self, body: &[Stmt], sc: &Scope) -> R<Flow> {
        for s in body {
            match self.exec(s, sc)? {
                Flow::Normal => {}
                f => return Ok(f),
            }
        }
        Ok(Flow::Normal)
    }

    /// Returns Some(flow) when the loop must exit with it, None to keep going.
    #[cfg(feature = "interp")]
    /// `labels`: the loop's label set (ECMA-262 §14.13.4: every label of a nested chain).
    fn loop_flow(f: Flow, labels: &[&str]) -> Option<Flow> {
        match f {
            Flow::Normal => None,
            Flow::Continue(None) => None,
            Flow::Continue(Some(l)) if labels.contains(&l.as_str()) => None,
            Flow::Break(None) => Some(Flow::Normal),
            Flow::Break(Some(l)) if labels.contains(&l.as_str()) => Some(Flow::Normal),
            f => Some(f),
        }
    }

    #[cfg(feature = "interp")]
    fn exec_loop(&mut self, s: &Stmt, sc: &Scope, labels: &[&str]) -> R<Flow> {
        match s {
            Stmt::For(init, test, update, body) => {
                let fr = if init.as_deref().is_some_and(declares) {
                    Self::child(sc)
                } else {
                    sc.clone()
                };
                if let Some(i) = init {
                    self.exec(i, &fr)?;
                }
                // `let` bindings get a fresh copy per iteration, so closures see their own.
                let per_iter = matches!(init.as_deref(), Some(Stmt::Var("let", _)))
                    && !fr.borrow().vars.is_empty();
                let mut fr = if per_iter {
                    Self::iteration_copy(&fr)
                } else {
                    fr
                };
                loop {
                    self.tick()?;
                    if let Some(t) = test
                        && !self.eval(t, &fr)?.truthy()
                    {
                        break;
                    }
                    // A block body makes its own frame.
                    let it = (!matches!(**body, Stmt::Block(_)) && declares(body))
                        .then(|| Self::child(&fr));
                    let f = self.exec(body, it.as_ref().unwrap_or(&fr))?;
                    if let Some(f) = Self::loop_flow(f, labels) {
                        return Ok(f);
                    }
                    if per_iter {
                        fr = Self::iteration_copy(&fr);
                    }
                    if let Some(u) = update {
                        self.eval(u, &fr)?;
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::ForOf(k, p, e, body) | Stmt::ForIn(k, p, e, body) => {
                let v = self.eval(e, sc)?;
                // `let`/`const` get a fresh binding per iteration; a `var` (hoisted to the
                // function) or an assignment target is assigned (ECMA-262 ForIn/OfBodyEvaluation).
                let lexical = k.is_some_and(|k| k != "var");
                if matches!(s, Stmt::ForOf(..)) {
                    let mut src = self.get_iterator(&v)?;
                    while let Some(it) = self.iter_step(&mut src)? {
                        let fr = if lexical && pat_declares(p) {
                            Self::child(sc)
                        } else {
                            sc.clone()
                        };
                        let f = match self
                            .tick()
                            .and_then(|_| self.bind(p, it, &fr, lexical))
                            .and_then(|_| self.exec(body, &fr))
                        {
                            Ok(f) => f,
                            Err(e) => return Err(self.iter_close_throw(&mut src, e)),
                        };
                        if let Some(f) = Self::loop_flow(f, labels) {
                            self.iter_close(&mut src)?;
                            return Ok(f);
                        }
                    }
                    return Ok(Flow::Normal);
                }
                let mut keys = self.for_in(&v);
                while let Some(k) = self.for_in_next(&mut keys)? {
                    let it = Value::str(&k);
                    self.tick()?;
                    let fr = if lexical && pat_declares(p) {
                        Self::child(sc)
                    } else {
                        sc.clone()
                    };
                    self.bind(p, it, &fr, lexical)?;
                    let f = self.exec(body, &fr)?;
                    if let Some(f) = Self::loop_flow(f, labels) {
                        return Ok(f);
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::While(t, body) => {
                while self.eval(t, sc)?.truthy() {
                    self.tick()?;
                    let f = self.exec(body, sc)?;
                    if let Some(f) = Self::loop_flow(f, labels) {
                        return Ok(f);
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::DoWhile(body, t) => {
                loop {
                    self.tick()?;
                    let f = self.exec(body, sc)?;
                    if let Some(f) = Self::loop_flow(f, labels) {
                        return Ok(f);
                    }
                    if !self.eval(t, sc)?.truthy() {
                        break;
                    }
                }
                Ok(Flow::Normal)
            }
            _ => self.exec(s, sc),
        }
    }

    #[cfg(feature = "interp")]
    pub fn exec(&mut self, s: &Stmt, sc: &Scope) -> R<Flow> {
        self.tick()?;
        match s {
            // A plain assignment statement: its value is unused.
            Stmt::Expr(Expr::Assign(AssignOp::Assign, p, rhs)) => {
                let t = self.target(p, sc)?;
                let v = self.eval(rhs, sc)?;
                self.target_put(p, t, v, sc)?;
            }
            Stmt::Expr(e @ (Expr::Assign(..) | Expr::Update(..))) => {
                self.eval_cold(e, sc)?;
            }
            Stmt::Expr(e) => {
                self.eval(e, sc)?;
            }
            Stmt::Var(kind, decls) => {
                for (p, init) in decls {
                    // `var n = e` under a dynamic scope assigns to the binding `n` resolves
                    // to, made before `e` runs (ECMA-262 §14.3.2.1); the `var` was hoisted.
                    if let Pat::Dynamic(..) = p {
                        if let Some(e) = init {
                            let t = self.target(p, sc)?;
                            let v = self.eval(e, sc)?;
                            self.target_put(p, t, v, sc)?;
                        }
                        continue;
                    }
                    let hoisted = *kind == "var"
                        && match p {
                            Pat::Id(n) => Self::find_var(sc, n).is_some(),
                            Pat::Local(..) => true,
                            _ => false,
                        };
                    if hoisted && init.is_none() {
                        continue;
                    }
                    let v = match init {
                        Some(e) => self.eval(e, sc)?,
                        None => Value::Undefined,
                    };
                    self.bind(p, v, sc, !hoisted)?;
                }
            }
            Stmt::Func(_) | Stmt::Empty => {}
            // ECMA-262 §14.11.2: ToObject of the value, then the body in an object
            // environment over it.
            Stmt::With(e, b) => {
                let v = self.eval(e, sc)?;
                if matches!(v, Value::Undefined | Value::Null) {
                    return self.not_an_object(&v);
                }
                let env = Self::child(sc);
                env.borrow_mut().with = Some(self.to_object(&v));
                return self.exec(b, &env);
            }
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e, sc)?,
                    None => Value::Undefined,
                };
                return Ok(Flow::Return(v));
            }
            Stmt::If(c, a, b) => {
                if self.eval(c, sc)?.truthy() {
                    return self.exec(a, sc);
                } else if let Some(b) = b {
                    return self.exec(b, sc);
                }
            }
            Stmt::Block(body) => {
                // A frame nothing is declared in is unobservable.
                if !body.iter().any(declares) {
                    return self.exec_block(body, sc);
                }
                let fr = Self::child(sc);
                return self.exec_block(body, &fr);
            }
            Stmt::For(..)
            | Stmt::ForOf(..)
            | Stmt::ForIn(..)
            | Stmt::While(..)
            | Stmt::DoWhile(..) => return self.exec_loop(s, sc, &[]),
            _ => return self.exec_cold(s, sc),
        }
        Ok(Flow::Normal)
    }

    /// Rarer statement kinds, out of line to keep `exec`'s frame small (no extra tick).
    #[inline(never)]
    #[cfg(feature = "interp")]
    fn exec_cold(&mut self, s: &Stmt, sc: &Scope) -> R<Flow> {
        match s {
            Stmt::Labeled(l, body) => {
                let mut labels = vec![l.as_str()];
                let mut body = &**body;
                while let Stmt::Labeled(l, b) = body {
                    labels.push(l);
                    body = b;
                }
                let f = self.exec_loop(body, sc, &labels)?;
                return Ok(match f {
                    Flow::Break(Some(x)) if labels.contains(&x.as_str()) => Flow::Normal,
                    f => f,
                });
            }
            Stmt::Switch(d, cases) => {
                let v = self.eval(d, sc)?;
                let fr = Self::child(sc);
                // One block scope for all cases: every case's functions exist from the start.
                for (_, body) in cases {
                    self.hoist_funcs(body, &fr);
                }
                let mut start = None;
                for (i, (t, _)) in cases.iter().enumerate() {
                    if let Some(t) = t
                        && self.eval(t, &fr)?.strict_eq(&v)
                    {
                        start = Some(i);
                        break;
                    }
                }
                let start = start.or_else(|| cases.iter().position(|(t, _)| t.is_none()));
                if let Some(st) = start {
                    for (_, body) in &cases[st..] {
                        match self.exec_stmts(body, &fr)? {
                            Flow::Normal => {}
                            Flow::Break(None) => return Ok(Flow::Normal),
                            f => return Ok(f),
                        }
                    }
                }
            }
            Stmt::Break(l) => return Ok(Flow::Break(l.clone())),
            Stmt::Continue(l) => return Ok(Flow::Continue(l.clone())),
            Stmt::Class(c) => {
                let cv = self.make_class(c, sc)?;
                if let Some(i) = c.slot {
                    Self::slot_set(sc, 0, i, cv);
                } else if let Some(n) = &c.name {
                    self.define(sc, n, cv);
                }
            }
            Stmt::Throw(e) => {
                let v = self.eval(e, sc)?;
                return Err(Ex::Throw(v));
            }
            Stmt::Try(body, catch, finally) => {
                let fr = Self::child(sc);
                let mut r = self.exec_block(body, &fr);
                if let (Err(Ex::Throw(err)), Some((p, cb))) = (&r, catch) {
                    let err = err.clone();
                    let cf = Self::child(sc);
                    // A parameter whose binding throws completes the catch clause abruptly:
                    // the `finally` block still runs (ECMA-262 §14.15.3).
                    let bound = match p {
                        Some(p) => self.bind(p, err, &cf, true),
                        None => Ok(()),
                    };
                    r = bound.and_then(|()| self.exec_block(cb, &cf));
                }
                if let Some(fb) = finally {
                    let ff = Self::child(sc);
                    match self.exec_block(fb, &ff)? {
                        Flow::Normal => {}
                        f => return Ok(f),
                    }
                }
                return r;
            }
            Stmt::Unsupported(x) => {
                return self.throw("SyntaxError", format!("unsupported statement {x}"));
            }
            _ => unreachable!("handled in exec"),
        }
        Ok(Flow::Normal)
    }

    /// Evaluate a lowered QML script body (expression bindings lower to `return expr`).
    #[cfg(feature = "interp")]
    pub fn run(&mut self, body: &[Stmt], sc: &Scope) -> R<Value> {
        let frame = self.enter()?;
        let code = self.code.replace(sc.clone());
        let qs = self.qs.take();
        let r = on_js_stack(|| {
            self.hoist_vars(body, sc);
            match self.exec_block(body, sc)? {
                Flow::Return(v) => Ok(v),
                _ => Ok(Value::Undefined),
            }
        });
        self.code = code;
        self.qs = qs;
        self.leave(frame, r)
    }

    /// Run a program `Qt.include` loaded into another file's context: its `var` and function
    /// declarations go to `decls`, the context's scope (Qt runs it in that QQmlContext, whose
    /// activation gets them), while its code and lexical declarations, which QV4 keeps to the
    /// script, live in `code`, the included file's own frame on top of `decls`.
    #[cfg(feature = "interp")]
    pub fn run_included(&mut self, body: &[Stmt], decls: &Scope, code: &Scope) -> R<Value> {
        let frame = self.enter()?;
        let saved = self.code.replace(code.clone());
        let r = on_js_stack(|| {
            self.hoist_vars(body, decls);
            self.hoist_lexical(body, code);
            for s in body {
                if let Stmt::Func(f) = s
                    && let Some(n) = &f.name
                {
                    let fv = fn_value(Callable::Closure(f.clone(), code.clone()));
                    self.define(decls, n, fv);
                }
            }
            for s in body {
                self.exec(s, code)?;
            }
            Ok(Value::Undefined)
        });
        self.code = saved;
        self.leave(frame, r)
    }

    /// The QML scope `(item, ctx)` the running code is written in (the context is Qt's
    /// `callingQmlContext()`); `None` for the code of a JS file and code outside any document.
    pub fn calling_qml_scope(&self) -> Option<(usize, usize)> {
        self.qs
            .or_else(|| self.code.as_ref().and_then(Self::qml_scope))
    }

    /// The JS file the running code is from and the scope holding the declarations of its
    /// context (Qt's `callingQmlContext()`, when that is a JS-file context); `None` for QML
    /// code and code outside any file.
    pub fn calling_script(&self) -> Option<(Rc<ScriptFile>, Scope)> {
        let mut sc = self.code.clone()?;
        let mut file = None;
        loop {
            let parent = {
                let f = sc.borrow();
                if let Some(s) = &f.script {
                    let file = file.get_or_insert_with(|| s.clone());
                    if s.ctx.is_some() {
                        let file = file.clone();
                        drop(f);
                        return Some((file, sc));
                    }
                }
                f.parent.clone()
            };
            sc = parent?;
        }
    }

    /// Evaluate code lowered by [`crate::ir::lower_eval`] as indirect eval (PerformEval,
    /// ECMA-262 §19.2.1.1): its `var` and function declarations go to `var_env` (the global
    /// scope, or for strict code a scope of its own), its lexical ones to a new scope.
    #[cfg(feature = "interp")]
    pub fn run_eval(&mut self, body: &[Stmt], var_env: &Scope) -> R<Value> {
        let _frame = self.enter()?;
        let lex = Self::child(var_env);
        on_js_stack(|| {
            self.hoist_vars(body, var_env);
            self.hoist_lexical(body, &lex);
            self.hoist_func_decls(body, var_env);
            match self.exec_stmts(body, &lex)? {
                Flow::Return(v) => Ok(v),
                _ => Ok(Value::Undefined),
            }
        })
    }
}

/// `binary` for two numbers; `None` for `in`/`instanceof`, which need an object.
#[inline]
pub fn num_binary(op: BinOp, a: f64, b: f64) -> Option<Value> {
    use BinOp::*;
    Some(match op {
        Add | Sub | Mul | Div | Rem | Exp | BitAnd | BitOr | BitXor | Shl | Shr | UShr => {
            Value::Num(num_arith(op, a, b))
        }
        Lt => Value::Bool(a < b),
        Gt => Value::Bool(a > b),
        Le => Value::Bool(a <= b),
        Ge => Value::Bool(a >= b),
        Eq | StrictEq => Value::Bool(a == b),
        Ne | StrictNe => Value::Bool(a != b),
        In | InstanceOf => return None,
    })
}

/// A numeric operator on two numbers (ECMA-262 §6.1.6.1); `op` is one of
/// `+ - * / % ** & | ^ << >> >>>`.
#[inline]
pub fn num_arith(op: BinOp, a: f64, b: f64) -> f64 {
    use BinOp::*;
    match op {
        Add => a + b,
        Sub => a - b,
        Mul => a * b,
        Div => a / b,
        Rem => a % b,
        // Number::exponentiate: NaN exponent, or |base| 1 with an infinite one, is NaN (powf says 1).
        Exp if b.is_nan() || (a.abs() == 1.0 && b.is_infinite()) => f64::NAN,
        Exp => a.powf(b),
        BitAnd => (to_i32(a) & to_i32(b)) as f64,
        BitOr => (to_i32(a) | to_i32(b)) as f64,
        BitXor => (to_i32(a) ^ to_i32(b)) as f64,
        Shl => to_i32(a).wrapping_shl(to_i32(b) as u32 & 31) as f64,
        Shr => (to_i32(a) >> (to_i32(b) as u32 & 31)) as f64,
        UShr => ((to_i32(a) as u32) >> (to_i32(b) as u32 & 31)) as f64,
        _ => unreachable!("{} is not a numeric operator", op.as_str()),
    }
}

/// `<` `>` `<=` `>=` on two primitives (IsLessThan, ECMA-262 §7.2.13): UTF-16 order for two
/// strings, numeric otherwise (false when either is NaN).
#[inline]
pub fn rel_prim(op: BinOp, x: &Value, y: &Value) -> bool {
    use BinOp::*;
    if let (Value::Str(a), Value::Str(b)) = (x, y) {
        let o = a.encode_utf16().cmp(b.encode_utf16());
        return match op {
            Lt => o.is_lt(),
            Gt => o.is_gt(),
            Le => o.is_le(),
            _ => o.is_ge(),
        };
    }
    if let (Some(a), Some(b)) = (x.js_str(), y.js_str()) {
        let o = a.cmp_units(b);
        return match op {
            Lt => o.is_lt(),
            Gt => o.is_gt(),
            Le => o.is_le(),
            _ => o.is_ge(),
        };
    }
    let (a, b) = (x.to_number(), y.to_number());
    match op {
        Lt => a < b,
        Gt => a > b,
        Le => a <= b,
        _ => a >= b,
    }
}

/// Outcome of [`Interp::store_var`].
pub enum Stored {
    Done,
    /// The binding is in its temporal dead zone.
    Uninitialized,
    /// Declared nowhere in the chain: the value, handed back.
    Undeclared(Value),
}

/// A property reference callee: its call's `this` is the base object.
#[cfg(feature = "interp")]
fn is_member_callee(e: &Expr) -> bool {
    match e {
        Expr::Member(o, ..) => !matches!(**o, Expr::Super),
        Expr::Index(..) => true,
        _ => false,
    }
}

/// Whether running `s` directly in a block's scope can declare a name there.
#[cfg(feature = "interp")]
fn declares(s: &Stmt) -> bool {
    match s {
        Stmt::Expr(_)
        | Stmt::Return(_)
        | Stmt::Empty
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Throw(_)
        | Stmt::Block(_)
        | Stmt::For(..)
        | Stmt::ForOf(..)
        | Stmt::ForIn(..)
        | Stmt::Switch(..)
        | Stmt::Try(..) => false,
        Stmt::If(_, a, b) => declares(a) || b.as_deref().is_some_and(declares),
        Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::Labeled(_, b) | Stmt::With(_, b) => {
            declares(b)
        }
        Stmt::Var(_, ds) => ds.iter().any(|(p, _)| pat_declares(p)),
        _ => true,
    }
}

/// Whether binding `p` as a declaration defines a name in the current frame (slot-resolved
/// names live in the function frame).
#[cfg(feature = "interp")]
fn pat_declares(p: &Pat) -> bool {
    match p {
        Pat::Local(..) | Pat::Member(_) | Pat::Const(..) | Pat::Init(_) => false,
        Pat::Dynamic(_, _, p) => pat_declares(p),
        Pat::Id(_) => true,
        Pat::Object(ps, rest) => {
            ps.iter().any(|(_, p)| pat_declares(p)) || rest.as_deref().is_some_and(pat_declares)
        }
        Pat::Array(ps, rest) => {
            ps.iter().flatten().any(pat_declares) || rest.as_deref().is_some_and(pat_declares)
        }
        Pat::Default(p, _) => pat_declares(p),
    }
}

pub fn to_i32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    (n.trunc().rem_euclid(4294967296.0) as u32) as i32
}

/// What a `.import` directive of a JS file names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptImport<'a> {
    /// `.import "file.js" as Alias`: the file, relative to the importing one.
    File(&'a str),
    /// `.import QtQuick 2.15 as Alias`: the module's URI.
    Module(&'a str),
}

/// The directives a JS file starts with.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScriptDirectives<'a> {
    /// `.pragma library`: one instance per engine.
    pub library: bool,
    /// Each `.import` with its qualifier.
    pub imports: Vec<(ScriptImport<'a>, &'a str)>,
    /// Offset of the end of the last directive: the program is the source from there.
    pub end: usize,
}

/// The directives of JS file source `src`, as `QQmlJS::Lexer::scanDirectives` (Qt 6.12) reads
/// them: they come before the first token of the program, so comments may precede them and
/// a `.import` inside a comment, a string or after code is none. An ill-formed directive is
/// an error (Qt then has no script to import).
pub fn script_directives(src: &str) -> Result<ScriptDirectives<'_>, String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Tok<'a> {
        Dot,
        Ident(&'a str),
        Str(&'a str),
        Num(&'a str),
        Other,
        End,
    }
    struct Lexer<'a> {
        src: &'a str,
        pos: usize,
        line: u32,
    }
    impl<'a> Lexer<'a> {
        /// The next token with the line and offset it starts at, comments skipped.
        fn next(&mut self) -> (Tok<'a>, u32, usize) {
            let b = self.src.as_bytes();
            loop {
                let rest = &self.src[self.pos..];
                let Some(c) = rest.chars().next() else {
                    return (Tok::End, self.line, self.pos);
                };
                if c == '\n' {
                    self.line += 1;
                    self.pos += 1;
                } else if c.is_whitespace() {
                    self.pos += c.len_utf8();
                } else if rest.starts_with("//") {
                    self.pos += rest.find('\n').unwrap_or(rest.len());
                } else if let Some(comment) = rest.strip_prefix("/*") {
                    let len = comment.find("*/").map_or(rest.len(), |k| k + 4);
                    self.line += rest[..len].matches('\n').count() as u32;
                    self.pos += len;
                } else {
                    break;
                }
            }
            let (line, start) = (self.line, self.pos);
            let rest = &self.src[start..];
            let c = rest.chars().next().unwrap_or_default();
            let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
            let (tok, len) = if c == '.' {
                (Tok::Dot, 1)
            } else if c == '"' || c == '\'' {
                let mut k = 1;
                while k < rest.len() && b[start + k] != c as u8 && b[start + k] != b'\n' {
                    k += if b[start + k] == b'\\' { 2 } else { 1 };
                }
                if k < rest.len() && b[start + k] == c as u8 {
                    (Tok::Str(&rest[1..k]), k + 1)
                } else {
                    (Tok::Other, rest.len().min(k))
                }
            } else if c.is_ascii_digit() {
                let len = rest
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(rest.len());
                (Tok::Num(&rest[..len]), len)
            } else if ident(c) {
                let len = rest.find(|c: char| !ident(c)).unwrap_or(rest.len());
                (Tok::Ident(&rest[..len]), len)
            } else {
                (Tok::Other, c.len_utf8())
            };
            self.pos += len;
            (tok, line, start)
        }
    }
    let mut lx = Lexer {
        src,
        pos: 0,
        line: 1,
    };
    let mut d = ScriptDirectives::default();
    while let (Tok::Dot, at, _) = lx.next() {
        let err = |m: &str| Err(format!("line {at}: {m}"));
        let (name, _, _) = lx.next();
        let (mut t, mut l, mut start) = lx.next();
        match name {
            Tok::Ident("pragma") => {
                if (t, l) != (Tok::Ident("library"), at) {
                    return err("Syntax error");
                }
                d.library = true;
            }
            Tok::Ident("import") => {
                let what = match t {
                    Tok::Str(file) => {
                        if !file.ends_with("js") {
                            return err("Imported file must be a script");
                        }
                        (t, l, _) = lx.next();
                        ScriptImport::File(file)
                    }
                    Tok::Ident(_) => {
                        // A dotted URI, then an optional version (`2`, `2.15`).
                        let uri = start;
                        let mut end = lx.pos;
                        (t, l, start) = lx.next();
                        while t == Tok::Dot && start == end {
                            let (Tok::Ident(_), _, _) = lx.next() else {
                                return err("Invalid module URI");
                            };
                            end = lx.pos;
                            (t, l, start) = lx.next();
                        }
                        if let Tok::Num(_) = t {
                            let after = lx.pos;
                            (t, l, start) = lx.next();
                            if t == Tok::Dot && start == after {
                                let (Tok::Num(_), _, _) = lx.next() else {
                                    return err("Incomplete version number (dot but no minor)");
                                };
                                (t, l, _) = lx.next();
                            }
                        }
                        ScriptImport::Module(&src[uri..end])
                    }
                    _ => return err("Syntax error"),
                };
                let requires = match what {
                    ScriptImport::File(_) => "File import requires a qualifier",
                    ScriptImport::Module(_) => "Module import requires a qualifier",
                };
                if (t, l) != (Tok::Ident("as"), at) {
                    return err(requires);
                }
                let (Tok::Ident(alias), l, _) = lx.next() else {
                    return err(requires);
                };
                if l != at {
                    return err(requires);
                }
                if !alias.starts_with(char::is_uppercase) {
                    return err("Invalid import qualifier");
                }
                d.imports.push((what, alias));
            }
            // No directive: the program starts with this `.`.
            _ => break,
        }
        d.end = lx.pos;
    }
    Ok(d)
}

/// Why the library at `path` cannot be loaded by a build without the interpreter: it, or a
/// library it `.import`s, is missing or has neither a native implementation nor code in
/// `aot`. Libraries in `loaded` (or already checked in `seen`) are available.
pub fn library_unavailable(
    path: &std::path::Path,
    aot: Option<&dyn crate::jsrt::Aot>,
    loaded: &HashMap<std::path::PathBuf, Value>,
    seen: &mut rustc_hash::FxHashSet<std::path::PathBuf>,
) -> Option<String> {
    let Ok(path) = path.canonicalize() else {
        return Some(format!("{}: no such file", path.display()));
    };
    if loaded.contains_key(&path) || !seen.insert(path.clone()) {
        return None;
    }
    if aot.and_then(|a| a.library(&path)).is_none() {
        return Some(format!("{}: no compiled code", path.display()));
    }
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => return Some(format!("{}: {e}", path.display())),
    };
    let directives = script_directives(&src).ok()?;
    directives.imports.iter().find_map(|(what, _)| match what {
        ScriptImport::File(dep) => {
            library_unavailable(&path.parent()?.join(dep), aot, loaded, seen)
        }
        ScriptImport::Module(_) => None,
    })
}

/// A JS file [`load_library`] read: its source is read and its code made once for all the
/// instances that run it (Qt compiles a file once and gives each importing context its own
/// script values).
pub struct LibraryFile {
    /// The canonical path.
    path: std::path::PathBuf,
    src: String,
    #[cfg(feature = "interp")]
    body: std::cell::OnceCell<Vec<crate::ir::Stmt>>,
}

/// What [`load_library`] loads JS files with.
pub struct Libraries<'a> {
    /// The namespace object of each `.pragma library` loaded so far.
    pub shared: &'a mut HashMap<std::path::PathBuf, Value>,
    /// The files read so far, by the path they were asked for with.
    pub files: &'a mut HashMap<std::path::PathBuf, Rc<LibraryFile>>,
    /// The type names QML module `uri` exports; `None` when there is no such module.
    pub modules: &'a dyn Fn(&str) -> Option<Vec<String>>,
    /// What the files threw while they ran.
    pub errors: &'a mut crate::warning::Warnings,
    /// The QML scope `(item, context)` of the document importing the files, asked for when a
    /// file that is no `.pragma library` loads: its code sees the names of that context (the
    /// ids, the root object's members) behind its own, as Qt's JS-file context does, which
    /// is a child of the importing document's. `None` outside a document.
    pub scope: &'a mut dyn FnMut() -> Option<(usize, usize)>,
}

/// Load a JS resource a QML document or another JS file imports, with the files and modules
/// its `.import` directives name. Returns its namespace object, which shows the file's
/// top-level names ([`ScriptCtx::export`]). `realm` is the global scope of the engine
/// importing it, whose builtins the file shares ([`Interp::for_library`]).
///
/// A file that throws is still imported, with what it declared until then, and the error
/// goes to `libs.errors` (QQmlScriptData::ownScriptValue, Qt 6.12). `Err` is a file Qt has
/// no script for: unreadable, with an ill-formed directive or with a syntax error.
pub fn load_library(
    path: &std::path::Path,
    realm: &Scope,
    libs: &mut Libraries,
) -> Result<Value, String> {
    let file = match libs.files.get(path) {
        Some(f) => f.clone(),
        None => {
            let real = path
                .canonicalize()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let src =
                std::fs::read_to_string(&real).map_err(|e| format!("{}: {e}", real.display()))?;
            let f = Rc::new(LibraryFile {
                path: real,
                src,
                #[cfg(feature = "interp")]
                body: Default::default(),
            });
            libs.files.insert(path.to_path_buf(), f.clone());
            f
        }
    };
    let (path, src) = (&file.path, &file.src);
    if let Some(v) = libs.shared.get(path) {
        return Ok(v.clone());
    }
    let compiled = crate::jsrt::lib_aot().and_then(|a| a.library(path));
    let directives = script_directives(src).map_err(|e| format!("{}: {e}", path.display()))?;
    // The qualifiers of the imports are names the file's code sees but its namespace object
    // does not show: they live in a scope of their own under the file's declarations.
    let mut it = Interp::for_library(realm);
    let imports = it.global.clone();
    // A library is one for every document: neither it nor the files it imports see one.
    let scope = match directives.library {
        true => None,
        false => (libs.scope)(),
    };
    let mut no_scope = || None;
    let mut nested = Libraries {
        shared: &mut *libs.shared,
        files: &mut *libs.files,
        modules: libs.modules,
        errors: &mut *libs.errors,
        scope: match directives.library {
            true => &mut no_scope,
            false => &mut *libs.scope,
        },
    };
    for (what, alias) in &directives.imports {
        let v = match what {
            ScriptImport::File(dep) => {
                load_library(&path.parent().unwrap().join(dep), realm, &mut nested)?
            }
            ScriptImport::Module(uri) => match (libs.modules)(uri) {
                Some(names) => module_namespace(&names),
                None => {
                    return Err(format!(
                        "{}: module \"{uri}\" is not installed",
                        path.display()
                    ));
                }
            },
        };
        it.define(&imports, alias, v);
    }
    let g = Interp::child(&imports);
    it.global = g.clone();
    let ctx = ScriptCtx::new(crate::jsrt::lib_aot());
    let exports = ctx.exports.clone();
    // `this` of the file's top-level code is the global object, as in Qt 6.12.
    let this = Value::Obj(it.global_object.clone());
    g.borrow_mut().this = Some(this.clone());
    g.borrow_mut().qml = scope;
    g.borrow_mut().script = Some(Rc::new(ScriptFile {
        file: path.clone(),
        ctx: Some(ctx),
    }));
    #[cfg(feature = "interp")]
    let body = match (compiled, file.body.get()) {
        (Some(_), _) => None,
        (None, Some(b)) => Some(b),
        (None, None) => {
            let b =
                crate::ir::lower_program(src).map_err(|e| format!("{}: {e}", path.display()))?;
            Some(file.body.get_or_init(|| b))
        }
    };
    // Only a `.pragma library` is one instance per engine; any other script runs again for
    // each import (QQmlScriptBlob), keeping its own variables.
    if directives.library {
        libs.shared
            .insert(path.clone(), Value::Obj(exports.clone()));
    }
    let script = g.borrow().script.clone();
    let ctx = script.as_ref().and_then(|s| s.ctx.as_ref());
    let r = match compiled {
        Some(e) => crate::jsrt::run_entry(
            &mut it,
            e,
            &crate::jsrt::Env::from_scope(g.clone()),
            this,
            &[],
        ),
        #[cfg(feature = "interp")]
        None => {
            // Lexical declarations are the script's own, on top of its context's `var`s and
            // functions.
            let code = Interp::child(&g);
            let r = it.run_included(body.map_or(&[][..], |b| b), &g, &code);
            if let Some(ctx) = ctx {
                ctx.export_vars(&g);
                let mut lexical: Vec<String> = code.borrow().vars.keys().cloned().collect();
                lexical.sort_unstable();
                for name in lexical {
                    ctx.export(&name, Binding::Var(code.clone()), false);
                }
            }
            r
        }
        #[cfg(not(feature = "interp"))]
        None => return Err(format!("{}: no compiled code", path.display())),
    };
    if let Some(ctx) = ctx {
        ctx.export_vars(&g);
    }
    if let Err(Ex::Throw(e)) = r {
        let m = it
            .get(&e, "message")
            .map(|v| v.to_js_string())
            .unwrap_or_default();
        libs.errors.push(format!("{}: {m}", path.display()));
    }
    Ok(Value::Obj(exports))
}

#[cfg(all(test, feature = "interp"))]
mod js_compat_tests {
    use super::*;

    fn run(src: &str) -> String {
        let mut it = Interp::default();
        let g = it.global.clone();
        let body = crate::ir::lower_program(src).unwrap();
        match it.run(&body, &g) {
            Ok(v) => v.to_js_string(),
            Err(Ex::Throw(e)) => {
                let m = it.get(&e, "message").unwrap().to_js_string();
                format!("{}: {m}", it.get(&e, "name").unwrap().to_js_string())
            }
            Err(Ex::Short) => "SHORT".into(),
        }
    }

    /// Qt 6.12 (QV4) results, except arrays: a frozen array's elements and `length` are
    /// read-only as ECMA-262 says (QV4 leaves them writable).
    /// `__proto__: v` in an object literal (ECMA-262 Annex B §B.3.1): a non-computed,
    /// non-shorthand, non-method `__proto__` (identifier or string) sets [[Prototype]] when the
    /// value is an object or null and defines no property; other forms define own properties.
    /// Expected values by hand from the spec.
    #[test]
    fn literal_proto_sets_prototype() {
        for (src, want) in [
            (
                "var p = {x: 1}; var o = {a: 1, __proto__: p, b: 2}; \
                 return [Object.getPrototypeOf(o) === p, o.hasOwnProperty('__proto__'), o.x, \
                 Object.getOwnPropertyNames(o).join(' ')].join()",
                "true,false,1,a b",
            ),
            (
                "var p = {}; return Object.getPrototypeOf({'__proto__': p}) === p",
                "true",
            ),
            (
                "var o = {__proto__: null}; return [Object.getPrototypeOf(o), \
                 Object.getOwnPropertyNames(o).length].join()",
                ",0",
            ),
            (
                "var o = {__proto__: 5}; return [Object.getPrototypeOf(o) === Object.prototype, \
                 Object.getOwnPropertyNames(o).length].join()",
                "true,0",
            ),
            (
                "var p = {}; var o = {['__proto__']: p}; \
                 return [Object.getPrototypeOf(o) === Object.prototype, o.__proto__ === p, \
                 Object.getOwnPropertyNames(o).join()].join()",
                "true,true,__proto__",
            ),
            (
                "var __proto__ = {}; var o = {__proto__}; \
                 return [Object.getPrototypeOf(o) === Object.prototype, \
                 o.hasOwnProperty('__proto__')].join()",
                "true,true",
            ),
            (
                "var o = {__proto__() { return 1 }}; \
                 return [Object.getPrototypeOf(o) === Object.prototype, typeof o.__proto__, \
                 o.__proto__.name].join()",
                "true,function,__proto__",
            ),
            (
                "var o = {get __proto__() { return 7 }}; \
                 return [Object.getPrototypeOf(o) === Object.prototype, o.__proto__].join()",
                "true,7",
            ),
            (
                "var o = {__proto__: function () {}}; var f = Object.getPrototypeOf(o); \
                 return [typeof f, JSON.stringify(f.name)].join()",
                r#"function,"""#,
            ),
            (
                "var p = {}; var o = {__proto__: p, ['__proto__']: 1}; \
                 return [Object.getPrototypeOf(o) === p, o.hasOwnProperty('__proto__')].join()",
                "true,true",
            ),
            (
                "var a, b; ({__proto__: a, __proto__: b} = {}); return a === Object.prototype && b === a",
                "true",
            ),
            (
                "var log = []; var o = {a: log.push('a'), __proto__: (log.push('p'), {}), \
                 b: log.push('b')}; return log.join()",
                "a,p,b",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
        for bad in [
            "({__proto__: 1, __proto__: 2})",
            "({__proto__: 1, '__proto__': 2})",
            "({'__proto__': 1, \"__proto__\": 2})",
        ] {
            assert!(crate::ir::lower_program(bad).is_err(), "{bad}");
        }
        for ok in [
            "({__proto__: 1, ['__proto__']: 2})",
            "var __proto__; ({__proto__: 1, __proto__})",
            "({__proto__: 1, __proto__() {}})",
            "({__proto__: 1, get __proto__() {}})",
        ] {
            assert!(crate::ir::lower_program(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn refused_writes_throw_in_strict_code_only() {
        let te = "TypeError";
        for (body, want) in [
            (
                "'use strict'; var o = Object.freeze({a: 1}); o.a = 2; return o.a",
                te,
            ),
            (
                "'use strict'; var o = Object.freeze({a: 1}); o.b = 2; return o.b",
                te,
            ),
            (
                "'use strict'; var o = Object.freeze({a: 1}); delete o.a; return o.a",
                te,
            ),
            (
                "'use strict'; var o = Object.freeze({a: 1}); o['a'] = 2; return o.a",
                te,
            ),
            (
                "'use strict'; var o = Object.freeze({a: 1}); delete o['a']; return o.a",
                te,
            ),
            (
                "'use strict'; var o = Object.create(Object.freeze({a: 1})); o.a = 2; return o.a",
                te,
            ),
            (
                "'use strict'; var o = { get a() { return 1 } }; o.a = 2; return o.a",
                te,
            ),
            (
                "'use strict'; var f = function (a) {}; f.length = 3; return f.length",
                te,
            ),
            ("'use strict'; var s = 's'; s.x = 1; return s.x", te),
            ("'use strict'; var n = 5; n.x = 1; return 'x'", te),
            ("'use strict'; true.x = 1; return 'x'", te),
            ("'use strict'; var o = Object.freeze({}); o.x = 1", te),
            (
                "'use strict'; var o = Object.preventExtensions({a: 1}); o.a = 2; o.b = 3; \
                 return o.a",
                te,
            ),
            (
                "'use strict'; var o = Object.preventExtensions({a: 1}); o.a = 2; delete o.a; \
                 return o.a + ',' + Object.isFrozen(o)",
                "undefined,true",
            ),
            (
                "class K { m() { var o = Object.freeze({a: 1}); o.a = 2; return o.a } } \
                 return new K().m()",
                te,
            ),
            (
                "class K { static f = Object.freeze({}).x = 1 } return 'x'",
                te,
            ),
            ("class K { static a = this } return K.a === K", "true"),
            ("class K { [Object.freeze({}).x = 1]() {} } return 'x'", te),
            (
                "class K extends (Object.freeze({}).x = 1, Object) {} return 'x'",
                te,
            ),
            (
                "class K { f = Object.freeze({}).x = 1 } new K(); return 'x'",
                te,
            ),
            (
                "class K { static f = delete Object.freeze({a: 1}).a } return 'x'",
                te,
            ),
            ("class K {} Object.freeze({}).x = 1; return 'x'", "x"),
            (
                "class K { f = (() => { Object.freeze([]).length = 1 })() } new K(); return 'x'",
                te,
            ),
            (
                "'use strict'; var v = 0; var o = Object.freeze({ set a(x) { v = x } }); \
                 o.a = 4; return v",
                "4",
            ),
            (
                "'use strict'; var o = Object.freeze({a: 1}); \
                 return (() => { { o.a = 2 } })()",
                te,
            ),
            (
                "var o = Object.freeze({a: 1}); o.a = 2; o.b = 3; \
                 return o.a + ',' + o.b + ',' + Object.isFrozen(o)",
                "1,undefined,true",
            ),
            (
                "var o = Object.freeze({a: 1}); var d = delete o.a; return d + ',' + o.a",
                "false,1",
            ),
            (
                "var p = Object.freeze({a: 1}); var o = Object.create(p); o.a = 2; \
                 return o.a + ',' + o.hasOwnProperty('a')",
                "1,false",
            ),
            (
                "var o = Object.freeze({n: {a: 1}}); o.n.a = 2; return o.n.a",
                "2",
            ),
            (
                "var o = {a: 1}; return (Object.freeze(o) === o) + ',' + Object.freeze(5)",
                "true,5",
            ),
            (
                "var o = Object.freeze({a: 1, b: 2}); return Object.keys(o).join() + \
                 JSON.stringify(o)",
                "a,b{\"a\":1,\"b\":2}",
            ),
            ("var o = { get a() { return 1 } }; o.a = 2; return o.a", "1"),
            (
                "var f = function (a) {}; f.length = 3; return f.length",
                "1",
            ),
            ("var s = 's'; s.x = 1; return typeof s.x", "undefined"),
            (
                "function f() { 'use strict'; } var o = Object.freeze({a: 1}); f(); o.a = 2; \
                 return o.a",
                "1",
            ),
            (
                "var o = Object.preventExtensions({a: 1}); o.b = 1; \
                 return [Object.isFrozen(o), Object.isFrozen(Object.preventExtensions({})), \
                 Object.isFrozen({}), Object.isFrozen(1), 'b' in o].join()",
                "false,true,false,true,false",
            ),
            // Arrays, as QV4: an element of a frozen array is written.
            (
                "'use strict'; var a = Object.freeze([1]); a[0] = 2; return a[0]",
                "2",
            ),
            (
                "'use strict'; var a = Object.freeze([1]); a.length = 0; return a[0]",
                te,
            ),
            (
                "'use strict'; var a = Object.preventExtensions([1]); a[1] = 2; return 'x'",
                te,
            ),
            (
                "var a = Object.freeze([1]); a[0] = 2; a[1] = 3; a.length = 0; \
                 var d = delete a[0]; return a + ':' + a.length + ':' + d",
                "2:1:false",
            ),
            (
                "var a = Object.preventExtensions([1, 2]); a[0] = 5; a[2] = 3; a.length = 1; \
                 return a + ':' + a.length",
                "5:1",
            ),
            ("var a = []; return delete a.length", "false"),
        ] {
            let src = format!(
                "function t() {{ {body} }} try {{ return t() }} catch (e) {{ return e.name }}"
            );
            assert_eq!(run(&src), want, "{body}");
        }
    }

    /// The mutating array methods on a frozen array, as QV4: `push` and `sort` work on the
    /// storage, each of the others throws, also in sloppy code, and leaves the array as it was.
    #[test]
    fn frozen_array_mutators_throw() {
        let te = "TypeError:1,2";
        for (m, want) in [
            ("push(2)", "ok:1,2,2"),
            ("push()", "ok:1,2"),
            ("pop()", te),
            ("shift()", te),
            ("unshift(0)", te),
            ("splice(0, 1)", te),
            ("splice(0, 0)", te),
            ("reverse()", te),
            ("sort()", "ok:1,2"),
            ("fill(0)", te),
            ("copyWithin(0, 1)", te),
        ] {
            let src = format!(
                "var a = Object.freeze([1, 2]); try {{ a.{m} }} catch (e) {{ return e.name + ':' + a }} return 'ok:' + a"
            );
            assert_eq!(run(&src), want, "{m}");
        }
        for (src, want) in [
            (
                "var a = Object.freeze([1]); return a.reverse() === a",
                "true",
            ),
            (
                "var a = Object.freeze([]); return a.sort() + a.fill(0, 0)",
                "",
            ),
            (
                "var a = Object.freeze([1, 2]); return a.fill(0, 1, 1) + ''",
                "1,2",
            ),
            (
                "var a = Object.freeze([1, 2]); return a.copyWithin(0, 1, 1) + ''",
                "1,2",
            ),
            (
                "var a = Object.preventExtensions([1, 2, 3]); return a.copyWithin(1, 0) + ''",
                "1,1,2",
            ),
            (
                "var a = Object.preventExtensions([1, 2]); var p = a.pop(); a.reverse(); \
                 a.fill(7); return p + ':' + a",
                "2:7",
            ),
            (
                "var a = Object.preventExtensions([3, 1]); a.sort(); a.splice(0, 1, 9); \
                 return a + ''",
                "9,3",
            ),
            (
                "var a = Object.preventExtensions([1]); a.push(2); return a + ''",
                "1,2",
            ),
            (
                "var a = Object.preventExtensions([1]); a.unshift(0); return a + ''",
                "0,1",
            ),
            (
                "var a = Object.preventExtensions([1, , 3]); a.sort(); \
                 return a + ':' + a.length + ':' + (1 in a) + (2 in a)",
                "1,3,:3:truefalse",
            ),
            (
                "var o = Object.freeze({length: 1, 0: 'a'}); \
                 try { Array.prototype.push.call(o, 'b') } catch (e) { return e.name + ':' + o.length }",
                "TypeError:1",
            ),
            (
                "var t = Object.freeze({a: 1}); try { Object.assign(t, {a: 2}) } \
                 catch (e) { return e.name + ':' + t.a }",
                "TypeError:1",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }

    #[test]
    fn recursive_getters_throw_range_error() {
        for src in [
            "var o = { get g() { return this.g } }; try { o.g } catch (e) { return e.name }",
            "var o = { get g() { return this.g + 1 } }; try { o.g } catch (e) { return e.name }",
            "var o = { get g() { return this.h }, get h() { return this.g } }; \
             try { o.g } catch (e) { return e.name }",
            "class A { get g() { return this.g } } try { new A().g } catch (e) { return e.name }",
        ] {
            assert_eq!(run(src), "RangeError", "{src}");
        }
        assert_eq!(
            run("var o = { n: 2, get g() { return this.n * 3 } }; return o.g"),
            "6"
        );
    }

    #[test]
    fn function_prototype_is_a_constructor_link() {
        for (src, want) in [
            ("function P() {} P.prototype.q = 1; return new P().q", "1"),
            (
                "function P() {} var o = new P(); \
                 return o.constructor === P && P.prototype.constructor === P",
                "true",
            ),
            (
                "function P() {} new P(); P.prototype.q = 1; \
                 var ks = []; for (var k in P) ks.push(k); \
                 return Object.keys(P).length + ',' + ks.length + ',' + Object.keys(P.prototype)",
                "0,0,q",
            ),
            (
                "function P() {} P.prototype = { q: 2 }; return new P().q + ',' + Object.keys(P)",
                "2,",
            ),
            (
                "class A {} return new A().constructor === A && Object.keys(A).length === 0",
                "true",
            ),
            (
                "var f = () => 1; try { new f(); return 'constructed' } \
                 catch (e) { return e.name + ',' + f.prototype }",
                "TypeError,undefined",
            ),
            ("return function () {}.bind(null).prototype", "undefined"),
            (
                "return new TypeError().constructor === TypeError && \
                 new Date(0).constructor === Date && Object.keys(Error).length === 0",
                "true",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }

    /// ToPropertyKey on every computed key, at the point ECMA-262 converts it: the first case
    /// is pinned to Qt 6.12's V4, the second holds ECMA-262's order where V4 deviates.
    #[test]
    fn computed_keys_go_through_to_property_key() {
        let case = include_str!("../../rigen/check/cases/property_keys.js");
        let pin = case
            .lines()
            .find_map(|l| l.strip_prefix("// v4: "))
            .unwrap();
        assert_eq!(run(&format!("{case}\nreturn result")), pin);
        let case = include_str!("../../rigen/check/cases/property_keys_spec.js");
        assert_eq!(
            run(&format!("{case}\nreturn result")),
            r#"["literal order => k,v,1","compound => k,v,2","update => k,2","null get => TypeError,0","null call => TypeError,0"]"#
        );
    }

    /// Destructuring assignment evaluates a member target's base and key before the iterator
    /// step or property get that reads its value (ECMA-262 §13.15.5.5-6); Qt 6.12's V4 steps
    /// first, so the expected values are ECMA-262's (V8 agrees).
    #[test]
    fn destructuring_targets_evaluate_before_their_values() {
        let case = include_str!("../../rigen/check/cases/destructure_target_order.js");
        assert_eq!(
            run(&format!("{case}\nreturn result")),
            r#"["array index => iter,o,k,step0,convk,return|1","array member => iter,o,step0,p,step1,return|12","array default => iter,o,k,step0,def,convk,return|5","array hole => iter,step0,o,k,step1,convk,return|2","array rest => iter,o,step0,p,k,step1,step2,step3,convk|1[2,3]","array nested => iter,step0,iter,o,k,step0,convk,return,step1,p,getb,return|12","object => o,k,geta,convk,p,getb|12","object computed => s,convs,o,k,geta,convk|1","object default => o,k,geta,def,convk|7","object rest => o,p,k,getb,convk|1{\"b\":2}","object nested => geta,iter,o,step0,return,getb,p,getc|12","target throws closes THROW TypeError iter,o,k,return","null base THROW TypeError iter,o,k,step0,return","generator => iter,o,k,step0,convk,p,step1,return|92"]"#
        );
    }

    /// Object rest in binding, assignment, parameter and generator patterns goes through
    /// CopyDataProperties (ECMA-262 §7.3.25): the case checks itself against the spec order.
    #[test]
    fn object_rest_copies_data_properties() {
        let case = include_str!("../../rigen/check/cases/object_rest_spec.js");
        assert_eq!(run(&format!("{case}\nreturn result")), "ok");
    }

    /// Class element keys (ECMA-262 §15.7.14): `super` in a computed key is the enclosing
    /// method's, `delete super.x` throws a ReferenceError (§13.5.1.2), and own keys keep
    /// creation order whatever their enumerability (§10.1.11.1). Expected values are
    /// ECMA-262's (V8 agrees except where `class_delete_super.js` says otherwise), but a class
    /// lists `prototype`, `length`, `name` first, as V4 does.
    #[test]
    fn class_elements_follow_ecma262() {
        for (case, want) in [
            (
                include_str!("../../rigen/check/cases/class_computed_key_super.js"),
                r#"[["constructor,outer,outerG","prototype,length,name,outerS,outerA"],"prototype,length,name,Bx","constructor,Py"]"#,
            ),
            (
                include_str!("../../rigen/check/cases/class_computed_key_super_fields.js"),
                r#"["outerF","outerS,outerA"]"#,
            ),
            (
                include_str!("../../rigen/check/cases/class_delete_super.js"),
                r#"["member:ReferenceError","key","index:ReferenceError","arrow:ReferenceError","static:ReferenceError","kept:function","before super():ReferenceError","sloppy literal:ReferenceError","literal kept:1"]"#,
            ),
            (
                include_str!("../../rigen/check/cases/class_key_order.js"),
                r#"[["1","2","prototype","length","name","b","g","c","d"],["constructor","m","n","h"],["prototype","length","name","z"],["constructor","x","y"]]"#,
            ),
            (
                include_str!("../../rigen/check/cases/class_key_order_fields.js"),
                r#"[["prototype","length","name","b","g","a","c"],["constructor","m"],["x","y","z"]]"#,
            ),
        ] {
            assert_eq!(run(&format!("{case}\nreturn result")), want);
        }
    }

    /// `new.target` and the NewTarget `super(...)` passes to the parent's [[Construct]],
    /// through bound functions and proxy `construct` traps (ECMA-262 §10.2.2, §13.3.7.1,
    /// §10.4.1.2, §10.5.13). Expected values are ECMA-262's (V8 agrees): Qt 6.12's V4 drops
    /// NewTarget through bound functions and reads NewTarget's `prototype` in no derived
    /// [[Construct]], and has no class fields. `super()` in an arrow function is QV4's
    /// ReferenceError.
    #[test]
    fn new_target_reaches_super_and_proxy_construct() {
        let case = include_str!("../../rigen/check/cases/new_target.js");
        assert_eq!(
            run(&format!("{case}\nreturn result")),
            r#"["call => true","method call => true","new => true","reflect => true","arrow => true","arrow call => true","nested function => true","bound => true","bound reflect => true","class => true","class reflect => true","derived implicit => true,true","derived explicit => true,true,true","derived reflect => true,true","derived two levels => true,true","derived function parent => true","super in arrow THROW ReferenceError","super proxy trap => true,true,true","super proxy trap explicit => true,7,true","super proxy trap reflect => true,true","super proxy trap args => 2:1:2","super proxy trap result => 1,2,false","super proxy no trap => true,true","super bound parent => true,true","super twice => ReferenceError,2,true","prototype read once => 1","super not constructor THROW TypeError"]"#
        );
        let case = include_str!("../../rigen/check/cases/new_target_fields.js");
        assert_eq!(
            run(&format!("{case}\nreturn result")),
            r#"["field => true","field arrow => true","derived field => true,true","derived field explicit => true,true","computed key => h"]"#
        );
    }

    /// Methods, accessors and arrows have no `[[Construct]]` and no own `prototype` (ECMA-262
    /// DefineMethod without MakeConstructor); `instanceof` needs a callable with an object
    /// `prototype` (OrdinaryHasInstance). Expected values are Qt 6.12's.
    #[test]
    fn methods_and_accessors_are_not_constructors() {
        let pre = "function t(f) { try { return 'ok:' + f() } catch (e) { return e.name } } \
                   var o = { m() { return 1 }, f: function () {} }; \
                   class C { constructor() {} m() {} static sm() {} } ";
        for (src, want) in [
            (
                "return [t(() => typeof new o.m()), typeof o.m.prototype, 'prototype' in o.m]",
                "TypeError,undefined,false",
            ),
            (
                "return [t(() => typeof new (new C().m)()), typeof C.prototype.m.prototype, \
                 t(() => typeof new C.sm()), typeof C.sm.prototype]",
                "TypeError,undefined,TypeError,undefined",
            ),
            (
                "return [t(() => typeof new o.f()), typeof o.f.prototype, \
                 t(() => typeof new C()), typeof C.prototype]",
                "ok:object,object,ok:object,object",
            ),
            (
                "return [t(() => ({} instanceof o.m)), t(() => 1 instanceof o.m), \
                 t(() => ({} instanceof (() => 1))), t(() => ({} instanceof {})), \
                 t(() => { function F() {} F.prototype = 3; return {} instanceof F }), \
                 t(() => new C() instanceof C), t(() => new Date() instanceof Date)]",
                "TypeError,ok:false,TypeError,TypeError,TypeError,ok:true,ok:true",
            ),
            (
                "o.m.prototype = 5; var p = {}; C.sm.prototype = p; \
                 return [o.m.prototype, Object.keys(o.m), Object.create(p) instanceof C.sm]",
                "5,prototype,true",
            ),
        ] {
            let src = format!("{pre}{src}");
            assert_eq!(run(&src), want, "{src}");
        }
    }

    #[test]
    fn in_and_for_in_follow_the_prototype_chain() {
        for (src, want) in [
            (
                "var p = { a: 1 }; var o = Object.create(p); o.b = 2; \
                 return ('a' in o) + ',' + ('b' in o) + ',' + ('c' in o)",
                "true,true,false",
            ),
            (
                "class A { m() {} } class B extends A {} var b = new B(); \
                 return ('m' in b) + ',' + b.hasOwnProperty('m')",
                "true,false",
            ),
            (
                "return ('hasOwnProperty' in {}) + ',' + ('push' in []) + ',' + \
                 ('call' in function () {})",
                "true,true,true",
            ),
            (
                "var p = { a: 1, b: 2, c: 3 }; var o = Object.create(p); o.b = 9; o.d = 4; \
                 var ks = []; for (var k in o) ks.push(k); return ks.join()",
                "b,d,a,c",
            ),
            (
                "class A { m() {} static s() {} } var ks = []; for (var k in new A()) ks.push(k); \
                 return ks.length + ',' + Object.keys(A.prototype).length",
                "0,0",
            ),
            (
                "var n = 0; for (var k in new Date(5)) n++; return [Math, JSON, Number, Object, \
                 Array, Date.prototype].map(o => Object.keys(o).length).join() + ',' + n",
                "0,0,0,0,0,0,0",
            ),
            (
                "Array.prototype.ap = 1; String.prototype.sp = 2; Object.prototype.op = 3; \
                 Object.prototype.push = 4; Object.prototype.length = 5; \
                 function ks(v) { var a = []; for (var k in v) a.push(k); return a.join() } \
                 return [ks([5]), ks('ab'), ks({ a: 1 }), ks(7), ks(function () {}), \
                 ks((function () { return arguments })(1)), ks(Object.create(null))].join('|')",
                "0,ap,op|0,1,sp,op,push|a,op,push,length|op,push,length|op,push|0,op,push|",
            ),
            (
                "try { return 'a' in 'abc' } catch (e) { return e.name }",
                "TypeError",
            ),
            (
                "try { return 'a' in undefined } catch (e) { return e.name }",
                "TypeError",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }

    #[test]
    fn mapped_arguments_alias_formals() {
        for (src, want) in [
            (
                "function f(a) { arguments[0] = 9; return a } return f(1)",
                "9",
            ),
            (
                "function f(a) { a = 9; return arguments[0] } return f(1)",
                "9",
            ),
            (
                "function f(a) { (function () { a = 5 })(); return arguments[0] } return f(1)",
                "5",
            ),
            (
                "function f(a) { (() => arguments[0] = 6)(); return a } return f(1)",
                "6",
            ),
            (
                "function f(a, a) { arguments[1] = 9; return a } return f(1, 2)",
                "9",
            ),
            (
                "function f(a) { arguments[0]++; a += 10; return a + ':' + arguments[0] } \
                 return f(1)",
                "12:12",
            ),
            // [[Delete]] unmaps: the formal keeps its value (§10.4.4.5).
            (
                "function f(a) { delete arguments[0]; arguments[0] = 3; \
                 return a + ':' + arguments[0] } return f(1)",
                "1:3",
            ),
            (
                "function f(a) { delete arguments[0]; a = 4; return a + ':' + arguments[0] } \
                 return f(1)",
                "4:undefined",
            ),
            // Only formals with an argument are mapped (§10.4.4.7 step 17.b); V4 maps all.
            (
                "function f(a, b) { b = 7; arguments[1] = 8; \
                 return b + ':' + arguments.length + ':' + arguments[1] } return f(1)",
                "7:1:8",
            ),
            // `length` is an ordinary property: setting it keeps the mapping; V4 drops it.
            (
                "function f(a) { arguments.length = 0; a = 2; \
                 return arguments[0] + ':' + arguments.length } return f(1)",
                "2:0",
            ),
            // A function declaration initializes the formal's binding (§10.2.11 step 36).
            (
                "function f(a) { function a() {} return typeof arguments[0] } return f(1)",
                "function",
            ),
            // Unmapped: strict code, non-simple parameter lists (§10.4.4.6).
            (
                "function f(a) { 'use strict'; arguments[0] = 9; return a } return f(1)",
                "1",
            ),
            (
                "function f(a = 0) { arguments[0] = 9; return a } return f(1)",
                "1",
            ),
            (
                "function f(a, ...r) { arguments[0] = 9; return a } return f(1)",
                "1",
            ),
            (
                "class C { m(a) { arguments[0] = 9; return a } } return new C().m(1)",
                "1",
            ),
            (
                "function f() { 'use strict'; return arguments.callee } \
                 try { f() } catch (e) { return e.name }",
                "TypeError",
            ),
            (
                "function f(a) { return [Array.isArray(arguments), arguments.callee === f, \
                 Object.keys(arguments).join(), typeof arguments.push, \
                 JSON.stringify(arguments)].join('|') } return f(1, 2)",
                "false|true|0,1|undefined|{\"0\":1,\"1\":2}",
            ),
            (
                "function f() { var r = []; for (var x of arguments) r.push(x); \
                 return r.join() + ':' + [...arguments].join() } return f(1, 2)",
                "1,2:1,2",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }

    /// Qt 6.12 (PySide6 probe): assigning a `const` (or a class's own name in its body) throws
    /// TypeError; a `let`/`const`/`class` binding read or written before its declaration ran
    /// throws ReferenceError (ECMA-262 §9.1.1.1), also from a closure and through `typeof`.
    #[test]
    fn const_assignment_and_tdz_throw() {
        let name = |src: &str| format!("try {{ {src} }} catch (e) {{ return e.name }}");
        for (src, want) in [
            ("const c = 1; c = 2; return c", "TypeError"),
            ("const c = 1; c += 1; return c", "TypeError"),
            ("const c = 1; c++; return c", "TypeError"),
            ("const c = 1; [c] = [2]; return c", "TypeError"),
            ("const c = 1; ({ c } = { c: 2 }); return c", "TypeError"),
            ("for (const i of [1]) { i = 5 } return 0", "TypeError"),
            (
                "const c = 1; function g() { c = 3 } g(); return c",
                "TypeError",
            ),
            ("{ const c = 1; c = 2 } return 0", "TypeError"),
            ("const c = 1; return (() => { c = 9 })()", "TypeError"),
            ("function f() { const c = 1; c = 2 } f()", "TypeError"),
            (
                "class K { m() { K = 1 } } new K().m(); return 0",
                "TypeError",
            ),
            ("var y = x; let x = 1; return y", "ReferenceError"),
            ("var t = typeof x; let x = 1; return t", "ReferenceError"),
            ("var y = k; const k = 1; return y", "ReferenceError"),
            ("x = 2; let x = 1; return x", "ReferenceError"),
            ("let x = x + 1; return x", "ReferenceError"),
            (
                "function g() { return x } var v = g(); let x = 1; return v",
                "ReferenceError",
            ),
            (
                "let x = 1; { var y = x; let x = 2 } return y",
                "ReferenceError",
            ),
            ("var o = new C(); class C {} return o", "ReferenceError"),
            (
                "switch (1) { case 0: let z = 1; break; case 1: return z }",
                "ReferenceError",
            ),
            (
                "for (let i = 0; i < 2; i++) { var v = j; let j = 1 } return v",
                "ReferenceError",
            ),
            ("x++; let x = 1", "ReferenceError"),
            (
                "function f() { var y = x; let x = 1; return y } return f()",
                "ReferenceError",
            ),
            (
                "function f() { function g() { return x } g(); let x } return f()",
                "ReferenceError",
            ),
        ] {
            assert_eq!(run(&name(src)), want, "{src}");
        }
        for (src, want) in [
            ("function g() { return x } let x = 1; return g()", "1"),
            ("let u; return typeof u", "undefined"),
            ("const c = 1; var r = c + 1; return r", "2"),
            (
                "var s = 0; const c = 1; try { c = (s = 5) } catch (e) { return s + e.name }",
                "5TypeError",
            ),
            (
                "const c = 1; try { c = 2 } catch (e) { return e.name + ':' + e.message }",
                "TypeError:",
            ),
            ("var x = 1; { let x = 2; x = 3 } return x", "1"),
            (
                "let r = []; for (let i = 0; i < 3; i++) { let j = i * 2; r.push(j) } return r.join()",
                "0,2,4",
            ),
            ("let a = 1; a = 2; return a", "2"),
            (
                "function f() { let a = 1; a += 2; return a } return f()",
                "3",
            ),
            ("let f = () => n; let n = 4; return f()", "4"),
            (
                "var o = class K { m() { return K === o } }; return new o().m()",
                "true",
            ),
            (
                "function g(a = b, b = 1) { return a } try { g(); return 'no' } catch (e) { return e.name }",
                "ReferenceError",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }

    #[test]
    fn function_bindings_and_generator_intrinsics() {
        for (src, want) in [
            (
                "return (function n() { n = 1; return typeof n })()",
                "function",
            ),
            (
                "return (function n() { 'use strict'; try { n = 1 } catch (e) { return e.name } return 'no' })()",
                "TypeError",
            ),
            (
                "function f(a = b, b) { return 'ran' } try { f(); return 'no' } catch (e) { return e.name }",
                "ReferenceError",
            ),
            ("function f(a, b = a) { return a + b } return f(2)", "4"),
            (
                "function f() { return this === undefined } return f()",
                "false",
            ),
            (
                "function f() { return typeof this } return f.call(null)",
                "object",
            ),
            (
                "function f() { return this } return f().Math === Math",
                "true",
            ),
            ("return typeof toString", "function"),
            (
                "var GF = Object.getPrototypeOf(function* () {}); var G = GF.constructor; \
                 return [GF[Symbol.toStringTag], GF.prototype[Symbol.toStringTag], G.name, \
                 G.length, G.prototype === GF, Object.getPrototypeOf(G) === Function, \
                 GF.prototype.next.length, GF.prototype.return.length, \
                 GF.prototype.throw.length].join()",
                "GeneratorFunction,Generator,GeneratorFunction,1,true,true,1,1,1",
            ),
            (
                "var G = Object.getPrototypeOf(function* () {}).constructor; var h = G('a', 'yield a; yield a + 1'); \
                 var i = h(3); return [i.next().value, i.next().value, Object.getPrototypeOf(h) === G.prototype].join()",
                "3,4,true",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }

    /// FunctionDeclarationInstantiation (ECMA-262 §10.2.11 steps 27-28): with parameter
    /// expressions the body's declarations live in their own VarEnvironment, which closures in
    /// the parameters never see; a body `var` named like a formal starts with its value.
    #[test]
    fn parameter_expressions_get_their_own_var_environment() {
        for (src, want) in [
            (
                "var x = 'outer'; function f(a = () => x) { var x; return a() } return f()",
                "outer",
            ),
            (
                "function f(a, b = () => a) { var a = 2; return [a, b()].join() } return f(1)",
                "2,1",
            ),
            (
                "function f(a, b = () => a) { var a; return a } return f(1)",
                "1",
            ),
            (
                "function f(x = 1, g = () => { x = 2 }, h = () => x) { var x; g(); return [x, h()].join() } return f()",
                "1,2",
            ),
            (
                "var h = 'outer'; function f(g = () => typeof h) { function h() {} return [typeof h, g()].join() } return f()",
                "function,string",
            ),
            (
                "var y = 'outer'; function f(g = () => y) { let y = 1; return [y, g()].join() } return f()",
                "1,outer",
            ),
            (
                "function f(a, g = () => arguments) { var arguments; return arguments === g() } return f(1)",
                "true",
            ),
            (
                "function f(a, g = () => arguments) { var arguments = 5; return [arguments, typeof g(), g()[0]].join() } return f(1)",
                "5,object,1",
            ),
            (
                "function f(a = 1) { var a = 2; return (() => a)() } return f()",
                "2",
            ),
            (
                "function f(a, b = () => a) { a = 2; return b() } return f(1)",
                "2",
            ),
            (
                "'use strict'; function f(a, b = () => a) { var a = 3; return [a, b()].join() } return f(1)",
                "3,1",
            ),
            (
                "function* g(a, b = () => a) { var a = 2; yield [a, b()].join() } return g(1).next().value",
                "2,1",
            ),
            (
                "function f({p} = {}, q = () => p) { var p = 'body'; return [p, q()].join() } return f({p: 'arg'})",
                "body,arg",
            ),
            (
                "function f(a, b = () => a) { var a; { var a = 4 } return [a, b()].join() } return f(1)",
                "4,1",
            ),
            (
                "function f(a = 0, g = () => k) { var k = 1; try { return g() } catch (e) { return e.name } } return f()",
                "ReferenceError",
            ),
            (
                "function f(a = 0) { var a; return (function () { return a })() } return f(6)",
                "6",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }

    #[test]
    fn undeclared_write_throws() {
        assert_eq!(
            run("nope = 1"),
            "Error: Invalid write to global property \"nope\""
        );
        assert_eq!(run("var a = 1; function f() { a = 2 } f(); return a"), "2");
        assert_eq!(
            run("try { x = 1 } catch (e) {} return typeof x"),
            "undefined"
        );
    }

    #[test]
    fn operators_use_to_primitive() {
        let p = "var o = { valueOf() { return 3 } }, s = { toString() { return '7' } }; \
                 var both = { valueOf() { return 10 }, toString() { return '20' } }; ";
        assert_eq!(
            run(&format!(
                "{p} return [o - 1, o * 2, o < 4, s * 2, s < 8, -o, ~o, o ** 2, o >>> 0].join()"
            )),
            "2,6,true,14,true,-3,-4,9,3"
        );
        assert_eq!(
            run(&format!(
                "{p} return [o + 1, s + 1, both + 1, both + '', o == 3, both == 10].join()"
            )),
            "4,71,11,10,true,true"
        );
        assert_eq!(
            run(
                "return [new Date(5) - new Date(2), new Date(9) > new Date(4), new Date(5) == 5].join()"
            ),
            "3,true,false"
        );
        assert_eq!(
            run("return [[5] * 2, [undefined] - 0, [1, 2] - 1].join()"),
            "10,0,NaN"
        );
        assert_eq!(
            run(
                "var x = { valueOf() { return 1 } }; x++; var y = { valueOf() { return 5 } }; y -= 2; return x + y"
            ),
            "5"
        );
        assert_eq!(
            run("var b = { valueOf() { return {} }, toString() { return {} } }; return b - 1"),
            "TypeError: Type error"
        );
        assert_eq!(
            run(
                "var l = []; var a = { valueOf() { l.push('a'); return 1 } }, b = { valueOf() { l.push('b'); return 2 } }; a > b; return l.join()"
            ),
            "a,b"
        );
    }

    #[test]
    fn arguments_object() {
        let mut it = Interp::default();
        let g = it.global.clone();
        let body = crate::ir::lower_program(
            "function f() { return arguments.length } \
             function h() { return (() => arguments[1])() } \
             function k(a) { return a } return [f(1, 2, 3), h(4, 5), k(6)].join(',')",
        )
        .unwrap();
        let v = match it.exec_block(&body, &g).unwrap() {
            Flow::Return(v) => v,
            _ => Value::Undefined,
        };
        assert_eq!(v.to_js_string(), "3,5,6");
    }

    #[test]
    fn hot_path_semantics() {
        let mut it = Interp::default();
        let g = it.global.clone();
        let body = crate::ir::lower_program(
            "class P { get v() { return this._v * 2 } set v(x) { this._v = x } }\n\
             class Q extends P { constructor() { super(); this.w = 1 }\n\
               m() { class L { f() { return 7 } } return new L().f() } }\n\
             var q = new Q(); q.v = 4; q.v = 5;\n\
             var fs = []; for (let i = 0; i < 2; i++) { const j = i * 10; fs.push(() => j) }\n\
             var s = 0; { let s = 5; s++ } { s = s + 1 }\n\
             function add(a, b) { var t = a + b; return t }\n\
             var obj = { add(a) { return a * 100 }, k() { return add(1, 2) } };\n\
             var out = [q.v, q.w, q.m(), fs[1](), s, obj.k(),\n\
               (0 / 0) < 1, (0 / 0) >= 1, 1 / 0 > 1e308, '3' * '4', 1 + '2', 2 - true,\n\
               '10' < '9', 10 < '9', null + 1].join(',')",
        )
        .unwrap();
        assert!(it.run(&body, &g).is_ok());
        let out = g.borrow().vars.get("out").cloned().unwrap();
        assert_eq!(
            out.to_js_string(),
            "10,1,7,10,1,3,false,false,true,12,12,1,true,false,1"
        );
    }

    /// `super.x = v` / `super[k] = v` is [[Set]] on the super base with `this` as the receiver
    /// (ECMA-262 §13.3.7.3, §6.2.5.6, §10.1.9.2). Values checked by hand against ECMA-262; the
    /// class case matches Qt 6.12's V4 (its `// v4:` pin), the literal one has no V4 home object.
    /// `super.ar = 1` in an arrow function writes to the `this` the arrow was called with, as
    /// V4's does, not to the method's.
    #[test]
    fn super_assignment_sets_on_this() {
        let run = |src: &str| {
            let mut it = Interp::default();
            let g = it.global.clone();
            let body = crate::ir::lower_program(src).unwrap();
            assert!(it.run(&body, &g).is_ok());
            let r = g.borrow().vars.get("result").cloned().unwrap();
            r.to_js_string()
        };
        assert_eq!(
            run(include_str!(
                "../../rigen/check/cases/class_super_assign.js"
            )),
            r#"[[true,5,0],[true,false],false,["A.s true 3"],"TypeError false","TypeError","TypeError 1","TypeError",[1,null],[8,9,2,false],"TypeError",["B","A",true],"ReferenceError","TypeError"]"#
        );
        assert_eq!(
            run(include_str!(
                "../../rigen/check/cases/super_assign_order.js"
            )),
            r#"[2,false,[true,3,0],false,[12,4,5,4],[9,false,0],true,6,["p1 1","rhs","key"]]"#
        );
    }

    /// SuperCall binds `this` to the parent's constructed object (ECMA-262 §13.3.7.1); a
    /// derived `this` is uninitialized until then. Expected values from V8 (QV4 6.12 has no
    /// class fields), but for QV4's own: `this.x = 1` before `super()` is a TypeError, and an
    /// arrow function of a derived constructor has no `this` and cannot call `super()`.
    #[test]
    fn super_binds_this() {
        let mut it = Interp::default();
        let g = it.global.clone();
        let body = crate::ir::lower_program(
            r#"function t(f) { try { return f() } catch (e) { return e.name } }
var o = { tag: 1 }, seen;
class B { bf = 1; constructor() { seen = this; return o } }
class C extends B { cf = 2; constructor() { super(); this.g = 3 } }
class D extends B { df = 4 }
class E extends B { constructor() { var f = () => super(); f(); this.e = 5 } }
class K extends B { constructor() { var g = () => this; super(); this.k = g() === o } }
class L extends B { constructor() { this.x = 1; super() } }
class M extends B { constructor() { super(); super() } }
class N extends B { constructor() { return 5 } }
class P extends B { constructor() {} }
function F() { return o }
class Q extends F { constructor() { var r = super(); this.q = r === o } }
var c = new C(), sc = seen, d = new D(), e = t(() => new E()), k = t(() => new K()), q = new Q();
var out = JSON.stringify([c === o, o.bf, o.cf, o.g, sc.bf, sc instanceof C, d === o, o.df,
    e === o, o.e, k === o, o.k, q === o, o.q, t(() => new L()), t(() => new M()),
    t(() => new N()), t(() => new P())]);"#,
        )
        .unwrap();
        assert!(it.run(&body, &g).is_ok());
        let out = g.borrow().vars.get("out").cloned().unwrap();
        assert_eq!(
            out.to_js_string(),
            r#"[true,null,2,3,1,true,true,4,false,null,false,null,true,true,"TypeError","ReferenceError","TypeError","ReferenceError"]"#
        );
    }

    #[test]
    fn inline_calls_match_v8() {
        let mut it = Interp::default();
        let g = it.global.clone();
        let body = crate::ir::lower_program(r#"var K = 10;
class P { constructor(x, y) { this.x = x; this.y = y } get s() { return this.x + this.y } get t() { return this.s * K } get u() { return this.missing } get n() { return new P(this.y, this.x) }
  plus(o) { return new P(this.x + o.x, this.y + o.y) } sc(k, m) { return this.x * k + m } neg() { return 0 - this.x } }
class Q extends P { get s() { return this.x - this.y } }
function mk(a) { return (b) => a * b + K }
function outer() { const z = 7; function inn(q) { return z + q } return inn(1) }
function arrowThis() { const o = { v: 3, f() { return (() => this.v * 2)() } }; return o.f() }
function str(a, b) { return a + b }
function noArg(a, b) { return b }
function tdef(a) { return typeof a }
function tg() { try { return new P(1).zz.q } catch (e) { return 'TE:' + typeof e } }
var p = new P(2, 5);
var out = [p.s, p.t, p.u, p.n.x, p.plus(new P(1, 1)).y, p.sc(3), p.sc(3, 1), p.neg(), new Q(9, 4).s, new Q(9, 4).t,
  mk(3)(4), outer(), arrowThis(), str('a', 1), str(1, 2), noArg(1), tg(), JSON.stringify(p.n)].join(',');
"#)
        .unwrap();
        if let Err(Ex::Throw(v)) = it.run(&body, &g) {
            let m = it.get(&v, "message").unwrap_or_default().to_js_string();
            panic!("threw {} {m}", v.to_js_string());
        }
        let out = g.borrow().vars.get("out").cloned().unwrap();
        assert_eq!(
            out.to_js_string(),
            r#"7,70,,5,6,NaN,7,-2,5,50,22,8,6,a1,3,,TE:object,{"x":5,"y":2}"#
        );
    }

    #[test]
    fn block_slots_match_v8() {
        let mut it = Interp::default();
        let g = it.global.clone();
        let body = crate::ir::lower_program(r#"function a(n) { let s = 0; for (let i = 0; i < n; i++) { const d = i * 2; let e; s += d + (e === undefined ? 1 : 0) } return s }
function b() { let x = 1; { let x = 2; { let x = 3; x++ } x += 10 } return x }
function c() { const out = []; for (const v of [1, 2, 3]) { let w = v * v; out.push(w) } return out.join('|') }
function d() { const fs = []; for (const v of [1, 2]) { let w = v; fs.push(() => w) } return fs.map(f => f()).join('') }
function e(k) { switch (k) { case 1: { let q = 5; return q } case 2: let r = 7; return r; default: return 0 } }
function f() { try { let t = 1; throw t + 1 } catch (err) { let u = err * 3; return u } finally { let z = 9 } }
function g() { let r = []; for (let i = 0; i < 3; i++) { let j = i; r.push(j) } return r.join(',') }
function h() { let acc = ''; for (let i = 0; i < 2; i++) for (let k = 0; k < 2; k++) acc += i + '' + k; return acc }
function m() { let x = 'outer'; let r; { r = x; let y = x + '!'; r += y } return r }
function n2() { var v = 1; { var v = 2 } return v }
function o() { let s = 0; for (let i in {p: 1, q: 2}) { let t = i.length; s += t } return s }
function p2() { let i = 10; for (let i = 0; i < 2; i++) {} return i }
function q() { let res = 0; let k = 0; while (k < 3) { let tmp = k * 10; res += tmp; k++ } return res }
function r2() { let s = ''; for (const [x, y] of [[1, 2], [3, 4]]) { s += x + y } return s }
function s2() { const fs = []; for (let i = 0; i < 3; i++) { const j = i; fs.push(function () { return j }) } return fs.map(f => f()).join('') }
function t2() { let v = 0; { let w = 1; v = (() => w + 1)() } return v }
function u2() { function inner() { let z = 4; { let z = 5; return z } } return inner() }
var out = [a(4), b(), c(), d(), e(1), e(2), e(3), f(), g(), h(), m(), n2(), o(), p2(), q(), r2(), s2(), t2(), u2()].join(',');
"#)
        .unwrap();
        assert!(it.run(&body, &g).is_ok());
        let out = g.borrow().vars.get("out").cloned().unwrap();
        assert_eq!(
            out.to_js_string(),
            "16,1,1|4|9,12,5,7,0,6,0,1,2,00011011,outerouter!,2,2,10,30,37,012,2,5"
        );
    }

    #[test]
    fn slot_resolved_locals_match_v8() {
        let mut it = Interp::default();
        let g = it.global.clone();
        let body = crate::ir::lower_program(r#"var G = 3;
function mk() { let c = 0; return () => ++c }
function f(x) { { let x = 2; var y = x } return x + y }
function h() { var r = typeof v; var v = 3; return r + v }
function d({a, b = a + 1}, [c, ...rest] = [9, 8, 7]) { return a + b + c + rest.length }
function ar(a) { return arguments.length + a }
function o(a) { function m(b) { return () => a + b } return m(2)() }
function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2) }
function cf() { class K { constructor(v) { this.v = v } get d() { return this.v * 2 } } return new K(4).d }
function cn() { class Z { static mk() { return new Z() } } return Z.mk() instanceof Z }
function fo() { const fs = []; for (const x of [1, 2]) fs.push(() => x); return fs.map(f => f()).join('') }
function ct() { try { throw 5 } catch (e) { return e + 1 } }
function sw(k) { switch (k) { case 1: let z = 10; return z; default: return -1 } }
function up() { let a = 1; a += 2; a++; ++a; return a }
function ty() { let u; return typeof u }
function dp(a, b = a * 2) { return b }
function hd() { return g(); function g() { return 'g' } }
function fv() { var fs = []; for (var i = 0; i < 2; i++) fs.push(() => i); return fs[0]() }
function gg() { G = G + 1; return G }
function rs(...xs) { let [p, q] = xs; return p * q }
function ob() { const k = 'w'; return JSON.stringify({ k, [k]: 1, f() { return k } }) }
var c1 = mk(); c1();
var out = [c1(), f(1), h(), d({a: 1}), d({a: 1, b: 0}, [5]), ar(1, 2, 3), o(1), fib(10), cf(), cn(), fo(), ct(),
  sw(1), sw(2), up(), ty(), dp(3), hd(), fv(), gg(), G, rs(6, 7), ob()].join(',');
"#)
        .unwrap();
        assert!(it.run(&body, &g).is_ok());
        let out = g.borrow().vars.get("out").cloned().unwrap();
        assert_eq!(
            out.to_js_string(),
            r#"2,3,undefined3,14,6,4,3,55,8,true,12,6,10,-1,5,undefined,6,g,2,4,4,42,{"k":"w","w":1}"#
        );
    }

    #[test]
    fn var_hoisting_array_ctor_normalize() {
        let mut it = Interp::default();
        let g = it.global.clone();
        let body = crate::ir::lower_program(
            "function f(c) { if (c) { var s = 1 } else { var s = 2 } return s }\n\
             class K extends Array {}\n\
             var k = new K(3); k[0] = 5;\n\
             var out = f(true) + ',' + f(false) + ',' + new Array(2).length + ',' + k.length \
             + ',' + k[0] + ',' + 'é'.normalize('NFD').length",
        )
        .unwrap();
        assert!(it.run(&body, &g).is_ok());
        let out = g.borrow().vars.get("out").cloned().unwrap();
        assert_eq!(out.to_js_string(), "1,2,2,3,5,2");
    }

    #[test]
    fn nested_calls_bind_own_params() {
        let mut it = Interp::default();
        let g = it.global.clone();
        let body = crate::ir::lower_program(
            "class P { constructor(x, y) { this.x = x; this.y = y } times(o) { return new P(this.x * o, this.y * o) } plus(b) { return new P(this.x + b.x, this.y + b.y) } }\n\
             function rc(radius, a, center = new P(0, 0)) { return new P(Math.cos(a), Math.sin(a)).times(radius).plus(center) }\n\
             function sv(n, radius, inner) { var r = []; for (let i = 0; i < n; i++) { let v = rc(radius, 0); r.push(v.x); v = rc(inner, 0); r.push(v.x) } return r }\n\
             var out = sv(2, 1, 0.8).join(',')",
        )
        .unwrap();
        assert!(it.run(&body, &g).is_ok());
        let out = g.borrow().vars.get("out").cloned().unwrap();
        assert_eq!(out.to_js_string(), "1,0.8,1,0.8");
    }

    #[test]
    fn library_chained_call_lines() {
        let mut it = Interp::default();
        let g = it.global.clone();
        let body = crate::ir::lower_program(
            ".pragma library\n.import \"x.js\" as X\nvar out = [1, 2]\n    .map(x => x * 2)\n    .join(',')",
        )
        .unwrap();
        assert!(it.run(&body, &g).is_ok());
        let out = g.borrow().vars.get("out").cloned().unwrap();
        assert_eq!(out.to_js_string(), "2,4");
    }
    #[test]
    fn callbacks_take_this_arg() {
        for (src, want) in [
            (
                "var c = { k: 2 }, s = 0; [1, 2].forEach(function (x) { s += x * this.k }, c); \
                 return s",
                "6",
            ),
            (
                "var c = { k: 1 }; return [1, 2].map(function (x) { return x + this.k }, c)",
                "2,3",
            ),
            (
                "var c = { k: 1 }; \
                 return [[1, 2].filter(function (x) { return x > this.k }, c), \
                 [1, 2].some(function (x) { return x === this.k }, c), \
                 [1, 2].every(function (x) { return x >= this.k }, c), \
                 [1, 2].find(function (x) { return x > this.k }, c), \
                 [1, 2].findIndex(function (x) { return x > this.k }, c), \
                 [1].flatMap(function (x) { return [x, this.k] }, c)].join('|')",
                "2|true|true|2|1|1,1",
            ),
            (
                "return [0].map(function () { 'use strict'; return typeof this + this }, 5)[0]",
                "number5",
            ),
            (
                "return [0].map(function () { 'use strict'; return typeof this })[0]",
                "undefined",
            ),
            (
                "var o = { t: 'o', f: function () { return [0].map(() => this.t, { t: 'a' })[0] } }; \
                 return o.f()",
                "o",
            ),
            (
                "var c = { k: 3 }, out = []; \
                 new Map([['a', 1]]).forEach(function (v, k) { out.push(k + v * this.k) }, c); \
                 new Set([2]).forEach(function (v) { out.push(v * this.k) }, c); \
                 return out.concat(Array.from([1], function (x) { return x * this.k }, c))",
                "a3,6,3",
            ),
            (
                "return [1].reduce(function () { 'use strict'; return typeof this }, 0)",
                "undefined",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }

    /// Qt 6.12: `new Number/Boolean/String` make wrapper objects (ECMA-262 §21.1.1.1, §20.3.1.1,
    /// §22.1.1.1) and non-strict code boxes a primitive `this` (§10.2.1.2); strict code keeps it.
    #[test]
    fn primitive_wrappers_and_sloppy_this() {
        for (src, want) in [
            ("return typeof new Number(1)", "object"),
            ("return typeof Number('1')", "number"),
            ("return new Boolean(false) ? 't' : 'f'", "t"),
            ("return Boolean(0) ? 't' : 'f'", "f"),
            (
                "return typeof new String('a') + typeof String(1)",
                "objectstring",
            ),
            (
                "var n = new Number(5); return [n + 1, n === 5, n == 5, n == new Number(5)].join()",
                "6,false,true,false",
            ),
            (
                "var s = new String('ab'); return [s.length, s[1], Object.keys(s), s + 'c'].join('|')",
                "2|b|0,1|abc",
            ),
            (
                "var s = new String('ab'); s[0] = 'z'; s.length = 9; delete s[1]; return s[0] + s[1] + s.length",
                "ab2",
            ),
            (
                "return JSON.stringify([new Number(3), new String('x'), new Boolean(false)])",
                "[3,\"x\",false]",
            ),
            (
                "return Object.prototype.toString.call(new Boolean(true))",
                "[object Boolean]",
            ),
            (
                "return new Number(255).toString(16) + new String('a').toUpperCase()",
                "ffA",
            ),
            (
                "try { Number.prototype.valueOf.call(new String('1')) } catch (e) { return e.name }",
                "TypeError",
            ),
            (
                "return Object(5) instanceof Number && Number.prototype.isPrototypeOf(new Number(1))",
                "true",
            ),
            (
                "return (function () { return this instanceof Number }).call(5)",
                "true",
            ),
            (
                "return (function () { 'use strict'; return typeof this }).call(5)",
                "number",
            ),
            (
                "Number.prototype.k = function () { return typeof this }; return (5).k()",
                "object",
            ),
            (
                "return [0].map(function () { return this instanceof Number }, 3)[0]",
                "true",
            ),
            (
                "return (function () { return (() => typeof this)() }).call('s')",
                "object",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }

    /// ECMA-262 §14.13.4: every label of a nested chain joins the loop's label set.
    #[test]
    fn nested_labels_share_the_label_set() {
        for (src, want) in [
            (
                "var n = 0; a: b: for (var i = 0; i < 3; i++) { n++; continue a } return n",
                "3",
            ),
            (
                "var n = 0; a: b: c: do { n++; if (n < 3) continue a } while (n < 5); return n",
                "5",
            ),
            (
                "var s = ''; a: b: for (var x of [1, 2, 3]) { s += x; continue a } return s",
                "123",
            ),
            (
                "var s = ''; a: b: for (var k in { p: 1, q: 2 }) { s += k; continue b } return s",
                "pq",
            ),
            (
                "var o = []; x: y: for (var i = 0; i < 3; i++) { for (;;) { if (i == 2) break x; \
                 o.push(i); continue y } } return o.join()",
                "0,1",
            ),
            (
                "var s = 'a'; p: q: { s += 'b'; break p; s += 'x' } return s",
                "ab",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }

    /// ECMA-262 §13.3.9.1: `(o?.f)()` keeps the Reference, so `this` is `o`.
    #[test]
    fn parenthesized_optional_chain_call_keeps_this() {
        for (src, want) in [
            (
                "var o = { f() { return this === o } }; return (o?.f)()",
                "true",
            ),
            (
                "var o = { f() { return this === o } }; return (o?.['f'])()",
                "true",
            ),
            (
                "var o = { g: { h() { return this === o.g } } }; return (o?.g.h)()",
                "true",
            ),
            (
                "var o = { f() { return this === o } }; return (o?.f)?.()",
                "true",
            ),
            (
                "var s = Symbol(), o = {}; o[s] = function () { return this === o }; \
                 return (o?.[s])()",
                "true",
            ),
            ("var z = null; return (z?.f)?.()", "undefined"),
            (
                "var z = null, n = 0; try { (z?.f)(n++) } catch (e) { return e.name + n }",
                "TypeError1",
            ),
        ] {
            assert_eq!(run(src), want, "{src}");
        }
    }
}

/// ri::jsprof: per-function call count, inclusive and self time, dumped every 5 s.
#[cfg(feature = "interp")]
mod jsprof {
    use super::Func;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct St {
        stack: Vec<(Instant, Duration)>,
        tab: HashMap<usize, (u64, Duration, Duration)>,
        names: HashMap<usize, String>,
        last: Option<Instant>,
    }
    thread_local!(static ST: RefCell<St> = RefCell::default());

    pub fn enter() {
        ST.with(|s| s.borrow_mut().stack.push((Instant::now(), Duration::ZERO)));
    }

    pub fn leave(f: &Func) {
        ST.with(|s| {
            let mut s = s.borrow_mut();
            let Some((t0, kids)) = s.stack.pop() else {
                return;
            };
            let inc = t0.elapsed();
            if let Some(p) = s.stack.last_mut() {
                p.1 += inc;
            }
            let key = f as *const Func as usize;
            s.names.entry(key).or_insert_with(|| match &f.name {
                Some(n) => n.clone(),
                None => {
                    let d = format!("{:?} {:?}", f.params, f.body.first());
                    d.chars().filter(|c| !c.is_whitespace()).take(160).collect()
                }
            });
            let e = s.tab.entry(key).or_default();
            e.0 += 1;
            e.1 += inc;
            e.2 += inc.saturating_sub(kids);
            let now = Instant::now();
            let last = *s.last.get_or_insert(now);
            if s.stack.is_empty() && now - last > Duration::from_secs(5) {
                s.last = Some(now);
                let mut v: Vec<_> = s.tab.drain().collect();
                v.sort_by_key(|e| std::cmp::Reverse(e.1.2));
                for (k, (n, inc, slf)) in v.into_iter().take(25) {
                    let k = s.names.get(&k).cloned().unwrap_or_default();
                    tracing::debug!(
                        target: "ri::jsprof",
                        "jsprof self {:>7}us incl {:>7}us n {n:>6} {k}",
                        slf.as_micros(),
                        inc.as_micros()
                    );
                }
            }
        });
    }
}

#[cfg(test)]
mod cycle_tests {
    use super::*;
    use crate::jsrt::{Cx, Env, FnKind, FnMeta};

    static META: FnMeta = FnMeta {
        name: "f",
        kind: FnKind::Arrow,
        nparams: 0,
        nslots: 0,
        strict: false,
    };

    fn body(_: &mut Cx, _: &Env, _: &Value, _: &[Value]) -> R<Value> {
        Ok(Value::Undefined)
    }

    /// `holder` <-> env slot, two closures over that env stored on `holder`.
    fn shared_env_cycle() -> Obj {
        let env = Env::from_scope(Rc::new(RefCell::new(Frame::default()))).child(1);
        let holder = new_obj();
        env.set(0, 0, Value::Obj(holder.clone()));
        set_own(&holder, "f", crate::jsrt::closure(body, &env, &META));
        set_own(&holder, "g", crate::jsrt::closure(body, &env, &META));
        set_own(&holder, "x", Value::Num(1.0));
        holder
    }

    #[test]
    fn shared_env_slots_count_once() {
        let holder = shared_env_cycle();
        collect_cycles();
        let b = holder.borrow();
        assert!(matches!(b.props.get("x"), Some(Value::Num(n)) if *n == 1.0));
        assert!(b.props.get("f").and_then(callable).is_some());
        assert!(b.props.get("g").and_then(callable).is_some());
    }

    #[test]
    fn unreachable_shared_env_cycle_is_freed() {
        let weak = Rc::downgrade(&shared_env_cycle());
        assert!(weak.upgrade().is_some());
        assert_eq!(collect_cycles(), 3);
        assert!(weak.upgrade().is_none());
    }
}
