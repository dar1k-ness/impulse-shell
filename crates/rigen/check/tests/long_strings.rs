//! Compiled code's calls on one long string cost work in proportion to its length, however
//! many of them a loop makes (`cases/long_string_loops.js`): a JS string is indexed by UTF-16
//! code unit while a well-formed one is held as UTF-8, and a call that reads the whole string
//! to get at its units makes such a loop cost the square of the length. The work is counted,
//! not timed ([`work`]).

#[path = "../../../qml/tests/support/work.rs"]
mod work;

use qml::eval::{Ex, Interp, ScriptCtx, ScriptFile};
use qml::value::Value;
use std::rc::Rc;
use work::Work;

const LOOPS: [&str; 26] = [
    "notes",
    "exec",
    "test",
    "match",
    "search",
    "matchGlobal",
    "matchAll",
    "replace",
    "replaceCall",
    "split",
    "length",
    "index",
    "charAt",
    "at",
    "charCodeAt",
    "codePointAt",
    "substring",
    "substr",
    "slice",
    "startsWith",
    "endsWith",
    "indexOf",
    "lastIndexOf",
    "has",
    "descriptor",
    "includes",
];

/// The case's compiled program, run: its `text` and `loops` are the globals of `cx`.
fn case() -> Interp {
    let (_, _, path, f) = rigen_check::CASES
        .iter()
        .find(|c| c.0 == "long_string_loops")
        .unwrap();
    let mut cx = Interp::default();
    let g = cx.global.clone();
    g.borrow_mut().script = Some(Rc::new(ScriptFile {
        file: (*path).into(),
        ctx: Some(ScriptCtx::new(None)),
    }));
    cx.code = Some(g.clone());
    let env = qml::jsrt::Env::from_scope(g);
    let r = cx.enter().and_then(|frame| {
        let r = f(&mut cx, &env, &Value::Undefined, &[]);
        cx.leave(frame, r)
    });
    assert!(r.is_ok(), "the case threw");
    cx
}

/// A top-level name of the case: the compiled body keeps them in its frame, and the
/// namespace object of the file reads them there.
fn global(cx: &mut Interp, name: &str) -> Value {
    let script = cx.global.borrow().script.clone().unwrap();
    let exports = Value::Obj(script.ctx.as_ref().unwrap().exports.clone());
    cx.get(&exports, name).ok().unwrap()
}

fn call(cx: &mut Interp, f: &Value, args: &[Value]) -> Value {
    match cx.call(f, Value::Undefined, args) {
        Ok(v) => v,
        Err(Ex::Throw(e)) => {
            let message = cx.get(&e, "message").ok().unwrap_or_default();
            panic!("a call threw {}", message.to_js_string())
        }
        Err(Ex::Short) => panic!("a call was cut short"),
    }
}

/// `s` in an allocation of its own: a string nothing has read yet.
fn copy(s: &Value) -> Value {
    match s {
        Value::Str(s) => Value::Str(Rc::from(&**s)),
        Value::Str16(u) => Value::Str16(Rc::from(&**u)),
        v => unreachable!("the text is {v:?}"),
    }
}

/// A text 16 times as long costs each loop about 16 times the work: under 64 times, where
/// copying or reading the whole string on each turn costs 256 times. No clock takes part:
/// the watchdog is off, so a loop that costs the square is counted to its end, which on
/// texts of this length takes it a minute.
/// The count does not see a loop that reads the whole string without allocating and
/// outside the four counted functions (`utf16::count`, `utf16::encode`, `JsStr::units`,
/// `JsStr::len`).
#[test]
fn compiled_calls_on_a_long_string_cost_its_length() {
    const SHORT: usize = 256;
    const GROWTH: usize = 16;
    qml::watchdog::set_timeout(None);
    let mut cx = case();
    let (text, loops) = (global(&mut cx, "text"), global(&mut cx, "loops"));
    let mut slow = Vec::new();
    for kind in 0..3 {
        let mut args = |lines: usize| {
            let n = Value::Num(lines as f64);
            let s = call(&mut cx, &text, &[n.clone(), Value::Num(kind as f64)]);
            let width = s.to_js_string().lines().next().unwrap().chars().count() + 1;
            [s, Value::Num(width as f64), n]
        };
        let (short, long) = (args(SHORT), args(SHORT * GROWTH));
        for name in LOOPS {
            let f = cx.get(&loops, name).ok().unwrap();
            let mut work = |args: &[Value; 3]| {
                let args = [copy(&args[0]), args[1].clone(), args[2].clone()];
                let start = Work::now();
                let r = call(&mut cx, &f, &args);
                (Work::now().since(start), r.to_js_string())
            };
            let ((a, _), (b, count)) = (work(&short), work(&long));
            println!("kind {kind} {name:12} {a} -> {b} -> {count}");
            if let Some(what) = b.grew_faster_than(a, GROWTH) {
                slow.push(format!(
                    "{name} on kind {kind}: {a} -> {b}, too much {what}"
                ));
            }
        }
    }
    assert!(
        slow.is_empty(),
        "{GROWTH} times the text cost {} times the work or more:\n{}",
        4 * GROWTH,
        slow.join("\n")
    );
}
