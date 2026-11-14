//! Lowering of generator function bodies (ECMA-262 §15.5) into resumable state machines.
//!
//! A generator function keeps its parameters and becomes
//!
//! ```text
//! function* g(params) {            function g(params) {  // FnKind::Generator
//!     body                            var <body vars, temps, renamed bindings>;
//! }                                   <body function declarations>
//!                                     return (%ctx) => { switch (%ctx.at()) { case 0: ... } };
//!                                 }
//! ```
//!
//! and calling it wraps the returned step function in a generator object
//! ([`crate::generator`]). Like TypeScript's downlevel generator transform, a statement that
//! contains `yield` is split into cases at its resumption points: each case ends by asking
//! the runtime for the next action through `%ctx` (`yield`, `delegate`, `jump`, `ret`,
//! `endfinally`), try statements register their labels (`%ctx.try`), and an expression that
//! yields evaluates its earlier operands into temporaries first so order is kept. Statements
//! without `yield` stay whole, with their `var`s hoisted, their `return`s and their
//! `break`/`continue`s out to split statements turned into runtime actions.
//!
//! Every binding the cases share lives in the outer function: `let`/`const`/class/function
//! declarations of a split block get fresh names, or, when a closure in the block may capture
//! them inside a split loop, become properties of a scope object made on each entry to the
//! block (and copied per iteration for `for (let ...)`), so every iteration keeps its own.
//! Case numbers are their positions, which try ranges compare; labels are numbered in a
//! first pass and the cases emitted in a second.
use std::collections::HashMap;

use super::{
    AssignOp, Class, Expr, Func, Key, LogicalOp, Pat, PropKind, Stmt, UnOp, bx, lexical_decls,
    lexical_names, pat_ids, var_names,
};
use crate::eval::BinOp;
use crate::jsrt::FnKind;
use crate::runtime::Sym;

pub(super) const CTX: &str = "%ctx";

/// The body of generator function code: what the outer function runs on a call.
/// `direct_eval`: the body calls `eval` by that name.
pub(super) fn lower(body: &[Stmt], strict: bool, direct_eval: bool) -> Vec<Stmt> {
    let first = L::new(None).run(body, strict, direct_eval);
    let known = first.pos.iter().map(|p| p.unwrap_or(0)).collect();
    L::new(Some(known)).run(body, strict, direct_eval).out
}

/// What a binding of a split block is replaced by: the scope object's name, the binding's
/// name, and the number of split `with` statements the block is in.
#[derive(Clone, Debug)]
struct Repl(String, String, usize);

impl Repl {
    fn expr(&self) -> Expr {
        Expr::Member(bx(Expr::Ident(self.0.clone())), Sym::new(&self.1), false)
    }
    fn pat(&self) -> Pat {
        Pat::Member(bx(self.expr()))
    }
    /// `e`, which stands for a use of the binding under `withs` `with` statements: those
    /// in between the use and the block are asked for the name first.
    fn under(&self, withs: usize, e: Expr) -> Expr {
        match withs - self.2 {
            0 => e,
            d => Expr::Dynamic(self.1.clone(), d as u32, bx(e)),
        }
    }
    fn pat_under(&self, withs: usize) -> Pat {
        match withs - self.2 {
            0 => self.pat(),
            d => Pat::Dynamic(self.1.clone(), d as u32, bx(self.pat())),
        }
    }
    /// The binding's initialization (its declaration), which ends its temporal dead zone.
    fn init(&self) -> Pat {
        Pat::Init(bx(self.expr()))
    }
}

/// A break/continue target among the split statements.
struct Target {
    labels: Vec<String>,
    brk: usize,
    cont: Option<usize>,
    /// A loop or switch (the target of an unlabelled `break`).
    breakable: bool,
}

/// A break/continue target inside a statement kept whole.
struct Local {
    labels: Vec<String>,
    is_loop: bool,
    breakable: bool,
}

#[derive(Default)]
struct Ax {
    /// Names bound inside the walked statement, which no replacement reaches.
    shadow: Vec<Vec<String>>,
    local: Vec<Local>,
    /// Nesting depth of functions inside the walked statement.
    fns: usize,
    labels: Vec<String>,
    /// Nesting depth of `with` statements inside the walked statement.
    withs: usize,
}

impl Ax {
    fn shadowed(&self, n: &str) -> bool {
        self.shadow.iter().any(|s| s.iter().any(|x| x == n))
    }
}

struct L {
    known: Option<Vec<u32>>,
    pos: Vec<Option<u32>>,
    cases: Vec<Vec<Stmt>>,
    hoisted: Vec<String>,
    temps: usize,
    scopes: Vec<HashMap<String, Repl>>,
    targets: Vec<Target>,
    loops: usize,
    /// The objects (temporaries) of the enclosing split `with` statements.
    withs: Vec<String>,
    out: Vec<Stmt>,
}

fn ctx(m: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(
        bx(Expr::Member(
            bx(Expr::Ident(CTX.into())),
            Sym::new(m),
            false,
        )),
        args,
        false,
    )
}

fn undefined() -> Expr {
    Expr::Unary(UnOp::Void, bx(Expr::Num(0.0)))
}

fn assign(p: Pat, e: Expr) -> Stmt {
    Stmt::Expr(Expr::Assign(AssignOp::Assign, bx(p), bx(e)))
}

fn not(e: Expr) -> Expr {
    Expr::Unary(UnOp::Not, bx(e))
}

fn is_loop(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::For(..) | Stmt::ForOf(..) | Stmt::ForIn(..) | Stmt::While(..) | Stmt::DoWhile(..)
    )
}

/// Whether `s` contains a `yield` of this function (not of a nested one).
fn stmt_yields(s: &Stmt) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => yields(e),
        Stmt::Var(_, ds) => ds
            .iter()
            .any(|(p, e)| pat_yields(p) || e.as_ref().is_some_and(yields)),
        Stmt::If(c, a, b) => yields(c) || stmt_yields(a) || b.as_deref().is_some_and(stmt_yields),
        Stmt::Block(b) => b.iter().any(stmt_yields),
        Stmt::For(i, t, u, b) => {
            i.as_deref().is_some_and(stmt_yields)
                || t.as_ref().is_some_and(yields)
                || u.as_ref().is_some_and(yields)
                || stmt_yields(b)
        }
        Stmt::ForOf(_, p, e, b) | Stmt::ForIn(_, p, e, b) => {
            pat_yields(p) || yields(e) || stmt_yields(b)
        }
        Stmt::While(e, b) | Stmt::DoWhile(b, e) => yields(e) || stmt_yields(b),
        Stmt::Labeled(_, b) => stmt_yields(b),
        Stmt::With(e, b) => yields(e) || stmt_yields(b),
        Stmt::Switch(d, cs) => {
            yields(d)
                || cs
                    .iter()
                    .any(|(t, b)| t.as_ref().is_some_and(yields) || b.iter().any(stmt_yields))
        }
        Stmt::Try(a, c, f) => {
            a.iter().any(stmt_yields)
                || c.as_ref().is_some_and(|(p, b)| {
                    p.as_ref().is_some_and(pat_yields) || b.iter().any(stmt_yields)
                })
                || f.iter().flatten().any(stmt_yields)
        }
        Stmt::Func(_)
        | Stmt::Class(_)
        | Stmt::Return(None)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Empty
        | Stmt::Unsupported(_) => false,
    }
}

fn yields(e: &Expr) -> bool {
    match e {
        Expr::Yield(..) => true,
        Expr::Template(_, v) | Expr::Array(v) | Expr::Seq(v) => v.iter().any(yields),
        Expr::Member(a, ..)
        | Expr::Chain(a)
        | Expr::Unary(_, a)
        | Expr::Spread(a)
        | Expr::Dynamic(_, _, a)
        | Expr::DirectEval(_, a) => yields(a),
        Expr::Index(a, b, _)
        | Expr::DefineField(a, b, _)
        | Expr::Binary(_, a, b)
        | Expr::Logical(_, a, b) => yields(a) || yields(b),
        Expr::Call(a, v, _) | Expr::New(a, v) => yields(a) || v.iter().any(yields),
        Expr::Cond(a, b, c) => yields(a) || yields(b) || yields(c),
        Expr::Assign(_, p, b) => pat_yields(p) || yields(b),
        Expr::Update(_, _, p) => pat_yields(p),
        Expr::Object(v) => v
            .iter()
            .any(|(k, x, _)| matches!(k, Key::Computed(k) if yields(k)) || yields(x)),
        Expr::Num(_)
        | Expr::Str(_)
        | Expr::Str16(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Ident(_)
        | Expr::Local(..)
        | Expr::This
        | Expr::Regex(..)
        | Expr::Hole
        | Expr::Super
        | Expr::NewTarget
        | Expr::Func(_)
        | Expr::Class(_)
        | Expr::TemplateObject(_)
        | Expr::Unsupported(_) => false,
    }
}

fn pat_yields(p: &Pat) -> bool {
    match p {
        Pat::Id(_) | Pat::Local(..) => false,
        Pat::Object(ps, r) => {
            ps.iter()
                .any(|(k, p)| matches!(k, Key::Computed(k) if yields(k)) || pat_yields(p))
                || r.as_deref().is_some_and(pat_yields)
        }
        Pat::Array(ps, r) => {
            ps.iter().flatten().any(pat_yields) || r.as_deref().is_some_and(pat_yields)
        }
        Pat::Default(p, e) => pat_yields(p) || yields(e),
        Pat::Member(e) | Pat::Init(e) => yields(e),
        Pat::Const(p, _) | Pat::Dynamic(_, _, p) => pat_yields(p),
    }
}

fn ids(p: &Pat) -> Vec<String> {
    let mut v = Vec::new();
    pat_ids(p, &mut v);
    v
}

impl L {
    fn new(known: Option<Vec<u32>>) -> L {
        L {
            known,
            pos: Vec::new(),
            cases: vec![Vec::new()],
            hoisted: Vec::new(),
            temps: 0,
            scopes: Vec::new(),
            targets: Vec::new(),
            loops: 0,
            withs: Vec::new(),
            out: Vec::new(),
        }
    }

    fn run(mut self, body: &[Stmt], strict: bool, direct_eval: bool) -> L {
        let mut names = Vec::new();
        var_names(body, &mut names);
        let funcs: Vec<String> = body
            .iter()
            .filter_map(|s| match s {
                Stmt::Func(f) => f.name.clone(),
                _ => None,
            })
            .collect();
        names.extend(funcs.iter().cloned());
        self.hoisted.extend(names);
        // The body's lexical bindings live in a scope object; its functions are `var`s, made
        // when the body starts (nothing outside the body can reach them before).
        let lexical: Vec<(String, bool)> = lexical_decls(body)
            .into_iter()
            .filter(|(n, _)| !funcs.contains(n))
            .collect();
        self.open(&lexical);
        self.hoist_funcs(body);
        self.block_body(body);
        let mut seen = std::collections::HashSet::new();
        let vars: Vec<(Pat, Option<Expr>)> = std::mem::take(&mut self.hoisted)
            .into_iter()
            .filter(|n| seen.insert(n.clone()))
            .map(|n| (Pat::Id(n), None))
            .collect();
        let cases = std::mem::take(&mut self.cases)
            .into_iter()
            .enumerate()
            .map(|(i, c)| (Some(Expr::Num(i as f64)), c))
            .collect();
        let step = Func {
            name: None,
            inferred: None,
            params: vec![Pat::Id(CTX.into())],
            rest: None,
            body: vec![Stmt::Switch(ctx("at", Vec::new()), cases)],
            kind: FnKind::Arrow,
            coroutine: None,
            strict,
            uses_arguments: false,
            direct_eval,
            uses_super: false,
            var_init: Vec::new(),
            hoisted: Default::default(),
            nslots: 0,
            slot: None,
            this_only: false,
            expression: false,
        };
        if !vars.is_empty() {
            self.out.push(Stmt::Var("var", vars));
        }
        self.out
            .push(Stmt::Return(Some(Expr::Func(std::rc::Rc::new(step)))));
        self
    }

    // ---- cases and labels ----

    fn label(&mut self) -> usize {
        self.pos.push(None);
        self.pos.len() - 1
    }

    fn lab(&self, id: usize) -> Expr {
        Expr::Num(self.known.as_ref().map_or(0.0, |k| k[id] as f64))
    }

    /// Adds `s` to the current case, in the object environments of the split `with`
    /// statements it is in: each case enters them anew.
    fn emit(&mut self, s: Stmt) {
        let s = self
            .withs
            .iter()
            .rev()
            .fold(s, |s, w| Stmt::With(Expr::Ident(w.clone()), bx(s)));
        self.cases.last_mut().unwrap().push(s);
    }

    fn terminated(&self) -> bool {
        let mut last = self.cases.last().unwrap().last();
        while let Some(Stmt::With(_, s)) = last {
            last = Some(s);
        }
        matches!(last, Some(Stmt::Return(_) | Stmt::Throw(_)))
    }

    fn jump_stmt(&self, id: usize) -> Stmt {
        Stmt::Return(Some(ctx("jump", vec![self.lab(id)])))
    }

    fn jump(&mut self, id: usize) {
        if !self.terminated() {
            let s = self.jump_stmt(id);
            self.emit(s);
        }
    }

    /// Starts the case of label `id`; the current one continues there.
    fn mark(&mut self, id: usize) {
        self.jump(id);
        self.cases.push(Vec::new());
        self.pos[id] = Some((self.cases.len() - 1) as u32);
    }

    fn temp(&mut self) -> String {
        let n = format!("%t{}", self.temps);
        self.temps += 1;
        self.hoisted.push(n.clone());
        n
    }

    fn spill(&mut self, e: Expr) -> Expr {
        match e {
            Expr::Num(_) | Expr::Str(_) | Expr::Str16(_) | Expr::Bool(_) | Expr::Null => e,
            Expr::Ident(ref n) if n.starts_with("%t") => e,
            e => {
                let t = self.temp();
                self.emit(assign(Pat::Id(t.clone()), e));
                Expr::Ident(t)
            }
        }
    }

    // ---- scopes ----

    fn repl(&self, n: &str) -> Option<&Repl> {
        self.scopes.iter().rev().find_map(|s| s.get(n))
    }

    /// Opens the scope of a split block declaring `names` (each with whether it is `const`):
    /// a fresh scope object whose bindings start in their temporal dead zone. Returns the
    /// scope object's binding.
    fn open(&mut self, names: &[(String, bool)]) -> Option<String> {
        let mut m = HashMap::new();
        if names.is_empty() {
            self.scopes.push(m);
            return None;
        }
        let s = self.temp();
        self.emit(assign(Pat::Id(s.clone()), ctx("scope", Vec::new())));
        for (n, c) in names {
            let args = vec![
                Expr::Ident(s.clone()),
                Expr::Str(n.as_str().into()),
                Expr::Bool(*c),
            ];
            self.emit(Stmt::Expr(ctx("declare", args)));
            m.insert(n.clone(), Repl(s.clone(), n.clone(), self.withs.len()));
        }
        self.scopes.push(m);
        Some(s)
    }

    fn close(&mut self) {
        self.scopes.pop();
    }

    /// The scope objects of the enclosing split blocks.
    fn scope_objs(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for r in self.scopes.iter().flat_map(|m| m.values()) {
            if !v.contains(&r.0) {
                v.push(r.0.clone());
            }
        }
        v.sort();
        v
    }

    /// A closure made in a split block keeps the scope objects current where it is made:
    /// `((%s) => closure)(%s)`, as each iteration's closures keep that iteration's bindings.
    fn capture(&self, e: Expr) -> Expr {
        let s = self.scope_objs();
        if s.is_empty() {
            return e;
        }
        let f = Func {
            name: None,
            inferred: None,
            params: s.iter().map(|n| Pat::Id(n.clone())).collect(),
            rest: None,
            body: vec![Stmt::Return(Some(e))],
            kind: FnKind::Arrow,
            coroutine: None,
            strict: false,
            uses_arguments: false,
            direct_eval: false,
            uses_super: false,
            var_init: Vec::new(),
            hoisted: Default::default(),
            nslots: 0,
            slot: None,
            this_only: false,
            expression: false,
        };
        Expr::Call(
            bx(Expr::Func(std::rc::Rc::new(f))),
            s.into_iter().map(Expr::Ident).collect(),
            false,
        )
    }

    // ---- statements ----

    fn block(&mut self, b: &[Stmt]) {
        self.open(&lexical_decls(b));
        self.hoist_funcs(b);
        self.block_body(b);
        self.close();
    }

    /// The function declarations of a split block, made on entry to it.
    fn hoist_funcs(&mut self, b: &[Stmt]) {
        for s in b {
            if let Stmt::Func(f) = s
                && let Some(n) = &f.name
            {
                let target = self.init_target(n);
                let f = self.atom_func(f);
                let f = self.capture(Expr::Func(std::rc::Rc::new(f)));
                self.emit(assign(target, f));
            }
        }
    }

    fn init_target(&self, n: &str) -> Pat {
        self.repl(n).map_or_else(|| Pat::Id(n.into()), Repl::init)
    }

    fn target_pat(&self, n: &str) -> Pat {
        self.repl(n).map_or_else(|| Pat::Id(n.into()), Repl::pat)
    }

    fn block_body(&mut self, b: &[Stmt]) {
        for s in b {
            match s {
                Stmt::Var(k, ds) if *k != "var" => self.decl(k, ds),
                Stmt::Class(c) => {
                    if let Some(n) = &c.name {
                        let target = self.init_target(n);
                        let c = self.atom_class(c);
                        let c = self.capture(Expr::Class(bx(c)));
                        self.emit(assign(target, c));
                    }
                }
                Stmt::Func(_) => {}
                s => self.stmt(s, Vec::new()),
            }
        }
    }

    fn decl(&mut self, k: &str, ds: &[(Pat, Option<Expr>)]) {
        for (p, init) in ds {
            if k == "var" {
                self.hoisted.extend(ids(p));
            }
            let v = match init {
                Some(e) => self.expr(e),
                None if k == "var" => continue,
                None => undefined(),
            };
            self.bind(p, v, k != "var");
        }
    }

    /// Binds pattern `p` to `v`: an initialization of split-block bindings (`init`) or an
    /// assignment.
    fn bind(&mut self, p: &Pat, v: Expr, init: bool) {
        if pat_yields(p) {
            return self.destructure(p, v, init);
        }
        let p = if init { self.init_pat(p) } else { self.dpat(p) };
        self.emit(assign(p, v));
    }

    /// A declaration pattern: its names replaced by initializations of their bindings.
    fn init_pat(&mut self, p: &Pat) -> Pat {
        let ax = &mut Ax::default();
        match p {
            Pat::Id(n) => self.init_target(n),
            Pat::Object(ps, r) => Pat::Object(
                ps.iter()
                    .map(|(k, p)| (self.atom_key(k, ax), self.init_pat(p)))
                    .collect(),
                r.as_ref().map(|r| bx(self.init_pat(r))),
            ),
            Pat::Array(ps, r) => Pat::Array(
                ps.iter()
                    .map(|p| p.as_ref().map(|p| self.init_pat(p)))
                    .collect(),
                r.as_ref().map(|r| bx(self.init_pat(r))),
            ),
            Pat::Default(p, e) => {
                let p = self.init_pat(p);
                Pat::Default(bx(p), bx(self.atom_expr(e, ax)))
            }
            p => self.atom_pat(p, ax),
        }
    }

    /// Destructuring whose defaults or computed keys yield, step by step (ECMA-262 §8.6.2
    /// BindingInitialization / §13.15.5 DestructuringAssignmentEvaluation): an array pattern
    /// steps the iterator and closes it, also when a step throws.
    fn destructure(&mut self, p: &Pat, v: Expr, init: bool) {
        match p {
            Pat::Default(q, d) => {
                // A member target's reference comes before the value and the default.
                let m = match &**q {
                    Pat::Member(m) => Some(self.member_target(m)),
                    _ => None,
                };
                let t = self.temp();
                self.emit(assign(Pat::Id(t.clone()), v));
                let end = self.label();
                let j = self.jump_stmt(end);
                let set =
                    Expr::Binary(BinOp::StrictNe, bx(Expr::Ident(t.clone())), bx(undefined()));
                self.emit(Stmt::If(set, bx(j), None));
                let dv = self.expr(d);
                self.emit(assign(Pat::Id(t.clone()), dv));
                self.mark(end);
                match m {
                    Some(m) => self.emit(assign(Pat::Member(bx(m)), Expr::Ident(t))),
                    None => self.destructure(q, Expr::Ident(t), init),
                }
            }
            Pat::Array(items, rest) => {
                let it = self.temp();
                self.emit(assign(Pat::Id(it.clone()), ctx("iter", vec![v])));
                let (start, catch, end) = (self.label(), self.label(), self.label());
                self.mark(start);
                let args = vec![
                    self.lab(start),
                    self.lab(catch),
                    Expr::Num(-1.0),
                    self.lab(end),
                ];
                self.emit(Stmt::Expr(ctx("try", args)));
                let itv = Expr::Ident(it);
                for item in items {
                    let x = Expr::Cond(
                        bx(ctx("step", vec![itv.clone()])),
                        bx(ctx("value", vec![itv.clone()])),
                        bx(undefined()),
                    );
                    match item {
                        Some(q) => self.destructure(q, x, init),
                        None => self.emit(Stmt::Expr(x)),
                    }
                }
                if let Some(r) = rest {
                    self.destructure(r, ctx("rest", vec![itv.clone()]), init);
                }
                self.emit(Stmt::Expr(ctx("close", vec![itv.clone()])));
                self.jump(end);
                self.mark(catch);
                self.emit(Stmt::Expr(ctx("closeQuiet", vec![itv])));
                self.emit(Stmt::Throw(ctx("sent", Vec::new())));
                self.mark(end);
            }
            Pat::Object(props, rest) => {
                let o = self.temp();
                self.emit(assign(Pat::Id(o.clone()), v));
                self.emit(Stmt::Expr(ctx("coercible", vec![Expr::Ident(o.clone())])));
                let mut keys = Vec::new();
                for (k, q) in props {
                    let (x, key) = match k {
                        Key::Name(n) => (
                            Expr::Member(bx(Expr::Ident(o.clone())), Sym::new(n), false),
                            Expr::Str(n.clone()),
                        ),
                        Key::Computed(e) => {
                            let kv = self.expr(e);
                            let kt = self.temp();
                            self.emit(assign(Pat::Id(kt.clone()), ctx("propKey", vec![kv])));
                            let x = Expr::Index(
                                bx(Expr::Ident(o.clone())),
                                bx(Expr::Ident(kt.clone())),
                                false,
                            );
                            (x, Expr::Ident(kt))
                        }
                    };
                    keys.push(key);
                    self.destructure(q, x, init);
                }
                if let Some(r) = rest {
                    let x = ctx("restObj", vec![Expr::Ident(o), Expr::Array(keys)]);
                    self.destructure(r, x, init);
                }
            }
            Pat::Member(m) => {
                let m = self.member_target(m);
                self.emit(assign(Pat::Member(bx(m)), v));
            }
            leaf => {
                let p = if init {
                    self.init_pat(leaf)
                } else {
                    self.dpat(leaf)
                };
                self.emit(assign(p, v));
            }
        }
    }

    /// A pattern of a split statement, with its bindings replaced.
    fn dpat(&mut self, p: &Pat) -> Pat {
        self.atom_pat(p, &mut Ax::default())
    }

    fn atomic(&mut self, s: &Stmt) {
        let s = self.atom_stmt(s, &mut Ax::default());
        self.emit(s);
    }

    fn stmt(&mut self, s: &Stmt, labels: Vec<String>) {
        if !stmt_yields(s) {
            return self.atomic(s);
        }
        match s {
            Stmt::Expr(Expr::Yield(a, d)) => {
                self.yield_op(a.as_deref(), *d);
                self.emit(Stmt::Expr(ctx("sent", Vec::new())));
            }
            Stmt::Expr(e) => {
                let e = self.expr(e);
                self.emit(Stmt::Expr(e));
            }
            Stmt::Var(k, ds) => self.decl(k, ds),
            Stmt::Return(e) => {
                let v = e.as_ref().map(|e| self.expr(e));
                self.emit(Stmt::Return(Some(ctx("ret", v.into_iter().collect()))));
            }
            Stmt::Throw(e) => {
                let e = self.expr(e);
                self.emit(Stmt::Throw(e));
            }
            Stmt::If(c, a, b) => {
                let c = self.expr(c);
                if !stmt_yields(a) && !b.as_deref().is_some_and(stmt_yields) {
                    let a = self.atom_stmt(a, &mut Ax::default());
                    let b = b
                        .as_ref()
                        .map(|b| bx(self.atom_stmt(b, &mut Ax::default())));
                    return self.emit(Stmt::If(c, bx(a), b));
                }
                let (els, end) = (self.label(), self.label());
                let to = if b.is_some() { els } else { end };
                let j = self.jump_stmt(to);
                self.emit(Stmt::If(not(c), bx(j), None));
                self.stmt(a, Vec::new());
                if let Some(b) = b {
                    self.jump(end);
                    self.mark(els);
                    self.stmt(b, Vec::new());
                }
                self.mark(end);
            }
            Stmt::Block(b) => self.block(b),
            Stmt::With(o, b) => {
                let o = self.expr(o);
                let t = self.temp();
                self.emit(assign(Pat::Id(t.clone()), ctx("object", vec![o])));
                self.withs.push(t);
                self.stmt(b, Vec::new());
                self.withs.pop();
            }
            Stmt::Labeled(l, b) => {
                let mut labels = labels;
                labels.push(l.clone());
                if is_loop(b) || matches!(**b, Stmt::Labeled(..)) {
                    return self.stmt(b, labels);
                }
                let end = self.label();
                self.targets.push(Target {
                    labels,
                    brk: end,
                    cont: None,
                    breakable: false,
                });
                self.stmt(b, Vec::new());
                self.targets.pop();
                self.mark(end);
            }
            Stmt::While(c, b) => {
                let (head, end) = (self.label(), self.label());
                self.mark(head);
                let c = self.expr(c);
                let j = self.jump_stmt(end);
                self.emit(Stmt::If(not(c), bx(j), None));
                self.loop_body(b, labels, end, head);
                self.jump(head);
                self.mark(end);
            }
            Stmt::DoWhile(b, c) => {
                let (top, test, end) = (self.label(), self.label(), self.label());
                self.mark(top);
                self.loop_body(b, labels, end, test);
                self.mark(test);
                let c = self.expr(c);
                let j = self.jump_stmt(top);
                self.emit(Stmt::If(c, bx(j), None));
                self.mark(end);
            }
            Stmt::For(init, test, update, b) => {
                let names: Vec<(String, bool)> = match init.as_deref() {
                    Some(Stmt::Var(k, ds)) if *k != "var" => ds
                        .iter()
                        .flat_map(|(p, _)| ids(p))
                        .map(|n| (n, *k == "const"))
                        .collect(),
                    _ => Vec::new(),
                };
                let scope = self.open(&names);
                match init.as_deref() {
                    Some(Stmt::Var(k, ds)) => self.decl(k, ds),
                    Some(i) => self.stmt(i, Vec::new()),
                    None => {}
                }
                let (head, next, end) = (self.label(), self.label(), self.label());
                self.mark(head);
                if let Some(t) = test {
                    let c = self.expr(t);
                    let j = self.jump_stmt(end);
                    self.emit(Stmt::If(not(c), bx(j), None));
                }
                self.loop_body(b, labels, end, next);
                self.mark(next);
                if let Some(s) = &scope {
                    let copy = ctx("scope", vec![Expr::Ident(s.clone())]);
                    self.emit(assign(Pat::Id(s.clone()), copy));
                }
                if let Some(u) = update {
                    let u = self.expr(u);
                    self.emit(Stmt::Expr(u));
                }
                self.jump(head);
                self.mark(end);
                self.close();
            }
            Stmt::ForOf(k, p, e, b) | Stmt::ForIn(k, p, e, b) => {
                let v = self.expr(e);
                let start = match s {
                    Stmt::ForIn(..) => "forIn",
                    _ => "iter",
                };
                let it = self.temp();
                self.emit(assign(Pat::Id(it.clone()), ctx(start, vec![v])));
                let value = ctx("value", vec![Expr::Ident(it.clone())]);
                let bind = match k {
                    Some(k) => Stmt::Var(k, vec![(p.clone(), Some(value))]),
                    None => Stmt::Expr(Expr::Assign(AssignOp::Assign, bx(p.clone()), bx(value))),
                };
                let mut lp = Stmt::While(
                    ctx("step", vec![Expr::Ident(it.clone())]),
                    bx(Stmt::Block(vec![bind, (**b).clone()])),
                );
                for l in labels.into_iter().rev() {
                    lp = Stmt::Labeled(l, bx(lp));
                }
                let e = format!("%e{}", self.temps);
                self.temps += 1;
                let it = Expr::Ident(it);
                let close = Stmt::Try(
                    vec![lp],
                    Some((
                        Some(Pat::Id(e.clone())),
                        vec![
                            Stmt::Expr(ctx("closeQuiet", vec![it.clone()])),
                            Stmt::Throw(Expr::Ident(e)),
                        ],
                    )),
                    Some(vec![Stmt::Expr(ctx("close", vec![it]))]),
                );
                self.stmt(&close, Vec::new());
            }
            Stmt::Switch(d, cs) => {
                let d = self.expr(d);
                let t = self.temp();
                self.emit(assign(Pat::Id(t.clone()), d));
                let names: Vec<(String, bool)> =
                    cs.iter().flat_map(|(_, b)| lexical_decls(b)).collect();
                self.open(&names);
                cs.iter().for_each(|(_, b)| self.hoist_funcs(b));
                let labels_c: Vec<usize> = cs.iter().map(|_| self.label()).collect();
                let end = self.label();
                let mut default = None;
                for (i, (test, _)) in cs.iter().enumerate() {
                    match test {
                        Some(x) => {
                            let x = self.expr(x);
                            let eq =
                                Expr::Binary(BinOp::StrictEq, bx(Expr::Ident(t.clone())), bx(x));
                            let j = self.jump_stmt(labels_c[i]);
                            self.emit(Stmt::If(eq, bx(j), None));
                        }
                        None => default = Some(labels_c[i]),
                    }
                }
                self.jump(default.unwrap_or(end));
                self.targets.push(Target {
                    labels,
                    brk: end,
                    cont: None,
                    breakable: true,
                });
                for (i, (_, b)) in cs.iter().enumerate() {
                    self.mark(labels_c[i]);
                    self.block_body(b);
                }
                self.targets.pop();
                self.mark(end);
                self.close();
            }
            Stmt::Try(a, c, f) => {
                let start = self.label();
                let catch = c.as_ref().map(|_| self.label());
                let fin = f.as_ref().map(|_| self.label());
                let end = self.label();
                self.mark(start);
                let opt = |l: &L, x: Option<usize>| x.map_or(Expr::Num(-1.0), |x| l.lab(x));
                let args = vec![
                    self.lab(start),
                    opt(self, catch),
                    opt(self, fin),
                    self.lab(end),
                ];
                self.emit(Stmt::Expr(ctx("try", args)));
                self.block(a);
                self.jump(end);
                if let (Some((p, cb)), Some(cl)) = (c, catch) {
                    self.mark(cl);
                    let names: Vec<(String, bool)> = p
                        .as_ref()
                        .map(ids)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|n| (n, false))
                        .collect();
                    self.open(&names);
                    if let Some(p) = p {
                        self.bind(p, ctx("sent", Vec::new()), true);
                    }
                    self.block(cb);
                    self.close();
                    self.jump(end);
                }
                if let (Some(fb), Some(fl)) = (f, fin) {
                    self.mark(fl);
                    self.block(fb);
                    if !self.terminated() {
                        self.emit(Stmt::Return(Some(ctx("endfinally", Vec::new()))));
                    }
                }
                self.mark(end);
            }
            _ => self.atomic(s),
        }
    }

    fn loop_body(&mut self, b: &Stmt, labels: Vec<String>, brk: usize, cont: usize) {
        self.targets.push(Target {
            labels,
            brk,
            cont: Some(cont),
            breakable: true,
        });
        self.loops += 1;
        self.stmt(b, Vec::new());
        self.loops -= 1;
        self.targets.pop();
    }

    // ---- expressions ----

    fn yield_op(&mut self, a: Option<&Expr>, delegate: bool) {
        let v = a.map_or_else(undefined, |a| self.expr(a));
        let r = self.label();
        let op = ctx(
            if delegate { "delegate" } else { "yield" },
            vec![self.lab(r), v],
        );
        self.emit(Stmt::Return(Some(op)));
        self.mark(r);
    }

    fn plain(&mut self, e: &Expr) -> Expr {
        self.atom_expr(e, &mut Ax::default())
    }

    /// Operands evaluated left to right: those before the last one that yields are kept in
    /// temporaries, so what the yield's resumption does cannot change them.
    fn operands(&mut self, es: &[&Expr]) -> Vec<Expr> {
        let last = es.iter().rposition(|e| yields(e));
        es.iter()
            .enumerate()
            .map(|(i, e)| match (e, last) {
                (Expr::Spread(x), Some(k)) if i < k => {
                    let x = self.expr(x);
                    Expr::Spread(bx(self.spill(x)))
                }
                (Expr::Hole, _) => Expr::Hole,
                (_, Some(k)) if i < k => {
                    let x = self.expr(e);
                    self.spill(x)
                }
                (_, Some(k)) if i == k => self.expr(e),
                _ => self.plain(e),
            })
            .collect()
    }

    fn expr(&mut self, e: &Expr) -> Expr {
        if !yields(e) {
            return self.plain(e);
        }
        match e {
            Expr::Yield(a, d) => {
                self.yield_op(a.as_deref(), *d);
                let t = self.temp();
                self.emit(assign(Pat::Id(t.clone()), ctx("sent", Vec::new())));
                Expr::Ident(t)
            }
            Expr::Binary(op, a, b) => {
                let v = self.operands(&[a, b]);
                let [a, b] = <[Expr; 2]>::try_from(v).unwrap();
                Expr::Binary(*op, bx(a), bx(b))
            }
            Expr::Logical(op, a, b) if yields(b) => {
                let t = self.temp();
                let a = self.expr(a);
                self.emit(assign(Pat::Id(t.clone()), a));
                let end = self.label();
                let tv = Expr::Ident(t.clone());
                let skip = match op {
                    LogicalOp::And => not(tv),
                    LogicalOp::Or => tv,
                    LogicalOp::Nullish => Expr::Binary(BinOp::Ne, bx(tv), bx(Expr::Null)),
                };
                let j = self.jump_stmt(end);
                self.emit(Stmt::If(skip, bx(j), None));
                let b = self.expr(b);
                self.emit(assign(Pat::Id(t.clone()), b));
                self.mark(end);
                Expr::Ident(t)
            }
            Expr::Logical(op, a, b) => {
                let a = self.expr(a);
                Expr::Logical(*op, bx(a), bx(self.plain(b)))
            }
            Expr::Cond(c, a, b) if yields(a) || yields(b) => {
                let t = self.temp();
                let c = self.expr(c);
                let (els, end) = (self.label(), self.label());
                let j = self.jump_stmt(els);
                self.emit(Stmt::If(not(c), bx(j), None));
                let a = self.expr(a);
                self.emit(assign(Pat::Id(t.clone()), a));
                self.jump(end);
                self.mark(els);
                let b = self.expr(b);
                self.emit(assign(Pat::Id(t.clone()), b));
                self.mark(end);
                Expr::Ident(t)
            }
            Expr::Cond(c, a, b) => {
                let c = self.expr(c);
                Expr::Cond(bx(c), bx(self.plain(a)), bx(self.plain(b)))
            }
            Expr::Unary(op, a) => Expr::Unary(*op, bx(self.expr(a))),
            Expr::Member(o, n, opt) => Expr::Member(bx(self.expr(o)), *n, *opt),
            Expr::Index(o, k, opt) => {
                let v = self.operands(&[o, k]);
                let [o, k] = <[Expr; 2]>::try_from(v).unwrap();
                Expr::Index(bx(o), bx(k), *opt)
            }
            Expr::Chain(inner) => {
                let t = self.temp();
                let end = self.label();
                let v = self.chain_part(inner, &t, end);
                self.emit(assign(Pat::Id(t.clone()), v));
                self.mark(end);
                Expr::Ident(t)
            }
            // An optional call is always the inside of a chain.
            Expr::Call(c, a, true) => {
                self.expr(&Expr::Chain(bx(Expr::Call(c.clone(), a.clone(), true))))
            }
            Expr::Call(callee, args, false) => self.call(callee, args, false),
            Expr::New(callee, args) => {
                let mut es: Vec<&Expr> = vec![callee];
                es.extend(args.iter());
                let mut v = self.operands(&es);
                let c = v.remove(0);
                Expr::New(bx(c), v)
            }
            Expr::Template(q, es) => {
                let es: Vec<&Expr> = es.iter().collect();
                Expr::Template(q.clone(), self.operands(&es))
            }
            Expr::Array(items) => {
                let es: Vec<&Expr> = items.iter().collect();
                Expr::Array(self.operands(&es))
            }
            Expr::Object(props) => {
                let mut es: Vec<&Expr> = Vec::new();
                for (k, v, _) in props {
                    if let Key::Computed(k) = k {
                        es.push(k);
                    }
                    es.push(v);
                }
                let mut v = self.operands(&es).into_iter();
                let props = props
                    .iter()
                    .map(|(k, _, kind)| {
                        let k = match k {
                            Key::Computed(_) => Key::Computed(v.next().unwrap()),
                            k => k.clone(),
                        };
                        (k, v.next().unwrap(), *kind)
                    })
                    .collect::<Vec<(Key, Expr, PropKind)>>();
                Expr::Object(props)
            }
            Expr::Seq(v) => {
                let (last, init) = v.split_last().unwrap();
                for x in init {
                    let x = self.expr(x);
                    self.emit(Stmt::Expr(x));
                }
                self.expr(last)
            }
            Expr::Spread(x) => Expr::Spread(bx(self.expr(x))),
            Expr::Assign(op, p, rhs) => self.assign_expr(*op, p, rhs),
            Expr::Update(op, prefix, p) => match &**p {
                Pat::Member(m) => {
                    let m = self.member_target(m);
                    Expr::Update(*op, *prefix, bx(Pat::Member(bx(m))))
                }
                p => Expr::Update(*op, *prefix, bx(self.dpat(p))),
            },
            _ => self.plain(e),
        }
    }

    /// Where an optional chain short-circuits: when `o` is nullish the chain's value `t` is
    /// `undefined` and evaluation continues after it (ECMA-262 §13.3.9.1).
    fn short(&mut self, o: Expr, t: &str, end: usize) -> Expr {
        let o = self.spill(o);
        let j = self.jump_stmt(end);
        let skip = Stmt::Block(vec![assign(Pat::Id(t.into()), undefined()), j]);
        let nullish = Expr::Binary(BinOp::Eq, bx(o.clone()), bx(Expr::Null));
        self.emit(Stmt::If(nullish, bx(skip), None));
        o
    }

    /// The links of an optional chain that yields, its `?.` points made explicit.
    fn chain_part(&mut self, e: &Expr, t: &str, end: usize) -> Expr {
        match e {
            Expr::Member(o, n, opt) if !matches!(**o, Expr::Super) => {
                let o = self.chain_part(o, t, end);
                let o = if *opt { self.short(o, t, end) } else { o };
                Expr::Member(bx(o), *n, false)
            }
            Expr::Index(o, k, opt) => {
                let o = self.chain_part(o, t, end);
                let o = if *opt {
                    self.short(o, t, end)
                } else if yields(k) {
                    self.spill(o)
                } else {
                    o
                };
                let k = self.expr(k);
                Expr::Index(bx(o), bx(k), false)
            }
            Expr::Call(c, args, opt) => {
                let (f, this) = match &**c {
                    Expr::Member(o, n, mopt) if !matches!(**o, Expr::Super) => {
                        let o = self.chain_part(o, t, end);
                        let o = self.spill(o);
                        let o = if *mopt { self.short(o, t, end) } else { o };
                        (Expr::Member(bx(o.clone()), *n, false), Some(o))
                    }
                    Expr::Index(o, k, iopt) => {
                        let o = self.chain_part(o, t, end);
                        let o = self.spill(o);
                        let o = if *iopt { self.short(o, t, end) } else { o };
                        let k = self.expr(k);
                        let k = self.spill(k);
                        (Expr::Index(bx(o.clone()), bx(k), false), Some(o))
                    }
                    c => (self.chain_part(c, t, end), None),
                };
                let es: Vec<&Expr> = args.iter().collect();
                if !*opt && !es.iter().any(|e| yields(e)) {
                    let args = self.operands(&es);
                    return Expr::Call(bx(f), args, false);
                }
                // The callee is read before the arguments run: call it with its receiver.
                let f = self.spill(f);
                let f = if *opt { self.short(f, t, end) } else { f };
                let args = self.operands(&es);
                match this {
                    Some(o) => ctx("call", [f, o].into_iter().chain(args).collect()),
                    None => Expr::Call(bx(f), args, false),
                }
            }
            e => self.expr(e),
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], opt: bool) -> Expr {
        let plain_callee = matches!(callee, Expr::Ident(_) | Expr::Super)
            || matches!(callee, Expr::Member(o, ..) if matches!(**o, Expr::Super));
        match callee {
            Expr::Member(o, n, mopt) if !plain_callee => {
                let mut es: Vec<&Expr> = vec![o];
                es.extend(args.iter());
                let mut v = self.operands(&es);
                let o = v.remove(0);
                Expr::Call(bx(Expr::Member(bx(o), *n, *mopt)), v, opt)
            }
            Expr::Index(o, k, iopt) => {
                let mut es: Vec<&Expr> = vec![o, k];
                es.extend(args.iter());
                let mut v = self.operands(&es);
                let o = v.remove(0);
                let k = v.remove(0);
                Expr::Call(bx(Expr::Index(bx(o), bx(k), *iopt)), v, opt)
            }
            _ if plain_callee => {
                let c = self.plain(callee);
                let es: Vec<&Expr> = args.iter().collect();
                Expr::Call(bx(c), self.operands(&es), opt)
            }
            _ => {
                let mut es: Vec<&Expr> = vec![callee];
                es.extend(args.iter());
                let mut v = self.operands(&es);
                let c = v.remove(0);
                Expr::Call(bx(c), v, opt)
            }
        }
    }

    /// The target of an assignment to a member, its object and key evaluated first.
    fn member_target(&mut self, m: &Expr) -> Expr {
        match m {
            Expr::Member(o, n, opt) if !matches!(**o, Expr::Super) => {
                let o = self.expr(o);
                Expr::Member(bx(self.spill(o)), *n, *opt)
            }
            Expr::Index(o, k, opt) if !matches!(**o, Expr::Super) => {
                let o = self.expr(o);
                let o = self.spill(o);
                let k = self.expr(k);
                Expr::Index(bx(o), bx(self.spill(k)), *opt)
            }
            m => self.expr(m),
        }
    }

    fn assign_expr(&mut self, op: AssignOp, p: &Pat, rhs: &Expr) -> Expr {
        let (target, read) = match p {
            Pat::Id(n) => (self.target_pat(n), self.plain(&Expr::Ident(n.clone()))),
            Pat::Member(m) => {
                let m = self.member_target(m);
                (Pat::Member(bx(m.clone())), m)
            }
            p if pat_yields(p) => {
                let v = self.expr(rhs);
                let t = self.temp();
                self.emit(assign(Pat::Id(t.clone()), v));
                self.destructure(p, Expr::Ident(t.clone()), false);
                return Expr::Ident(t);
            }
            p => {
                let p = self.dpat(p);
                let v = self.expr(rhs);
                return Expr::Assign(op, bx(p), bx(v));
            }
        };
        match op {
            AssignOp::Assign => Expr::Assign(op, bx(target), bx(self.expr(rhs))),
            AssignOp::Bin(b) => {
                let cur = self.temp();
                self.emit(assign(Pat::Id(cur.clone()), read));
                let v = self.expr(rhs);
                Expr::Assign(
                    AssignOp::Assign,
                    bx(target),
                    bx(Expr::Binary(b, bx(Expr::Ident(cur)), bx(v))),
                )
            }
            AssignOp::Logical(l) => {
                let t = self.temp();
                self.emit(assign(Pat::Id(t.clone()), read));
                let end = self.label();
                let tv = Expr::Ident(t.clone());
                let skip = match l {
                    LogicalOp::And => not(tv),
                    LogicalOp::Or => tv,
                    LogicalOp::Nullish => Expr::Binary(BinOp::Ne, bx(tv), bx(Expr::Null)),
                };
                let j = self.jump_stmt(end);
                self.emit(Stmt::If(skip, bx(j), None));
                let v = self.expr(rhs);
                let set = Expr::Assign(AssignOp::Assign, bx(target), bx(v));
                self.emit(assign(Pat::Id(t.clone()), set));
                self.mark(end);
                Expr::Ident(t)
            }
        }
    }

    // ---- statements kept whole ----

    fn rename(&self, n: &str, ax: &Ax) -> Option<&Repl> {
        if ax.shadowed(n) {
            return None;
        }
        self.repl(n)
    }

    fn atom_expr(&mut self, e: &Expr, ax: &mut Ax) -> Expr {
        let b = |l: &mut L, x: &Expr, ax: &mut Ax| bx(l.atom_expr(x, ax));
        match e {
            Expr::Ident(n) => {
                let withs = self.withs.len() + ax.withs;
                self.rename(n, ax)
                    .map_or_else(|| e.clone(), |r| r.under(withs, r.expr()))
            }
            Expr::Dynamic(n, w, x) => Expr::Dynamic(n.clone(), *w, b(self, x, ax)),
            Expr::DirectEval(s, x) => Expr::DirectEval(s.clone(), b(self, x, ax)),
            Expr::Template(q, v) => {
                Expr::Template(q.clone(), v.iter().map(|x| self.atom_expr(x, ax)).collect())
            }
            Expr::Array(v) => Expr::Array(v.iter().map(|x| self.atom_expr(x, ax)).collect()),
            Expr::Seq(v) => Expr::Seq(v.iter().map(|x| self.atom_expr(x, ax)).collect()),
            Expr::Yield(a, d) => Expr::Yield(a.as_ref().map(|a| b(self, a, ax)), *d),
            Expr::Member(o, n, opt) => Expr::Member(b(self, o, ax), *n, *opt),
            Expr::Chain(a) => Expr::Chain(b(self, a, ax)),
            // A binding cannot be deleted (ECMA-262 §13.5.1.2): `delete x` is false.
            Expr::Unary(UnOp::Delete, a)
                if self.withs.len() + ax.withs == 0
                    && matches!(&**a, Expr::Ident(n) if self.rename(n, ax).is_some()) =>
            {
                Expr::Bool(false)
            }
            Expr::Unary(op, a) => Expr::Unary(*op, b(self, a, ax)),
            Expr::Spread(a) => Expr::Spread(b(self, a, ax)),
            Expr::Index(o, k, opt) => Expr::Index(b(self, o, ax), b(self, k, ax), *opt),
            Expr::Binary(op, x, y) => Expr::Binary(*op, b(self, x, ax), b(self, y, ax)),
            Expr::DefineField(x, y, n) => Expr::DefineField(b(self, x, ax), b(self, y, ax), *n),
            Expr::Logical(op, x, y) => Expr::Logical(*op, b(self, x, ax), b(self, y, ax)),
            // A direct eval sees the block bindings that became scope-object properties.
            Expr::Call(c, args, false)
                if matches!(&**c, Expr::Ident(n) if n == "eval" && self.rename(n, ax).is_none())
                    && !self.scopes.iter().all(HashMap::is_empty) =>
            {
                let withs = self.withs.len() + ax.withs;
                let mut renamed: Vec<(String, u32, Expr)> = Vec::new();
                for (n, r) in self.scopes.iter().flatten() {
                    if !ax.shadowed(n) {
                        renamed.retain(|x| x.0 != *n);
                        renamed.push((n.clone(), (withs - r.2) as u32, r.expr()));
                    }
                }
                let args = args.iter().map(|x| self.atom_expr(x, ax)).collect();
                let scope = super::EvalScope {
                    scopes: Vec::new(),
                    strict: false,
                    renamed: std::rc::Rc::new(renamed),
                    renamed_at: 0,
                };
                Expr::DirectEval(
                    std::rc::Rc::new(scope),
                    bx(Expr::Call(c.clone(), args, false)),
                )
            }
            Expr::Call(c, args, opt) => {
                // A call of a binding that became a scope-object property keeps `this`
                // undefined: `(0, scope.f)()`.
                let callee = match &**c {
                    Expr::Ident(n) => {
                        let withs = self.withs.len() + ax.withs;
                        match self.rename(n, ax) {
                            Some(r) => r.under(withs, Expr::Seq(vec![Expr::Num(0.0), r.expr()])),
                            None => self.atom_expr(c, ax),
                        }
                    }
                    c => self.atom_expr(c, ax),
                };
                let args = args.iter().map(|x| self.atom_expr(x, ax)).collect();
                Expr::Call(bx(callee), args, *opt)
            }
            Expr::New(c, args) => {
                let c = b(self, c, ax);
                Expr::New(c, args.iter().map(|x| self.atom_expr(x, ax)).collect())
            }
            Expr::Cond(x, y, z) => Expr::Cond(b(self, x, ax), b(self, y, ax), b(self, z, ax)),
            Expr::Assign(op, p, v) => {
                let p = self.atom_pat(p, ax);
                Expr::Assign(*op, bx(p), b(self, v, ax))
            }
            Expr::Update(op, prefix, p) => Expr::Update(*op, *prefix, bx(self.atom_pat(p, ax))),
            Expr::Object(v) => Expr::Object(
                v.iter()
                    .map(|(k, x, kind)| (self.atom_key(k, ax), self.atom_expr(x, ax), *kind))
                    .collect(),
            ),
            Expr::Func(f) => {
                let f = Expr::Func(std::rc::Rc::new(self.atom_func_in(f, ax)));
                if ax.fns == 0 { self.capture(f) } else { f }
            }
            Expr::Class(c) => {
                let c = Expr::Class(bx(self.atom_class_in(c, ax)));
                if ax.fns == 0 { self.capture(c) } else { c }
            }
            Expr::Num(_)
            | Expr::Str(_)
            | Expr::Str16(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Local(..)
            | Expr::This
            | Expr::Regex(..)
            | Expr::Hole
            | Expr::Super
            | Expr::NewTarget
            | Expr::TemplateObject(_)
            | Expr::Unsupported(_) => e.clone(),
        }
    }

    fn atom_key(&mut self, k: &Key, ax: &mut Ax) -> Key {
        match k {
            Key::Computed(e) => Key::Computed(self.atom_expr(e, ax)),
            k => k.clone(),
        }
    }

    fn atom_pat(&mut self, p: &Pat, ax: &mut Ax) -> Pat {
        match p {
            Pat::Id(n) => {
                let withs = self.withs.len() + ax.withs;
                self.rename(n, ax)
                    .map_or_else(|| p.clone(), |r| r.pat_under(withs))
            }
            Pat::Dynamic(n, w, q) => Pat::Dynamic(n.clone(), *w, bx(self.atom_pat(q, ax))),
            Pat::Local(..) => p.clone(),
            Pat::Object(ps, r) => Pat::Object(
                ps.iter()
                    .map(|(k, p)| (self.atom_key(k, ax), self.atom_pat(p, ax)))
                    .collect(),
                r.as_ref().map(|r| bx(self.atom_pat(r, ax))),
            ),
            Pat::Array(ps, r) => Pat::Array(
                ps.iter()
                    .map(|p| p.as_ref().map(|p| self.atom_pat(p, ax)))
                    .collect(),
                r.as_ref().map(|r| bx(self.atom_pat(r, ax))),
            ),
            Pat::Default(p, e) => {
                let p = self.atom_pat(p, ax);
                Pat::Default(bx(p), bx(self.atom_expr(e, ax)))
            }
            Pat::Member(e) => Pat::Member(bx(self.atom_expr(e, ax))),
            Pat::Init(e) => Pat::Init(bx(self.atom_expr(e, ax))),
            Pat::Const(p, t) => Pat::Const(bx(self.atom_pat(p, ax)), *t),
        }
    }

    fn atom_func(&mut self, f: &Func) -> Func {
        self.atom_func_in(f, &mut Ax::default())
    }

    /// A nested function: only its references to replaced bindings change.
    fn atom_func_in(&mut self, f: &Func, ax: &mut Ax) -> Func {
        let mut names = Vec::new();
        f.params
            .iter()
            .chain(&f.rest)
            .for_each(|p| pat_ids(p, &mut names));
        var_names(&f.body, &mut names);
        lexical_names(&f.body, &mut names);
        if !f.kind.is_arrow() {
            names.push("arguments".into());
        }
        ax.shadow.push(names);
        ax.fns += 1;
        let saved = std::mem::take(&mut ax.local);
        let params = f.params.iter().map(|p| self.atom_pat(p, ax)).collect();
        let rest = f.rest.as_ref().map(|p| self.atom_pat(p, ax));
        let body = f.body.iter().map(|s| self.atom_stmt(s, ax)).collect();
        ax.local = saved;
        ax.fns -= 1;
        ax.shadow.pop();
        Func {
            params,
            rest,
            body,
            hoisted: Default::default(),
            ..f.clone()
        }
    }

    fn atom_class(&mut self, c: &Class) -> Class {
        self.atom_class_in(c, &mut Ax::default())
    }

    fn atom_class_in(&mut self, c: &Class, ax: &mut Ax) -> Class {
        let extends = c.extends.as_ref().map(|e| self.atom_expr(e, ax));
        ax.shadow.push(c.name.iter().cloned().collect());
        let ctor = c.ctor.as_ref().map(|f| self.atom_func_in(f, ax));
        let methods = c
            .methods
            .iter()
            .map(|m| super::Method {
                key: self.atom_key(&m.key, ax),
                func: self.atom_func_in(&m.func, ax),
                ..m.clone()
            })
            .collect();
        ax.fns += 1;
        let fields = c
            .fields
            .iter()
            .map(|(k, e, st)| {
                (
                    self.atom_key(k, ax),
                    e.as_ref().map(|e| self.atom_expr(e, ax)),
                    *st,
                )
            })
            .collect();
        let blocks = c
            .blocks
            .iter()
            .map(|(i, e)| (*i, self.atom_expr(e, ax)))
            .collect();
        ax.fns -= 1;
        ax.shadow.pop();
        Class {
            extends,
            ctor,
            methods,
            fields,
            blocks,
            ..c.clone()
        }
    }

    fn atom_block(&mut self, b: &[Stmt], ax: &mut Ax) -> Vec<Stmt> {
        let mut names = Vec::new();
        lexical_names(b, &mut names);
        ax.shadow.push(names);
        // Inside a split block with scope objects, a block's function declarations become
        // `let`s made on entry to it, so they can keep the current scope objects.
        let lift = ax.fns == 0 && !self.scope_objs().is_empty();
        let mut out: Vec<Stmt> = Vec::new();
        let mut rest = Vec::new();
        for s in b {
            match s {
                Stmt::Func(f) if lift && f.name.is_some() => {
                    let f = self.atom_func_in(f, ax);
                    let n = f.name.clone().unwrap();
                    let v = self.capture(Expr::Func(std::rc::Rc::new(f)));
                    out.push(Stmt::Var("let", vec![(Pat::Id(n), Some(v))]));
                }
                Stmt::Class(c) if lift && c.name.is_some() => {
                    let c = self.atom_class_in(c, ax);
                    let n = c.name.clone().unwrap();
                    let v = self.capture(Expr::Class(bx(c)));
                    rest.push(Stmt::Var("let", vec![(Pat::Id(n), Some(v))]));
                }
                s => rest.push(self.atom_stmt(s, ax)),
            }
        }
        ax.shadow.pop();
        out.append(&mut rest);
        out
    }

    /// `var` declarations of this function's code become assignments to the hoisted names.
    fn atom_var(&mut self, ds: &[(Pat, Option<Expr>)], ax: &mut Ax) -> Stmt {
        let mut es = Vec::new();
        for (p, e) in ds {
            self.hoisted.extend(ids(p));
            if let Some(e) = e {
                let p = self.atom_pat(p, ax);
                let e = self.atom_expr(e, ax);
                es.push(Expr::Assign(AssignOp::Assign, bx(p), bx(e)));
            }
        }
        match es.len() {
            0 => Stmt::Empty,
            1 => Stmt::Expr(es.pop().unwrap()),
            _ => Stmt::Expr(Expr::Seq(es)),
        }
    }

    fn atom_loop(&mut self, s: &Stmt, ax: &mut Ax) -> Stmt {
        let labels = std::mem::take(&mut ax.labels);
        ax.local.push(Local {
            labels,
            is_loop: true,
            breakable: true,
        });
        let r = self.atom_loop_inner(s, ax);
        ax.local.pop();
        r
    }

    fn atom_loop_inner(&mut self, s: &Stmt, ax: &mut Ax) -> Stmt {
        let top = ax.fns == 0;
        match s {
            Stmt::While(c, b) => {
                let c = self.atom_expr(c, ax);
                Stmt::While(c, bx(self.atom_stmt(b, ax)))
            }
            Stmt::DoWhile(b, c) => {
                let b = self.atom_stmt(b, ax);
                Stmt::DoWhile(bx(b), self.atom_expr(c, ax))
            }
            Stmt::For(init, t, u, b) => {
                let names = match init.as_deref() {
                    Some(Stmt::Var(k, ds)) if *k != "var" => {
                        ds.iter().flat_map(|(p, _)| ids(p)).collect()
                    }
                    _ => Vec::new(),
                };
                let init = init.as_ref().and_then(|i| match &**i {
                    Stmt::Var("var", ds) if top => match self.atom_var(ds, ax) {
                        Stmt::Empty => None,
                        s => Some(bx(s)),
                    },
                    _ => {
                        ax.shadow.push(names.clone());
                        let s = self.atom_stmt(i, ax);
                        ax.shadow.pop();
                        Some(bx(s))
                    }
                });
                ax.shadow.push(names);
                let t = t.as_ref().map(|t| self.atom_expr(t, ax));
                let u = u.as_ref().map(|u| self.atom_expr(u, ax));
                let b = self.atom_stmt(b, ax);
                ax.shadow.pop();
                Stmt::For(init, t, u, bx(b))
            }
            Stmt::ForOf(k, p, e, b) | Stmt::ForIn(k, p, e, b) => {
                let e = self.atom_expr(e, ax);
                let (k, names) = match k {
                    Some("var") if top => {
                        self.hoisted.extend(ids(p));
                        (None, Vec::new())
                    }
                    Some("var") | None => (*k, Vec::new()),
                    Some(k) => (Some(*k), ids(p)),
                };
                ax.shadow.push(names);
                let p = self.atom_pat(p, ax);
                let b = self.atom_stmt(b, ax);
                ax.shadow.pop();
                match s {
                    Stmt::ForOf(..) => Stmt::ForOf(k, p, e, bx(b)),
                    _ => Stmt::ForIn(k, p, e, bx(b)),
                }
            }
            _ => unreachable!(),
        }
    }

    /// The split target `break`/`continue` (`cont`) with `label` leaves to, unless a target
    /// inside the walked statement takes it.
    fn exit(&self, label: Option<&String>, cont: bool, ax: &Ax) -> Option<usize> {
        let local = ax.local.iter().rev().any(|t| match label {
            Some(l) => t.labels.contains(l),
            None => t.breakable && (!cont || t.is_loop),
        });
        if local {
            return None;
        }
        let t = self.targets.iter().rev().find(|t| match label {
            Some(l) => t.labels.contains(l),
            None => t.breakable && (!cont || t.cont.is_some()),
        })?;
        if cont { t.cont } else { Some(t.brk) }
    }

    fn atom_stmt(&mut self, s: &Stmt, ax: &mut Ax) -> Stmt {
        let top = ax.fns == 0;
        if !matches!(s, Stmt::Labeled(..)) && !is_loop(s) {
            ax.labels.clear();
        }
        match s {
            Stmt::Expr(e) => Stmt::Expr(self.atom_expr(e, ax)),
            Stmt::Var("var", ds) if top => self.atom_var(ds, ax),
            Stmt::Var(k, ds) => Stmt::Var(
                k,
                ds.iter()
                    .map(|(p, e)| {
                        (
                            self.atom_pat(p, ax),
                            e.as_ref().map(|e| self.atom_expr(e, ax)),
                        )
                    })
                    .collect(),
            ),
            Stmt::Func(f) => Stmt::Func(std::rc::Rc::new(self.atom_func_in(f, ax))),
            Stmt::Class(c) => Stmt::Class(bx(self.atom_class_in(c, ax))),
            Stmt::Return(e) => {
                let e = e.as_ref().map(|e| self.atom_expr(e, ax));
                if top {
                    Stmt::Return(Some(ctx("ret", e.into_iter().collect())))
                } else {
                    Stmt::Return(e)
                }
            }
            Stmt::If(c, a, b) => {
                let c = self.atom_expr(c, ax);
                let a = self.atom_stmt(a, ax);
                Stmt::If(c, bx(a), b.as_ref().map(|b| bx(self.atom_stmt(b, ax))))
            }
            Stmt::Block(b) => Stmt::Block(self.atom_block(b, ax)),
            Stmt::For(..)
            | Stmt::ForOf(..)
            | Stmt::ForIn(..)
            | Stmt::While(..)
            | Stmt::DoWhile(..) => self.atom_loop(s, ax),
            Stmt::Switch(d, cs) => {
                let d = self.atom_expr(d, ax);
                let mut names = Vec::new();
                cs.iter().for_each(|(_, b)| lexical_names(b, &mut names));
                ax.shadow.push(names);
                let labels = std::mem::take(&mut ax.labels);
                ax.local.push(Local {
                    labels,
                    is_loop: false,
                    breakable: true,
                });
                let cs = cs
                    .iter()
                    .map(|(t, b)| {
                        (
                            t.as_ref().map(|t| self.atom_expr(t, ax)),
                            b.iter().map(|s| self.atom_stmt(s, ax)).collect(),
                        )
                    })
                    .collect();
                ax.local.pop();
                ax.shadow.pop();
                Stmt::Switch(d, cs)
            }
            Stmt::Break(l) | Stmt::Continue(l) if top => {
                let cont = matches!(s, Stmt::Continue(_));
                match self.exit(l.as_ref(), cont, ax) {
                    Some(id) => self.jump_stmt(id),
                    None => s.clone(),
                }
            }
            Stmt::Labeled(l, b) => {
                ax.labels.push(l.clone());
                if is_loop(b) || matches!(**b, Stmt::Labeled(..)) {
                    let b = self.atom_stmt(b, ax);
                    return Stmt::Labeled(l.clone(), bx(b));
                }
                let labels = std::mem::take(&mut ax.labels);
                ax.local.push(Local {
                    labels,
                    is_loop: false,
                    breakable: false,
                });
                let b = self.atom_stmt(b, ax);
                ax.local.pop();
                Stmt::Labeled(l.clone(), bx(b))
            }
            Stmt::With(o, b) => {
                let o = self.atom_expr(o, ax);
                ax.withs += 1;
                let b = self.atom_stmt(b, ax);
                ax.withs -= 1;
                Stmt::With(o, bx(b))
            }
            Stmt::Throw(e) => Stmt::Throw(self.atom_expr(e, ax)),
            Stmt::Try(a, c, f) => {
                let a = self.atom_block(a, ax);
                let c = c.as_ref().map(|(p, b)| {
                    ax.shadow.push(p.as_ref().map(ids).unwrap_or_default());
                    let p = p.as_ref().map(|p| self.atom_pat(p, ax));
                    let b = self.atom_block(b, ax);
                    ax.shadow.pop();
                    (p, b)
                });
                let f = f.as_ref().map(|b| self.atom_block(b, ax));
                Stmt::Try(a, c, f)
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Empty | Stmt::Unsupported(_) => s.clone(),
        }
    }
}
