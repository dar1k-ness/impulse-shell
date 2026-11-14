//! The `MusicRecognizer` singleton of module `Impulse` (`Impulse/musicrecognizer.qmltypes`):
//! `start(interval, timeout, source)` listens with `songrec` (crate::ports::songrec) and
//! `finished(text, exitCode)` reports what it printed (songrec's JSON line of the first
//! match, empty when the timeout came first) and its exit code (1: songrec is not installed
//! or the source does not exist). `stop()` ends a run without a `finished`.

use crate::eval::{Interp, R};
use crate::native::ProcEv;
use crate::runtime::World;
use crate::value::Value;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// The QML name of the singleton.
pub const NAME: &str = "MusicRecognizer";

thread_local! {
    /// The stop of the run in progress: a run that is not this one any more reports nothing.
    static RUN: RefCell<Option<Arc<crate::svc::Stop>>> = const { RefCell::new(None) };
}

fn set_running(w: &mut World, on: bool) {
    if let Some(&root) = w.singletons.get(NAME) {
        w.write(root, "running", Value::Bool(on));
    }
}

fn end(w: &mut World) {
    if let Some(stop) = RUN.with(|r| r.borrow_mut().take()) {
        stop.store(true, Ordering::Relaxed);
    }
    set_running(w, false);
}

/// The run of `stop` ended with `code` having printed `out`: `finished`, unless it was
/// stopped or another run took its place.
fn finished(w: &mut World, stop: &Arc<crate::svc::Stop>, out: &str, code: i32) {
    let current = RUN.with(|r| {
        let mut r = r.borrow_mut();
        let same = r.as_ref().is_some_and(|s| Arc::ptr_eq(s, stop));
        if same {
            *r = None;
        }
        same
    });
    if !current {
        return;
    }
    set_running(w, false);
    if let Some(&root) = w.singletons.get(NAME) {
        let args = vec![Value::str(out), Value::Num(code as f64)];
        crate::native::emit(w, root, "finished", &["text", "exitCode"], args);
    }
}

fn start(it: &mut Interp, _: &Value, args: &[Value]) -> R<Value> {
    let Some(w) = crate::runtime::world_of(it) else {
        return Ok(Value::Undefined);
    };
    let mut w = w.borrow_mut();
    end(&mut w);
    let s = |i: usize| args.get(i).map(|v| v.to_js_string()).unwrap_or_default();
    let a = crate::ports::songrec::Args {
        interval: s(0),
        total: s(1),
        source: s(2),
    };
    let back = crate::impulse::replies(&mut w);
    if crate::impulse::hooked(NAME, "start", &[&a.interval, &a.total, &a.source]) {
        // Nothing listens: the run ends as one without songrec does.
        let stop: Arc<crate::svc::Stop> = Default::default();
        RUN.with(|r| *r.borrow_mut() = Some(stop.clone()));
        back.send(move |w| finished(w, &stop, "", 1));
        set_running(&mut w, true);
        return Ok(Value::Undefined);
    }
    let (tx, rx) = w.waker.handle(crate::svc::Svc::Impulse).channel();
    let stop: Arc<crate::svc::Stop> = Default::default();
    let io = crate::ports::Io {
        tx: Some(tx),
        stop: stop.clone(),
    };
    RUN.with(|r| *r.borrow_mut() = Some(stop.clone()));
    workpool::long(move || {
        let code = crate::jobs::caught(|| {
            crate::child::with_stop(&io.stop, || crate::ports::songrec::run(&a, &io))
        });
        drop(io);
        let out: String = rx
            .try_iter()
            .filter_map(|e| match e {
                ProcEv::Out(s) => Some(s),
                _ => None,
            })
            .collect();
        back.send(move |w| finished(w, &stop, &out, code));
    });
    set_running(&mut w, true);
    Ok(Value::Undefined)
}

fn stop(it: &mut Interp, _: &Value, _: &[Value]) -> R<Value> {
    if let Some(w) = crate::runtime::world_of(it) {
        end(&mut w.borrow_mut());
    }
    Ok(Value::Undefined)
}

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: NAME,
    methods: &[("start", start), ("stop", stop)],
    init: None,
};
