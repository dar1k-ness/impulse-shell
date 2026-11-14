//! Hyprland IPC requests (refresh queries, dispatches) run on the `hypr-req` worker: a slow
//! compositor must not stall the main loop.

mod hypr_ipc;

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};
use testenv::Wakeups;

/// Requests the fake compositor has read.
static REQS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Requests whose reply the fake holds, each with the sender that releases it. The test loop
/// releases a reply only after a whole loop turn ran while it was held: a loop that waited on
/// Hyprland inside its turn would never finish that turn, and the test hangs until nextest's
/// slow-timeout fails it.
static HELD: Mutex<Vec<(String, Sender<()>)>> = Mutex::new(Vec::new());
/// Requests answered after a loop turn released them.
static ANSWERED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The test loop's wakeup, pinged by the fake when a request comes in or is answered.
static PING: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

fn ping() {
    if let Some(p) = PING.get() {
        p();
    }
}

fn reply(cmd: &str) -> &'static str {
    match cmd.strip_prefix("j/").unwrap_or(cmd) {
        "monitors" => {
            r#"[{"id":0,"name":"DP-1","width":1920,"height":1080,"activeWorkspace":{"id":1,"name":"1"},"focused":true}]"#
        }
        "workspaces" => r#"[{"id":1,"name":"1","monitor":"DP-1"}]"#,
        "activeworkspace" => r#"{"id":1,"name":"1","monitor":"DP-1"}"#,
        "clients" => "[]",
        "activewindow" => "{}",
        _ => "ok",
    }
}

/// In its own process (`testenv::run_in_own_process`), whose `XDG_RUNTIME_DIR` and
/// `HYPRLAND_INSTANCE_SIGNATURE` name the fake compositor's sockets.
#[test]
fn slow_compositor_does_not_block_the_loop() {
    const TEST: &str = concat!(module_path!(), "::slow_compositor_does_not_block_the_loop");
    if !testenv::in_own_process(TEST) {
        let rt_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(rt_dir.path().join("hypr/SLOW")).unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_RUNTIME_DIR", rt_dir.path().as_os_str()),
                ("HYPRLAND_INSTANCE_SIGNATURE", std::ffi::OsStr::new("SLOW")),
            ],
        );
        return;
    }
    let dir = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("hypr/SLOW");
    let req = UnixListener::bind(dir.join(".socket.sock")).unwrap();
    let ev = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
    // Hyprland writes its lock once its sockets listen.
    std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
    std::thread::spawn(move || {
        for mut s in req.incoming().flatten() {
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
                REQS.lock().unwrap().push(cmd.clone());
                let (release, released) = channel();
                HELD.lock().unwrap().push((cmd.clone(), release));
                ping();
                released.recv().unwrap();
                ANSWERED.lock().unwrap().push(cmd.clone());
                ping();
                let _ = s.write_all(hypr_ipc::answer(&cmd, |c| reply(c).into()).as_bytes());
            });
        }
    });
    std::thread::spawn(move || {
        let held: Vec<_> = ev.incoming().flatten().collect();
        drop(held);
    });

    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut rt = Runtime::new(Some(ri));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let _ = PING.set(Box::new(wake.ping()));
    let r = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Hyprland\nItem {\n  property string m: Hyprland.focusedMonitor ? Hyprland.focusedMonitor.name : \"\"\n  Component.onCompleted: Hyprland.dispatch(\"workspace 2\")\n}",
        )
        .unwrap();
    let name = |rt: &mut Runtime| match rt.get(r, "m") {
        Value::Str(s) => s.to_string(),
        _ => String::new(),
    };
    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for the refresh and the dispatch, each reply held through a loop turn");
    let ep = rt.process_fd();
    loop {
        let held: Vec<_> = std::mem::take(&mut *HELD.lock().unwrap());
        rt.poll_native();
        rt.settle();
        // This turn ran while these replies were held: it did not wait for Hyprland.
        for (_, release) in held {
            release.send(()).unwrap();
        }
        let reqs = REQS.lock().unwrap().clone();
        let dispatched = reqs.iter().any(|c| c == "dispatch workspace 2");
        let answered = ANSWERED.lock().unwrap().len() == reqs.len();
        if name(&mut rt) == "DP-1" && dispatched && answered {
            break;
        }
        if wake.wait(ep, None).fd {
            rt.process_fd_ready();
        }
    }
}
