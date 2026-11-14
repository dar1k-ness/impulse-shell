//! Hyprland 0.57 workspace objects: identity is `address` ("1", "special:magic"), and `id` is
//! only sent for numbered workspaces. `Hyprland.workspaces` keys by address like Quickshell,
//! keeps the numeric `id` for numbered ones and gives special workspaces id -1.

mod hypr_ipc;

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::{Condvar, Mutex, OnceLock};
use testenv::Wakeups;

/// Requests the fake compositor has read; [`RECEIVED`] is notified on each.
static REQS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static RECEIVED: Condvar = Condvar::new();
/// The test loop's wakeup: the fake pings it after recording a request, which no reply to the
/// runtime may follow (a dispatch is not answered back to the loop).
static PING: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

const SPECIAL: &str =
    r#"{"address":"special:magic","type":"special","name":"special:magic","monitor":"DP-1"}"#;
const ONE: &str = r#"{"address":"1","id":1,"type":"normal","name":"1","monitor":"DP-1"}"#;

fn hooks() -> usize {
    hooks_in(&REQS.lock().unwrap())
}

fn hooks_in(reqs: &[String]) -> usize {
    reqs.iter()
        .filter(|c| c.starts_with("eval ") && c.contains("rustshell:searchToggleReleaseInterrupt"))
        .count()
}

fn reply(cmd: &str) -> String {
    match cmd.strip_prefix("j/").unwrap_or(cmd) {
        "monitors" => r#"[{"id":0,"name":"DP-1","width":1920,"height":1080,"activeWorkspace":{"address":"1","id":1,"type":"normal","name":"1"},"specialWorkspace":{"address":"special:magic","type":"special","name":"special:magic"},"focused":true}]"#.into(),
        "workspaces" => format!("[{ONE},{SPECIAL}]"),
        "activeworkspace" => ONE.into(),
        "clients" => r#"[{"address":"0xa1","mapped":true,"class":"foot","title":"one","monitor":0,"workspace":{"address":"1","id":1,"type":"normal","name":"1"}},{"address":"0xb2","mapped":true,"class":"foot","title":"magic","monitor":0,"workspace":{"address":"special:magic","type":"special","name":"special:magic"}}]"#.into(),
        "activewindow" => "{}".into(),
        _ => "ok".into(),
    }
}

/// In its own process (`testenv::run_in_own_process`), whose `XDG_RUNTIME_DIR` and
/// `HYPRLAND_INSTANCE_SIGNATURE` name the fake compositor's sockets.
#[test]
fn workspaces_keyed_by_address() {
    const TEST: &str = concat!(module_path!(), "::workspaces_keyed_by_address");
    if !testenv::in_own_process(TEST) {
        let rt_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(rt_dir.path().join("hypr/H057")).unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_RUNTIME_DIR", rt_dir.path().as_os_str()),
                ("HYPRLAND_INSTANCE_SIGNATURE", std::ffi::OsStr::new("H057")),
            ],
        );
        return;
    }
    let dir = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("hypr/H057");
    let req = UnixListener::bind(dir.join(".socket.sock")).unwrap();
    let ev = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
    // Hyprland writes its lock once its sockets listen.
    std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
    std::thread::spawn(move || {
        for mut s in req.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
            REQS.lock().unwrap().push(cmd.clone());
            RECEIVED.notify_all();
            if let Some(ping) = PING.get() {
                ping();
            }
            let _ = s.write_all(hypr_ipc::answer(&cmd, reply).as_bytes());
        }
    });
    // Once the Super interrupt hook is in, reload the config (which drops Hyprland's Lua state).
    std::thread::spawn(move || {
        let mut held = vec![];
        for mut s in ev.incoming().flatten() {
            drop(
                RECEIVED
                    .wait_while(REQS.lock().unwrap(), |r| hooks_in(r) == 0)
                    .unwrap(),
            );
            let _ = s.write_all(b"configreloaded>>\n");
            held.push(s);
        }
    });

    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut rt = Runtime::new(Some(ri));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let _ = PING.set(Box::new(wake.ping()));
    let r = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Hyprland\nItem {\n\
             property string wss: Hyprland.workspaces.values.map(w => [w.id, w.address, w.name, w.focused, w.active, w.toplevels.values.map(t => t.title).join('+')].join('|')).join(',')\n\
             property string focused: Hyprland.focusedWorkspace ? Hyprland.focusedWorkspace.address : ''\n\
             property string monWs: Hyprland.focusedMonitor && Hyprland.focusedMonitor.activeWorkspace ? '' + Hyprland.focusedMonitor.activeWorkspace.id : ''\n\
             Component.onCompleted: Hyprland.dispatch(\"hl.config({cursor = {no_warps = true}})\")\n\
             property string tops: Hyprland.toplevels.values.map(t => t.title + ':' + (t.workspace ? t.workspace.address : 'null')).join(',')\n}",
        )
        .unwrap();
    let want = "-1|special:magic|special:magic|false|false|magic,1|1|1|true|true|one";
    let s = |rt: &Runtime, p| match rt.get(r, p) {
        Value::Str(s) => s.to_string(),
        v => format!("{v:?}"),
    };
    let cfg = "eval hl.config({cursor = {no_warps = true}})";
    let sent = || REQS.lock().unwrap().iter().any(|c| c == cfg);
    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for the workspaces, the config request and the second hook");
    let ep = rt.process_fd();
    loop {
        settle(&mut rt);
        if s(&rt, "wss") == want && sent() && hooks() >= 2 {
            break;
        }
        if wake.wait(ep, None).fd {
            rt.process_fd_ready();
        }
    }
    assert_eq!(s(&rt, "wss"), want);
    assert_eq!(s(&rt, "focused"), "1");
    assert_eq!(s(&rt, "monWs"), "1");
    assert_eq!(s(&rt, "tops"), "one:1,magic:special:magic");
    // Not a dispatcher: `dispatch` would apply it and then fail `hl.dispatch(nil)`.
    assert!(sent(), "{:?}", REQS.lock().unwrap());
    // Sent at startup and again after `configreloaded`.
    assert_eq!(hooks(), 2, "{:?}", REQS.lock().unwrap());
    // Super is recognised by keysym in Hyprland's own key state, not by raw keycode.
    let reqs = REQS.lock().unwrap();
    let hook = reqs
        .iter()
        .find(|c| c.contains("searchToggleReleaseInterrupt"))
        .unwrap();
    assert!(hook.contains("hl.is_key_down(\"Super_L\")"), "{hook}");
    assert!(hook.contains("hl.is_key_down(\"Super_R\")"), "{hook}");
    assert!(!hook.contains("133") && !hook.contains("134"), "{hook}");
    // Super + mouse button or wheel is not a search either: each has a bind that passes the
    // press on, and a hook sent again first removes the binds of the one before.
    for k in [
        "mouse:272",
        "mouse:273",
        "mouse:274",
        "mouse:275",
        "mouse:276",
        "mouse:277",
        "mouse_up",
        "mouse_down",
        "mouse_left",
        "mouse_right",
    ] {
        assert!(hook.contains(&format!("\"{k}\"")), "{k}: {hook}");
    }
    assert!(
        hook.contains("ignore_mods = true, non_consuming = true"),
        "{hook}"
    );
    assert!(
        hook.contains("__ri_superIntrBinds or {}) do pcall(b.remove, b) end"),
        "{hook}"
    );
    // One batched request per refresh, never the separate queries.
    assert!(
        reqs.iter().any(|c| c.starts_with("[[BATCH]]j/monitors;")),
        "{reqs:?}"
    );
    assert!(!reqs.iter().any(|c| c == "j/monitors"), "{reqs:?}");
}

/// Loop turns until one changes nothing: what a poll writes counts only for the next one.
fn settle(rt: &mut Runtime) {
    loop {
        let rev = rt.revision();
        rt.poll_native();
        rt.settle();
        if rt.revision() == rev {
            return;
        }
    }
}
