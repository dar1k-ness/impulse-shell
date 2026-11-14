//! A `Hyprland` singleton made before its Hyprland instance is up (the shell started ahead of
//! the compositor, or with its environment imported early) fills in once the instance comes
//! up, woken by the instance's directory and sockets appearing.

mod hypr_ipc;

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use testenv::Wakeups;

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

#[test]
fn singleton_made_before_the_instance_fills_in_when_it_comes_up() {
    const TEST: &str = concat!(
        module_path!(),
        "::singleton_made_before_the_instance_fills_in_when_it_comes_up"
    );
    // The singleton finds the instance through the environment, so the test runs in a
    // process of its own with a fresh runtime directory and an instance signature set.
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_RUNTIME_DIR", dir.path().as_os_str()),
                ("HYPRLAND_INSTANCE_SIGNATURE", "LATE".as_ref()),
            ],
        );
        return;
    }
    let rt_dir = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());

    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut rt = Runtime::new(Some(ri));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let r = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Hyprland\nItem {\n  property string m: Hyprland.focusedMonitor ? Hyprland.focusedMonitor.name : \"\"\n  property int n: Hyprland.workspaces.values.length\n}",
        )
        .unwrap();
    let state = |rt: &mut Runtime| {
        let m = match rt.get(r, "m") {
            Value::Str(s) => s.to_string(),
            _ => String::new(),
        };
        (m, rt.get(r, "n").to_js_string())
    };
    rt.poll_native();
    rt.settle();
    assert_eq!(state(&mut rt), (String::new(), "0".into()));

    // As `CCompositor`: `$XDG_RUNTIME_DIR/hypr`, the instance directory, the event socket,
    // the request socket, then the lock.
    let dir = rt_dir.join("hypr").join("LATE");
    std::fs::create_dir(dir.parent().unwrap()).unwrap();
    std::fs::create_dir(&dir).unwrap();
    let ev = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
    let req = UnixListener::bind(dir.join(".socket.sock")).unwrap();
    std::thread::spawn(move || {
        let held: Vec<_> = ev.incoming().flatten().collect();
        drop(held);
    });
    std::thread::spawn(move || {
        for mut s in req.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = s.write_all(hypr_ipc::answer(&cmd, |c| reply(c).into()).as_bytes());
        }
    });
    std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n").unwrap();

    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for the Hyprland state");
    loop {
        rt.poll_native();
        rt.settle();
        if state(&mut rt) == ("DP-1".into(), "1".into()) {
            break;
        }
        wake.wait(None, None);
    }
}

/// A socket bound at `path` but not listening yet, as Hyprland's `.socket.sock` is between
/// bind(2) and listen(2); it appears at the path at once (renamed from a scratch name).
fn bound_unlistened(path: &Path) -> std::os::fd::OwnedFd {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    let scratch = path.with_extension("new");
    // SAFETY: socket(2) takes plain integers.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    assert!(fd >= 0);
    // SAFETY: `fd` is the socket created above, open and owned by nothing else.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: sockaddr_un is integers and a byte array, for which all zeros is valid.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (d, s) in addr.sun_path.iter_mut().zip(scratch.as_os_str().as_bytes()) {
        *d = *s as libc::c_char;
    }
    let len = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    // SAFETY: `addr` is a live sockaddr_un and `len` its size.
    let r = unsafe { libc::bind(fd.as_raw_fd(), (&raw const addr).cast(), len) };
    assert_eq!(r, 0);
    std::fs::rename(&scratch, path).unwrap();
    fd
}

#[test]
fn singleton_made_while_the_request_socket_only_binds_waits_for_the_lock() {
    // `.socket.sock` exists from bind(2), before Hyprland listens on it; a singleton made
    // then connected at once, was refused, and stayed empty until Hyprland's first event.
    const TEST: &str = concat!(
        module_path!(),
        "::singleton_made_while_the_request_socket_only_binds_waits_for_the_lock"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("hypr/BOUND")).unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_RUNTIME_DIR", dir.path().as_os_str()),
                ("HYPRLAND_INSTANCE_SIGNATURE", "BOUND".as_ref()),
            ],
        );
        return;
    }
    let rt_dir = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let dir = rt_dir.join("hypr").join("BOUND");
    // As `CCompositor::initManagers`: the event socket listens, the request socket is bound.
    let ev = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
    std::thread::spawn(move || {
        let held: Vec<_> = ev.incoming().flatten().collect();
        drop(held);
    });
    let bound = bound_unlistened(&dir.join(".socket.sock"));

    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut rt = Runtime::new(Some(ri));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let r = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Hyprland\nItem {\n  property string m: Hyprland.focusedMonitor ? Hyprland.focusedMonitor.name : \"\"\n  property int n: Hyprland.workspaces.values.length\n}",
        )
        .unwrap();
    let state = |rt: &mut Runtime| {
        let m = match rt.get(r, "m") {
            Value::Str(s) => s.to_string(),
            _ => String::new(),
        };
        (m, rt.get(r, "n").to_js_string())
    };
    rt.poll_native();
    rt.settle();
    assert_eq!(state(&mut rt), (String::new(), "0".into()));
    // Not connected: it waits for the instance to be up.
    assert!(rt.world.borrow().hypr.is_none());
    assert!(rt.world.borrow().hypr_wait.is_some());

    // listen(2), then the lock (`createLockFile` in `startCompositor`).
    use std::os::fd::AsRawFd;
    // SAFETY: `bound` is an open, bound stream socket.
    assert_eq!(unsafe { libc::listen(bound.as_raw_fd(), 16) }, 0);
    let req = UnixListener::from(bound);
    std::thread::spawn(move || {
        for mut s in req.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = s.write_all(hypr_ipc::answer(&cmd, |c| reply(c).into()).as_bytes());
        }
    });
    std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n").unwrap();

    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for the Hyprland state");
    loop {
        rt.poll_native();
        rt.settle();
        if state(&mut rt) == ("DP-1".into(), "1".into()) {
            break;
        }
        wake.wait(None, None);
    }
}
