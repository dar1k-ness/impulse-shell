//! Second oracle (`--v4 PYTHON`): Qt's own JS engine. The differential run proves the compiled
//! code equal to the interpreter, not either of them equal to Qt, so a semantic slip shared
//! by both backends passes it. Here each sampled the original binding is replayed as a closed program:
//! its source text verbatim, with every free name it reads bound to the value the reference
//! runtime holds (a JSON-like literal), and its result canonicalised in JS. The program runs
//! in the native interpreter and in QV4 (PySide6's QQmlEngine, the Qt that runs `qs -c rusty-impulse`);
//! any difference is an engine divergence (McKeeman, "Differential Testing for Software").
use crate::{Rc_ptr, St};
use qml::eval::{Ex, Frame, Interp, Scope};
use qml::ir::{Expr, Pat, Stmt};
use qml::value::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::rc::Rc;

/// Distinct programs kept per binding key.
const PER_KEY: usize = 3;
/// A key whose inputs failed to serialise this often is not tried again.
const MAX_MISSES: usize = 4;
/// Programs longer than this (a binding over a whole model) are skipped.
const MAX_PROGRAM: usize = 16 * 1024;

/// ECMAScript globals that both engines provide and that are pure.
const GLOBALS: &[&str] = &[
    "Math",
    "JSON",
    "Number",
    "String",
    "Boolean",
    "Array",
    "Object",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "NaN",
    "Infinity",
    "undefined",
    "encodeURIComponent",
    "decodeURIComponent",
    "encodeURI",
    "decodeURI",
    "RegExp",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "Map",
    "Set",
    "Symbol",
    "qsTr",
];
/// Globals whose results depend on the clock, randomness or the console.
const VOLATILE: &[&str] = &["Date", "console", "print", "Qt", "eval", "arguments"];

/// The canonical result: the same JS on both engines, so its own semantics are under test too.
const CANON: &str = r#"function __canon(v, d) {
    d = d || 0;
    if (v === undefined) return "undefined";
    if (v === null) return "null";
    var t = typeof v;
    if (t === "number") return v === 0 && 1 / v < 0 ? "-0" : String(v);
    if (t === "string") return JSON.stringify(v);
    if (t === "boolean") return String(v);
    if (t === "function") return "<function>";
    if (d > 4) return "{..}";
    if (Array.isArray(v)) {
        var a = [];
        for (var i = 0; i < v.length; i++) a.push(__canon(v[i], d + 1));
        return "[" + a.join(",") + "]";
    }
    var ks = Object.keys(v), o = [];
    for (var j = 0; j < ks.length; j++) o.push(JSON.stringify(ks[j]) + ":" + __canon(v[ks[j]], d + 1));
    return "{" + o.join(",") + "}";
}"#;

/// The QV4 side, written next to the report and run with `--v4 PYTHON`.
const ORACLE: &str = include_str!("v4oracle.py");
/// What the oracle imports first, written next to it: the Qt that answers is the system's.
const SYSTEM_QT: &str = include_str!("../../../tools/testenv/system_qt.py");

struct Sample {
    keys: String,
    native: String,
}

/// Per reference worker: what its bindings were sampled into.
#[derive(Default)]
pub struct Sink {
    /// Program (a JS expression) -> its first site and the native interpreter's result.
    programs: BTreeMap<String, Sample>,
    per_key: HashMap<String, usize>,
    misses: HashMap<String, usize>,
    /// Binding key -> why its source cannot be replayed (first reason seen).
    skipped: BTreeMap<String, String>,
    /// Binding keys live in the reference while sampled.
    seen: HashSet<String>,
}

/// Names declared anywhere in the body: never free, wherever they are used.
fn declared(body: &[Stmt], out: &mut HashSet<String>) {
    fn pat(p: &Pat, out: &mut HashSet<String>) {
        match p {
            Pat::Dynamic(_, _, p) => pat(p, out),
            Pat::Id(n) | Pat::Local(_, _, n) => {
                out.insert(n.clone());
            }
            Pat::Object(v, r) => {
                v.iter().for_each(|(_, p)| pat(p, out));
                if let Some(r) = r {
                    pat(r, out)
                }
            }
            Pat::Array(v, r) => {
                v.iter().flatten().for_each(|p| pat(p, out));
                if let Some(r) = r {
                    pat(r, out)
                }
            }
            Pat::Default(p, x) => {
                pat(p, out);
                expr(x, out)
            }
            Pat::Member(x) | Pat::Init(x) => expr(x, out),
            Pat::Const(p, _) => pat(p, out),
        }
    }
    fn func(f: &qml::ir::Func, out: &mut HashSet<String>) {
        if let Some(n) = &f.name {
            out.insert(n.clone());
        }
        f.params
            .iter()
            .chain(f.rest.iter())
            .for_each(|p| pat(p, out));
        declared(&f.body, out);
    }
    fn expr(x: &Expr, out: &mut HashSet<String>) {
        match x {
            Expr::Func(f) => func(f, out),
            Expr::Template(_, v) | Expr::Array(v) | Expr::Seq(v) => {
                v.iter().for_each(|x| expr(x, out))
            }
            Expr::Yield(a, _) => a.iter().for_each(|x| expr(x, out)),
            Expr::Member(a, ..) | Expr::Chain(a) | Expr::Unary(_, a) | Expr::Spread(a) => {
                expr(a, out)
            }
            Expr::Index(a, b, _) | Expr::Binary(_, a, b) | Expr::Logical(_, a, b) => {
                expr(a, out);
                expr(b, out)
            }
            Expr::Call(a, v, _) | Expr::New(a, v) => {
                expr(a, out);
                v.iter().for_each(|x| expr(x, out))
            }
            Expr::Cond(a, b, c) => [a, b, c].iter().for_each(|x| expr(x, out)),
            // An assignment target is not a declaration: a free one is rejected later.
            Expr::Assign(_, _, b) => expr(b, out),
            Expr::Object(v) => v.iter().for_each(|(k, x, _)| {
                if let qml::ir::Key::Computed(k) = k {
                    expr(k, out)
                }
                expr(x, out)
            }),
            _ => {}
        }
    }
    for s in body {
        match s {
            Stmt::Expr(x) | Stmt::Throw(x) | Stmt::Return(Some(x)) => expr(x, out),
            Stmt::Var(_, d) => d.iter().for_each(|(p, x)| {
                pat(p, out);
                if let Some(x) = x {
                    expr(x, out)
                }
            }),
            Stmt::Func(f) => func(f, out),
            Stmt::If(c, a, b) => {
                expr(c, out);
                declared(std::slice::from_ref(a), out);
                if let Some(b) = b {
                    declared(std::slice::from_ref(b), out)
                }
            }
            Stmt::Block(b) => declared(b, out),
            Stmt::Labeled(_, b) => declared(std::slice::from_ref(b), out),
            Stmt::For(i, t, u, b) => {
                if let Some(i) = i {
                    declared(std::slice::from_ref(i), out)
                }
                t.iter().chain(u.iter()).for_each(|x| expr(x, out));
                declared(std::slice::from_ref(b), out)
            }
            Stmt::ForOf(_, p, x, b) | Stmt::ForIn(_, p, x, b) => {
                pat(p, out);
                expr(x, out);
                declared(std::slice::from_ref(b), out)
            }
            Stmt::While(x, b) | Stmt::DoWhile(b, x) => {
                expr(x, out);
                declared(std::slice::from_ref(b), out)
            }
            Stmt::Switch(x, cs) => {
                expr(x, out);
                cs.iter().for_each(|(t, b)| {
                    if let Some(t) = t {
                        expr(t, out)
                    }
                    declared(b, out)
                })
            }
            Stmt::Try(a, h, f) => {
                declared(a, out);
                if let Some((p, h)) = h {
                    if let Some(p) = p {
                        pat(p, out)
                    }
                    declared(h, out)
                }
                if let Some(f) = f {
                    declared(f, out)
                }
            }
            Stmt::Class(c) => {
                out.insert(c.name.clone().unwrap_or_default());
            }
            _ => {}
        }
    }
}

/// The free inputs of a body: member paths from a free name, each read as a whole value.
#[derive(Default)]
struct Inputs {
    locals: HashSet<String>,
    /// Path -> the expression that reads it (keeps `?.`).
    reads: BTreeMap<Vec<String>, Expr>,
}

impl Inputs {
    /// `a.b.c` from a free name `a`.
    fn path(&self, x: &Expr) -> Option<Vec<String>> {
        match x {
            Expr::Ident(n) if !self.locals.contains(n) && !GLOBALS.contains(&n.as_str()) => {
                (!VOLATILE.contains(&n.as_str())).then(|| vec![n.clone()])
            }
            Expr::Member(o, m, _) => {
                let mut p = self.path(o)?;
                p.push(m.as_str().to_string());
                Some(p)
            }
            Expr::Chain(o) => self.path(o),
            _ => None,
        }
    }

    fn read(&mut self, x: &Expr) -> Result<bool, String> {
        match self.path(x) {
            Some(p) => {
                self.reads.entry(p).or_insert_with(|| x.clone());
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn pat(&mut self, p: &Pat) -> Result<(), String> {
        match p {
            Pat::Id(n) if !self.locals.contains(n) => Err("assigns a free name".into()),
            Pat::Dynamic(..) => Err("assigns a name a `with` or an `eval` can bind".into()),
            Pat::Member(_) | Pat::Init(_) => Err("assigns a member".into()),
            Pat::Object(v, r) => {
                for (_, p) in v {
                    self.pat(p)?
                }
                r.as_deref().map_or(Ok(()), |r| self.pat(r))
            }
            Pat::Array(v, r) => {
                for p in v.iter().flatten() {
                    self.pat(p)?
                }
                r.as_deref().map_or(Ok(()), |r| self.pat(r))
            }
            Pat::Default(p, x) => {
                self.pat(p)?;
                self.expr(x)
            }
            Pat::Const(p, _) => self.pat(p),
            _ => Ok(()),
        }
    }

    fn expr(&mut self, x: &Expr) -> Result<(), String> {
        match x {
            Expr::Ident(n) => {
                if VOLATILE.contains(&n.as_str()) && !self.locals.contains(n) {
                    return Err(format!("reads `{n}`"));
                }
                self.read(x)?;
                Ok(())
            }
            Expr::Member(o, m, _) => {
                if let Expr::Ident(g) = &**o
                    && g == "Math"
                    && m.as_str() == "random"
                    && !self.locals.contains(g)
                {
                    return Err("reads `Math.random`".into());
                }
                if self.read(x)? { Ok(()) } else { self.expr(o) }
            }
            Expr::Chain(o) => {
                if self.read(x)? {
                    Ok(())
                } else {
                    self.expr(o)
                }
            }
            Expr::Call(c, args, _) => {
                match &**c {
                    // A method on an input value: the receiver is the input.
                    Expr::Member(r, _, _) if self.path(r).is_some() => {
                        self.read(r)?;
                    }
                    Expr::Member(r, m, _) => {
                        if matches!(&**r, Expr::Ident(g) if g == "Math" && !self.locals.contains(g))
                            && m.as_str() == "random"
                        {
                            return Err("reads `Math.random`".into());
                        }
                        self.expr(r)?
                    }
                    c => self.expr(c)?,
                }
                args.iter().try_for_each(|a| self.expr(a))
            }
            Expr::New(c, args) => {
                self.expr(c)?;
                args.iter().try_for_each(|a| self.expr(a))
            }
            Expr::Index(a, b, _) => {
                if !self.read(a)? {
                    self.expr(a)?
                }
                self.expr(b)
            }
            Expr::This | Expr::Super => Err("reads `this`".into()),
            Expr::NewTarget => Err("reads `new.target`".into()),
            Expr::Class(_) => Err("defines a class".into()),
            Expr::Yield(..) => Err("yields".into()),
            Expr::Dynamic(..) => Err("reads a name a `with` or an `eval` can bind".into()),
            Expr::DirectEval(..) => Err("reads `eval`".into()),
            Expr::Unsupported(u) => Err(format!("unsupported `{u}`")),
            Expr::Template(_, v) | Expr::Array(v) | Expr::Seq(v) => {
                v.iter().try_for_each(|x| self.expr(x))
            }
            Expr::Unary(_, a) | Expr::Spread(a) => self.expr(a),
            Expr::Binary(_, a, b) | Expr::Logical(_, a, b) => {
                self.expr(a)?;
                self.expr(b)
            }
            Expr::Cond(a, b, c) => [a, b, c].into_iter().try_for_each(|x| self.expr(x)),
            Expr::Assign(_, p, b) => {
                self.pat(p)?;
                self.expr(b)
            }
            Expr::Update(_, _, p) => self.pat(p),
            Expr::Object(v) => v.iter().try_for_each(|(k, x, _)| {
                if let qml::ir::Key::Computed(k) = k {
                    self.expr(k)?
                }
                self.expr(x)
            }),
            Expr::Func(f) => self.func(f),
            _ => Ok(()),
        }
    }

    fn func(&mut self, f: &qml::ir::Func) -> Result<(), String> {
        match f.coroutine {
            Some(c) if !c.runs() => Err(format!("unsupported `{}`", c.name())),
            _ => self.body(&f.body),
        }
    }

    fn body(&mut self, b: &[Stmt]) -> Result<(), String> {
        b.iter().try_for_each(|s| self.stmt(s))
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        let one = |me: &mut Self, s: &Stmt| me.stmt(s);
        match s {
            Stmt::Expr(x) | Stmt::Throw(x) | Stmt::Return(Some(x)) => self.expr(x),
            Stmt::Var(_, d) => d.iter().try_for_each(|(p, x)| {
                self.pat(p)?;
                x.as_ref().map_or(Ok(()), |x| self.expr(x))
            }),
            Stmt::Func(f) => self.func(f),
            Stmt::If(c, a, b) => {
                self.expr(c)?;
                one(self, a)?;
                b.as_deref().map_or(Ok(()), |b| one(self, b))
            }
            Stmt::Block(b) => self.body(b),
            Stmt::Labeled(_, b) => one(self, b),
            Stmt::For(i, t, u, b) => {
                if let Some(i) = i {
                    one(self, i)?
                }
                t.iter().chain(u.iter()).try_for_each(|x| self.expr(x))?;
                one(self, b)
            }
            Stmt::ForOf(_, p, x, b) | Stmt::ForIn(_, p, x, b) => {
                self.pat(p)?;
                self.expr(x)?;
                one(self, b)
            }
            Stmt::While(x, b) | Stmt::DoWhile(b, x) => {
                self.expr(x)?;
                one(self, b)
            }
            Stmt::Switch(x, cs) => {
                self.expr(x)?;
                cs.iter().try_for_each(|(t, b)| {
                    t.as_ref().map_or(Ok(()), |t| self.expr(t))?;
                    self.body(b)
                })
            }
            Stmt::Try(a, h, f) => {
                self.body(a)?;
                if let Some((p, h)) = h {
                    if let Some(p) = p {
                        self.pat(p)?
                    }
                    self.body(h)?
                }
                f.as_deref().map_or(Ok(()), |f| self.body(f))
            }
            Stmt::Class(_) => Err("defines a class".into()),
            Stmt::With(..) => Err("unsupported `with`".into()),
            Stmt::Unsupported(u) => Err(format!("unsupported `{u}`")),
            _ => Ok(()),
        }
    }
}

/// Whether the block's value is its completion value (`x: { a; b }` is `b` in QML), which a
/// function body does not reproduce.
fn completion(s: &Stmt) -> bool {
    match s {
        Stmt::Expr(_) => true,
        Stmt::Block(b) => b.last().is_some_and(completion),
        Stmt::If(_, a, b) => completion(a) || b.as_deref().is_some_and(completion),
        Stmt::Switch(_, cs) => cs.iter().any(|(_, b)| b.iter().any(completion)),
        Stmt::Try(a, h, _) => {
            a.last().is_some_and(completion)
                || h.as_ref()
                    .is_some_and(|(_, h)| h.last().is_some_and(completion))
        }
        Stmt::Labeled(_, b) => completion(b),
        _ => false,
    }
}

fn function_body(body: &str) -> Result<Vec<Stmt>, String> {
    let f = format!("function __v4() {{\n{body}\n}}");
    match qml::ir::lower(&qml::Script::new(&f, 0, qml::ScriptKind::Function))?
        .into_iter()
        .next()
    {
        Some(Stmt::Func(f)) => Ok(f.body.clone()),
        _ => Err("not a function body".into()),
    }
}

/// The binding as a JS function body, or why it cannot be one. A QML binding is an
/// expression or a statement (`x: { ... }`, `x: switch (k) { ... }`) whose value is its
/// completion value; a statement is replayed only when it returns its value explicitly.
fn body_text(src: &str, kind: qml::ScriptKind) -> Result<String, String> {
    if kind == qml::ScriptKind::Function {
        return Err("function".into());
    }
    if kind == qml::ScriptKind::Expression {
        let ret = format!("return ({src}\n);");
        if function_body(&ret).is_ok() {
            return Ok(ret);
        }
    }
    let body = function_body(src)?;
    if body.iter().any(completion) {
        return Err("statement value is its completion value".into());
    }
    Ok(src.to_string())
}

pub fn js_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                write!(out, "\\u{:04x}", c as u32).unwrap()
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A JS string literal of code units `u`, an unpaired surrogate as its `\uXXXX` escape.
pub fn js_str16(u: &[u16], out: &mut String) {
    let mut lit = String::new();
    out.push('"');
    for c in char::decode_utf16(u.iter().copied()) {
        match c {
            Ok(c) => {
                lit.clear();
                js_str(c.encode_utf8(&mut [0; 4]), &mut lit);
                out.push_str(&lit[1..lit.len() - 1]);
            }
            Err(e) => write!(out, "\\u{:04x}", e.unpaired_surrogate()).unwrap(),
        }
    }
    out.push('"');
}

struct Protos {
    object: Option<usize>,
    array: Option<usize>,
}

fn protos(it: &mut Interp) -> Protos {
    let g = it.global.clone();
    let mut proto = |name: &str| {
        let c = it.lookup(&g, name).ok()?;
        match it.get(&c, "prototype").ok()? {
            Value::Obj(o) => Some(Rc_ptr(&o)),
            _ => None,
        }
    };
    Protos {
        object: proto("Object"),
        array: proto("Array"),
    }
}

/// `v` as a JS literal, if it is plain data (no items, colours, functions or host objects).
fn literal(v: &Value, p: &Protos, depth: u32, out: &mut String) -> Result<(), &'static str> {
    match v {
        Value::Undefined | Value::Empty => out.push_str("undefined"),
        Value::Null => out.push_str("null"),
        Value::Bool(b) => write!(out, "{b}").unwrap(),
        Value::Num(n) if n.is_nan() => out.push_str("NaN"),
        Value::Num(n) if n.is_infinite() => {
            out.push_str(if *n > 0.0 { "Infinity" } else { "-Infinity" })
        }
        Value::Num(n) if *n == 0.0 && n.is_sign_negative() => out.push_str("-0"),
        Value::Num(n) => write!(out, "{n}").unwrap(),
        Value::Str(s) => js_str(s, out),
        Value::Str16(u) => js_str16(u, out),
        Value::Item(_) => return Err("input is a QML object"),
        Value::Native(_) => return Err("input is a host object"),
        Value::Color(_) => return Err("input is a color"),
        Value::Symbol(_) => return Err("input is a symbol"),
        Value::Obj(o) => {
            if depth > 6 {
                return Err("input nests too deep");
            }
            let b = o.borrow();
            if b.call.is_some() {
                return Err("input is a function");
            }
            let proto = b.proto.as_ref().map(Rc_ptr);
            if b.native.is_some()
                || b.accessors.is_some()
                || !matches!(b.internal, qml::value::Internal::Ordinary)
            {
                return Err("input is a host object");
            }
            if let Some(a) = &b.array {
                if proto != p.array || !b.keys.is_empty() {
                    return Err("input is a host object");
                }
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    literal(x, p, depth + 1, out)?;
                }
                out.push(']');
            } else {
                if proto != p.object {
                    return Err("input is a host object");
                }
                out.push('{');
                for (i, k) in b.keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    js_str(k, out);
                    out.push(':');
                    literal(
                        b.props.get(k).unwrap_or(&Value::Undefined),
                        p,
                        depth + 1,
                        out,
                    )?;
                }
                out.push('}');
            }
        }
    }
    if out.len() > MAX_PROGRAM {
        return Err("input too large");
    }
    Ok(())
}

/// Nested object literal of the read paths under one free name.
#[derive(Default)]
struct Tree {
    whole: Option<String>,
    kids: BTreeMap<String, Tree>,
}

impl Tree {
    fn put(&mut self, path: &[String], lit: String) {
        match path.split_first() {
            None => self.whole = Some(lit),
            Some((k, rest)) => self.kids.entry(k.clone()).or_default().put(rest, lit),
        }
    }
    fn write(&self, out: &mut String) {
        if let Some(w) = &self.whole {
            return out.push_str(w);
        }
        out.push('{');
        for (i, (k, t)) in self.kids.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            js_str(k, out);
            out.push(':');
            t.write(out);
        }
        out.push('}');
    }
}

/// The closed program (a JS expression whose value is the canonical result string).
fn program(roots: &BTreeMap<String, Tree>, body: &str) -> String {
    let mut p = String::from("(function () {\n");
    for (name, t) in roots {
        write!(p, "var {name} = ").unwrap();
        t.write(&mut p);
        p.push_str(";\n");
    }
    write!(p, "{CANON}\ntry {{\nreturn __canon((function () {{\n{body}\n}})());\n}} catch (e) {{\nreturn \"throw \" + (e && e.name);\n}}\n}})()").unwrap();
    p
}

/// Runs `program` in the reference runtime's interpreter, like the bindings it came from.
pub fn run_native(it: &mut Interp, program: &str) -> String {
    let body = match qml::ir::lower_program(&format!("var result = {program};")) {
        Ok(b) => b,
        Err(e) => return format!("native lowering error: {e}"),
    };
    let sc: Scope = Rc::new(RefCell::new(Frame::default()));
    sc.borrow_mut().parent = Some(it.global.clone());
    match it.run(&body, &sc) {
        Ok(_) => {
            let v = sc
                .borrow()
                .vars
                .get("result")
                .cloned()
                .or_else(|| it.global.borrow().vars.get("result").cloned());
            match v {
                Some(Value::Str(s)) => s.to_string(),
                v => format!("native non-string result {v:?}"),
            }
        }
        Err(Ex::Throw(v)) => format!("native uncaught {}", v.to_js_string()),
        Err(Ex::Short) => "native uncaught short".into(),
    }
}

/// Samples every live binding of the reference runtime whose key still needs programs.
pub fn sample(st: &mut St) {
    let St { rt, inv, v4: s, .. } = st;
    let mut sites = Vec::new();
    {
        let w = rt.world.borrow();
        for id in 0..w.items.len() {
            if w.dead.contains(&id) {
                continue;
            }
            let Some(it) = w.items.get(id) else { continue };
            for (k, p) in it.props.iter() {
                if let Some(c) = &p.binding {
                    sites.push((id, k.as_str(), c.body.clone()));
                }
            }
        }
    }
    let protos = protos(&mut rt.it);
    for (item, name, body) in sites {
        let Some(ks) = inv.by_body.get(&Rc_ptr(&body)) else {
            continue;
        };
        let Some(&first) = ks.first() else { continue };
        let k = &inv.keys[first];
        if k.kind != "binding" {
            continue;
        }
        let key = k.key.clone();
        s.seen.insert(key.clone());
        if s.skipped.contains_key(&key)
            || s.per_key.get(&key).copied().unwrap_or(0) >= PER_KEY
            || s.misses.get(&key).copied().unwrap_or(0) >= MAX_MISSES
        {
            continue;
        }
        let Some((src, kind)) = k.src.clone() else {
            continue;
        };
        let text = match body_text(&src, kind) {
            Ok(t) => t,
            Err(e) => {
                s.skipped.insert(key, e);
                continue;
            }
        };
        let mut inputs = Inputs::default();
        declared(&body, &mut inputs.locals);
        if let Err(e) = inputs.body(&body) {
            s.skipped.insert(key, e);
            continue;
        }
        let mut roots: BTreeMap<String, Tree> = BTreeMap::new();
        let mut miss = None;
        // Paths are sorted, so a prefix read whole comes first and covers the longer ones.
        let mut covered: Vec<&Vec<String>> = Vec::new();
        for (path, x) in &inputs.reads {
            if covered.iter().any(|c| path.starts_with(c)) {
                continue;
            }
            let v = match rt.eval_in_binding(item, name, x) {
                Some(Ok(v)) => v,
                Some(Err(_)) => {
                    miss = Some("input read throws");
                    break;
                }
                None => {
                    miss = Some("no binding");
                    break;
                }
            };
            let mut lit = String::new();
            if let Err(e) = literal(&v, &protos, 0, &mut lit) {
                miss = Some(e);
                break;
            }
            roots
                .entry(path[0].clone())
                .or_default()
                .put(&path[1..], lit);
            covered.push(path);
        }
        if let Some(e) = miss {
            let n = s.misses.entry(key.clone()).or_default();
            *n += 1;
            if *n >= MAX_MISSES {
                s.skipped.insert(key, e.to_string());
            }
            continue;
        }
        let prog = program(&roots, &text);
        if prog.len() > MAX_PROGRAM || s.programs.contains_key(&prog) {
            continue;
        }
        let native = run_native(&mut rt.it, &prog);
        let keys = ks
            .iter()
            .map(|&i| inv.keys[i].key.clone())
            .collect::<Vec<_>>()
            .join(", ");
        *s.per_key.entry(key).or_default() += 1;
        s.programs.insert(prog, Sample { keys, native });
    }
}

/// The worker's samples as records for the main thread: `p` keys native program, `s` key
/// reason, `n` key.
pub fn drain(st: &mut St) -> Vec<String> {
    let s = std::mem::take(&mut st.v4);
    let mut out = Vec::new();
    for (prog, x) in s.programs {
        out.extend(["p".into(), x.keys, x.native, prog]);
    }
    for (k, r) in s.skipped {
        out.extend(["s".into(), k, r]);
    }
    for k in s.seen {
        out.extend(["n".into(), k]);
    }
    out
}

/// One QV4 divergence: binding keys, QV4's result, the native interpreter's result.
pub struct Diverge {
    pub keys: Vec<String>,
    pub v4: String,
    pub native: String,
}

#[derive(Default)]
pub struct Outcome {
    pub programs: usize,
    pub keys: usize,
    pub seen: usize,
    /// Skip reason -> binding keys.
    pub skipped: BTreeMap<String, usize>,
    pub divergences: Vec<Diverge>,
    /// The oracle could not run (a broken `--v4` setup fails `--strict`).
    pub error: Option<String>,
}

/// Runs the drained samples of every scene through QV4 and compares; writes `v4.tsv`.
pub fn check(python: &std::path::Path, out: &std::path::Path, records: &[String]) -> Outcome {
    let mut programs: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    let mut skipped: BTreeMap<&str, &str> = BTreeMap::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut it = records.iter().map(String::as_str);
    while let Some(tag) = it.next() {
        match tag {
            "p" => {
                let (Some(k), Some(n), Some(p)) = (it.next(), it.next(), it.next()) else {
                    break;
                };
                programs.entry(p).or_insert((k, n));
            }
            "s" => {
                let (Some(k), Some(r)) = (it.next(), it.next()) else {
                    break;
                };
                skipped.entry(k).or_insert(r);
            }
            _ => {
                if let Some(k) = it.next() {
                    seen.insert(k);
                }
            }
        }
    }
    // One program stands for every binding with its text: its keys are joined with ", ".
    let sampled: HashSet<&str> = programs.values().flat_map(|(k, _)| k.split(", ")).collect();
    let mut o = Outcome {
        programs: programs.len(),
        keys: sampled.len(),
        seen: seen.len(),
        ..Default::default()
    };
    for (k, r) in &skipped {
        if !sampled.contains(k) {
            *o.skipped.entry(r.to_string()).or_default() += 1;
        }
    }
    let _ = std::fs::create_dir_all(out);
    let list: Vec<&str> = programs.keys().copied().collect();
    let results = match run_oracle(python, out, &list) {
        Ok(r) => r,
        Err(e) => {
            o.error = Some(e);
            return o;
        }
    };
    let mut tsv = String::from("keys\tagree\tqv4\tnative\tprogram\n");
    for ((prog, (keys, native)), v4) in programs.iter().zip(results) {
        let agree = v4 == *native;
        let esc = |s: &str| {
            s.replace('\\', "\\\\")
                .replace('\n', "\\n")
                .replace('\t', "\\t")
        };
        writeln!(
            tsv,
            "{keys}\t{agree}\t{}\t{}\t{}",
            esc(&v4),
            esc(native),
            esc(prog)
        )
        .unwrap();
        if !agree {
            o.divergences.push(Diverge {
                keys: keys.split(", ").map(String::from).collect(),
                v4,
                native: native.to_string(),
            });
        }
    }
    let _ = std::fs::write(out.join("v4.tsv"), tsv);
    o
}

/// Runs every program in QV4: one result per program, or why the oracle could not run.
fn run_oracle(
    python: &std::path::Path,
    out: &std::path::Path,
    programs: &[&str],
) -> Result<Vec<String>, String> {
    let script = out.join("v4oracle.py");
    std::fs::write(&script, ORACLE).map_err(|e| format!("{}: {e}", script.display()))?;
    let system_qt = out.join("system_qt.py");
    std::fs::write(&system_qt, SYSTEM_QT).map_err(|e| format!("{}: {e}", system_qt.display()))?;
    let mut jsonl = String::new();
    for p in programs {
        js_str(p, &mut jsonl);
        jsonl.push('\n');
    }
    let input = out.join("v4-programs.jsonl");
    std::fs::write(&input, jsonl).map_err(|e| format!("{}: {e}", input.display()))?;
    let o = std::process::Command::new(python)
        .arg(&script)
        .arg(&input)
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .map_err(|e| format!("{}: {e}", python.display()))?;
    if !o.status.success() {
        return Err(format!(
            "{} exited {}: {}",
            python.display(),
            o.status,
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    let lines: Vec<String> = String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(unescape)
        .collect();
    if lines.len() != programs.len() {
        return Err(format!(
            "{} results for {} programs: {}",
            lines.len(),
            programs.len(),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(lines)
}

/// The oracle prints one result per line with `\\`, `\n` and `\r` escaped.
fn unescape(l: &str) -> String {
    let mut s = String::with_capacity(l.len());
    let mut it = l.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            s.push(c);
            continue;
        }
        match it.next() {
            Some('n') => s.push('\n'),
            Some('r') => s.push('\r'),
            Some(c) => s.push(c),
            None => s.push('\\'),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(src: &str) -> Result<Vec<String>, String> {
        let body = qml::ir::lower(&qml::Script::new(src, 0, qml::ScriptKind::Expression))?;
        let mut i = Inputs::default();
        declared(&body, &mut i.locals);
        i.body(&body)?;
        Ok(i.reads.keys().map(|p| p.join(".")).collect())
    }

    #[test]
    fn free_inputs_are_member_paths() {
        assert_eq!(
            inputs("root.a.b + Math.max(w, 2)").unwrap(),
            ["root.a.b", "w"]
        );
        assert_eq!(
            inputs("list.map(x => x.v * k).join(s.sep)").unwrap(),
            ["k", "list", "s.sep"]
        );
        assert_eq!(inputs("m[i].name").unwrap(), ["i", "m"]);
        assert_eq!(inputs("a?.b ?? 0").unwrap(), ["a.b"]);
        assert!(inputs("Date.now() - t").is_err());
        assert!(inputs("Math.random()").is_err());
        assert!(inputs("this.x").is_err());
        assert!(inputs("x = 1").is_err());
    }

    #[test]
    fn blocks_need_explicit_returns() {
        assert!(body_text("{ if (a) return 1; return 2 }", qml::ScriptKind::Block).is_ok());
        assert!(body_text("{ var x = 1; x + 1 }", qml::ScriptKind::Block).is_err());
        let sw = "switch (k) { case 1: return 2; default: return 3 }";
        assert_eq!(body_text(sw, qml::ScriptKind::Expression).unwrap(), sw);
        assert!(
            body_text(
                "switch (k) { case 1: 2; break }",
                qml::ScriptKind::Expression
            )
            .is_err()
        );
    }

    #[test]
    fn programs_run_natively() {
        let mut roots = BTreeMap::new();
        let mut t = Tree::default();
        t.put(&["a".into()], "1.5".into());
        t.put(&["s".into()], "\"x\\ny\"".into());
        roots.insert("root".to_string(), t);
        let p = program(
            &roots,
            "return [root.a.toFixed(2), root.s, -0, 1 / 3, {k: undefined}];",
        );
        let mut it = Interp::default();
        assert_eq!(
            run_native(&mut it, &p),
            r#"["1.50","x\ny",-0,0.3333333333333333,{"k":undefined}]"#
        );
        let p = program(&BTreeMap::new(), "return null.x;");
        assert_eq!(run_native(&mut it, &p), "throw TypeError");
    }

    #[test]
    fn literals_reject_host_values() {
        let mut it = Interp::default();
        let p = protos(&mut it);
        let mut s = String::new();
        assert!(literal(&Value::Item(3), &p, 0, &mut s).is_err());
        let mut s = String::new();
        literal(&Value::Num(-0.0), &p, 0, &mut s).unwrap();
        assert_eq!(s, "-0");
    }
}
