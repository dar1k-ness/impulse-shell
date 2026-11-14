//! Expressions, assignment targets and destructuring.
use std::fmt::Write;
use std::rc::Rc;

use qml::ir::{
    AssignOp, BinOp, Class, Expr, Func, Key, LogicalOp, Pat, PropKind, Stmt, UnOp, UpdateOp,
};

use crate::scope::{
    ACTIVE_FN, ClassCtx, CtorMode, Fx, HOME, NEW_TARGET, SUPER, Sc, Sink, THIS, emit_func_item,
    field_key, js_str, js_str16, lit_str, num,
};

/// Stands for the changed color in the store code of [`Fx::color_target`].
const COLOR: &str = "\u{1}color\u{1}";

const MUTATING: [&str; 9] = [
    "push",
    "pop",
    "shift",
    "unshift",
    "splice",
    "sort",
    "reverse",
    "fill",
    "copyWithin",
];

/// The scope chain (scopes, env, depth) a class element key is lowered in.
type KeyScope = (Vec<Sc>, String, u32);

impl Fx<'_> {
    /// Value of a name: lexical binding or the QML scope.
    /// The unchecked read of `this` where it may still be unbound: in a derived constructor
    /// before `super(...)`, or in an arrow function made there.
    fn raw_this(&self) -> Option<String> {
        match self.find(THIS) {
            Some(s) if self.maybe_uninit(THIS) => Some(self.read(&s)),
            _ => None,
        }
    }

    /// [`Self::raw_this`] in the derived constructor itself (`Interp::unbound_this`).
    fn unbound_this(&self) -> Option<String> {
        self.raw_this().filter(|_| self.derived_ctor)
    }

    /// [`Self::unbound_this`] where `e` is `this`.
    fn unbound_this_of(&self, e: &Expr) -> Option<String> {
        match e.shown() {
            Expr::This => self.unbound_this(),
            _ => None,
        }
    }

    /// `this` (`Interp::this_binding`): QV4's ReferenceError for the unbound one has no name
    /// where the derived constructor itself reads it. In an arrow function of a derived
    /// constructor `this` is never bound, and the error names it.
    fn this_binding(&mut self) -> String {
        if self.arrow && self.class.as_ref().is_some_and(|c| c.derived) {
            return self.t("rt::bound(cx, Value::Empty, \"this\")");
        }
        match self.unbound_this() {
            Some(r) => self.t(&format!("rt::bound(cx, {r}, \"\")")),
            None => self.read_name(THIS),
        }
    }

    /// The `this` of a `super.x` reference (`Interp::super_this`): in an arrow function the
    /// `this` of the arrow's own call.
    fn super_this(&mut self) -> String {
        if self.arrow {
            return "this.clone()".into();
        }
        match self.raw_this() {
            Some(r) => self.t(&format!("rt::super_this(cx, {r})")),
            None => self.read_name(THIS),
        }
    }

    pub fn read_name(&mut self, n: &str) -> String {
        match self.find(n) {
            Some(s) => self.read_checked(n, &s),
            None => match n {
                THIS => "this.clone()".into(),
                SUPER | HOME | NEW_TARGET | ACTIVE_FN => "Value::Undefined".into(),
                "undefined" => "Value::Undefined".into(),
                _ if self.qml_only(n) => {
                    self.t(&format!("rt::qml(cx, &{}, sym!({}))", self.env, lit_str(n)))
                }
                _ => {
                    self.qs = true;
                    self.t(&format!(
                        "rt::name(cx, &{}, qs__, sym!({}))",
                        self.env,
                        lit_str(n)
                    ))
                }
            },
        }
    }

    /// A free name resolved by `rt::qml` (no scope-chain walk).
    fn qml_only(&self, n: &str) -> bool {
        !self.g.scope_vars && qml_only(n)
    }

    /// Store `v` (a Rust `Value` expression) into a name.
    pub fn write_name(&mut self, n: &str, v: &str) -> String {
        match self.find(n) {
            Some(s) if self.maybe_uninit(n) => {
                let c = self.t(&format!(
                    "rt::assignable(cx, &{}, {})",
                    self.read(&s),
                    lit_str(n)
                ));
                format!("{{ {c}; {} }}", self.write(&s, v))
            }
            Some(s) => self.write(&s, v),
            None if self.qml_only(n) => {
                format!(
                    "{};",
                    self.t(&format!(
                        "rt::assign_qml(cx, &{}, sym!({}), {v})",
                        self.env,
                        lit_str(n)
                    ))
                )
            }
            None => {
                self.qs = true;
                format!(
                    "{};",
                    self.t(&format!(
                        "rt::assign(cx, &{}, qs__, sym!({}), {v})",
                        self.env,
                        lit_str(n)
                    ))
                )
            }
        }
    }

    /// Rust expression (of type `Value`) evaluating `e`.
    pub fn expr(&mut self, e: &Expr) -> String {
        match e {
            Expr::Dynamic(..) | Expr::DirectEval(..) => self.expr(e.shown()),
            Expr::Num(n) => format!("Value::Num({})", num(*n)),
            Expr::Str(s) => js_str(s),
            Expr::Str16(u) => js_str16(u),
            Expr::Bool(b) => format!("Value::Bool({b})"),
            Expr::Null => "Value::Null".into(),
            Expr::Hole => "Value::Undefined".into(),
            Expr::Ident(n) | Expr::Local(_, _, n) => self.read_name(n),
            Expr::This => self.this_binding(),
            Expr::Super => self.read_name(SUPER),
            Expr::NewTarget => self.read_name(NEW_TARGET),
            Expr::Regex(p, f) => format!("jsrt::regex({}, {})", lit_str(p), lit_str(f)),
            Expr::Template(q, es) => {
                let s = self.tmp();
                let mut o = format!("{{ let mut {s} = qml::value::JsBuf::new(); ");
                for (i, part) in q.iter().enumerate() {
                    if !part.is_empty() {
                        let _ = write!(o, "{s}.push_str({}); ", lit_str(part));
                    }
                    if let Some(e) = es.get(i) {
                        let v = self.expr(e);
                        let t = self.tmp();
                        let st = self.t(&format!("cx.push_substitution(&mut {s}, &{t})"));
                        let _ = write!(o, "let {t} = {v}; {st}; ");
                    }
                }
                let _ = write!(o, "{s}.finish() }}");
                o
            }
            Expr::TemplateObject(t) => {
                let n = t.raw.len();
                let cooked: Vec<String> = t
                    .cooked
                    .iter()
                    .map(|c| {
                        c.as_deref()
                            .map_or("None".into(), |s| format!("Some({})", lit_str(s)))
                    })
                    .collect();
                let raw: Vec<String> = t.raw.iter().map(|s| lit_str(s)).collect();
                format!(
                    "{{ static C: [Option<&str>; {n}] = [{}]; static R: [&str; {n}] = [{}]; \
                     jsrt::template_object(cx, &C, &R) }}",
                    cooked.join(", "),
                    raw.join(", ")
                )
            }
            Expr::Member(o, n, opt) => {
                let n = n.as_str();
                if matches!(**o, Expr::Super) {
                    // MakeSuperPropertyReference: GetThisBinding first.
                    let this = self.super_this();
                    let h = self.read_name(HOME);
                    let t = self.tmp();
                    let r = self.t(&format!("rt::super_member(cx, &{h}, &{t}, {})", lit_str(n)));
                    return format!("{{ let {t} = {this}; {r} }}");
                }
                let ov = self.expr(o);
                let t = self.tmp();
                let chk = self.opt_check(&t, *opt);
                let r = self.t(&format!(
                    "rt::member_of(cx, {t}, {0}, sym!({0}))",
                    lit_str(n)
                ));
                format!("{{ let {t} = {ov}; {chk}{r} }}")
            }
            Expr::Index(o, k, _) if matches!(**o, Expr::Super) => {
                let this = self.super_this();
                let h = self.read_name(HOME);
                let (tt, tk) = (self.tmp(), self.tmp());
                let kv = self.expr(k);
                let r = self.t(&format!("rt::super_index(cx, &{h}, &{tt}, &{tk})"));
                format!("{{ let {tt} = {this}; let {tk} = {kv}; {r} }}")
            }
            Expr::Index(o, k, opt) => {
                let ov = self.expr(o);
                let t = self.tmp();
                let chk = self.opt_check(&t, *opt);
                let kv = self.expr(k);
                let tk = self.tmp();
                let r = self.t(&format!("rt::index(cx, &{t}, &{tk})"));
                format!("{{ let {t} = {ov}; {chk}let {tk} = {kv}; {r} }}")
            }
            Expr::Call(c, a, opt) => self.call(c, a, *opt),
            Expr::New(c, a) => {
                let f = self.expr(c);
                let t = self.tmp();
                let (decl, av) = self.args(a);
                let r = self.t(&format!("cx.construct(&{t}, &{av})"));
                format!("{{ let {t} = {f}; {decl} {r} }}")
            }
            Expr::Chain(inner) => {
                let l = self.g.id("c");
                let saved = self.chain.replace(l.clone());
                let v = self.expr(inner);
                self.chain = saved;
                format!("('{l}: {{ {v} }})")
            }
            Expr::Unary(op, a) => self.unary(*op, a),
            Expr::Update(op, prefix, p) => {
                let (pre, lr) = self.target(p);
                let cur = self.target_get(p, &lr);
                let (c, o, n) = (self.tmp(), self.tmp(), self.tmp());
                let d = if *op == UpdateOp::Inc { "+" } else { "-" };
                let old = self.t(&format!("rt::to_num(cx, &{c})"));
                let mut s =
                    format!("{{ {pre}let {c} = {cur}; let {o} = {old}; let {n} = {o} {d} 1.0; ");
                s.push_str(&self.target_put(p, &lr, &format!("Value::Num({n})")));
                let _ = write!(s, " Value::Num({}) }}", if *prefix { &n } else { &o });
                s
            }
            Expr::Binary(op, a, b) => {
                let x = self.expr(a);
                let y = self.expr(b);
                let (tx, ty) = (self.tmp(), self.tmp());
                let r = self.binop(*op, &tx, &ty);
                format!("{{ let {tx} = {x}; let {ty} = {y}; {r} }}")
            }
            Expr::Logical(op, a, b) => {
                let x = self.expr(a);
                let t = self.tmp();
                let take = match *op {
                    LogicalOp::And => format!("!{t}.truthy()"),
                    LogicalOp::Or => format!("{t}.truthy()"),
                    LogicalOp::Nullish => format!("!rt::nullish(&{t})"),
                };
                let y = self.expr(b);
                format!("{{ let {t} = {x}; if {take} {{ {t} }} else {{ {y} }} }}")
            }
            Expr::Cond(c, a, b) => {
                let c = self.expr(c);
                let a = self.expr(a);
                let b = self.expr(b);
                format!("(if ({c}).truthy() {{ {a} }} else {{ {b} }})")
            }
            Expr::Assign(op, p, rhs) => self.assign(*op, p, rhs),
            Expr::Array(items) if items.iter().any(|e| matches!(e, Expr::Hole)) => {
                let (t, h) = (self.tmp(), self.tmp());
                let mut s = format!(
                    "let mut {t}: Vec<Value> = Vec::new(); let mut {h}: Vec<usize> = Vec::new(); "
                );
                for e in items {
                    match e {
                        Expr::Hole => {
                            let _ = write!(s, "{h}.push({t}.len()); {t}.push(Value::Undefined); ");
                        }
                        Expr::Spread(x) => {
                            let v = self.expr(x);
                            let tv = self.tmp();
                            let it = self.t(&format!("cx.iterate(&{tv})"));
                            let _ = write!(s, "let {tv} = {v}; {t}.extend({it}); ");
                        }
                        _ => {
                            let v = self.expr(e);
                            let _ = write!(s, "{t}.push({v}); ");
                        }
                    }
                }
                format!("{{ {s}Value::array_holes({t}, {h}) }}")
            }
            Expr::Array(items) => {
                let (decl, av) = self.args(items);
                format!("{{ {decl} Value::array({av}.into()) }}")
            }
            Expr::Object(props) => {
                let o = self.tmp();
                let mut s = format!("{{ let {o} = jsrt::object({}); ", props.len());
                // MakeMethod (§10.2.7): the literal is the [[HomeObject]] of its methods,
                // which close over a scope binding it; the other values are outside it.
                let mut home = None;
                if props.iter().any(|(_, v, _)| qml::ir::is_super_method(v)) {
                    let out = std::mem::take(&mut self.out);
                    self.cap.insert(HOME.into());
                    let saved = self.open(&[HOME.into()], false);
                    let w = self.write_name(HOME, &format!("{o}.clone()"));
                    let _ = write!(s, "{w} ");
                    let sc = self.scopes.pop().unwrap();
                    let inner = (
                        std::mem::replace(&mut self.env, saved.0.clone()),
                        self.depth,
                    );
                    self.depth = saved.1;
                    home = Some((out, saved, sc, inner));
                }
                for (k, v, kind) in props {
                    if let Expr::Spread(src) = v {
                        let sv = self.expr(src);
                        let t = self.tmp();
                        let sp = self.t(&format!("jsrt::object_spread(cx, &{o}, &{t})"));
                        let _ = write!(s, "let {t} = {sv}; {sp}; ");
                        continue;
                    }
                    let (key, pk) = match k {
                        Key::Name(n) => (lit_str(n), ""),
                        Key::Computed(e) => {
                            let kv = self.expr(e);
                            let t = self.tmp();
                            let pk = self.t(&format!("cx.to_property_key(&{t})"));
                            let _ = write!(s, "let {t} = {kv}; let {t} = {pk}; ");
                            (format!("&{t}"), "_pk")
                        }
                    };
                    let vv = match &mut home {
                        Some((_, _, sc, (env, depth))) if qml::ir::is_super_method(v) => {
                            self.scopes.push(std::mem::take(sc));
                            let outer_env = std::mem::replace(&mut self.env, env.clone());
                            let outer_depth = std::mem::replace(&mut self.depth, *depth);
                            let vv = self.expr(v);
                            *sc = self.scopes.pop().unwrap();
                            self.env = outer_env;
                            self.depth = outer_depth;
                            vv
                        }
                        _ => self.expr(v),
                    };
                    let t = self.tmp();
                    if matches!(k, Key::Computed(_)) && qml::ir::is_anonymous_fn(v) {
                        let prefix = match kind {
                            PropKind::Init | PropKind::Proto => "",
                            PropKind::Get => "get ",
                            PropKind::Set => "set ",
                        };
                        let _ = write!(
                            s,
                            "let {t} = {vv}; jsrt::name_function{pk}(&{t}, {}, {key}); ",
                            lit_str(prefix)
                        );
                    } else {
                        let _ = write!(s, "let {t} = {vv}; ");
                    }
                    let set = match kind {
                        PropKind::Init => format!("jsrt::object_set{pk}(&{o}, {key}, {t})"),
                        PropKind::Proto => format!("jsrt::object_proto(&{o}, &{t})"),
                        PropKind::Get => {
                            format!("jsrt::object_accessor{pk}(&{o}, {key}, true, {t})")
                        }
                        PropKind::Set => {
                            format!("jsrt::object_accessor{pk}(&{o}, {key}, false, {t})")
                        }
                    };
                    let _ = write!(s, "{set}; ");
                }
                let _ = write!(s, "{o} }}");
                match home {
                    Some((out, saved, sc, _)) => {
                        self.scopes.push(sc);
                        self.close(saved);
                        let body = std::mem::replace(&mut self.out, out);
                        format!("{{ {body} {s} }}")
                    }
                    None => s,
                }
            }
            Expr::Spread(e) => self.expr(e),
            Expr::DefineField(k, v, name) => {
                let kv = self.expr(k);
                let vv = self.expr(v);
                let th = self.read_name(THIS);
                let (k, v) = (self.tmp(), self.tmp());
                let pk = self.t(&format!("cx.to_property_key(&{k})"));
                let r = self.t(&format!(
                    "jsrt::define_field_pk(cx, &{th}, &{k}, {v}, {name})"
                ));
                format!(
                    "{{ let {k} = {kv}; let {k} = {pk}; let {v} = {vv}; {r}; Value::Undefined }}"
                )
            }
            Expr::Seq(es) => {
                let mut s = String::from("{ ");
                for (i, e) in es.iter().enumerate() {
                    let v = self.expr(e);
                    if i + 1 == es.len() {
                        s.push_str(&v);
                    } else {
                        let _ = write!(s, "let _ = {v}; ");
                    }
                }
                if es.is_empty() {
                    s.push_str("Value::Undefined");
                }
                s.push_str(" }");
                s
            }
            Expr::Func(f) => self.closure(f),
            Expr::Class(c) => self.class_value(c),
            // A generator body's yields are lowered away (`qml::ir` genlower).
            Expr::Yield(..) => {
                let r = self.t(&format!("rt::unsupported(cx, {})", lit_str("Yield")));
                format!("{{ {r} }}")
            }
            Expr::Unsupported(k) => {
                let r = self.t(&format!("rt::unsupported(cx, {})", lit_str(k)));
                format!("{{ {r} }}")
            }
        }
    }

    /// `if t is nullish { end the chain }` for an optional `?.`.
    fn opt_check(&self, t: &str, opt: bool) -> String {
        if opt {
            format!("if rt::nullish(&{t}) {{ {} }} ", self.short())
        } else {
            String::new()
        }
    }

    /// Argument list: (declaration code, name of the `[Value; N]` / `Vec<Value>`).
    pub fn args(&mut self, a: &[Expr]) -> (String, String) {
        let t = self.tmp();
        if !a.iter().any(|e| matches!(e, Expr::Spread(_))) {
            let vs: Vec<String> = a.iter().map(|e| self.expr(e)).collect();
            return (
                format!("let {t}: [Value; {}] = [{}];", a.len(), vs.join(", ")),
                t,
            );
        }
        let mut s = format!("let mut {t}: Vec<Value> = Vec::new(); ");
        for e in a {
            if let Expr::Spread(x) = e {
                let v = self.expr(x);
                let tv = self.tmp();
                let it = self.t(&format!("cx.iterate(&{tv})"));
                let _ = write!(s, "let {tv} = {v}; {t}.extend({it}); ");
            } else {
                let v = self.expr(e);
                let _ = write!(s, "{t}.push({v}); ");
            }
        }
        (s, t)
    }

    /// A call. V4 order: the callee's base object (and computed key) before the arguments.
    fn call(&mut self, c: &Expr, a: &[Expr], opt: bool) -> String {
        // `super(...)` in an arrow function: QV4 finds the arrow frame's `this` bound, before
        // the arguments run (`Interp::call_expr`).
        if self.arrow && matches!(c.shown(), Expr::Super) {
            return self.t("rt::super_called(cx)");
        }
        let (decl, av) = self.args(a);
        match c.shown() {
            // SuperCall (ECMA-262 §13.3.7.1): construct the parent for NewTarget, bind
            // `this` to the result, then initialize this class's fields on it.
            Expr::Super => {
                let sup = self.read_name(ACTIVE_FN);
                let nt = self.read_name(NEW_TARGET);
                let th = self.find(THIS).unwrap();
                let bound = self.read(&th);
                let (ts, tn, tt) = (self.tmp(), self.tmp(), self.tmp());
                let r = self.t(&format!("rt::super_construct(cx, &{ts}, &{av}, &{tn})"));
                let chk = self.t(&format!("cx.bind_this(&{bound})"));
                let w = self.write(&th, &format!("{tt}.clone()"));
                let fields = self.fields_call(&tt);
                format!(
                    "{{ {decl} let {ts} = {sup}; let {tn} = {nt}; let {tt} = {r}; {chk}; {w} {fields} {tt} }}"
                )
            }
            Expr::Member(o, n, _) if matches!(**o, Expr::Super) => {
                let th = self.super_this();
                let h = self.read_name(HOME);
                let (tt, tf) = (self.tmp(), self.tmp());
                let f = self.t(&format!(
                    "rt::super_member(cx, &{h}, &{tt}, {})",
                    lit_str(n.as_str())
                ));
                let r = self.ts(&format!(
                    "rt::call_super_method(cx, {tf}, &{tt}, &jsrt::PropKey::Str({}.into()), &{av}, {opt})",
                    lit_str(n.as_str())
                ));
                format!("{{ let {tt} = {th}; let {tf} = {f}; {decl} {r} }}")
            }
            Expr::Index(o, k, _) if matches!(**o, Expr::Super) => {
                let th = self.super_this();
                let h = self.read_name(HOME);
                let kv = self.expr(k);
                let (tt, tk, tf) = (self.tmp(), self.tmp(), self.tmp());
                let f = self.t(&format!("rt::super_index(cx, &{h}, &{tt}, &{tk})"));
                let r = self.ts(&format!(
                    "rt::call_super_method(cx, {tf}, &{tt}, &jsrt::PropKey::of(&{tk}), &{av}, {opt})"
                ));
                format!("{{ let {tt} = {th}; let {tk} = {kv}; let {tf} = {f}; {decl} {r} }}")
            }
            Expr::Member(o, n, mopt) => {
                // QV4's `this.name(...)` in a derived constructor before `super(...)`: the
                // arguments run first, and the call has no function if `this` is unbound
                // after them (`Interp::call_expr`); `this?.name(...)` reads `this` checked.
                if let Some(raw) = self.unbound_this_of(o).filter(|_| !*mopt) {
                    let t = self.tmp();
                    let ov = self.t(&format!(
                        "rt::this_callee(cx, {raw}, {})",
                        lit_str(n.as_str())
                    ));
                    let bind = format!("let {t} = {ov};");
                    return match opt || *mopt {
                        true => self.call_member(o, n.as_str(), &bind, &t, &decl, &av, opt),
                        false => {
                            let decl = format!("{decl} {bind}");
                            self.call_member(o, n.as_str(), "", &t, &decl, &av, opt)
                        }
                    };
                }
                let ov = self.expr(o);
                let t = self.tmp();
                let chk = self.opt_check(&t, *mopt);
                self.call_member(
                    o,
                    n.as_str(),
                    &format!("let {t} = {ov}; {chk}"),
                    &t,
                    &decl,
                    &av,
                    opt,
                )
            }
            Expr::Index(o, k, iopt) => {
                let ov = self.expr(o);
                let t = self.tmp();
                let chk = self.opt_check(&t, *iopt);
                let kv = self.expr(k);
                let tk = self.tmp();
                let ck = self.t(&format!("cx.callee_key(&{t}, {tk})"));
                let bind = format!("let {t} = {ov}; {chk}let {tk} = {kv}; let {tk} = {ck}; ");
                self.call_index(&bind, &t, &tk, &decl, &av, opt)
            }
            // `(o?.f)()`: a parenthesized optional chain is still a property reference, so
            // the call's `this` is its base (ECMA-262 §13.3.9.1); a short-circuit makes the
            // whole chain, and so the callee, `undefined`.
            Expr::Chain(inner) if is_member_callee(inner) => {
                let (l, lo, tb) = (self.g.id("c"), self.g.id("o"), self.tmp());
                let saved = self.chain.replace(l.clone());
                let (t, tk) = (self.tmp(), self.tmp());
                let base = match &**inner {
                    Expr::Member(o, _, mopt) => {
                        let ov = self.expr(o);
                        let chk = self.opt_check(&t, *mopt);
                        format!("let {t} = {ov}; {chk}break '{lo} Some(({t}, Value::Undefined));")
                    }
                    Expr::Index(o, k, iopt) => {
                        let ov = self.expr(o);
                        let chk = self.opt_check(&t, *iopt);
                        let kv = self.expr(k);
                        let ck = self.t(&format!("cx.callee_key(&{t}, {tk})"));
                        format!(
                            "let {t} = {ov}; {chk}let {tk} = {kv}; let {tk} = {ck}; \
                             break '{lo} Some(({t}, {tk}));"
                        )
                    }
                    _ => unreachable!(),
                };
                self.chain = saved;
                let call = match &**inner {
                    Expr::Member(o, n, _) => {
                        self.call_member(o, n.as_str(), "", &t, &decl, &av, opt)
                    }
                    _ => self.call_index("", &t, &tk, &decl, &av, opt),
                };
                let key = if matches!(**inner, Expr::Member(..)) {
                    "_"
                } else {
                    &tk
                };
                let tu = self.tmp();
                let r = self.ts(&format!("rt::call_value(cx, &{tu}, &{av}, {opt})"));
                let chk = self.opt_check(&tu, opt);
                format!(
                    "{{ let {tb}: Option<(Value, Value)> = '{lo}: {{ let _: Value = '{l}: {{ {base} }}; None }}; \
                     match {tb} {{ Some(({t}, {key})) => {call}, \
                     None => {{ let {tu} = Value::Undefined; {chk}{decl} {r} }} }} }}"
                )
            }
            Expr::Ident(n) | Expr::Local(_, _, n) => {
                if let Some(s) = self.find(n) {
                    let f = self.read_checked(n, &s);
                    let t = self.tmp();
                    let r = self.ts(&format!("rt::call_local(cx, &{t}, &{av}, {opt})"));
                    if opt {
                        let chk = self.opt_check(&t, true);
                        return format!("{{ let {t} = {f}; {chk}{decl} {r} }}");
                    }
                    format!("{{ {decl} let {t} = {f}; {r} }}")
                } else if self.qml_only(n) {
                    let r = self.ts(&format!(
                        "rt::call_qml(cx, &{}, sym!({}), &{av}, {opt})",
                        self.env,
                        lit_str(n)
                    ));
                    if opt {
                        let pre = self.ts(&format!(
                            "rt::opt_qml(cx, &{}, sym!({}))",
                            self.env,
                            lit_str(n)
                        ));
                        return format!("{{ {pre}; {decl} {r} }}");
                    }
                    format!("{{ {decl} {r} }}")
                } else {
                    self.qs = true;
                    let r = self.ts(&format!(
                        "rt::call_name(cx, &{}, qs__, sym!({}), &{av}, {opt})",
                        self.env,
                        lit_str(n)
                    ));
                    if opt {
                        let pre = self.ts(&format!(
                            "rt::opt_name(cx, &{}, qs__, sym!({}))",
                            self.env,
                            lit_str(n)
                        ));
                        return format!("{{ {pre}; {decl} {r} }}");
                    }
                    format!("{{ {decl} {r} }}")
                }
            }
            _ => {
                let f = self.expr(c);
                let t = self.tmp();
                let r = self.ts(&format!("rt::call_value(cx, &{t}, &{av}, {opt})"));
                if opt {
                    let chk = self.opt_check(&t, true);
                    return format!("{{ let {t} = {f}; {chk}{decl} {r} }}");
                }
                format!("{{ {decl} let {t} = {f}; {r} }}")
            }
        }
    }

    /// A method call `o.n(...)`; `bind` binds the base object (the value of `o`) to `t`.
    #[allow(clippy::too_many_arguments)]
    fn call_member(
        &mut self,
        o: &Expr,
        n: &str,
        bind: &str,
        t: &str,
        decl: &str,
        av: &str,
        opt: bool,
    ) -> String {
        if opt {
            let (tf, r, m) = (self.tmp(), self.tmp(), self.tmp());
            let pre = self.ts(&format!("rt::opt_method(cx, &{t}, sym!({}))", lit_str(n)));
            let call = self.ts(&format!(
                "rt::call_method_pre(cx, &{t}, sym!({}), {tf}, &{av})",
                lit_str(n)
            ));
            let head = format!("{{ {bind}let {tf} = {pre}; {decl} let ({r}, {m}) = {call};");
            if !MUTATING.contains(&n) {
                let wb = self.write_back_date(o, n, t);
                return format!("{head} let _ = {m}; {wb} {r} }}");
            }
            let wb = self.write_back(o, t);
            return format!("{head} if {m} {{ {wb} }} {r} }}");
        }
        if !MUTATING.contains(&n) {
            let wb = self.write_back_date(o, n, t);
            if !wb.is_empty() {
                let r = self.tmp();
                let call = self.ts(&format!(
                    "rt::call_method_of(cx, {t}.clone(), sym!({}), &{av}, {opt})",
                    lit_str(n)
                ));
                return format!("{{ {bind}{decl} let {r} = {call}; {wb} {r} }}");
            }
            let call = self.ts(&format!(
                "rt::call_method_of(cx, {t}, sym!({}), &{av}, {opt})",
                lit_str(n)
            ));
            return format!("{{ {bind}{decl} {call} }}");
        }
        let (r, m) = (self.tmp(), self.tmp());
        let call = self.ts(&format!(
            "rt::call_method(cx, &{t}, sym!({}), &{av}, {opt})",
            lit_str(n)
        ));
        let wb = self.write_back(o, t);
        format!("{{ {bind}{decl} let ({r}, {m}) = {call}; if {m} {{ {wb} }} {r} }}")
    }

    /// A call `o[k](...)`; `bind` binds the base object to `t` and the key to `tk`.
    fn call_index(
        &mut self,
        bind: &str,
        t: &str,
        tk: &str,
        decl: &str,
        av: &str,
        opt: bool,
    ) -> String {
        if opt {
            let tf = self.tmp();
            let pre = self.ts(&format!("rt::opt_index(cx, &{t}, &{tk})"));
            let r = self.ts(&format!("rt::call_index_pre(cx, &{t}, &{tk}, {tf}, &{av})"));
            return format!("{{ {bind}let {tf} = {pre}; {decl} {r} }}");
        }
        let r = self.ts(&format!("rt::call_index(cx, &{t}, &{tk}, &{av}, {opt})"));
        format!("{{ {bind}{decl} {r} }}")
    }

    /// The write-back of a Date read from a `date` property after setter `n` was called on
    /// that read (`ov`, the value of `o`); empty when `o` is not a property read or `n` is no
    /// Date setter.
    fn write_back_date(&mut self, o: &Expr, n: &str, ov: &str) -> String {
        if !qml::builtins::is_date_setter(n) {
            return String::new();
        }
        let prop = match o.shown() {
            Expr::Member(_, p, _) => p.as_str(),
            Expr::Index(_, k, _) => match &**k {
                Expr::Str(p) => &**p,
                _ => return String::new(),
            },
            Expr::Ident(p) | Expr::Local(_, _, p) if self.find(p).is_none() => p.as_str(),
            _ => return String::new(),
        };
        format!("jsrt::write_back_date(cx, &{ov}, {});", lit_str(prop))
    }

    /// Code before and after the write of member `n` of `t` that notifies the property owning
    /// the sequence `t`, unless the write was of the length it already had.
    fn notify_owner(&mut self, n: &str, t: &str) -> (String, String) {
        if n == "length" {
            let l = self.tmp();
            (
                format!("let {l} = rt::seq_len(&{t}); "),
                format!("if rt::seq_len(&{t}) != {l} {{ rt::notify_owner(cx, &{t}); }}"),
            )
        } else {
            (String::new(), format!("rt::notify_owner(cx, &{t});"))
        }
    }

    /// QML sequence write-back after an in-place mutation of `ov` (the value of `o`).
    fn write_back(&mut self, o: &Expr, ov: &str) -> String {
        match o.shown() {
            Expr::Member(b, p, _)
                if matches!(b.shown(), Expr::Ident(_) | Expr::Local(..) | Expr::This) =>
            {
                let bv = self.expr(b);
                let t = self.tmp();
                format!(
                    "let {t} = {bv}; jsrt::write_back_member(cx, &{t}, {}, &{ov});",
                    lit_str(p.as_str())
                )
            }
            Expr::Ident(n) | Expr::Local(_, _, n) if self.find(n).is_none() && self.qml_only(n) => {
                format!(
                    "rt::write_back_qml(cx, &{}, sym!({}), &{ov});",
                    self.env,
                    lit_str(n)
                )
            }
            Expr::Ident(n) | Expr::Local(_, _, n) if self.find(n).is_none() => {
                self.qs = true;
                format!(
                    "rt::write_back_name(cx, &{}, qs__, sym!({}), &{ov});",
                    self.env,
                    lit_str(n)
                )
            }
            _ => format!("jsrt::write_back_owner(cx, &{ov});"),
        }
    }

    fn unary(&mut self, op: UnOp, a: &Expr) -> String {
        if op == UnOp::Typeof
            && let Expr::Ident(n) | Expr::Local(_, _, n) = a.shown()
        {
            return match self.find(n) {
                Some(s) => format!("Value::str(({}).type_of())", self.read_checked(n, &s)),
                None if self.qml_only(n) => {
                    format!("rt::type_of_qml(cx, &{}, sym!({}))", self.env, lit_str(n))
                }
                None => {
                    self.qs = true;
                    format!(
                        "rt::type_of_name(cx, &{}, qs__, sym!({}))",
                        self.env,
                        lit_str(n)
                    )
                }
            };
        }
        if op == UnOp::Delete {
            // A Super Reference is made (the this binding, then the key) and cannot be
            // deleted (ECMA-262 §13.5.1.2 step 5.b).
            if let Expr::Member(o, _, _) = a
                && matches!(**o, Expr::Super)
            {
                let this = self.super_this();
                let d = self.t("cx.delete_super()");
                return format!("{{ let _ = {this}; {d} }}");
            }
            if let Expr::Index(o, k, _) = a
                && matches!(**o, Expr::Super)
            {
                let this = self.super_this();
                let kv = self.expr(k);
                let tk = self.tmp();
                let pk = self.t(&format!("cx.to_property_key(&{tk})"));
                let d = self.t("cx.delete_super()");
                return format!("{{ let _ = {this}; let {tk} = {kv}; let _ = {pk}; {d} }}");
            }
            // `delete a?.b`: a short-circuit anywhere in the chain deletes nothing.
            if let Expr::Chain(inner) = a
                && matches!(**inner, Expr::Member(..) | Expr::Index(..))
            {
                let l = self.g.id("c");
                let saved = self.chain.replace(l.clone());
                let d = self.unary(UnOp::Delete, inner);
                self.chain = saved;
                let t = self.tmp();
                return format!(
                    "{{ let {t} = ('{l}: {{ {d} }}); \
                     if matches!({t}, Value::Undefined) {{ Value::Bool(true) }} else {{ {t} }} }}"
                );
            }
            if let Expr::Member(o, n, opt) = a {
                let unbound = self.unbound_this_of(o);
                let ov = match &unbound {
                    Some(raw) => raw.clone(),
                    None => self.expr(o),
                };
                let t = self.tmp();
                let chk = self.opt_check(&t, *opt);
                let d = self.t(&format!(
                    "jsrt::{}(cx, &{t}, {})",
                    self.delete_fn(),
                    lit_str(n.as_str())
                ));
                // QV4: nothing is deleted from the unbound `this` of a derived constructor.
                if unbound.is_some() {
                    return format!(
                        "{{ let {t} = {ov}; \
                         if matches!({t}, Value::Empty) {{ Value::Bool(true) }} else {{ {d} }} }}"
                    );
                }
                return format!("{{ let {t} = {ov}; {chk}{d} }}");
            }
            if let Expr::Index(o, k, opt) = a {
                let ov = self.expr(o);
                let (t, tk) = (self.tmp(), self.tmp());
                let chk = self.opt_check(&t, *opt);
                let kv = self.expr(k);
                let pk = self.t(&format!("cx.reference_key(&{t}, &{tk})"));
                let d = self.t(&format!("jsrt::{}_pk(cx, &{t}, &{tk})", self.delete_fn()));
                return format!("{{ let {t} = {ov}; {chk}let {tk} = {kv}; let {tk} = {pk}; {d} }}");
            }
            // A binding declared in a function is not deletable (ECMA-262 §9.1.1.1.7). Any
            // other name deletes nothing and gives true, as V4 does for an unresolvable name
            // and for a library's top-level `var` and function.
            match a.shown() {
                Expr::Local(..) => return "Value::Bool(false)".into(),
                Expr::Ident(n) => return format!("Value::Bool({})", !self.declared_in_function(n)),
                _ => {}
            }
            // Not a reference: the operand still runs (ECMA-262 §13.5.1.2 step 2).
            let v = self.expr(a);
            return format!("{{ let _ = {v}; Value::Bool(true) }}");
        }
        let v = self.expr(a);
        let f = match op {
            UnOp::Not => return format!("rt::not(&({v}))"),
            UnOp::Typeof => return format!("Value::str(({v}).type_of())"),
            UnOp::Void | UnOp::Delete => return format!("{{ let _ = {v}; Value::Undefined }}"),
            UnOp::Neg => "neg",
            UnOp::Pos => "pos",
            UnOp::BitNot => "bnot",
        };
        let t = self.tmp();
        let r = self.t(&format!("rt::{f}(cx, &{t})"));
        format!("{{ let {t} = {v}; {r} }}")
    }

    /// `x op y` on two evaluated temps: the operator's own `rt` function, picked here.
    fn binop(&self, op: BinOp, x: &str, y: &str) -> String {
        use BinOp::*;
        let f = match op {
            Add => return self.t(&format!("rt::add(cx, {x}, {y})")),
            StrictEq => return format!("rt::seq(&{x}, &{y})"),
            StrictNe => return format!("rt::sne(&{x}, &{y})"),
            Sub => "sub",
            Mul => "mul",
            Div => "div",
            Rem => "rem",
            Exp => "exp",
            Eq => "leq",
            Ne => "lne",
            Lt => "lt",
            Gt => "gt",
            Le => "le",
            Ge => "ge",
            BitAnd => "band",
            BitOr => "bor",
            BitXor => "bxor",
            Shl => "shl",
            Shr => "shr",
            UShr => "ushr",
            In => "in_op",
            InstanceOf => "instance_of",
        };
        self.t(&format!("rt::{f}(cx, &{x}, &{y})"))
    }

    fn assign(&mut self, op: AssignOp, p: &Pat, rhs: &Expr) -> String {
        let v = self.tmp();
        let (pre, lr) = self.target(p);
        match op {
            AssignOp::Assign => {
                let r = self.expr(rhs);
                let b = self.target_put(p, &lr, &format!("{v}.clone()"));
                format!("{{ {pre}let {v} = {r}; {b} {v} }}")
            }
            AssignOp::Logical(l) => {
                let cur = self.target_get(p, &lr);
                let c = self.tmp();
                let keep = match l {
                    LogicalOp::Or => format!("{c}.truthy()"),
                    LogicalOp::And => format!("!{c}.truthy()"),
                    LogicalOp::Nullish => format!("!rt::nullish(&{c})"),
                };
                let r = self.expr(rhs);
                let b = self.target_put(p, &lr, &format!("{v}.clone()"));
                format!(
                    "{{ {pre}let {c} = {cur}; if {keep} {{ {c} }} else {{ let {v} = {r}; {b} {v} }} }}"
                )
            }
            AssignOp::Bin(bop) => {
                let cur = self.target_get(p, &lr);
                let (c, r) = (self.tmp(), self.tmp());
                let rv = self.expr(rhs);
                let res = self.binop(bop, &c, &r);
                let b = self.target_put(p, &lr, &format!("{v}.clone()"));
                format!("{{ {pre}let {c} = {cur}; let {r} = {rv}; let {v} = {res}; {b} {v} }}")
            }
        }
    }

    /// Code (ending in a space) evaluating the base (and key) of a member assignment target
    /// into temps, and those temps: JS evaluates them once and before the right-hand side.
    fn target(&mut self, p: &Pat) -> (String, Option<(String, String)>) {
        let Pat::Member(e) = p.shown() else {
            return (String::new(), None);
        };
        match &**e {
            Expr::Member(o, n, _) if matches!(**o, Expr::Super) => {
                self.super_target(&js_str(n.as_str()))
            }
            Expr::Index(o, k, _) if matches!(**o, Expr::Super) => {
                let kv = self.expr(k);
                self.super_target(&kv)
            }
            // `Target::This` of the interpreter: `this` is read when the target is.
            Expr::Member(o, _, _) if self.unbound_this_of(o).is_some() => {
                (String::new(), Some((String::new(), String::new())))
            }
            Expr::Member(o, n, _) if qml::valuetypes::is_color_field(n) => self.color_target(o),
            Expr::Member(o, _, _) => {
                let ov = self.expr(o);
                let t = self.tmp();
                (format!("let {t} = {ov}; "), Some((t, String::new())))
            }
            Expr::Index(o, k, _) => {
                let ov = self.expr(o);
                let kv = self.expr(k);
                let (t, tk) = (self.tmp(), self.tmp());
                (
                    format!("let {t} = {ov}; let mut {tk} = {kv}; "),
                    Some((t, tk)),
                )
            }
            _ => (String::new(), None),
        }
    }

    /// [`Self::target`] of a member whose name a color has (`o.r = v`): the base temp, and in
    /// place of a key the code that stores the changed color `COLOR` where `o` was read, used
    /// when the base turns out to be a color (a value, which QML writes back).
    fn color_target(&mut self, o: &Expr) -> (String, Option<(String, String)>) {
        let t = self.tmp();
        match o {
            Expr::Member(b, p, _) if !matches!(**b, Expr::Super) => {
                let bv = self.expr(b);
                let tb = self.tmp();
                let get = self.t(&format!(
                    "rt::member(cx, &{tb}, {0}, sym!({0}))",
                    lit_str(p.as_str())
                ));
                let put = self.t(&format!(
                    "rt::{}(cx, &{tb}, sym!({}), {COLOR})",
                    self.put_member_fn(),
                    lit_str(p.as_str())
                ));
                (
                    format!("let {tb} = {bv}; let {t} = {get}; "),
                    Some((t, format!("{put};"))),
                )
            }
            Expr::Index(b, k, _) if !matches!(**b, Expr::Super) => {
                let bv = self.expr(b);
                let kv = self.expr(k);
                let (tb, tk) = (self.tmp(), self.tmp());
                let conv = self.t(&format!("cx.convert_key(&{tb}, &mut {tk})"));
                let get = self.t(&format!("cx.get_key(&{tb}, &{tk})"));
                let put = self.t(&format!("cx.{}_key(&{tb}, &{tk}, {COLOR})", self.put_fn()));
                (
                    format!("let {tb} = {bv}; let mut {tk} = {kv}; {conv}; let {t} = {get}; "),
                    Some((t, format!("{put};"))),
                )
            }
            Expr::Ident(n) | Expr::Local(_, _, n) => {
                let ov = self.expr(o);
                let put = self.write_name(n, COLOR);
                (format!("let {t} = {ov}; "), Some((t, put)))
            }
            _ => {
                let ov = self.expr(o);
                (
                    format!("let {t} = {ov}; "),
                    Some((t, format!("let _ = {COLOR};"))),
                )
            }
        }
    }

    /// A Super Reference target (MakeSuperPropertyReference, ECMA-262 §13.3.7.1, §13.3.7.3):
    /// GetThisBinding, key value `kv`, then GetSuperBase, into an `rt::SuperRef` and a key temp.
    fn super_target(&mut self, kv: &str) -> (String, Option<(String, String)>) {
        let this = self.super_this();
        let h = self.read_name(HOME);
        let (tt, tk, t) = (self.tmp(), self.tmp(), self.tmp());
        (
            format!(
                "let {tt} = {this}; let mut {tk}: Value = {kv}; let {t} = rt::super_ref(cx, &{h}, {tt}); "
            ),
            Some((t, tk)),
        )
    }

    /// Current value of target `p` (temps from [`Self::target`]).
    fn target_get(&mut self, p: &Pat, lr: &Option<(String, String)>) -> String {
        let Some((t, tk)) = lr else {
            return self.read_pat(p);
        };
        let r = match p.shown() {
            Pat::Member(e) => match &**e {
                Expr::Member(o, ..) | Expr::Index(o, ..) if matches!(**o, Expr::Super) => {
                    let c = self.t(&format!("cx.convert_key(&{t}.base, &mut {tk})"));
                    let g = self.t(&format!("rt::super_ref_get(cx, &{t}, &{tk})"));
                    format!("{c}; {g}")
                }
                Expr::Member(o, n, _) if self.unbound_this_of(o).is_some() => {
                    let (th, t) = (self.this_binding(), self.tmp());
                    let g = self.t(&format!(
                        "rt::member(cx, &{t}, {0}, sym!({0}))",
                        lit_str(n.as_str())
                    ));
                    format!("let {t} = {th}; {g}")
                }
                Expr::Member(_, n, _) => self.t(&format!(
                    "rt::member(cx, &{t}, {0}, sym!({0}))",
                    lit_str(n.as_str())
                )),
                _ => {
                    let c = self.t(&format!("cx.convert_key(&{t}, &mut {tk})"));
                    let g = self.t(&format!("cx.get_key(&{t}, &{tk})"));
                    format!("{c}; {g}")
                }
            },
            _ => unreachable!(),
        };
        format!("{{ {r} }}")
    }

    /// Code (ending in `;` or `}`) storing value expression `v` into target `p`.
    fn target_put(&mut self, p: &Pat, lr: &Option<(String, String)>, v: &str) -> String {
        let Some((t, tk)) = lr else {
            return self.bind_code(p, v, false);
        };
        let Pat::Member(e) = p.shown() else {
            unreachable!()
        };
        let tv = self.tmp();
        match &**e {
            Expr::Member(o, ..) | Expr::Index(o, ..) if matches!(**o, Expr::Super) => {
                let strict = if self.strict_region {
                    "true"
                } else {
                    "cx.strict"
                };
                let put = self.t(&format!("rt::super_put(cx, &{t}, &{tk}, {tv}, {strict})"));
                format!("{{ let {tv}: Value = {v}; {put}; }}")
            }
            Expr::Member(o, n, _) => {
                // QV4's store on the `this` that is unbound now is a TypeError.
                let (t, this) = match self.unbound_this_of(o) {
                    Some(raw) => {
                        let t = self.tmp();
                        let chk = self.t(&format!("rt::unbound_store(cx, &{t})"));
                        (t.clone(), format!("let {t} = {raw}; {chk}; "))
                    }
                    None => (t.clone(), String::new()),
                };
                let put = self.t(&format!(
                    "rt::{}(cx, &{t}, sym!({}), {tv})",
                    self.put_member_fn(),
                    lit_str(n.as_str())
                ));
                let (len, notify) = self.notify_owner(n, &t);
                if !tk.is_empty() {
                    let (c, name) = (self.tmp(), lit_str(n.as_str()));
                    let changed = self.t(&format!("jsrt::color_put(cx, &{t}, {name}, &{tv})"));
                    let store = tk.replace(COLOR, &c);
                    return format!(
                        "{{ let {tv}: Value = {v}; if let Some({c}) = {changed} {{ {store} }} \
                         else {{ {len}{put}; {notify} }} }}"
                    );
                }
                format!("{{ let {tv}: Value = {v}; {this}{len}{put}; {notify} }}")
            }
            Expr::Index(o, _, _) => {
                let put = self.t(&format!("cx.{}_key(&{t}, &{tk}, {tv})", self.put_fn()));
                let wb = self.write_back(o, t);
                let put = self.color_key_put(o, t, tk, &tv, format!("{put}; {wb}"));
                format!("{{ let {tv}: Value = {v}; {put} }}")
            }
            _ => unreachable!(),
        }
    }

    /// `put`, the code of `t[tk] = tv` where `t` holds the value of `o`, unless `t` is a color
    /// and `tk` names one of its fields: the changed color is then stored where `o` was read.
    /// `o` is not evaluated again; the base of a member is, when reading it has no effect.
    fn color_key_put(&mut self, o: &Expr, t: &str, tk: &str, tv: &str, put: String) -> String {
        let c = self.tmp();
        let store = match o {
            Expr::Member(b, p, _)
                if matches!(**b, Expr::Ident(_) | Expr::Local(..) | Expr::This) =>
            {
                let bv = self.expr(b);
                let tb = self.tmp();
                let put = self.t(&format!(
                    "rt::{}(cx, &{tb}, sym!({}), {c})",
                    self.put_member_fn(),
                    lit_str(p.as_str())
                ));
                format!("let {tb} = {bv}; {put};")
            }
            Expr::Ident(n) | Expr::Local(_, _, n) => self.write_name(n, &c),
            _ => return put,
        };
        let changed = self.t(&format!("jsrt::color_put_key(cx, &{t}, &{tk}, &{tv})"));
        format!("if let Some({c}) = {changed} {{ {store} }} else {{ {put} }}")
    }

    /// Current value of a compound-assignment / update target.
    fn read_pat(&mut self, p: &Pat) -> String {
        match p.shown() {
            Pat::Id(n) | Pat::Local(_, _, n) => self.read_name(n),
            Pat::Const(p, _) => self.read_pat(p),
            Pat::Member(e) => self.expr(e),
            _ => {
                let r = self.t(
                    "cx.throw::<Value>(\"SyntaxError\", \"invalid compound assignment target\")",
                );
                format!("{{ {r} }}")
            }
        }
    }

    /// Statement: bind `p` to the value expression `v` (declaration or assignment).
    pub fn bind(&mut self, p: &Pat, v: &str, decl: bool) {
        let s = self.bind_code(p, v, decl);
        self.line(&s);
    }

    /// Code binding `p` to `v`; `v` is evaluated first, once.
    pub fn bind_code(&mut self, p: &Pat, v: &str, decl: bool) -> String {
        let tv = self.tmp();
        let mut s = format!("{{ let {tv}: Value = {v}; ");
        match p.shown() {
            Pat::Id(n) | Pat::Local(_, _, n) if decl && self.find(n).is_some() => {
                // InitializeBinding: the declaration ends the binding's temporal dead zone.
                let st = self.find(n).unwrap();
                s.push_str(&self.write(&st, &tv));
                self.initialized(n);
            }
            Pat::Id(n) | Pat::Local(_, _, n) => s.push_str(&self.write_name(n, &tv)),
            Pat::Dynamic(..) => unreachable!("Pat::shown gives no Pat::Dynamic"),
            Pat::Const(p, throws) => {
                let (Pat::Id(n) | Pat::Local(_, _, n)) = &**p else {
                    unreachable!("Pat::Const wraps a name")
                };
                if *throws {
                    // SetMutableBinding (ECMA-262 §9.1.1.1.5) checks that the binding is
                    // initialized (ReferenceError) before that it is mutable (TypeError).
                    if let Some(st) = self.find(n)
                        && self.maybe_uninit(n)
                    {
                        let c = self.t(&format!(
                            "rt::assignable(cx, &{}, {})",
                            self.read(&st),
                            lit_str(n)
                        ));
                        let _ = write!(s, "{c};");
                    }
                    let r = self.t("cx.const_write::<()>()");
                    let _ = write!(s, "{r};");
                } else {
                    let _ = write!(s, "let _ = {tv};");
                }
            }
            Pat::Init(e) => match &**e {
                Expr::Member(o, n, _) => {
                    let ov = self.expr(o);
                    let t = self.tmp();
                    let _ = write!(
                        s,
                        "let {t} = {ov}; jsrt::init_member(&{t}, {}, {tv});",
                        lit_str(n.as_str())
                    );
                }
                _ => unreachable!("Pat::Init initializes a scope-object member"),
            },
            Pat::Member(e) => match &**e {
                Expr::Member(o, ..) | Expr::Index(o, ..) if matches!(**o, Expr::Super) => {
                    let (pre, lr) = self.target(p);
                    let put = self.target_put(p, &lr, &tv);
                    let _ = write!(s, "{pre}{put}");
                }
                Expr::Member(_, n, _) if qml::valuetypes::is_color_field(n) => {
                    let (pre, lr) = self.target(p);
                    let put = self.target_put(p, &lr, &tv);
                    let _ = write!(s, "{pre}{put}");
                }
                Expr::Member(o, n, _) => {
                    if let Some(raw) = self.unbound_this_of(o) {
                        let e = self.t(&format!("rt::unbound_store(cx, &{raw})"));
                        let _ = write!(s, "{e};");
                    }
                    let ov = self.expr(o);
                    let t = self.tmp();
                    let put = self.t(&format!(
                        "rt::{}(cx, &{t}, sym!({}), {tv})",
                        self.put_member_fn(),
                        lit_str(n.as_str())
                    ));
                    let (len, notify) = self.notify_owner(n, &t);
                    let _ = write!(s, "let {t} = {ov}; {len}{put}; {notify}");
                }
                Expr::Index(o, k, _) => {
                    let ov = self.expr(o);
                    let t = self.tmp();
                    let kv = self.expr(k);
                    let tk = self.tmp();
                    let put = self.t(&format!("cx.{}_key(&{t}, &{tk}, {tv})", self.put_fn()));
                    let wb = self.write_back(o, &t);
                    let put = self.color_key_put(o, &t, &tk, &tv, format!("{put}; {wb}"));
                    let _ = write!(s, "let {t} = {ov}; let {tk} = {kv}; {put}");
                }
                _ => {
                    let r =
                        self.t("cx.throw::<()>(\"SyntaxError\", \"invalid assignment target\")");
                    let _ = write!(s, "{r};");
                }
            },
            Pat::Default(inner, d) => {
                let dv = self.expr(d);
                let t = self.tmp();
                let _ = write!(
                    s,
                    "let {t} = if matches!({tv}, Value::Undefined) {{ {dv} }} else {{ {tv} }}; "
                );
                s.push_str(&self.bind_code(inner, &t, decl));
            }
            Pat::Array(items, rest) => {
                // IteratorBindingInitialization (ECMA-262 §8.6.3): elements bind as the
                // iterator steps; it is closed after, and on a throw while binding.
                let (src, r) = (self.tmp(), self.tmp());
                let b = self.g.id("b");
                let gi = self.t(&format!("cx.get_iterator(&{tv})"));
                let _ = write!(
                    s,
                    "let mut {src} = {gi}; let {r}: Result<(), Ex> = '{b}: {{ "
                );
                let sink = std::mem::replace(&mut self.sink, Sink::Res(b.clone()));
                for p in items {
                    let step = self.t(&format!("cx.iter_step(&mut {src})"));
                    match p {
                        Some(p) => s.push_str(&self.elem_code(
                            p,
                            &format!("{step}.unwrap_or_default()"),
                            decl,
                        )),
                        None => {
                            let _ = write!(s, "let _ = {step}; ");
                        }
                    }
                }
                if let Some(rp) = rest {
                    let all = self.t(&format!("cx.iter_rest(&mut {src})"));
                    s.push_str(&self.elem_code(rp, &format!("Value::array({all})"), decl));
                }
                self.sink = sink;
                let close = self.t(&format!("cx.iter_close(&mut {src})"));
                let rethrow = self.err_code("e");
                let _ = write!(
                    s,
                    "Ok(()) }}; match {r} {{ Ok(()) => {{ {close}; }} Err(e) => {{ let e = cx.iter_close_throw(&mut {src}, e); {rethrow}; }} }} "
                );
            }
            Pat::Object(props, rest) => {
                let rc = self.t(&format!("rt::require_object_coercible(cx, &{tv})"));
                let _ = write!(s, "{rc}; ");
                // Static keys are literals (`used` a static slice for `...rest`); only computed
                // keys are strings made at run time.
                let computed = props.iter().any(|(k, _)| matches!(k, Key::Computed(_)));
                let used = self.tmp();
                if computed && rest.is_some() {
                    let _ = write!(
                        s,
                        "let mut {used}: Vec<jsrt::PropKey> = Vec::with_capacity({}); ",
                        props.len()
                    );
                }
                for (k, p) in props {
                    match k {
                        Key::Name(n) => {
                            let g = self.t(&format!("cx.get(&{tv}, {})", lit_str(n)));
                            s.push_str(&self.elem_code(p, &g, decl));
                            if computed && rest.is_some() {
                                let _ = write!(
                                    s,
                                    "{used}.push(jsrt::PropKey::Str({}.to_string())); ",
                                    lit_str(n)
                                );
                            }
                        }
                        Key::Computed(e) => {
                            let kv = self.expr(e);
                            let name = self.tmp();
                            let pk = self.t(&format!("cx.to_property_key(&{name})"));
                            let g = self.t(&format!("cx.get_pk(&{tv}, &{name})"));
                            let _ = write!(s, "let {name} = {kv}; let {name} = {pk}; ");
                            s.push_str(&self.elem_code(p, &g, decl));
                            if rest.is_some() {
                                let _ = write!(s, "{used}.push({name}); ");
                            }
                        }
                    }
                }
                if let Some(r) = rest {
                    let used = if computed {
                        format!("&{used}")
                    } else {
                        let keys: Vec<String> = props
                            .iter()
                            .map(|(k, _)| match k {
                                Key::Name(n) => lit_str(n),
                                Key::Computed(_) => unreachable!(),
                            })
                            .collect();
                        format!("&[{}] as &[&str]", keys.join(", "))
                    };
                    let rv = self.t(&format!(
                        "rt::object_rest{}(cx, &{tv}, {used})",
                        if computed { "_pk" } else { "" }
                    ));
                    s.push_str(&self.elem_code(r, &rv, decl));
                }
            }
        }
        s.push_str(" }");
        s
    }

    /// Code binding one element, property or rest `p` of a destructuring pattern to `v`: in
    /// an assignment a member target's reference is evaluated first, then `v`, the default
    /// and PutValue (ECMA-262 §13.15.5.5 AssignmentElement, §13.15.5.6
    /// KeyedDestructuringAssignmentEvaluation, AssignmentRestElement/RestProperty).
    fn elem_code(&mut self, p: &Pat, v: &str, decl: bool) -> String {
        let (q, d) = match p {
            Pat::Default(q, d) => (&**q, Some(d)),
            q => (q, None),
        };
        if decl {
            return self.bind_code(p, v, decl);
        }
        let (pre, lr) = self.target(q);
        if lr.is_none() {
            return self.bind_code(p, v, decl);
        }
        let tv = self.tmp();
        let mut s = format!("{{ {pre}let {tv}: Value = {v}; ");
        if let Some(d) = d {
            let dv = self.expr(d);
            let _ = write!(
                s,
                "let {tv} = if matches!({tv}, Value::Undefined) {{ {dv} }} else {{ {tv} }}; "
            );
        }
        s.push_str(&self.target_put(q, &lr, &tv));
        s.push_str(" }");
        s
    }

    fn class_ctx_for_arrow(&self) -> Option<ClassCtx> {
        self.class.clone().map(|mut c| {
            c.mode = CtorMode::Inner;
            c
        })
    }

    /// A function expression / declaration value closing over the current env.
    pub fn closure(&mut self, f: &Func) -> String {
        let id = self.g.id("f");
        let name = format!("{}__{id}", self.g.base);
        let meta = format!("M_{}", name.to_uppercase());
        let class = if f.is_arrow() {
            self.class_ctx_for_arrow()
        } else {
            None
        };
        emit_func_item(
            self.g,
            f,
            self.scopes.clone(),
            self.depth,
            &name,
            &meta,
            class,
        );
        format!("jsrt::closure({name}, &{}, &{meta})", self.env)
    }

    /// Code (ending in `;`) running the instance field initializers on `this` (`tt`).
    pub(crate) fn fields_call(&mut self, tt: &str) -> String {
        let Some(c) = self.class.clone() else {
            return String::new();
        };
        let hops = self.depth - c.depth;
        let r = self.t(&format!(
            "jsrt::call_compiled(cx, {}, &rt::up(&{}, {hops}), &{}, {tt}.clone(), &[])",
            c.fields, self.env, c.fmeta
        ));
        format!("{r};")
    }

    /// Statement: field initializers on this function's `this`.
    pub fn call_fields(&mut self) {
        let th = self.read_name(THIS);
        let t = self.tmp();
        let f = self.fields_call(&t);
        self.line(&format!("{{ let {t} = {th}; {f} }}"));
    }

    /// A class value: constructor + methods closing over a class scope frame
    /// (`name`, `super`).
    pub fn class_value(&mut self, c: &Class) -> String {
        // All parts of a class are strict mode code (ECMA-262 §11.2.2).
        let region = std::mem::replace(&mut self.strict_region, true);
        let v = self.class_definition(c);
        self.strict_region = region;
        v
    }

    /// Evaluates the computed key of field `i` into its class-scope binding.
    fn field_key_code(&mut self, c: &Class, i: usize, at: &KeyScope) {
        let k = &c.fields[i].0;
        if matches!(k, Key::Computed(_)) {
            let (key, _) = self.in_scope(at, |s| s.key_code(k));
            let w = self.write_name(&field_key(i), &format!("{key}.to_value()"));
            self.line(&w);
        }
    }

    /// Lowers with the scope chain `at`: class element keys are evaluated in the class scope
    /// (ECMA-262 §15.7.14), outside the [[HomeObject]] frames of the class's methods, so a
    /// `super` in a key is the enclosing method's.
    fn in_scope<T>(&mut self, at: &KeyScope, f: impl FnOnce(&mut Self) -> T) -> T {
        let scopes = std::mem::replace(&mut self.scopes, at.0.clone());
        let env = std::mem::replace(&mut self.env, at.1.clone());
        let depth = std::mem::replace(&mut self.depth, at.2);
        let r = f(self);
        (self.scopes, self.env, self.depth) = (scopes, env, depth);
        r
    }

    fn class_definition(&mut self, c: &Class) -> String {
        let saved_out = std::mem::take(&mut self.out);
        let pv = self.tmp();
        if let Some(e) = &c.extends {
            let v = self.expr(e);
            self.line(&format!("let {pv} = {v};"));
        } else {
            self.line(&format!("let {pv} = Value::Undefined;"));
        }
        let mut names: Vec<String> = c.name.iter().cloned().collect();
        names.push(SUPER.into());
        for (i, (k, _, _)) in c.fields.iter().enumerate() {
            if matches!(k, Key::Computed(_)) {
                names.push(field_key(i));
            }
        }
        for n in &names {
            self.cap.insert(n.clone());
        }
        let saved = self.open(&names, false);
        // The [[HomeObject]] of the constructor, instance methods and field initializers is
        // the prototype, in a scope of its own; that of static methods, static fields and
        // static blocks the class, in one inside it.
        let key_scope: KeyScope = (self.scopes.clone(), self.env.clone(), self.depth);
        self.cap.insert(HOME.into());
        let proto_saved = self.open(&[HOME.into()], false);
        let class_depth = self.depth;
        let scopes: Vec<Sc> = self.scopes.clone();
        let proto_env = self.env.clone();
        let id = self.g.id("k");
        let base = format!("{}__{id}", self.g.base);
        let fields_name = format!("{base}_fields");
        let fields_meta = format!("M_{}", fields_name.to_uppercase());
        // Instance fields as DefineField steps on `this` in their own function.
        let body = field_stores(c, false);
        let ff = Func {
            name: None,
            inferred: None,
            params: Vec::new(),
            rest: None,
            body,
            kind: qml::ir::FnKind::Method,
            coroutine: None,
            strict: true,
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
        emit_func_item(
            self.g,
            &ff,
            scopes.clone(),
            class_depth,
            &fields_name,
            &fields_meta,
            None,
        );
        let mode = match (&c.ctor, &c.extends) {
            (_, None) => CtorMode::FieldsFirst,
            (Some(_), Some(_)) => CtorMode::AfterSuper,
            (None, Some(_)) => CtorMode::Implicit,
        };
        let ctor = c.ctor.clone().unwrap_or_else(|| Func {
            name: c.name.clone(),
            ..ff.clone()
        });
        let ctor = Func {
            name: c.name.clone(),
            inferred: c.inferred.clone(),
            kind: match c.extends {
                Some(_) => qml::ir::FnKind::DerivedConstructor,
                None => qml::ir::FnKind::ClassConstructor,
            },
            body: if c.ctor.is_some() {
                ctor.body
            } else {
                Vec::new()
            },
            ..ctor
        };
        let ctor_name = format!("{base}_ctor");
        let ctor_meta = format!("M_{}", ctor_name.to_uppercase());
        let cctx = ClassCtx {
            depth: class_depth,
            fields: fields_name,
            fmeta: fields_meta,
            mode,
            derived: mode == CtorMode::AfterSuper,
        };
        emit_func_item(
            self.g,
            &ctor,
            scopes.clone(),
            class_depth,
            &ctor_name,
            &ctor_meta,
            Some(cctx),
        );
        let cv = self.tmp();
        self.line(&format!(
            "let {cv} = jsrt::closure({ctor_name}, &{proto_env}, &{ctor_meta});"
        ));
        if let Some(n) = &c.name {
            let w = self.write_name(n, &format!("{cv}.clone()"));
            self.line(&w);
        }
        let w = self.write_name(SUPER, &format!("{pv}.clone()"));
        self.line(&w);
        let proto = self.tmp();
        let parent = if c.extends.is_some() {
            format!("Some(&{pv})")
        } else {
            "None".into()
        };
        self.line(&format!(
            "let {proto} = rt::class_proto(cx, &{cv}, {parent})?;"
        ));
        let w = self.write_name(HOME, &format!("{proto}.clone()"));
        self.line(&w);
        let static_saved = self.open(&[HOME.into()], false);
        let static_depth = self.depth;
        let static_scopes: Vec<Sc> = self.scopes.clone();
        let static_env = self.env.clone();
        let w = self.write_name(HOME, &format!("{cv}.clone()"));
        self.line(&w);
        let mut next_field = 0;
        for (j, m) in c.methods.iter().enumerate() {
            while c
                .field_after
                .get(next_field)
                .is_some_and(|&a| a as usize <= j)
            {
                self.field_key_code(c, next_field, &key_scope);
                next_field += 1;
            }
            let (key, pk) = self.in_scope(&key_scope, |s| s.key_code(&m.key));
            let mname = format!("{base}_{}", self.g.id("m"));
            let mmeta = format!("M_{}", mname.to_uppercase());
            let (target, mscopes, mdepth, menv) = match m.is_static {
                true => (&cv, &static_scopes, static_depth, &static_env),
                false => (&proto, &scopes, class_depth, &proto_env),
            };
            emit_func_item(
                self.g,
                &m.func,
                mscopes.clone(),
                mdepth,
                &mname,
                &mmeta,
                None,
            );
            let f = self.tmp();
            self.line(&format!(
                "let {f} = jsrt::closure({mname}, &{menv}, &{mmeta});"
            ));
            if matches!(m.key, Key::Computed(_)) && m.func.display_name().is_empty() {
                let prefix = match m.kind {
                    "get" => "get ",
                    "set" => "set ",
                    _ => "",
                };
                self.line(&format!(
                    "jsrt::name_function{pk}(&{f}, {}, &{key});",
                    lit_str(prefix)
                ));
            }
            self.line(&format!(
                "rt::define_method{pk}(&{target}, &{key}, {}, {f});",
                lit_str(m.kind)
            ));
        }
        for i in next_field..c.fields.len() {
            self.field_key_code(c, i, &key_scope);
        }
        if c.fields.iter().any(|f| f.2) || !c.blocks.is_empty() {
            // Static field initializers and static blocks run in order as a strict method of
            // the class on the class.
            let sname = format!("{base}_statics");
            let smeta = format!("M_{}", sname.to_uppercase());
            let sf = Func {
                body: static_elements(c),
                ..ff.clone()
            };
            emit_func_item(
                self.g,
                &sf,
                static_scopes,
                static_depth,
                &sname,
                &smeta,
                None,
            );
            let r = self.t(&format!(
                "jsrt::call_compiled(cx, {sname}, &{static_env}, &{smeta}, {cv}.clone(), &[])"
            ));
            self.line(&format!("{r};"));
        }
        self.close(static_saved);
        self.close(proto_saved);
        self.close(saved);
        let body = std::mem::replace(&mut self.out, saved_out);
        format!("{{ {body} {cv} }}")
    }

    /// A class element key: a string literal, or (with the `_pk` suffix of the functions
    /// taking it) a computed [`PropKey`](qml::eval::PropKey).
    fn key_code(&mut self, k: &Key) -> (String, &'static str) {
        match k {
            Key::Name(n) => (lit_str(n), ""),
            Key::Computed(e) => {
                let t = self.tmp();
                let v = self.expr(e);
                let pk = self.t(&format!("cx.to_property_key(&{t})"));
                self.line(&format!("let {t} = {v}; let {t} = {pk};"));
                (t, "_pk")
            }
        }
    }
}

/// A free name no JS global can shadow (none of [`qml::builtins::GLOBALS`] or
/// [`qml::builtins::VALUE_GLOBALS`]): it skips the scope-frame walk (`rt::qml`).
pub(crate) fn qml_only(n: &str) -> bool {
    use qml::builtins::{GLOBALS, VALUE_GLOBALS};
    !GLOBALS.contains(&n) && !VALUE_GLOBALS.contains(&n)
}

/// The `this.k = v` stores of a class's static or instance fields.
fn field_stores(c: &Class, statics: bool) -> Vec<Stmt> {
    c.fields
        .iter()
        .enumerate()
        .filter(|(_, f)| f.2 == statics)
        .map(|(i, _)| field_store(c, i))
        .collect()
}

/// A property reference callee: its call's `this` is the base object.
fn is_member_callee(e: &Expr) -> bool {
    match e {
        Expr::Member(o, ..) => !matches!(**o, Expr::Super),
        Expr::Index(..) => true,
        _ => false,
    }
}

fn field_store(c: &Class, i: usize) -> Stmt {
    let (k, v, _) = &c.fields[i];
    let key = match k {
        Key::Name(n) => Expr::Str(Rc::from(&**n)),
        Key::Computed(_) => Expr::Ident(field_key(i)),
    };
    let init = v.clone().unwrap_or(Expr::Ident("undefined".into()));
    let name = qml::ir::names_field(k, v.as_ref());
    Stmt::Expr(Expr::DefineField(Box::new(key), Box::new(init), name))
}

/// The static fields' stores and the static blocks of a class, in source order.
fn static_elements(c: &Class) -> Vec<Stmt> {
    let mut blocks = c.blocks.iter().peekable();
    let mut out = Vec::new();
    for (i, f) in c.fields.iter().enumerate() {
        while let Some((_, b)) = blocks.next_if(|(at, _)| *at as usize <= i) {
            out.push(Stmt::Expr(b.clone()));
        }
        if f.2 {
            out.push(field_store(c, i));
        }
    }
    out.extend(blocks.map(|(_, b)| Stmt::Expr(b.clone())));
    out
}
