//! `Impulse.Processes`: which programs run, stopping them, starting one unless it runs.
//! What the original did with `pidof`, `killall` and `bash -c "pidof X || X"` (killDialog,
//! ConflictKiller, SessionWarnings, LockScreen). `/proc` is read on a pool thread; a query's
//! answer comes back as the `answered` signal.
//!
//! Under a spawn hook (aotcheck, tests) the equivalent command line goes to the hook and
//! nothing is read, signalled or started; a query is answered with no program running.

use crate::eval::{Interp, R};
use crate::runtime::World;
use crate::value::Value;

/// Where a singleton keeps the id it gave out last: a property QML cannot name. Each world
/// counts its own calls, so two worlds of one process give the same call the same id.
const LAST: &str = "\x01last";

/// The next id of a call of singleton `name` that answers with a signal: 1, 2, ... and 1
/// again after `i32::MAX`; -1 before the singleton exists.
pub(crate) fn next_id(w: &mut World, name: &str) -> i32 {
    let Some(&item) = w.singletons.get(name) else {
        return -1;
    };
    let last = &mut w.items[item].props.entry(LAST).or_default().value;
    let id = match last {
        Value::Num(n) => (*n as i32).checked_add(1).unwrap_or(1),
        _ => 1,
    };
    *last = Value::Num(id as f64);
    id
}

fn answer(w: &mut World, id: i32, running: &[String]) {
    let Some(&item) = w.singletons.get("Processes") else {
        return;
    };
    let running = running.iter().map(|n| Value::str(n)).collect();
    crate::native::emit(
        w,
        item,
        "answered",
        &["id", "running"],
        vec![Value::Num(id as f64), Value::array(running)],
    );
}

fn names(v: Option<&Value>) -> Vec<String> {
    let list = match v {
        Some(Value::Obj(o)) => o
            .borrow()
            .array
            .as_ref()
            .map(|a| a.iter().map(Value::to_js_string).collect())
            .unwrap_or_default(),
        Some(Value::Str(s)) => vec![s.to_string()],
        _ => vec![],
    };
    list.into_iter().filter(|n| !n.is_empty()).collect()
}

/// Under a spawn hook (aotcheck, tests) nothing is probed, signalled or started: the hook
/// gets the call instead.
fn hooked(method: &str, args: &[String]) -> bool {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::native::hooked_call(TYPE.name, method, &args).is_some()
}

/// The names of `names` a process runs under, as procps `pidof NAME` finds it.
pub fn running(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|n| !crate::osutil::pidof(n).is_empty())
        .cloned()
        .collect()
}

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "Processes",
    methods: &[
        ("query", query),
        ("terminate", terminate),
        ("startUnlessRunning", start_unless_running_call),
    ],
    init: None,
};

/// `int query(list<string> names)`: the id `answered(id, running)` will carry.
pub fn query(it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let names = names(args.first());
    let Some(w) = crate::runtime::world_of(it) else {
        return Ok(Value::Num(-1.0));
    };
    let (back, id) = {
        let mut w = w.borrow_mut();
        (crate::impulse::replies(&mut w), next_id(&mut w, TYPE.name))
    };
    if hooked("query", &names) {
        back.send(move |w| answer(w, id, &[]));
    } else {
        workpool::run(move || {
            let running = running(&names);
            back.send(move |w| answer(w, id, &running));
        });
    }
    Ok(Value::Num(id as f64))
}

/// `void terminate(list<string> names)`: SIGTERM to every process named so (psmisc `killall`).
pub fn terminate(_: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let names = names(args.first());
    if names.is_empty() {
        return Ok(Value::Undefined);
    }
    if !hooked("terminate", &names) {
        workpool::run(move || {
            crate::probe::killall(&names);
        });
    }
    Ok(Value::Undefined)
}

/// Start PROGRAM, detached, unless a process of that name runs; whether it was started.
pub fn start_unless_running(program: &str) -> bool {
    if !crate::osutil::pidof(program).is_empty() {
        return false;
    }
    crate::native::detach_program(&[program.to_string()]);
    true
}

/// `void startUnlessRunning(string program)`.
pub fn start_unless_running_call(_: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let program = args.first().map(Value::to_js_string).unwrap_or_default();
    if program.is_empty() {
        return Ok(Value::Undefined);
    }
    if !hooked("startUnlessRunning", std::slice::from_ref(&program)) {
        workpool::run(move || {
            start_unless_running(&program);
        });
    }
    Ok(Value::Undefined)
}
