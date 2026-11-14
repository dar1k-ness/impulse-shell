//! The `Warp` singleton of module `Impulse` (`Impulse/warp.qmltypes`): Cloudflare WARP
//! through `warp-cli`, the only interface its daemon has.
//!
//! `refresh()` asks `warp-cli status`: `available` is whether it answered and `connected`
//! what it said; a client that is not registered ("Unable") is registered
//! (`warp-cli registration new`, reported by `registrationExited(request, exitCode)`) and
//! then connected. `connect()` runs `warp-cli connect`: `connected` is true while it runs
//! and its result after (`connectExited(request, exitCode)`), so a failed connect does not
//! leave a toggle on. Both return the `request` their signals carry (the call's number,
//! counted in `requests`), so that each caller reports its own failure only. `disconnect()` runs `warp-cli disconnect` and does not wait
//! for it.
//!
//! `warp-cli` waits for its daemon, which may never answer. The calls that wait for it run
//! one after another on a lane of their own ([`LANE`]), never on a worker of the shared pool,
//! and one still running when the shell's objects go is ended with them.

use crate::eval::{Interp, R};
use crate::impulse::Replies;
use crate::runtime::World;
use crate::value::Value;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// The QML name of the singleton.
pub const NAME: &str = "Warp";

/// The `warp-cli` runs that are waited for: one at a time, in the order asked. A daemon that
/// does not answer holds this lane's thread only.
static LANE: workpool::Lane = workpool::Lane::new();

/// `warp-cli ARGS`: its exit code (127 when it cannot be started, -1 when it was ended by
/// `stop` or a signal) and what it printed.
pub(crate) fn cli(args: &[&str], stop: &Arc<crate::svc::Stop>) -> (i32, String) {
    if stop.load(Ordering::Acquire) {
        return (-1, String::new());
    }
    let mut c = workpool::userenv::command("warp-cli");
    c.args(args).stdin(Stdio::null());
    match crate::child::run(&mut c, stop) {
        Ok(o) => (
            o.status.and_then(|s| s.code()).unwrap_or(-1),
            String::from_utf8_lossy(&o.out).into_owned(),
        ),
        Err(_) => (127, String::new()),
    }
}

const SIGNAL_PARAMS: &[&str] = &["request", "exitCode"];

fn exited(w: &mut World, signal: &str, request: i32, code: i32) {
    if let Some(&root) = w.singletons.get(NAME) {
        let args = vec![Value::Num(request as f64), Value::Num(code as f64)];
        crate::native::emit(w, root, signal, SIGNAL_PARAMS, args);
    }
}

fn set(w: &mut World, name: &str, on: bool) {
    if let Some(&root) = w.singletons.get(NAME) {
        w.write(root, name, Value::Bool(on));
    }
}

/// `warp-cli connect` on this (lane) thread; its result goes back to the main thread.
fn connect_now(back: &Replies, stop: &Arc<crate::svc::Stop>, request: i32) {
    let code = cli(&["connect"], stop).0;
    back.send(move |w| {
        set(w, "connected", code == 0);
        exited(w, "connectExited", request, code);
    });
}

/// The number of this call: one more than `requests`, which it becomes.
fn next_request(it: &mut Interp) -> i32 {
    let Some(w) = crate::runtime::world_of(it) else {
        return 0;
    };
    let mut w = w.borrow_mut();
    let Some(&root) = w.singletons.get(NAME) else {
        return 0;
    };
    let n = w.items[root]
        .props
        .get("requests")
        .map_or(0, |p| p.value.to_number() as i32)
        + 1;
    w.write(root, "requests", Value::Num(n as f64));
    n
}

/// The way back for a `warp-cli` run and the stop that ends it with the shell's objects;
/// None where nothing of the real system is started.
fn replies(it: &mut Interp) -> Option<(Replies, Arc<crate::svc::Stop>)> {
    if crate::native::sandboxed() {
        return None;
    }
    let w = crate::runtime::world_of(it)?;
    let w = &mut *w.borrow_mut();
    Some((crate::impulse::replies(w), w.jobs_stop.0.clone()))
}

fn set_connected(it: &mut Interp, on: bool) {
    if let Some(w) = crate::runtime::world_of(it) {
        set(&mut w.borrow_mut(), "connected", on);
    }
}

fn refresh(it: &mut Interp, _: &Value, _: &[Value]) -> R<Value> {
    let request = next_request(it);
    if let Some((back, stop)) = replies(it) {
        LANE.run(move || {
            let text = cli(&["status"], &stop).1;
            let unregistered = text.contains("Unable");
            back.send(move |w| {
                set(w, "available", !text.is_empty());
                if text.contains("Unable") {
                } else if text.contains("Connected") {
                    set(w, "connected", true);
                } else if text.contains("Disconnected") {
                    set(w, "connected", false);
                }
            });
            if unregistered {
                let code = cli(&["registration", "new"], &stop).0;
                back.send(move |w| exited(w, "registrationExited", request, code));
                if code == 0 {
                    connect_now(&back, &stop, request);
                }
            }
        });
    }
    Ok(Value::Num(request as f64))
}

fn connect(it: &mut Interp, _: &Value, _: &[Value]) -> R<Value> {
    let request = next_request(it);
    set_connected(it, true);
    if let Some((back, stop)) = replies(it) {
        LANE.run(move || connect_now(&back, &stop, request));
    }
    Ok(Value::Num(request as f64))
}

/// `warp-cli disconnect`, not waited for.
pub(crate) fn start_disconnect() {
    if let Ok(ch) = workpool::userenv::command("warp-cli")
        .arg("disconnect")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        workpool::reap(ch);
    }
}

fn disconnect(it: &mut Interp, _: &Value, _: &[Value]) -> R<Value> {
    set_connected(it, false);
    if !crate::native::sandboxed() {
        start_disconnect();
    }
    Ok(Value::Undefined)
}

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: NAME,
    methods: &[
        ("refresh", refresh),
        ("connect", connect),
        ("disconnect", disconnect),
    ],
    init: None,
};
