//! `HyprlandKeybinds` refetches on `configreloaded`. A reload arriving while the previous
//! `j/binds` is still out was dropped (the Process was busy), leaving the binds of the old config.

mod hypr_ipc;

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::{Condvar, Mutex, OnceLock};
use testenv::Wakeups;

static BINDS: Mutex<usize> = Mutex::new(0);
static EVENTS: Mutex<Vec<std::os::unix::net::UnixStream>> = Mutex::new(Vec::new());
/// Whether the test let the first `j/binds` reply go: the fake holds it until the config
/// reload has reached the shell, so the reload lands while that fetch is out.
static RELEASED: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
/// The test loop's wakeup, pinged by the fake when a `j/binds` request or an event connection
/// comes in.
static PING: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

fn ping() {
    if let Some(p) = PING.get() {
        p();
    }
}

fn reply(cmd: &str) -> String {
    match cmd.strip_prefix("j/").unwrap_or(cmd) {
        "monitors" => r#"[{"id":0,"name":"DP-1","width":1920,"height":1080,"activeWorkspace":{"id":1,"name":"1"},"focused":true}]"#.into(),
        "workspaces" => r#"[{"id":1,"name":"1","monitor":"DP-1"}]"#.into(),
        "activeworkspace" => r#"{"id":1,"name":"1","monitor":"DP-1"}"#.into(),
        "clients" => "[]".into(),
        "activewindow" => "{}".into(),
        "binds" => {
            let n = {
                let mut b = BINDS.lock().unwrap();
                *b += 1;
                *b
            };
            ping();
            if n == 1 {
                let (released, cv) = &RELEASED;
                drop(cv.wait_while(released.lock().unwrap(), |r| !*r).unwrap());
            }
            format!(r#"[{{"key":"K{n}","description":"Apps: Bind {n}"}}]"#)
        }
        _ => "ok".into(),
    }
}

/// In its own process (`testenv::run_in_own_process`), whose `XDG_RUNTIME_DIR` and
/// `HYPRLAND_INSTANCE_SIGNATURE` name the fake compositor's sockets.
#[test]
fn reload_during_fetch_refetches() {
    const TEST: &str = concat!(module_path!(), "::reload_during_fetch_refetches");
    if !testenv::in_own_process(TEST) {
        let rt_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(rt_dir.path().join("hypr/LATEST")).unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_RUNTIME_DIR", rt_dir.path().as_os_str()),
                (
                    "HYPRLAND_INSTANCE_SIGNATURE",
                    std::ffi::OsStr::new("LATEST"),
                ),
                ("XDG_CONFIG_HOME", rt_dir.path().join("config").as_os_str()),
            ],
        );
        return;
    }
    let dir = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("hypr/LATEST");
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
                let _ = s.write_all(hypr_ipc::answer(&cmd, reply).as_bytes());
            });
        }
    });
    std::thread::spawn(move || {
        for c in ev.incoming().flatten() {
            EVENTS.lock().unwrap().push(c);
            ping();
        }
    });

    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut rt = Runtime::new(Some(ri.clone()));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let _ = PING.set(Box::new(wake.ping()));
    rt.load_singletons(&[ri.join("services"), ri.join("modules/common")]);
    let r = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Hyprland\nimport ri.services\nItem {\n\
             property string binds: JSON.stringify(HyprlandKeybinds.keybinds)\n\
             property int reloads: 0\n\
             Connections { target: Hyprland\n\
             function onRawEvent(e) { if (e.name == \"configreloaded\") reloads++ } }\n}",
        )
        .unwrap();
    let binds = |rt: &Runtime| match rt.get(r, "binds") {
        Value::Str(s) => s.to_string(),
        v => format!("{v:?}"),
    };
    let ep = rt.process_fd();
    let until = |rt: &mut Runtime, what: &str, done: &dyn Fn(&Runtime) -> bool| {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        loop {
            loop {
                let rev = rt.revision();
                rt.poll_native();
                rt.settle();
                if rt.revision() == rev {
                    break;
                }
            }
            if done(rt) {
                return;
            }
            if wake.wait(ep, None).fd {
                rt.process_fd_ready();
            }
        }
    };
    // The first fetch is out (held); the config reloads under it.
    until(&mut rt, "the first fetch and the event connection", &|_| {
        *BINDS.lock().unwrap() == 1 && !EVENTS.lock().unwrap().is_empty()
    });
    assert_eq!(*BINDS.lock().unwrap(), 1, "{:?}", rt.errors());
    for c in EVENTS.lock().unwrap().iter_mut() {
        c.write_all(b"configreloaded>>\n").unwrap();
    }
    until(&mut rt, "the reload to reach the shell", &|rt| {
        rt.get(r, "reloads").to_number() >= 1.0
    });
    *RELEASED.0.lock().unwrap() = true;
    RELEASED.1.notify_all();
    until(&mut rt, "the binds of the reloaded config", &|rt| {
        binds(rt).contains("Bind 2")
    });
    assert!(binds(&rt).contains("Bind 2"), "{}", binds(&rt));
    assert_eq!(*BINDS.lock().unwrap(), 2);
}
