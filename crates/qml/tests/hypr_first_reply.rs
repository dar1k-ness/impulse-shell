//! The first Hyprland reply reaches the bindings that read Hyprland state as property
//! changes; no other binding runs again (Qt has no pass over every binding).

mod hypr_ipc;

use qml::runtime::Runtime;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;

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
fn first_reply_reruns_only_its_dependents() {
    const TEST: &str = concat!(module_path!(), "::first_reply_reruns_only_its_dependents");
    if !testenv::in_own_process(TEST) {
        let rt_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(rt_dir.path().join("hypr/FIRST")).unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_RUNTIME_DIR", rt_dir.path().as_os_str()),
                ("HYPRLAND_INSTANCE_SIGNATURE", std::ffi::OsStr::new("FIRST")),
            ],
        );
        return;
    }
    let dir = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("hypr/FIRST");
    let req = UnixListener::bind(dir.join(".socket.sock")).unwrap();
    let ev = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
    // Hyprland writes its lock once its sockets listen.
    std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
    std::thread::spawn(move || {
        for mut s in req.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = s.write_all(hypr_ipc::answer(&cmd, |c| reply(c).into()).as_bytes());
        }
    });
    std::thread::spawn(move || {
        let held: Vec<_> = ev.incoming().flatten().collect();
        drop(held);
    });
    // Each refresh is answered in the turn that asked for it.
    qml::hypr::set_sync(true);
    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut rt = Runtime::new(Some(ri));
    let r = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Hyprland\nItem {\n  property string log: \"\"\n  property int k: 1\n  property var o: ({ v: k })\n  onOChanged: log += \"o\"\n  property var arr: [k, 2]\n  onArrChanged: log += \"a\"\n  property string m: Hyprland.focusedMonitor ? Hyprland.focusedMonitor.name : \"\"\n  onMChanged: log += \"m\"\n}",
        )
        .unwrap();
    rt.settle();
    assert_eq!(rt.get(r, "log").to_js_string(), "oa");
    rt.poll_native();
    rt.settle();
    assert_eq!(rt.get(r, "m").to_js_string(), "DP-1");
    assert_eq!(rt.get(r, "log").to_js_string(), "oam");
}
