//! `qs ipc prop get|listen|wait|call` and `qs kill` from the real quickshell client against the
//! real quickshell serving a fixture vs the native shell (`impulse-shell -p`) serving the same one:
//! same output for typed arguments, return values, properties and signals. Where quickshell
//! is bad native differs on purpose: a handler that throws is reported as an error.
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;

use testenv::Log;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const FIXTURE: &str = r##"import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    IpcHandler {
        target: "t"
        property string name: "héllo wörld"
        property int count: 3
        property bool on: true
        property real ratio: 0.1234567
        property color tint: "#80ff0000"
        property var hidden: 1
        signal ping(msg: string)
        signal tick()
        signal level(n: int)
        function bump(): void { count += 1 }
        function fire(m: string): void { ping(m) }
        function fireTick(): void { tick() }
        function fireLevel(n: int): void { level(n) }
        function add(a: int, b: int): int { return a + b }
        function kind(s: string): string { return typeof s + ":" + s }
        function flag(b: bool): string { return typeof b + ":" + b }
        function half(x: real): real { return x / 2 }
        function tone(c: color): color { return c }
        function third(x: int): int { return x / 3 }
        function boom(): void { throw new Error("kaput") }
        function untyped(x) { return 1 }
    }
    Component.onCompleted: console.log("LOADED")
}
"##;

/// Quickshell's `qs` finds an instance in Quickshell's part of the runtime directory, and
/// the native shell registers in its own. The tests here ask both through `qs`, so the native
/// instance `pid` is entered in Quickshell's part too, under the path of its config.
fn known_to_qs(pid: u32) {
    let rt = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let own = rt.join("impulse-shell");
    let id = std::fs::read_link(own.join("by-pid").join(pid.to_string())).unwrap();
    let id = id.file_name().unwrap().to_owned();
    let theirs = rt.join("quickshell");
    for d in ["by-id", "by-path"] {
        std::fs::create_dir_all(theirs.join(d)).unwrap();
    }
    let link = |to: std::path::PathBuf, at: std::path::PathBuf| {
        let _ = std::os::unix::fs::symlink(to, at);
    };
    link(own.join("by-id").join(&id), theirs.join("by-id").join(&id));
    for e in std::fs::read_dir(own.join("by-path")).unwrap() {
        let e = e.unwrap();
        if e.path().join(&id).exists() {
            link(e.path(), theirs.join("by-path").join(e.file_name()));
        }
    }
}

fn qs(dir: &Path, args: &[&str]) -> Output {
    Command::new("qs")
        .arg("-p")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "out:{}err:{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A shell serving a fixture, and its console (stdout and stderr) as it arrives.
struct Server {
    proc: Kill,
    log: Arc<Log>,
}

/// The server's log line for a registered `qs ipc listen|wait` (Quickshell's
/// `quickshell.ipc` debug message under `-vv`; the native shell's under `RI_LOG=ri::ipc=debug`).
const LISTENER_CREATED: &str = "Remote listener created for";

fn listeners_created(log: &str) -> usize {
    log.matches(LISTENER_CREATED).count()
}

/// Start `bin -p dir` (the fixture's root logs `LOADED` in `Component.onCompleted`) and wait
/// for that line: both shells register the instance before the QML runs and answer a call
/// once their event loop runs, so the `t` target answers the first call from then on.
fn serve(bin: &str, dir: &Path) -> Server {
    let (rd, wr) = std::io::pipe().unwrap();
    let mut c = Command::new(bin);
    c.arg("-p").arg(dir);
    if bin == "qs" {
        // Debug verbosity: `quickshell.ipc` debug lines and the QML console reach stderr.
        c.arg("-vv");
    }
    let child = c
        .env("RI_INTERP", "1")
        .env("RI_LOG", "ri::ipc=debug")
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    drop(c);
    let s = Server {
        proc: Kill(child),
        log: Log::capture(rd),
    };
    s.log
        .wait(|l| l.lines().any(|l| l.contains("LOADED")))
        .unwrap_or_else(|l| panic!("{bin} never loaded the fixture:\n{l}"));
    if bin != "qs" {
        known_to_qs(s.proc.0.id());
    }
    let o = qs(dir, &["ipc", "call", "t", "add", "1", "1"]);
    assert_eq!(text(&o), "out:2\nerr:", "{bin} did not serve t");
    s
}

/// `qs ipc wait|listen t <signal>`: once the server registered the listener, `fire` is called
/// `lines` times and the client's output read until that many values arrived.
fn listened(
    srv: &Server,
    dir: &Path,
    how: &str,
    signal: &str,
    fire: &[&str],
    lines: usize,
) -> String {
    let before = listeners_created(&srv.log.text());
    let (rd, wr) = std::io::pipe().unwrap();
    let mut k = Kill(
        Command::new("qs")
            .arg("-p")
            .arg(dir)
            .args(["ipc", how, "t", signal])
            .stdin(Stdio::null())
            .stdout(wr)
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let out = Log::capture(rd);
    srv.log
        .wait(|l| listeners_created(l) > before)
        .unwrap_or_else(|l| panic!("qs ipc {how} t {signal}: no listener registered:\n{l}"));
    let mut a = vec!["ipc", "call", "t"];
    a.extend_from_slice(fire);
    for _ in 0..lines {
        let o = qs(dir, &a);
        assert!(o.status.success(), "{a:?}: {}", text(&o));
    }
    out.wait(|o| o.lines().count() >= lines)
        .unwrap_or_else(|o| panic!("qs ipc {how} t {signal}: {lines} values expected: {o:?}"));
    if how == "wait" {
        let status = testenv::wait_exit(&mut k.0);
        assert!(status.success(), "wait: {status}");
    }
    out.text()
}

/// Everything both servers must answer alike.
fn transcript(srv: &Server, dir: &Path) -> Vec<String> {
    let mut t = Vec::new();
    for p in ["name", "count", "on", "ratio", "tint", "hidden", "nope"] {
        t.push(format!(
            "prop {p}: {}",
            text(&qs(dir, &["ipc", "prop", "get", "t", p]))
        ));
    }
    t.push(format!(
        "prop no target: {}",
        text(&qs(dir, &["ipc", "prop", "get", "x", "name"]))
    ));
    let calls: &[&[&str]] = &[
        &["add", "2", "3"],
        &["add", "2"],
        &["add", "2", "3", "4"],
        &["add", "2", "x"],
        &["add", "2", " 7 "],
        &["add", "1.5", "1"],
        &["kind", "1"],
        &["kind", "true"],
        &["flag", "1"],
        &["flag", "0"],
        &["flag", "false"],
        &["flag", "yes"],
        &["half", "0.1234567"],
        &["half", "1e3"],
        &["half", "abc"],
        &["tone", "f00"],
        &["tone", "#80ff0000"],
        &["tone", "red"],
        &["tone", "nocolor"],
        &["third", "10"],
        &["untyped", "1"],
        &["nope"],
    ];
    for c in calls {
        let mut a = vec!["ipc", "call", "t"];
        a.extend_from_slice(c);
        t.push(format!("call {}: {}", c.join(" "), text(&qs(dir, &a))));
    }
    t.push(format!(
        "wait ping: {}",
        listened(srv, dir, "wait", "ping", &["fire", "hello there"], 1)
    ));
    t.push(format!(
        "wait tick: {}",
        listened(srv, dir, "wait", "tick", &["fireTick"], 1)
    ));
    t.push(format!(
        "wait level: {}",
        listened(srv, dir, "wait", "level", &["fireLevel", "-4"], 1)
    ));
    t.push(format!(
        "wait countChanged: {}",
        listened(srv, dir, "wait", "countChanged", &["bump"], 1)
    ));
    t.push(format!(
        "prop count after bumps: {}",
        text(&qs(dir, &["ipc", "prop", "get", "t", "count"]))
            .replace(|c: char| c.is_ascii_digit(), "N")
    ));
    let streamed = listened(srv, dir, "listen", "ping", &["fire", "again"], 2);
    t.push(format!(
        "listen ping: {}",
        streamed.lines().take(2).collect::<Vec<_>>().join("|")
    ));
    t.push(format!(
        "wait nosignal: {}",
        text(&qs(dir, &["ipc", "wait", "t", "nosignal"]))
    ));
    t.push(format!(
        "wait no target: {}",
        text(&qs(dir, &["ipc", "wait", "x", "ping"]))
    ));
    t
}

/// `qs kill`: the instance exits with status 0.
fn killed(dir: &Path, mut srv: Server) {
    let o = qs(dir, &["kill"]);
    assert!(o.status.success(), "kill: {}", text(&o));
    let s = testenv::wait_exit(&mut srv.proc.0);
    assert!(s.success(), "instance exit status after kill: {s}");
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn ipc_matches_quickshell() {
    if !testenv::require("qs") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let a = base.path().join("orig");
    let b = base.path().join("native");
    for d in [&a, &b] {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join("shell.qml"), FIXTURE).unwrap();
    }

    let k = serve("qs", &a);
    let orig = transcript(&k, &a);
    // quickshell swallows the exception and reports success with no output.
    let boom_orig = qs(&a, &["ipc", "call", "t", "boom"]);
    killed(&a, k);

    let native_bin = env!("CARGO_BIN_EXE_impulse-shell");
    let k = serve(native_bin, &b);
    let native = transcript(&k, &b);
    // A throwing handler is an error: quickshell's client sees an answer it can't decode, the
    // native client prints the message and fails; the shell keeps serving.
    let boom_qs = text(&qs(&b, &["ipc", "call", "t", "boom"]));
    let boom_native = Command::new(native_bin)
        .arg("-p")
        .arg(&b)
        .args(["ipc", "call", "t", "boom"])
        .output()
        .unwrap();
    let still = text(&qs(&b, &["ipc", "call", "t", "add", "1", "1"]));
    killed(&b, k);

    for (o, n) in orig.iter().zip(&native) {
        assert_eq!(n, o);
    }
    assert_eq!(native.len(), orig.len());
    assert_eq!(
        text(&boom_orig),
        "out:err:",
        "quickshell's answer to a throwing handler"
    );
    assert!(boom_qs.contains("invalid IPC response"), "{boom_qs}");
    assert_eq!(boom_native.status.code(), Some(255));
    assert!(
        String::from_utf8_lossy(&boom_native.stderr).contains("kaput"),
        "{}",
        text(&boom_native)
    );
    assert_eq!(still, "out:2\nerr:");
}

/// Native only: a `qs ipc listen` on a handler that goes away (its Loader unloads) ends; the
/// client is not left waiting forever on a signal nothing can emit any more.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn listen_ends_when_its_handler_is_destroyed() {
    if !testenv::require("qs") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let dir = base.path();
    std::fs::write(
        dir.join("shell.qml"),
        r#"import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    IpcHandler {
        target: "t"
        function add(a: int, b: int): int { return a + b }
        function unload(): void { inner.active = false }
    }
    Loader {
        id: inner
        sourceComponent: Component {
            IpcHandler {
                target: "inner"
                signal ping()
                function fire(): void { ping() }
            }
        }
    }
    Component.onCompleted: console.log("LOADED")
}
"#,
    )
    .unwrap();
    let k = serve(env!("CARGO_BIN_EXE_impulse-shell"), dir);
    let before = listeners_created(&k.log.text());
    let (rd, wr) = std::io::pipe().unwrap();
    let mut l = Kill(
        Command::new("qs")
            .arg("-p")
            .arg(dir)
            .args(["ipc", "listen", "inner", "ping"])
            .stdin(Stdio::null())
            .stdout(wr)
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let out = Log::capture(rd);
    k.log
        .wait(|l| listeners_created(l) > before)
        .unwrap_or_else(|l| panic!("qs ipc listen inner ping: no listener registered:\n{l}"));
    let o = qs(dir, &["ipc", "call", "inner", "fire"]);
    assert!(o.status.success(), "fire: {}", text(&o));
    out.wait(|o| !o.is_empty())
        .unwrap_or_else(|o| panic!("listen never received ping: {o:?}"));
    let o = qs(dir, &["ipc", "call", "t", "unload"]);
    assert!(o.status.success(), "unload: {}", text(&o));
    testenv::wait_exit(&mut l.0);
    killed(dir, k);
}

/// Native only: the instance is registered before the shell's QML runs, as in Quickshell, so a
/// client told "ready" by `Component.onCompleted` reaches it. The shell is held inside that
/// handler (its next console line blocks on a stderr pipe nobody drains) while the client
/// looks for it, so the ordering is checked without depending on timing.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn instance_is_registered_before_component_completed() {
    use std::io::{BufRead, BufReader};
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let dir = base.path();
    std::fs::write(
        dir.join("shell.qml"),
        r#"import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    IpcHandler {
        target: "t"
        function add(a: int, b: int): int { return a + b }
    }
    Component.onCompleted: {
        console.log("ORDER ready")
        console.log("x".repeat(1 << 20))
    }
}
"#,
    )
    .unwrap();
    let bin = env!("CARGO_BIN_EXE_impulse-shell");
    let (rd, wr) = std::io::pipe().unwrap();
    let k = Kill(
        Command::new(bin)
            .arg("-p")
            .arg(dir)
            .env("RI_INTERP", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(wr)
            .spawn()
            .unwrap(),
    );
    let mut err = BufReader::new(rd);
    let mut seen = String::new();
    loop {
        let mut l = String::new();
        let n = err.read_line(&mut l).unwrap();
        assert!(n > 0, "shell exited before ready:\n{seen}");
        if l.trim_end().ends_with("ORDER ready") {
            break;
        }
        seen += &l;
    }
    // The shell is now blocked writing the 1 MiB line, still inside Component.onCompleted.
    let cli = |args: &[&str]| {
        Command::new(bin)
            .arg("-p")
            .arg(dir)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let list = cli(&["list", "--json"]);
    let found: serde_json::Value = serde_json::from_slice(&list.stdout)
        .unwrap_or_else(|_| panic!("no instance listed during onCompleted: {}", text(&list)));
    let pids: Vec<_> = found
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["pid"].as_i64())
        .collect();
    assert_eq!(pids, [Some(i64::from(k.0.id()))], "{found}");
    let call = Command::new(bin)
        .arg("-p")
        .arg(dir)
        .args(["ipc", "call", "t", "add", "1", "2"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let drain = std::thread::spawn(move || {
        let _ = std::io::copy(&mut err, &mut std::io::sink());
    });
    let o = call.wait_with_output().unwrap();
    assert_eq!(text(&o), "out:3\nerr:");
    drop(k);
    drain.join().unwrap();
}

/// Copies one direction of a Wayland connection, passing on the fds each chunk carries.
fn pump(from: &std::os::unix::net::UnixStream, to: &std::os::unix::net::UnixStream) {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let mut buf = [0u8; 4096];
    loop {
        let mut ctl = [0u64; 64];
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        };
        // SAFETY: msghdr is integers and pointers, for which all zeros (null) is valid.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = ctl.as_mut_ptr().cast();
        msg.msg_controllen = std::mem::size_of_val(&ctl) as _;
        // SAFETY: `msg` points at `iov` and `ctl`, which outlive the call; the kernel writes at
        // most the lengths given.
        let n = unsafe { libc::recvmsg(from.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC) };
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if n <= 0 {
            let _ = to.shutdown(std::net::Shutdown::Write);
            return;
        }
        let mut fds = Vec::new();
        // SAFETY: walking the control messages the kernel just wrote into `ctl`.
        let mut c = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        while !c.is_null() {
            // SAFETY: a non-null header CMSG_FIRSTHDR/CMSG_NXTHDR returned lies within `ctl`.
            let h = unsafe { &*c };
            if h.cmsg_level == libc::SOL_SOCKET && h.cmsg_type == libc::SCM_RIGHTS {
                // SAFETY: CMSG_LEN only computes a length.
                let len = h.cmsg_len as usize - unsafe { libc::CMSG_LEN(0) } as usize;
                // SAFETY: `c` is a valid header; its `len` data bytes follow it in `ctl`.
                let data = unsafe { libc::CMSG_DATA(c) }.cast::<libc::c_int>();
                for i in 0..len / size_of::<libc::c_int>() {
                    // SAFETY: fd `i` of the `len` bytes at `data`, read unaligned.
                    let fd = unsafe { data.wrapping_add(i).read_unaligned() };
                    // SAFETY: SCM_RIGHTS fds are new descriptors this process now owns.
                    fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
                }
            }
            // SAFETY: `c` is a header of `msg`'s control buffer.
            c = unsafe { libc::CMSG_NXTHDR(&msg, c) };
        }
        let mut off = 0;
        while off < n as usize {
            let mut ctl = [0u64; 64];
            let mut iov = libc::iovec {
                iov_base: buf[off..].as_mut_ptr().cast(),
                iov_len: n as usize - off,
            };
            // SAFETY: msghdr is integers and pointers, for which all zeros (null) is valid.
            let mut out: libc::msghdr = unsafe { std::mem::zeroed() };
            out.msg_iov = &mut iov;
            out.msg_iovlen = 1;
            if off == 0 && !fds.is_empty() {
                let bytes = (fds.len() * size_of::<libc::c_int>()) as u32;
                out.msg_control = ctl.as_mut_ptr().cast();
                // SAFETY: CMSG_SPACE only computes a length; at most 64 fds fit `ctl`.
                out.msg_controllen = unsafe { libc::CMSG_SPACE(bytes) } as _;
                // SAFETY: `msg_controllen` is CMSG_SPACE(bytes) of `ctl`, room for one header.
                let h = unsafe { libc::CMSG_FIRSTHDR(&out) };
                // SAFETY: CMSG_FIRSTHDR of that buffer is non-null and lies within `ctl`.
                let hdr = unsafe { &mut *h };
                hdr.cmsg_level = libc::SOL_SOCKET;
                hdr.cmsg_type = libc::SCM_RIGHTS;
                // SAFETY: CMSG_LEN only computes a length.
                hdr.cmsg_len = unsafe { libc::CMSG_LEN(bytes) } as _;
                // SAFETY: `h` is a valid header; its data follows it within `ctl`.
                let data = unsafe { libc::CMSG_DATA(h) }.cast::<libc::c_int>();
                for (i, fd) in fds.iter().enumerate() {
                    // SAFETY: fd `i` of the `bytes` CMSG_SPACE reserved at `data`, unaligned.
                    unsafe { data.wrapping_add(i).write_unaligned(fd.as_raw_fd()) };
                }
            }
            // SAFETY: `out` points at `iov` (within `buf`) and `ctl`, which outlive the call.
            let w = unsafe { libc::sendmsg(to.as_raw_fd(), &out, libc::MSG_NOSIGNAL) };
            if w < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return;
            }
            off += w as usize;
        }
    }
}

/// `qs ipc call` right after the launch must reach the shell, which registers itself before
/// it talks to the compositor, as Quickshell does. The shell's Wayland display is a socket this
/// test holds unanswered: the client looks for the instance and makes its call while the shell
/// is stuck in its first roundtrip, and only then is the connection handed to the compositor,
/// so the ordering is checked without depending on timing.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn instance_is_registered_before_the_wayland_connection() {
    use std::os::unix::net::{UnixListener, UnixStream};
    let Some(display) = std::env::var_os("WAYLAND_DISPLAY") else {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    };
    let compositor = match std::path::PathBuf::from(&display) {
        p if p.is_absolute() => p,
        p => std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join(p),
    };
    let base = tempfile::tempdir().unwrap();
    let dir = base.path();
    std::fs::write(
        dir.join("shell.qml"),
        r#"import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    IpcHandler {
        target: "t"
        function add(a: int, b: int): int { return a + b }
    }
}
"#,
    )
    .unwrap();
    let held = dir.join("wayland-held");
    let listener = UnixListener::bind(&held).unwrap();
    let bin = env!("CARGO_BIN_EXE_impulse-shell");
    let cli = || {
        let mut c = Command::new(bin);
        c.arg("-p")
            .arg(dir)
            .env("WAYLAND_DISPLAY", &held)
            .stdin(Stdio::null());
        c
    };
    let k = Kill(
        cli()
            .env("RI_INTERP", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let (shell, _) = listener.accept().unwrap();
    // The shell is connected to the display and waits for an answer nobody gives yet.
    let list = cli().args(["list", "--json"]).output().unwrap();
    let found: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap_or_else(|_| {
        panic!(
            "no instance listed before the Wayland connection: {}",
            text(&list)
        )
    });
    let pids: Vec<_> = found
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["pid"].as_i64())
        .collect();
    assert_eq!(pids, [Some(i64::from(k.0.id()))], "{found}");
    let call = cli()
        .args(["ipc", "call", "t", "add", "1", "2"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let compositor = UnixStream::connect(compositor).unwrap();
    let (back_from, back_to) = (compositor.try_clone().unwrap(), shell.try_clone().unwrap());
    std::thread::spawn(move || pump(&shell, &compositor));
    std::thread::spawn(move || pump(&back_from, &back_to));
    let o = call.wait_with_output().unwrap();
    assert_eq!(text(&o), "out:3\nerr:");
}
