//! `Impulse/ShellControl`: what the shell's QML asks of the shell itself. `ipcCall(target,
//! function)` calls one of the shell's `IpcHandler` functions, as `qs ipc call` does from
//! outside: the call waits in [`World::self_ipc`] for the shell's event loop, which runs the
//! handler in a later turn (from an app window's process, in the running shell), once the
//! compositor has applied what the caller did to the shell's windows: a window the caller
//! closed first is off the screen when the handler runs, as it was for the original's `qs ipc call`.
//! `openWindow(file)` shows one of the shell's app windows (settings.qml, welcome.qml,
//! killDialog.qml), see [`crate::native::open_own_window`].
//!
//! [`World::self_ipc`]: crate::runtime::World::self_ipc

use crate::eval::{Interp, R};
use crate::value::Value;

/// The QML name of the singleton.
pub const NAME: &str = "ShellControl";

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: NAME,
    methods: &[("ipcCall", ipc_call), ("openWindow", open_window)],
    init: None,
};

fn text(args: &[Value], i: usize) -> String {
    args.get(i).map(Value::to_js_string).unwrap_or_default()
}

fn ipc_call(it: &mut Interp, _: &Value, args: &[Value]) -> R<Value> {
    let (target, function) = (text(args, 0), text(args, 1));
    if crate::native::hooked_call(NAME, "ipcCall", &[&target, &function]).is_none()
        && let Some(w) = crate::runtime::world_of(it)
    {
        w.borrow_mut().self_ipc.push((target, function));
    }
    Ok(Value::Undefined)
}

fn open_window(it: &mut Interp, _: &Value, args: &[Value]) -> R<Value> {
    if let Some(w) = crate::runtime::world_of(it) {
        crate::native::open_own_window(&mut w.borrow_mut(), &text(args, 0));
    }
    Ok(Value::Undefined)
}
