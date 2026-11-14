//! Static scope resolution, frame layout and the per-function emitter state.
//!
//! Every JS function becomes one Rust `CompiledFn`. Names are resolved at codegen time:
//! a binding referenced from a nested function lives in an [`Env`](qml::jsrt::Env) slot of
//! the frame its scope creates (mirroring the interpreter's frames: one per call, block
//! entry, `for (let ..)` loop, `for-of/in` iteration, `switch`, `catch`, class body);
//! every other binding is a Rust local. Names bound nowhere go through the QML scope
//! (`jsrt::lookup`/`assign`/`rt::call_name`).
//!
//! Generated files start with [`HEADER`]; [`PRELUDE`] is the crate module `crate::rt`.
use std::collections::HashSet;
use std::fmt::Write;

use qml::ir::{Class, Expr, Func, Key, Pat, Stmt};

use crate::{BodyKind, EmitCtx};

/// Binding names of a function's `this` and of a class body's `super` heritage in the static
/// scope table. Both are reserved words, so no JS binding can collide with them (V8's
/// `DeclarationScope::DeclareThis` keys its receiver variable the same way).
pub(crate) const THIS: &str = "this";
pub(crate) const SUPER: &str = "super";
/// A function's [[NewTargetValue]] (ECMA-262 §9.1.1.3), read by `new.target` and passed on
/// by `super(...)` (not an identifier: no JS name can resolve to it).
pub(crate) const NEW_TARGET: &str = "%new_target";
/// A derived constructor's own function object, whose prototype `super(...)` constructs.
pub(crate) const ACTIVE_FN: &str = "%active_function";
/// The [[HomeObject]] of the methods closed over a scope (ECMA-262 §10.2.7): a class's
/// prototype or constructor, or an object literal, which `super.x` resolves through.
pub(crate) const HOME: &str = "%home";

/// Class-scope binding holding the property key of computed field `i`, evaluated once at
/// class definition; `%` keeps it apart from any JS identifier.
pub(crate) fn field_key(i: usize) -> String {
    format!("%field{i}")
}
/// The mapped arguments object holding a function's formals (no JS name can reach it).
pub(crate) const ARGS: &str = "\u{0}arguments";

/// Where a binding is stored.
#[derive(Clone, Debug)]
pub(crate) enum Store {
    /// Rust local `vN` of the current function.
    Local(String),
    /// Env slot of the frame at static `depth`.
    Slot(u32, u32),
    /// The function's `this` parameter.
    This,
    /// Formal `k` of a function with a mapped arguments object (held in the given binding).
    Arg(usize, Box<Store>),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Sc {
    pub names: Vec<(String, Store)>,
    /// Function boundary: `Local`s beyond it belong to another Rust function.
    pub fun: bool,
    /// `let`/`const`/`class` names of this scope that may still be in their temporal dead
    /// zone where code is emitted: their reads and writes check for `Value::Empty`.
    pub tdz: HashSet<String>,
    /// A `switch` scope: a case can run without the cases before it, so its bindings are
    /// never known to be initialized.
    pub switch: bool,
}

/// A break/continue target.
#[derive(Clone, Debug)]
pub(crate) enum Target {
    Loop {
        labels: Vec<String>,
        brk: String,
        cont: String,
    },
    Switch {
        brk: String,
    },
    Block {
        labels: Vec<String>,
        brk: String,
    },
    /// `try` with `finally`: jumps leaving it go through `Cpl`.
    Finally {
        lab: String,
        jumps: Vec<(bool, Option<String>)>,
        ret: bool,
    },
}

/// Where a thrown error goes.
#[derive(Clone, Debug)]
pub(crate) enum Sink {
    Fn,
    Res(String),
    Cpl(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CtorMode {
    /// An arrow inside a constructor: only `super()` needs the class.
    Inner,
    /// No `extends`: fields are initialized before the body.
    FieldsFirst,
    /// `extends` with a constructor: fields after `super()`.
    AfterSuper,
    /// `extends` without a constructor: parent constructor with all args, then fields.
    Implicit,
}

#[derive(Clone, Debug)]
pub(crate) struct ClassCtx {
    /// Static depth of the class scope frame.
    pub depth: u32,
    pub fields: String,
    pub fmeta: String,
    pub mode: CtorMode,
    /// The constructor is a derived class's own (`extends` with a constructor); arrow
    /// functions in it keep this.
    pub derived: bool,
}

/// Output shared by all functions of one emitted unit.
pub(crate) struct Gen {
    /// Handler body: the runtime binds signal params as scope vars, so free names keep the
    /// scope-chain walk (`rt::name`) here and in its closures.
    pub scope_vars: bool,
    pub items: String,
    pub base: String,
    pub n: usize,
}

impl Gen {
    pub fn id(&mut self, p: &str) -> String {
        self.n += 1;
        format!("{p}{}", self.n)
    }
}

/// Emitter state of one Rust function.
pub(crate) struct Fx<'g> {
    pub g: &'g mut Gen,
    pub out: String,
    pub scopes: Vec<Sc>,
    pub env: String,
    pub depth: u32,
    pub targets: Vec<Target>,
    pub sink: Sink,
    pub chain: Option<String>,
    /// Names referenced inside nested functions of this function (they live in slots).
    pub cap: HashSet<String>,
    /// Class constructor: `return` values are ignored.
    pub ctor: bool,
    /// The function is a derived class constructor: `this` is a mutable binding, uninitialized
    /// until `super(...)` binds it, and the [[Construct]] result is checked on return.
    pub derived_ctor: bool,
    /// The function is an arrow function: QV4 keeps the `this` of its call in its frame
    /// (`Frame::arrow_this` in the interpreter), here the `this` parameter.
    pub arrow: bool,
    /// The class whose constructor (or an arrow in it) this is, for `super()`.
    pub class: Option<ClassCtx>,
    /// The body reads its QML scope (`qs__`, found once per call).
    pub qs: bool,
    /// Emitting the inline parts of a class (heritage, computed keys): strict mode code in a
    /// possibly sloppy function, so refused writes and deletes throw whatever `cx.strict` is.
    pub strict_region: bool,
}

impl<'g> Fx<'g> {
    /// The [[Set]] of `o.name = v`: `Set(o, name, v, true)` in a strict region, else per
    /// `cx.strict`.
    pub fn put_member_fn(&self) -> &'static str {
        if self.strict_region {
            "put_member_strict"
        } else {
            "put_member"
        }
    }

    /// The [[Set]] of `o[k] = v`, see [`Fx::put_member_fn`].
    pub fn put_fn(&self) -> &'static str {
        if self.strict_region {
            "put_or_throw"
        } else {
            "put"
        }
    }

    /// `delete o[k]`, see [`Fx::put_member_fn`].
    pub fn delete_fn(&self) -> &'static str {
        if self.strict_region {
            "delete_strict"
        } else {
            "delete"
        }
    }

    pub fn new(g: &'g mut Gen, scopes: Vec<Sc>, depth: u32, cap: HashSet<String>) -> Self {
        Fx {
            g,
            out: String::new(),
            scopes,
            env: "env".into(),
            depth,
            targets: Vec::new(),
            sink: Sink::Fn,
            chain: None,
            cap,
            ctor: false,
            derived_ctor: false,
            arrow: false,
            class: None,
            qs: false,
            strict_region: false,
        }
    }

    pub fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    pub fn tmp(&mut self) -> String {
        self.g.id("t")
    }

    /// Open a scope declaring `names`; captured ones get a fresh Env frame. Returns the
    /// previous (env, depth) for [`Fx::close`].
    pub fn open(&mut self, names: &[String], fun_frame: bool) -> (String, u32) {
        let saved = (self.env.clone(), self.depth);
        let mut sc = Sc {
            names: Vec::new(),
            fun: fun_frame,
            ..Sc::default()
        };
        let mut nslots = 0u32;
        let mut seen = HashSet::new();
        for n in names {
            if !seen.insert(n.clone()) {
                continue;
            }
            if self.cap.contains(n) {
                sc.names.push((n.clone(), Store::Slot(u32::MAX, nslots)));
                nslots += 1;
            } else if n == THIS && !self.derived_ctor {
                sc.names.push((n.clone(), Store::This));
            } else {
                let v = self.g.id("v");
                let _ = writeln!(self.out, "let mut {v}: Value = Value::Undefined;");
                sc.names.push((n.clone(), Store::Local(v)));
            }
        }
        if nslots > 0 {
            let e = self.g.id("e");
            let _ = writeln!(self.out, "let {e} = Env::child(&{}, {nslots});", self.env);
            self.env = e;
            self.depth += 1;
        }
        for (_, s) in &mut sc.names {
            if let Store::Slot(d, _) = s {
                *d = self.depth;
            }
        }
        self.scopes.push(sc);
        saved
    }

    pub fn close(&mut self, saved: (String, u32)) {
        self.scopes.pop();
        self.env = saved.0;
        self.depth = saved.1;
    }

    pub fn slots_of_top(&self) -> u32 {
        self.scopes.last().map_or(0, |s| {
            s.names
                .iter()
                .filter(|(_, s)| matches!(s, Store::Slot(..)))
                .count() as u32
        })
    }

    pub fn find(&self, n: &str) -> Option<Store> {
        let mut crossed = false;
        for sc in self.scopes.iter().rev() {
            if let Some((_, s)) = sc.names.iter().rev().find(|(x, _)| x == n) {
                let captured = match &s {
                    Store::Arg(_, a) => matches!(**a, Store::Slot(..)),
                    s => matches!(s, Store::Slot(..)),
                };
                if crossed && !captured {
                    panic!("rigen: `{n}` used across a function boundary but not captured");
                }
                return Some(s.clone());
            }
            if sc.fun {
                crossed = true;
            }
        }
        None
    }

    /// `n` resolves to a binding of a function scope or a scope inside one.
    pub fn declared_in_function(&self, n: &str) -> bool {
        self.scopes
            .iter()
            .rposition(|sc| sc.names.iter().any(|(x, _)| x == n))
            .is_some_and(|i| self.scopes[..=i].iter().any(|sc| sc.fun))
    }

    /// The innermost scope declaring `n`.
    fn scope_of(&mut self, n: &str) -> Option<&mut Sc> {
        self.scopes
            .iter_mut()
            .rev()
            .find(|sc| sc.names.iter().any(|(x, _)| x == n))
    }

    /// `n` may be in its temporal dead zone here.
    pub fn maybe_uninit(&self, n: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .find(|sc| sc.names.iter().any(|(x, _)| x == n))
            .is_some_and(|sc| sc.tdz.contains(n))
    }

    /// BlockDeclarationInstantiation for the scope just opened for `body`: its
    /// `let`/`const`/`class` bindings start uninitialized (`Value::Empty`).
    pub fn enter_tdz(&mut self, body: &[Stmt]) {
        for s in body {
            let mut ns = Vec::new();
            match s {
                Stmt::Var(k, ds) if *k != "var" => ds.iter().for_each(|(p, _)| pat_ids(p, &mut ns)),
                Stmt::Class(c) => ns.extend(c.name.clone()),
                _ => {}
            }
            for n in ns {
                let Some(st) = self.find(&n) else { continue };
                let w = self.write(&st, "Value::Empty");
                self.line(&w);
                if let Some(sc) = self.scope_of(&n) {
                    sc.tdz.insert(n);
                }
            }
        }
    }

    /// The declaration of `n` has just been emitted: code after it in the same activation
    /// sees `n` initialized, unless a closure (`Slot`) or a later `switch` case reads it.
    pub fn initialized(&mut self, n: &str) {
        let local = matches!(self.find(n), Some(Store::Local(_)));
        if let Some(sc) = self.scope_of(n)
            && local
            && !sc.switch
        {
            sc.tdz.remove(n);
        }
    }

    /// Read of `n`, resolved to `s`.
    pub fn read_checked(&self, n: &str, s: &Store) -> String {
        let r = self.read(s);
        if self.maybe_uninit(n) {
            self.t(&format!("rt::bound(cx, {r}, {})", lit_str(n)))
        } else {
            r
        }
    }

    /// Read of a resolved binding.
    pub fn read(&self, s: &Store) -> String {
        match s {
            Store::Local(v) => format!("{v}.clone()"),
            Store::Slot(d, i) => format!("{}.get({}, {i})", self.env, self.depth - d),
            Store::This => "this.clone()".into(),
            Store::Arg(k, a) => format!("jsrt::arg_get(&{}, {k})", self.read(a)),
        }
    }

    pub fn write(&self, s: &Store, v: &str) -> String {
        match s {
            Store::Local(x) => format!("{x} = {v};"),
            Store::Slot(d, i) => format!("{}.set({}, {i}, {v});", self.env, self.depth - d),
            Store::This => format!("let _ = {v};"),
            Store::Arg(k, a) => format!("jsrt::arg_set(&{}, {k}, {v});", self.read(a)),
        }
    }

    /// `e` is `R<T>`: unwrap it or send the error to the current sink.
    pub fn t(&self, e: &str) -> String {
        match &self.sink {
            Sink::Fn => format!("({e})?"),
            Sink::Res(l) => format!("tb!({e}, '{l})"),
            Sink::Cpl(l) => format!("tc!({e}, '{l})"),
        }
    }

    /// Like [`Fx::t`], but `Ex::Short` (optional call on nullish) ends the current chain.
    pub fn ts(&self, e: &str) -> String {
        match &self.chain {
            None => self.t(e),
            Some(c) => format!(
                "match {e} {{ Ok(v) => v, Err(Ex::Short) => break '{c} Value::Undefined, Err(e) => {} }}",
                self.err_code("e")
            ),
        }
    }

    pub fn err_code(&self, e: &str) -> String {
        match &self.sink {
            Sink::Fn => format!("return Err({e})"),
            Sink::Res(l) => format!("break '{l} Err({e})"),
            Sink::Cpl(l) => format!("break '{l} Cpl::T({e})"),
        }
    }

    /// Code that ends the current optional chain with `undefined` (or `Ex::Short` outside one).
    pub fn short(&self) -> String {
        match &self.chain {
            Some(c) => format!("break '{c} Value::Undefined"),
            None => self.err_code("Ex::Short"),
        }
    }
}

pub(crate) fn lit_str(s: &str) -> String {
    format!("{s:?}")
}

thread_local! {
    static LITS: std::cell::RefCell<(std::collections::HashMap<String, u32>, Vec<String>)> =
        Default::default();
}

/// `js_str!(i, "s")` for a string literal: `i` is its index in the module's literal table.
pub(crate) fn js_str(s: &str) -> String {
    let i = LITS.with(|l| {
        let (idx, all) = &mut *l.borrow_mut();
        *idx.entry(s.to_string()).or_insert_with(|| {
            all.push(s.to_string());
            all.len() as u32 - 1
        })
    });
    format!("js_str!({i}, {})", lit_str(s))
}

/// A string literal with an unpaired surrogate ([`Expr::Str16`]): its code units, made into
/// a `Value::Str16` once per thread.
pub(crate) fn js_str16(u: &[u16]) -> String {
    let units: Vec<String> = u.iter().map(|c| format!("{c:#06x}")).collect();
    format!(
        "{{ thread_local! {{ static L: Value = Value::from_utf16(&[{}]); }} L.with(Clone::clone) }}",
        units.join(", ")
    )
}

/// The literal table of the bodies emitted since the last call (on this thread): items the
/// module holding those bodies must contain once, as `js_str!` refers to them.
pub fn take_literals() -> String {
    let all = LITS.with(|l| {
        let (idx, all) = &mut *l.borrow_mut();
        idx.clear();
        std::mem::take(all)
    });
    let mut s = format!(
        "#[allow(dead_code)]\nstatic LITS: [&str; {}] = [",
        all.len()
    );
    for l in &all {
        s.push_str(&lit_str(l));
        s.push_str(", ");
    }
    s.push_str(
        "];\nthread_local! { static LIT_CELLS: rt::LitCells = rt::LitCells::new(&LITS); }\n",
    );
    s
}

pub(crate) fn num(n: f64) -> String {
    if n.is_nan() {
        "f64::NAN".into()
    } else if n.is_infinite() {
        if n > 0.0 {
            "f64::INFINITY".into()
        } else {
            "f64::NEG_INFINITY".into()
        }
    } else {
        format!("f64::from_bits({:#x})", n.to_bits())
    }
}

// ---------- name collection ----------

/// `var` names of a function body, as the interpreter's `var_names`.
pub(crate) fn var_names(body: &[Stmt], out: &mut Vec<String>) {
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
            Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::Labeled(_, b) => walk(b, out),
            Stmt::Switch(_, cs) => cs.iter().flat_map(|c| &c.1).for_each(|s| walk(s, out)),
            Stmt::Try(t, c, f) => {
                t.iter().for_each(|s| walk(s, out));
                c.iter().flat_map(|c| &c.1).for_each(|s| walk(s, out));
                f.iter().flatten().for_each(|s| walk(s, out));
            }
            _ => {}
        }
    }
    body.iter().for_each(|s| walk(s, out));
}

pub(crate) fn pat_ids(p: &Pat, out: &mut Vec<String>) {
    match p {
        Pat::Dynamic(_, _, p) => pat_ids(p, out),
        Pat::Id(n) | Pat::Local(_, _, n) => out.push(n.clone()),
        Pat::Object(ps, rest) => {
            ps.iter().for_each(|(_, p)| pat_ids(p, out));
            rest.iter().for_each(|p| pat_ids(p, out));
        }
        Pat::Array(ps, rest) => {
            ps.iter().flatten().for_each(|p| pat_ids(p, out));
            rest.iter().for_each(|p| pat_ids(p, out));
        }
        Pat::Default(p, _) => pat_ids(p, out),
        Pat::Const(p, _) => pat_ids(p, out),
        Pat::Member(_) | Pat::Init(_) => {}
    }
}

/// `let`/`const`/function/class names declared directly in `body`.
pub(crate) fn lexical_names(body: &[Stmt], out: &mut Vec<String>) {
    for s in body {
        match s {
            Stmt::Var(k, ds) if *k != "var" => ds.iter().for_each(|(p, _)| pat_ids(p, out)),
            Stmt::Func(f) => out.extend(f.name.clone()),
            Stmt::Class(c) => out.extend(c.name.clone()),
            _ => {}
        }
    }
}

/// Every name referenced inside a nested function or class of `body` (any depth).
pub(crate) fn captured(body: &[Stmt], extra: &[&Expr]) -> HashSet<String> {
    let mut c = Cap {
        out: HashSet::new(),
        inside: false,
        own: true,
        new_target: false,
    };
    body.iter().for_each(|s| c.stmt(s));
    extra.iter().for_each(|e| c.expr(e));
    c.out
}

/// [`captured`] for the body of `f`, and whether its own function environment's NewTarget
/// is read (`new.target` or `super(...)`, also from nested arrows).
pub(crate) fn captured_func(f: &Func) -> (HashSet<String>, bool) {
    let mut c = Cap {
        out: HashSet::new(),
        inside: false,
        own: true,
        new_target: false,
    };
    f.params.iter().chain(f.rest.iter()).for_each(|p| c.pat(p));
    f.body.iter().for_each(|s| c.stmt(s));
    (c.out, c.new_target)
}

struct Cap {
    out: HashSet<String>,
    inside: bool,
    /// Still in the function environment the walk started in (only arrows nested since).
    own: bool,
    new_target: bool,
}

impl Cap {
    fn name(&mut self, n: &str) {
        if self.inside {
            self.out.insert(n.to_string());
        }
    }
    fn nested(&mut self, f: impl FnOnce(&mut Self)) {
        let was = self.inside;
        self.inside = true;
        f(self);
        self.inside = was;
    }
    fn func(&mut self, f: &Func) {
        let own = self.own;
        self.own &= f.is_arrow();
        self.nested(|c| {
            f.params.iter().chain(f.rest.iter()).for_each(|p| c.pat(p));
            f.body.iter().for_each(|s| c.stmt(s));
        });
        self.own = own;
    }
    /// GetNewTarget of the walk's own function environment.
    fn new_target(&mut self) {
        if self.own {
            self.name(NEW_TARGET);
            self.new_target = true;
        }
    }
    fn class(&mut self, k: &Class) {
        if let Some(e) = &k.extends {
            self.expr(e);
        }
        self.nested(|c| {
            c.out.insert(SUPER.into());
            for (i, (key, _, _)) in k.fields.iter().enumerate() {
                if matches!(key, qml::ir::Key::Computed(_)) {
                    c.out.insert(field_key(i));
                }
            }
            if let Some(n) = &k.name {
                c.out.insert(n.clone());
            }
            k.ctor.iter().for_each(|f| c.func(f));
            for m in &k.methods {
                c.key(&m.key);
                c.func(&m.func);
            }
            for (key, v, _) in &k.fields {
                c.key(key);
                // A field initializer is a method of its own (ECMA-262 §15.7.10).
                let own = std::mem::replace(&mut c.own, false);
                v.iter().for_each(|v| c.expr(v));
                c.own = own;
            }
            k.blocks.iter().for_each(|(_, b)| c.expr(b));
        });
        // Static fields and computed keys run in the class scope: `this` of the outside.
        self.out.insert(THIS.into());
    }
    fn key(&mut self, k: &Key) {
        if let Key::Computed(e) = k {
            self.expr(e);
        }
    }
    fn pat(&mut self, p: &Pat) {
        match p {
            Pat::Dynamic(_, _, p) => self.pat(p),
            Pat::Id(n) | Pat::Local(_, _, n) => self.name(n),
            Pat::Object(ps, r) => {
                for (k, p) in ps {
                    self.key(k);
                    self.pat(p);
                }
                r.iter().for_each(|p| self.pat(p));
            }
            Pat::Array(ps, r) => {
                ps.iter().flatten().for_each(|p| self.pat(p));
                r.iter().for_each(|p| self.pat(p));
            }
            Pat::Default(p, e) => {
                self.pat(p);
                self.expr(e);
            }
            Pat::Member(e) | Pat::Init(e) => self.expr(e),
            Pat::Const(p, _) => self.pat(p),
        }
    }
    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Dynamic(_, _, x) | Expr::DirectEval(_, x) => self.expr(x),
            Expr::Ident(n) | Expr::Local(_, _, n) => self.name(n),
            Expr::This => self.name(THIS),
            Expr::Super => {
                self.name(SUPER);
                self.name(HOME);
            }
            Expr::NewTarget => self.new_target(),
            Expr::Template(_, v) | Expr::Array(v) | Expr::Seq(v) => {
                v.iter().for_each(|e| self.expr(e))
            }
            Expr::Yield(a, _) => a.iter().for_each(|e| self.expr(e)),
            Expr::Member(o, _, _) | Expr::Chain(o) | Expr::Unary(_, o) | Expr::Spread(o) => {
                if matches!(**o, Expr::Super) {
                    self.name(THIS);
                }
                self.expr(o)
            }
            Expr::Index(a, b, _) if matches!(**a, Expr::Super) => {
                self.name(THIS);
                self.name(HOME);
                self.expr(b);
            }
            Expr::Index(a, b, _)
            | Expr::DefineField(a, b, _)
            | Expr::Binary(_, a, b)
            | Expr::Logical(_, a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Call(c, a, _) | Expr::New(c, a) => {
                if matches!(**c, Expr::Super) {
                    self.name(THIS);
                    self.new_target();
                    if self.own {
                        self.name(ACTIVE_FN);
                    }
                }
                self.expr(c);
                a.iter().for_each(|e| self.expr(e));
            }
            Expr::Update(_, _, p) => self.pat(p),
            Expr::Assign(_, p, v) => {
                self.pat(p);
                self.expr(v);
            }
            Expr::Cond(a, b, c) => {
                self.expr(a);
                self.expr(b);
                self.expr(c);
            }
            Expr::Object(ps) => ps.iter().for_each(|(k, v, _)| {
                self.key(k);
                self.expr(v);
            }),
            Expr::Func(f) => {
                self.func(f);
                if f.is_arrow() {
                    // `this`/`arguments` of an arrow are the enclosing function's.
                    self.out.insert(THIS.into());
                    self.out.insert("arguments".into());
                }
            }
            Expr::Class(c) => self.class(c),
            Expr::Num(_)
            | Expr::Str(_)
            | Expr::Str16(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Regex(..)
            | Expr::TemplateObject(_)
            | Expr::Hole
            | Expr::Unsupported(_) => {}
        }
    }
    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => self.expr(e),
            Stmt::Var(_, ds) => ds.iter().for_each(|(p, e)| {
                self.pat(p);
                e.iter().for_each(|e| self.expr(e));
            }),
            Stmt::Func(f) => self.func(f),
            Stmt::If(c, a, b) => {
                self.expr(c);
                self.stmt(a);
                b.iter().for_each(|b| self.stmt(b));
            }
            Stmt::Block(b) => b.iter().for_each(|s| self.stmt(s)),
            Stmt::For(i, t, u, b) => {
                i.iter().for_each(|s| self.stmt(s));
                t.iter().chain(u.iter()).for_each(|e| self.expr(e));
                self.stmt(b);
            }
            Stmt::ForOf(_, p, e, b) | Stmt::ForIn(_, p, e, b) => {
                self.pat(p);
                self.expr(e);
                self.stmt(b);
            }
            Stmt::While(e, b) | Stmt::DoWhile(b, e) => {
                self.expr(e);
                self.stmt(b);
            }
            Stmt::Switch(d, cs) => {
                self.expr(d);
                for (t, b) in cs {
                    t.iter().for_each(|e| self.expr(e));
                    b.iter().for_each(|s| self.stmt(s));
                }
            }
            Stmt::Labeled(_, b) => self.stmt(b),
            Stmt::With(e, b) => {
                self.expr(e);
                self.stmt(b);
            }
            Stmt::Class(c) => self.class(c),
            Stmt::Try(a, c, f) => {
                a.iter().for_each(|s| self.stmt(s));
                if let Some((p, b)) = c {
                    p.iter().for_each(|p| self.pat(p));
                    b.iter().for_each(|s| self.stmt(s));
                }
                f.iter().flatten().for_each(|s| self.stmt(s));
            }
            Stmt::Return(None)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::Empty
            | Stmt::Unsupported(_) => {}
        }
    }
}

// ---------- entry points ----------

const ALLOW: &str = "#[allow(unused_mut, unused_variables, unused_labels, unreachable_code, unused_assignments, unused_braces, unused_parens, non_snake_case, clippy::all)]";

/// A `CompiledFn` body for the function `f` itself (a QML `function` member: the runtime makes
/// the value with `jsrt::closure(f, &env, &meta)` over the member's root env): a block expression of type
/// `R<Value>` with `cx`, `env`, `this`, `args` in scope. Nested functions are items inside it.
pub fn emit_function(f: &Func) -> String {
    let mut g = Gen {
        items: String::new(),
        base: "u".into(),
        n: 0,
        scope_vars: false,
    };
    let (out, _) = coroutine_or_body(&mut g, f, Vec::new(), 0, None);
    format!("{{\n{}{out}Ok(Value::Undefined)\n}}", g.items)
}

/// A `CompiledFn` body for a binding / handler / library (called with a root `env`; it opens a
/// frame only for captured bindings): a block expression of type `R<Value>` with `cx`, `env`,
/// `this`, `args` in scope. Bindings and handlers return their completion value; libraries
/// `jsrt::export_slot` their top-level names. A `Function` body (`[Stmt::Func(f)]`)
/// is [`emit_function`] of `f`.
pub fn emit_body(body: &[Stmt], kind: BodyKind, ctx: &EmitCtx) -> String {
    if kind == BodyKind::Function
        && let [Stmt::Func(f)] = body
    {
        return emit_function(f);
    }
    // A library that includes files shares its top-level names with them: by name, in its
    // context's scope.
    let script = ctx.script || (kind == BodyKind::Library && qml::ir::uses_include(body));
    let mut g = Gen {
        items: String::new(),
        base: "u".into(),
        n: 0,
        scope_vars: kind == BodyKind::Handler || script,
    };
    let fn_handler = kind == BodyKind::Handler && qml::ir::is_function_handler(body);
    let params: Vec<String> = if kind == BodyKind::Handler && !fn_handler {
        ctx.params.clone()
    } else {
        Vec::new()
    };
    let mut cap = captured(body, &[]);
    let mut names: Vec<String> = vec![THIS.into()];
    names.extend(params.iter().cloned());
    let mut decls: Vec<String> = Vec::new();
    var_names(body, &mut decls);
    let mut lexical: Vec<String> = Vec::new();
    lexical_names(body, &mut lexical);
    // The top-level names of a library are properties of its namespace object, which reads
    // and writes their slots: all of them live in the body's frame.
    let library = kind == BodyKind::Library && !ctx.script;
    if library {
        cap.extend(decls.iter().chain(&lexical).cloned());
    }
    let mut fx = Fx::new(&mut g, Vec::new(), 0, cap);
    let funcs: Vec<String> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::Func(f) => f.name.clone(),
            _ => None,
        })
        .collect();
    if script {
        // `var`s and functions are names of the context the script runs in, read and written by
        // name; its lexical declarations stay its own (QV4 keeps them to the script).
        lexical.retain(|n| !funcs.contains(n));
        decls.extend(funcs.iter().cloned());
    } else {
        decls.append(&mut lexical.clone());
    }
    names.extend(if script { &lexical } else { &decls }.iter().cloned());
    let saved = fx.open(&names, false);
    fx.init_this();
    if library {
        // `var`s and functions, which a library that includes files keeps by name in its
        // context's scope, are enumerable and writable there; lexical names are read-only.
        let mut exported: Vec<&String> = if script { &lexical } else { &decls }.iter().collect();
        exported.sort_unstable();
        exported.dedup();
        for n in exported {
            if let Some(Store::Slot(_, slot)) = fx.find(n) {
                let var_like = funcs.contains(n) || !lexical.contains(n);
                let (name, env) = (lit_str(n), &fx.env);
                let export = format!("jsrt::export_slot(cx, {name}, &{env}, {slot}, {var_like});");
                fx.line(&export);
            }
        }
    }
    if script {
        let mut seen = HashSet::new();
        for n in decls.iter().filter(|n| seen.insert(*n)) {
            fx.line(&format!("jsrt::declare_global(cx, {});", lit_str(n)));
        }
    }
    for (i, p) in params.iter().enumerate() {
        let s = fx.find(p).unwrap();
        let w = fx.write(
            &s,
            &format!(
                "rt::arg_or_lookup(cx, env, qs__, args, {i}, sym!({}))",
                lit_str(p)
            ),
        );
        fx.line(&w);
        fx.qs = true;
    }
    match body {
        // The handler is the function: called with the signal's arguments, `this` the object;
        // what it returns is the handler's value.
        [Stmt::Expr(f) | Stmt::Return(Some(f))] if fn_handler => {
            let f = fx.expr(f);
            let t = fx.tmp();
            fx.line(&format!("let {t} = {f};"));
            fx.line(&format!("return cx.call(&{t}, this.clone(), args);"));
        }
        _ => fx.block_body(body),
    }
    fx.close(saved);
    let out = fx.take_out();
    let env = if fn_handler {
        "let env = &rt::without_signal_params(env);\n"
    } else {
        ""
    };
    format!("{{\n{}{env}{out}Ok(Value::Undefined)\n}}", g.items)
}

impl Fx<'_> {
    /// The emitted statements, preceded by the QML scope lookup when the body uses it.
    fn take_out(&mut self) -> String {
        let out = std::mem::take(&mut self.out);
        if self.qs {
            format!("let qs__ = rt::qscope(env);\n{out}")
        } else {
            out
        }
    }

    /// Store `this` into its binding when it was captured into a slot.
    pub fn init_this(&mut self) {
        let Some(sc) = self.scopes.last_mut() else {
            return;
        };
        if let Some(i) = sc.names.iter().position(|(n, _)| n == THIS) {
            match sc.names[i].1.clone() {
                Store::Local(_) => sc.names[i].1 = Store::This,
                Store::This => {}
                s => {
                    let w = self.write(&s, "this.clone()");
                    self.line(&w);
                }
            }
        }
    }
}

/// Statements of the Rust fn for `f` (without the final `Ok(Value::Undefined)`) and the slot
/// count of its frame. `scopes`/`depth` describe the lexical position of the closure (its env is
/// at `depth`; the body makes its own frame when it has captured bindings).
/// [`func_body`], or for an async function (which compiled code does not run, and
/// `ir::unsupported` fails the build on) a body throwing like an unsupported construct.
fn coroutine_or_body(
    g: &mut Gen,
    f: &Func,
    scopes: Vec<Sc>,
    depth: u32,
    class: Option<ClassCtx>,
) -> (String, u32) {
    match f.coroutine {
        Some(c) if !c.runs() => (
            format!("return rt::unsupported(cx, {});\n", lit_str(c.name())),
            0,
        ),
        _ => func_body(g, f, scopes, depth, class),
    }
}

pub(crate) fn func_body(
    g: &mut Gen,
    f: &Func,
    scopes: Vec<Sc>,
    depth: u32,
    class: Option<ClassCtx>,
) -> (String, u32) {
    let mode = class.as_ref().map(|c| c.mode);
    let mapped = f.mapped_arguments();
    let formals: Vec<String> = match mapped {
        true => f
            .params
            .iter()
            .flat_map(|p| {
                let mut n = Vec::new();
                pat_ids(p, &mut n);
                n
            })
            .collect(),
        false => Vec::new(),
    };
    let (mut cap, reads_new_target) = captured_func(f);
    if formals.iter().any(|n| cap.contains(n)) {
        cap.insert(ARGS.into());
    }
    let derived = mode == Some(CtorMode::AfterSuper);
    let new_target = !f.is_arrow() && (reads_new_target || mode == Some(CtorMode::Implicit));
    let mut fx = Fx::new(g, scopes, depth, cap);
    fx.ctor = matches!(mode, Some(m) if m != CtorMode::Inner);
    fx.derived_ctor = derived;
    fx.arrow = f.is_arrow() && !f.is_gen_step();
    fx.class = class;
    let mut names: Vec<String> = Vec::new();
    if new_target {
        names.push(NEW_TARGET.into());
    }
    if derived && new_target {
        names.push(ACTIVE_FN.into());
    }
    if !f.is_arrow() {
        names.push(THIS.into());
        if f.uses_arguments {
            names.push("arguments".into());
        }
    }
    f.params
        .iter()
        .chain(f.rest.iter())
        .for_each(|p| pat_ids(p, &mut names));
    // With parameter expressions the body's declarations get an environment of their own
    // (ECMA-262 §10.2.11 step 28), opened once the formals are bound.
    let split = f.param_exprs();
    let mut vars = Vec::new();
    var_names(&f.body, &mut vars);
    let mut body = vars.clone();
    lexical_names(&f.body, &mut body);
    if !split {
        names.append(&mut body);
    }
    if mapped {
        names.retain(|n| !formals.contains(n));
        names.push(ARGS.into());
    }
    let saved = fx.open(&names, true);
    if !f.is_arrow() && !f.strict {
        fx.line("let boxed_this = jsrt::sloppy_this(cx, this);");
        fx.line("let this = boxed_this.as_ref().unwrap_or(this);");
    }
    if new_target {
        let n = fx.find(NEW_TARGET).unwrap();
        let w = fx.write(&n, "env.new_target.clone()");
        fx.line(&w);
    }
    if derived && new_target {
        let n = fx.find(ACTIVE_FN).unwrap();
        let w = fx.write(&n, "env.callee.clone().unwrap_or_default()");
        fx.line(&w);
    }
    if derived {
        // OrdinaryCallBindThis is skipped: `this` stays uninitialized until `super(...)`.
        let t = fx.find(THIS).unwrap();
        let w = fx.write(&t, "Value::Empty");
        fx.line(&w);
        if let Some(sc) = fx.scope_of(THIS) {
            sc.tdz.insert(THIS.into());
        }
    } else {
        fx.init_this();
    }
    if mapped {
        // The formals' bindings live in the mapped arguments object (ECMA-262 §10.4.4.7).
        let ids = (0..formals.len())
            .map(|k| match formals[k + 1..].contains(&formals[k]) {
                true => "u32::MAX".to_string(),
                false => k.to_string(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let a = fx.find(ARGS).unwrap();
        let w = fx.write(
            &a,
            &format!("jsrt::mapped_arguments(cx, env, args, &[{ids}])"),
        );
        fx.line(&w);
        let s = fx.find("arguments").unwrap();
        let w = fx.write(&s, &fx.read(&a));
        fx.line(&w);
        let sc = fx.scopes.last_mut().unwrap();
        for (k, n) in formals.iter().enumerate() {
            if !formals[k + 1..].contains(n) {
                sc.names
                    .push((n.clone(), Store::Arg(k, Box::new(a.clone()))));
            }
        }
    } else if !f.is_arrow() && f.uses_arguments {
        let s = fx.find("arguments").unwrap();
        let w = fx.write(&s, "jsrt::arguments(cx, args)");
        fx.line(&w);
    }
    if !f.simple_params() {
        // The formals start uninitialized, so a parameter expression reading a later one
        // throws (ECMA-262 §10.2.11 step 28, `Interp::activate`).
        let mut ns = Vec::new();
        f.params
            .iter()
            .chain(f.rest.iter())
            .for_each(|p| pat_ids(p, &mut ns));
        for n in ns {
            let Some(st) = fx.find(&n) else { continue };
            let w = fx.write(&st, "Value::Empty");
            fx.line(&w);
            if let Some(sc) = fx.scope_of(&n) {
                sc.tdz.insert(n);
            }
        }
    }
    for (i, p) in f.params.iter().enumerate() {
        if mapped {
            break;
        }
        let v = format!("args.get({i}).cloned().unwrap_or_default()");
        fx.bind(p, &v, true);
    }
    if let Some(r) = &f.rest {
        let n = f.params.len();
        fx.bind(
            r,
            &format!("Value::array(args.get({n}..).unwrap_or(&[]).to_vec())"),
            true,
        );
    }
    match mode {
        Some(CtorMode::FieldsFirst) => fx.call_fields(),
        Some(CtorMode::Implicit) => {
            // `constructor(...args) { super(...args) }`: the parent's object is the result.
            let nt = fx.read_name(NEW_TARGET);
            let c = fx.t(&format!(
                "rt::super_construct(cx, &env.callee.clone().unwrap_or_default(), args, &{nt})"
            ));
            let t = fx.tmp();
            let fields = fx.fields_call(&t);
            fx.line(&format!("let {t} = {c}; {fields} return Ok({t});"));
        }
        _ => {}
    }
    let body_scope = split.then(|| {
        // Step 28.f: a `var` named like a formal, or `arguments` where that names the
        // arguments object, starts as that binding's value; any other starts undefined.
        let funcs: Vec<&String> = f
            .body
            .iter()
            .filter_map(|s| match s {
                Stmt::Func(g) => g.name.as_ref(),
                _ => None,
            })
            .collect();
        let mut bound = Vec::new();
        f.params
            .iter()
            .chain(f.rest.iter())
            .for_each(|p| pat_ids(p, &mut bound));
        if !f.is_arrow() && f.uses_arguments {
            bound.push("arguments".into());
        }
        let mut seen = HashSet::new();
        let inits: Vec<(String, String)> = vars
            .iter()
            .filter(|n| seen.insert(*n) && !funcs.contains(n) && bound.contains(n))
            .filter_map(|n| {
                let st = fx.find(n)?;
                let t = fx.tmp();
                let r = fx.read(&st);
                fx.line(&format!("let {t} = {r};"));
                Some((n.clone(), t))
            })
            .collect();
        let saved = fx.open(&body, false);
        for (n, t) in inits {
            let st = fx.find(&n).unwrap();
            let w = fx.write(&st, &t);
            fx.line(&w);
        }
        saved
    });
    fx.block_body(&f.body);
    if derived {
        fx.ret("Value::Undefined");
    }
    if let Some(saved) = body_scope {
        fx.close(saved);
    }
    let nslots = fx.slots_of_top();
    fx.close(saved);
    (fx.take_out(), nslots)
}

/// Emit `f` as a Rust fn item `name` plus its `static meta: FnMeta` into `g.items`.
pub(crate) fn emit_func_item(
    g: &mut Gen,
    f: &Func,
    scopes: Vec<Sc>,
    depth: u32,
    name: &str,
    meta: &str,
    class: Option<ClassCtx>,
) {
    let (out, nslots) = coroutine_or_body(g, f, scopes, depth, class);
    let _ = writeln!(
        g.items,
        "{ALLOW}\nfn {name}(cx: &mut Cx, env: &Env, this: &Value, args: &[Value]) -> R<Value> {{\n{out}Ok(Value::Undefined)\n}}\nstatic {meta}: FnMeta = FnMeta {{ name: {}, kind: FnKind::{:?}, nparams: {}, nslots: {nslots}, strict: {} }};\n",
        lit_str(f.display_name()),
        f.kind,
        f.length(),
        f.strict,
    );
}

/// Per generated file: imports and the macros generated code uses.
pub const HEADER: &str = r#"#[allow(unused_imports)]
use std::rc::Rc;
#[allow(unused_imports)]
use qml::eval::{Ex, R};
#[allow(unused_imports)]
use qml::jsrt::{self, Cx, Env, FnKind, FnMeta};
#[allow(unused_imports)]
use qml::runtime::Sym;
#[allow(unused_imports)]
use qml::value::Value;
#[allow(unused_imports)]
use crate::rt::{self, Cpl};
#[allow(unused_macros)]
macro_rules! tb { ($e:expr, $l:lifetime) => { match $e { Ok(v) => v, Err(e) => break $l Err(e) } } }
#[allow(unused_macros)]
macro_rules! tc { ($e:expr, $l:lifetime) => { match $e { Ok(v) => v, Err(e) => break $l Cpl::T(e) } } }
#[allow(unused_macros)]
macro_rules! js_str { ($i:literal, $s:literal) => { rt::lit(&LIT_CELLS, $i) } }
#[allow(unused_macros)]
macro_rules! sym { ($s:literal) => {{ static S: std::sync::OnceLock<Sym> = std::sync::OnceLock::new(); rt::sym(&S, $s) }} }
"#;

/// The `crate::rt` module of the generated crate: semantics of the interpreter's expression
/// forms that need more than `jsrt`, written against public `qml` API only.
pub const PRELUDE: &str = include_str!("rt.rs.in");
