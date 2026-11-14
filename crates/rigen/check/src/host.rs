//! Interpreter vs compiled under a mock QML `Host`: free names, host calls, sequence
//! write-back and `this` of scope methods (cases in `host_cases.rs`).
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use qml::eval::{Ex, Frame, Host, HostError, HostR, Interp, Scope};
use qml::jsrt::Env;
use qml::value::Value;

include!("host_cases.rs");

#[derive(Default)]
struct World {
    items: Vec<BTreeMap<String, Value>>,
    ids: BTreeMap<String, usize>,
    /// (item, property) refusing JavaScript writes, like a C++ property without WRITE.
    readonly: BTreeSet<(usize, String)>,
    log: Vec<String>,
}

fn render(v: &Value, depth: usize) -> String {
    match v {
        Value::Undefined | Value::Empty => "undefined".into(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Num(_) | Value::Symbol(_) => v.to_js_string(),
        Value::Str(s) => format!("{s:?}"),
        Value::Str16(u) => format!("str16{u:x?}"),
        Value::Item(i) => format!("#{i}"),
        Value::Native(n) => format!("native{n}"),
        Value::Color(c) => format!("{c:?}"),
        Value::Obj(_) if depth > 4 => "…".into(),
        Value::Obj(o) => {
            let b = o.borrow();
            if b.call.is_some() {
                return "fn".into();
            }
            if let Some(a) = &b.array {
                let xs: Vec<String> = a.iter().map(|x| render(x, depth + 1)).collect();
                return format!("[{}]", xs.join(","));
            }
            let xs: Vec<String> = b
                .keys
                .iter()
                .map(|k| {
                    format!(
                        "{k}:{}",
                        render(b.props.get(k).unwrap_or(&Value::Undefined), depth + 1)
                    )
                })
                .collect();
            format!("{{{}}}", xs.join(","))
        }
    }
}

struct Mock(Rc<RefCell<World>>);

impl Mock {
    /// Property read; sequences carry their owner so in-place mutation can notify it.
    fn read(&self, item: usize, name: &str) -> Option<Value> {
        let w = self.0.borrow();
        let v = w.items.get(item)?.get(name)?.clone();
        if let Value::Obj(o) = &v {
            let mut b = o.borrow_mut();
            if b.array.is_some() {
                b.owner = Some((item, name.into()));
            }
        }
        Some(v)
    }
}

impl Host for Mock {
    fn lookup(&mut self, scope: Option<(usize, usize)>, name: &str) -> Option<Value> {
        let (item, _ctx) = scope?;
        if let Some(&i) = self.0.borrow().ids.get(name) {
            return Some(Value::Item(i));
        }
        self.read(item, name).or_else(|| self.read(0, name))
    }

    fn assign(
        &mut self,
        item: usize,
        _ctx: usize,
        name: &str,
        v: Value,
    ) -> Result<bool, HostError> {
        let mut w = self.0.borrow_mut();
        let Some(target) = [item, 0]
            .into_iter()
            .find(|&i| w.items[i].contains_key(name))
        else {
            return Ok(false);
        };
        if w.readonly.contains(&(target, name.to_string())) {
            return Err(HostError::read_only(name));
        }
        w.log
            .push(format!("assign #{target}.{name} = {}", render(&v, 0)));
        w.items[target].insert(name.into(), v);
        Ok(true)
    }

    fn get(&mut self, item: usize, name: &str) -> Option<Value> {
        self.read(item, name)
    }

    fn native(&mut self, item: usize, name: &str, args: Option<&[Value]>) -> Option<HostR> {
        if !matches!(
            (item, name),
            (0, "reload") | (0, "sum") | (0, "fire") | (1, "describe")
        ) {
            return None;
        }
        let Some(args) = args else {
            return Some(Ok(Value::Undefined));
        };
        if name == "fire" && args.len() < 2 {
            return Some(Err(HostError::new("Error", "Insufficient arguments")));
        }
        let r = match (item, name) {
            (0, "reload") => Value::str(&format!("reloaded{}", args.len())),
            (0, "sum") => Value::Num(args.iter().map(|a| a.to_number()).sum()),
            (0, "fire") => Value::Undefined,
            (1, "describe") => Value::str(&format!(
                "child({})",
                args.first().map(|a| a.to_js_string()).unwrap_or_default()
            )),
            _ => return None,
        };
        let a: Vec<String> = args.iter().map(|x| render(x, 0)).collect();
        self.0
            .borrow_mut()
            .log
            .push(format!("call #{item}.{name}({})", a.join(",")));
        Some(Ok(r))
    }

    fn notify(&mut self, item: usize, name: &str) {
        let mut w = self.0.borrow_mut();
        let v = w
            .items
            .get(item)
            .and_then(|p| p.get(name))
            .map(|v| render(v, 0))
            .unwrap_or_default();
        w.log.push(format!("notify #{item}.{name} {v}"));
    }

    fn read_only(&mut self, item: usize, name: &str) -> bool {
        self.0.borrow().readonly.contains(&(item, name.to_string()))
    }

    fn set(&mut self, item: usize, name: &str, v: Value) -> bool {
        let mut w = self.0.borrow_mut();
        w.log
            .push(format!("set #{item}.{name} = {}", render(&v, 0)));
        w.items[item].insert(name.into(), v);
        true
    }
}

fn fixture() -> Rc<RefCell<World>> {
    let obj = |kv: &[(&str, Value)]| {
        let o = qml::eval::new_obj();
        for (k, v) in kv {
            qml::eval::set_own(&o, k, v.clone());
        }
        Value::Obj(o)
    };
    let n = Value::Num;
    let root: BTreeMap<String, Value> = [
        ("w", n(3.0)),
        ("h", n(4.0)),
        ("name", Value::str("r")),
        ("list", Value::array(vec![n(1.0), n(2.0)])),
        ("model", obj(&[("arr", Value::array(vec![n(7.0)]))])),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let child: BTreeMap<String, Value> = [
        ("x", n(10.0)),
        ("list2", Value::array(vec![])),
        ("size", n(5.0)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let ids = [("root".to_string(), 0), ("child".to_string(), 1)]
        .into_iter()
        .collect();
    Rc::new(RefCell::new(World {
        items: vec![root, child],
        ids,
        readonly: [(1, "size".to_string())].into_iter().collect(),
        log: Vec::new(),
    }))
}

/// The runtime's binding/handler frame (`scope_in`).
fn scope(cx: &Interp, item: usize) -> Scope {
    let mut f = Frame::default();
    f.parent = Some(cx.global.clone());
    f.this = Some(Value::Item(item));
    f.qml = Some((item, 0));
    Rc::new(RefCell::new(f))
}

fn script(src: &str, kind: qml::ScriptKind) -> qml::Script {
    qml::Script::new(src, 0, kind)
}

fn err(cx: &mut Interp, e: Ex) -> String {
    match e {
        Ex::Throw(v) => {
            let m = cx
                .get(&v, "message")
                .map(|m| m.to_js_string())
                .unwrap_or_default();
            format!("throw {m}")
        }
        Ex::Short => "short".into(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Interp,
    Compiled,
}

/// Result, host log and final world of case `i` in `mode`.
fn run(mode: Mode, i: usize) -> String {
    let world = fixture();
    let mut cx = Interp::new(Box::new(Mock(world.clone())));
    for (j, (item, src)) in HOST_FUNCS.iter().enumerate() {
        let sc = scope(&cx, *item);
        let body = qml::ir::lower(&script(src, qml::ScriptKind::Function)).unwrap();
        let Some(qml::ir::Stmt::Func(fd)) = body.first() else {
            unreachable!()
        };
        let name = fd.name.clone().unwrap();
        let f = match mode {
            Mode::Interp => {
                cx.run(&body, &sc).map_err(|e| err(&mut cx, e)).unwrap();
                sc.borrow_mut().vars.remove(&name).unwrap()
            }
            Mode::Compiled => {
                let (cf, meta) = crate::HOST_FUNCS_C[j];
                qml::jsrt::closure(cf, &Env::from_scope(sc), meta)
            }
        };
        world.borrow_mut().items[*item].insert(name, f);
    }
    let (_, kind, item, src, params) = HOST_CASES[i];
    let sc = scope(&cx, item);
    let all = [Value::Num(2.0), Value::Num(3.0), Value::str("s")];
    let args = &all[..params.len()];
    if kind == "handler" {
        for (p, a) in params.iter().zip(args) {
            sc.borrow_mut().vars.insert(p.to_string(), a.clone());
        }
    }
    let r = match mode {
        Mode::Interp => {
            let sk = if src.trim_start().starts_with('{') {
                qml::ScriptKind::Block
            } else {
                qml::ScriptKind::Expression
            };
            let body = qml::ir::lower(&script(src, sk)).unwrap();
            // A handler written as a function is that function, called with the signal's
            // arguments; compiled code does so itself (`qml::runtime`, `run_code`).
            match cx.run(&body, &sc) {
                Ok(f) if kind == "handler" && qml::ir::is_function_handler(&body) => {
                    cx.call(&f, Value::Item(item), args)
                }
                r => r,
            }
        }
        Mode::Compiled => crate::HOST_CASES_C[i](
            &mut cx,
            &Env::from_scope(sc.clone()),
            &Value::Item(item),
            args,
        ),
    };
    let r = match r {
        Ok(v) => render(&v, 0),
        Err(e) => err(&mut cx, e),
    };
    let g = cx
        .global
        .borrow()
        .vars
        .get("undeclaredGlobal")
        .map(|v| render(v, 0));
    let w = world.borrow();
    let mut out = format!("result {r}\n");
    for l in &w.log {
        out += &format!("  {l}\n");
    }
    for (n, props) in w.items.iter().enumerate() {
        let ps: Vec<String> = props
            .iter()
            .map(|(k, v)| format!("{k}={}", render(v, 0)))
            .collect();
        out += &format!("  #{n} {}\n", ps.join(" "));
    }
    if let Some(g) = g {
        out += &format!("  global undeclaredGlobal={g}\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(prefix: &str) {
        let mut bad = Vec::new();
        for (i, (name, ..)) in HOST_CASES.iter().enumerate() {
            if !name.starts_with(prefix) {
                continue;
            }
            let a = run(Mode::Interp, i);
            let b = run(Mode::Compiled, i);
            if a != b {
                bad.push(format!(
                    "{name} ({}):\n interp:\n{a} compiled:\n{b}",
                    HOST_CASES[i].3
                ));
            } else {
                println!("{name}: {a}");
            }
        }
        assert!(
            bad.is_empty(),
            "{} mismatches\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    #[test]
    fn host_lookup() {
        check("lookup_");
    }

    #[test]
    fn host_assign() {
        check("assign_");
    }

    #[test]
    fn host_call() {
        check("call_");
    }

    #[test]
    fn host_call_throws() {
        let i = HOST_CASES
            .iter()
            .position(|c| c.0 == "call_throws")
            .unwrap();
        let e = "Error:Insufficient arguments:true;";
        let want = format!("result \"{e}{e}{e}ok;\"\n");
        assert!(run(Mode::Compiled, i).starts_with(&want));
    }

    #[test]
    fn host_write_back() {
        check("wb_");
    }

    #[test]
    fn host_params() {
        check("params_");
    }

    #[test]
    fn host_this() {
        check("this_");
    }
}
