//! What a Hyprland event costs and what follows a reconnect, against a fake compositor with
//! 2 monitors, 10 workspaces and 30 windows: an event that changes no monitor, workspace or
//! window takes no snapshot, a snapshot equal to the last one writes no `lastIpcObject`, a
//! reconnect sets the Super hook again, a failed dispatch reports Hyprland's reply, a
//! snapshot that does not parse is not taken again on every poll, an `urgent` event marks its
//! window and that window's workspace until either is focused, and `usingLua` is what
//! `j/status` says.

mod hypr_ipc;

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Condvar, Mutex, OnceLock};
use testenv::Wakeups;

/// Requests the fake compositor has read; [`RECEIVED`] is notified on each.
static REQS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static RECEIVED: Condvar = Condvar::new();
static PING: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
/// The title of window `0xa0`.
static TITLE: Mutex<String> = Mutex::new(String::new());
/// While set, `j/clients` answers with text that is no JSON.
static BROKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The focused workspace's id and the active window's index (`WINDOWS`: none).
static FOCUSED: AtomicUsize = AtomicUsize::new(1);
static ACTIVE: AtomicUsize = AtomicUsize::new(WINDOWS);
/// What `j/status` answers.
static STATUS: Mutex<&str> = Mutex::new("ok");

const INSTANCE: &str = "HEVENTS";
const WINDOWS: usize = 30;

fn ws(i: usize) -> String {
    format!(r#"{{"id":{i},"name":"{i}"}}"#)
}

fn reply(cmd: &str) -> String {
    let body = match cmd.strip_prefix("j/").unwrap_or(cmd) {
        "monitors" => (0..2)
            .map(|m| {
                format!(
                    r#"{{"id":{m},"name":"DP-{m}","width":1920,"height":1080,"x":{},"y":0,"activeWorkspace":{},"specialWorkspace":{{"id":0,"name":""}},"focused":{}}}"#,
                    m * 1920,
                    ws(m + 1),
                    m == 0
                )
            })
            .collect::<Vec<_>>()
            .join(","),
        "workspaces" => (1..=10)
            .map(|i| {
                format!(
                    r#"{{"id":{i},"name":"{i}","monitor":"DP-{m}","monitorID":{m},"windows":3,"hasfullscreen":false,"lastwindow":"0x{:x}","lastwindowtitle":"foot"}}"#,
                    0xa0 + i - 1,
                    m = (i - 1) % 2
                )
            })
            .collect::<Vec<_>>()
            .join(","),
        "activeworkspace" => {
            let i = FOCUSED.load(SeqCst);
            format!(
                r#"{{"id":{i},"name":"{i}","monitor":"DP-{m}","monitorID":{m}}}"#,
                m = (i - 1) % 2
            )
        }
        "clients" => (0..WINDOWS).map(client).collect::<Vec<_>>().join(","),
        "activewindow" => match ACTIVE.load(SeqCst) {
            WINDOWS => "{}".into(),
            i => client(i),
        },
        "status" => STATUS.lock().unwrap().to_string(),
        c if c.starts_with("dispatch nosuch") => "Invalid dispatcher nosuch".into(),
        _ => "ok".into(),
    };
    if cmd == "j/clients" && BROKEN.load(std::sync::atomic::Ordering::SeqCst) {
        return "not json".into();
    }
    if matches!(cmd, "j/monitors" | "j/workspaces" | "j/clients") {
        format!("[{body}]")
    } else {
        body
    }
}

fn client(i: usize) -> String {
    let title = if i == 0 {
        TITLE.lock().unwrap().clone()
    } else {
        format!("window {i}")
    };
    format!(
        r#"{{"address":"0x{:x}","mapped":true,"hidden":false,"at":[{},{}],"size":[800,600],"workspace":{},"floating":false,"monitor":{},"class":"foot","title":"{title}","initialClass":"foot","initialTitle":"foot","pid":{},"xwayland":false,"pinned":false,"fullscreen":0,"grouped":[],"tags":[],"focusHistoryID":{i}}}"#,
        0xa0 + i,
        i * 10,
        i * 5,
        ws(i % 10 + 1),
        i % 2,
        1000 + i
    )
}

fn snapshots() -> usize {
    REQS.lock()
        .unwrap()
        .iter()
        .filter(|c| c.starts_with("[[BATCH]]"))
        .count()
}

fn is_hook(c: &str) -> bool {
    c.starts_with("eval ") && c.contains("rustshell:searchToggleReleaseInterrupt")
}

/// In its own process (`testenv::run_in_own_process`) whose `XDG_RUNTIME_DIR` and
/// `HYPRLAND_INSTANCE_SIGNATURE` name the fake compositor's sockets: true there.
fn own_process(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let rt_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(rt_dir.path().join("hypr").join(INSTANCE)).unwrap();
    testenv::run_in_own_process(test, env(rt_dir.path()));
    false
}

fn env(rt_dir: &Path) -> [(&'static str, std::ffi::OsString); 2] {
    [
        ("XDG_RUNTIME_DIR", rt_dir.as_os_str().to_owned()),
        ("HYPRLAND_INSTANCE_SIGNATURE", INSTANCE.into()),
    ]
}

/// The fake compositor's sockets: requests are answered on a thread, the event socket's
/// connections are returned as they come (the shell connects in its first poll).
fn compositor() -> std::sync::mpsc::Receiver<UnixStream> {
    *TITLE.lock().unwrap() = "window 0".into();
    let dir = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap())
        .join("hypr")
        .join(INSTANCE);
    let req = UnixListener::bind(dir.join(".socket.sock")).unwrap();
    let ev = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
    std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
    std::thread::spawn(move || {
        for mut s in req.incoming().flatten() {
            let mut buf = vec![0u8; 65536];
            let n = s.read(&mut buf).unwrap_or(0);
            let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
            let out = hypr_ipc::answer(&cmd, reply);
            REQS.lock().unwrap().push(cmd);
            RECEIVED.notify_all();
            if let Some(ping) = PING.get() {
                ping();
            }
            let _ = s.write_all(out.as_bytes());
        }
    });
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for s in ev.incoming().flatten() {
            if tx.send(s).is_err() {
                return;
            }
        }
    });
    rx
}

const ROOT: &str = "import QtQuick\nimport Rustshell.Hyprland\nItem {\n\
     property int events: 0\n\
     property int ipcChanges: 0\n\
     property var ipc: [Hyprland.monitors.values.map(m => m.lastIpcObject), Hyprland.workspaces.values.map(w => w.lastIpcObject), Hyprland.toplevels.values.map(t => t.lastIpcObject)]\n\
     onIpcChanged: ipcChanges++\n\
     property int tops: Hyprland.toplevels.values.length\n\
     property string title: Hyprland.toplevels.values.length ? Hyprland.toplevels.values[0].title : ''\n\
     Connections { target: Hyprland; function onRawEvent(e) { events++ } }\n}";

struct Shell {
    rt: Runtime,
    wake: Wakeups,
    root: usize,
}

impl Shell {
    fn new(qml: &str) -> Shell {
        let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
        let mut rt = Runtime::new(Some(ri));
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let _ = PING.set(Box::new(wake.ping()));
        let root = rt.load_str(qml).unwrap();
        Shell { rt, wake, root }
    }

    fn num(&self, p: &str) -> f64 {
        match self.rt.get(self.root, p) {
            Value::Num(n) => n,
            v => panic!("{p}: {v:?}"),
        }
    }

    fn str(&self, p: &str) -> String {
        match self.rt.get(self.root, p) {
            Value::Str(s) => s.to_string(),
            v => format!("{v:?}"),
        }
    }

    /// Turns the loop until `done`, then until the requests it queued are answered and their
    /// replies applied.
    fn until(&mut self, done: impl Fn(&Shell) -> bool) {
        let ep = self.rt.process_fd();
        loop {
            settle(&mut self.rt);
            if done(self) {
                break;
            }
            if self.wake.wait(ep, None).fd {
                self.rt.process_fd_ready();
            }
        }
        loop {
            qml::hypr::drain();
            let n = snapshots();
            settle(&mut self.rt);
            qml::hypr::drain();
            if snapshots() == n {
                settle(&mut self.rt);
                return;
            }
        }
    }
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

#[test]
fn only_state_events_take_a_snapshot() {
    if !own_process(concat!(
        module_path!(),
        "::only_state_events_take_a_snapshot"
    )) {
        return;
    }
    let ev = compositor();
    let mut sh = Shell::new(ROOT);
    eprintln!("waiting for the first snapshot");
    sh.until(|sh| sh.num("tops") == WINDOWS as f64);
    let mut es = ev.recv().unwrap();
    let (s0, c0) = (snapshots(), sh.num("ipcChanges"));
    // A keyboard layout, a submap, a bell, a plugin's event and a layer: none is state.
    es.write_all(b"activelayout>>kb,us\nsubmap>>resize\nbell>>a0\ncustom>>x\nopenlayer>>rofi\n")
        .unwrap();
    sh.until(|sh| sh.num("events") == 5.0);
    assert_eq!(snapshots(), s0, "{:?}", REQS.lock().unwrap().last());
    // Focus back on workspace 1: a snapshot, equal to the last one.
    es.write_all(b"workspacev2>>1,1\n").unwrap();
    sh.until(|sh| sh.num("events") == 6.0);
    assert_eq!(snapshots(), s0 + 1);
    assert_eq!(
        sh.num("ipcChanges"),
        c0,
        "an equal snapshot rewrote lastIpcObject"
    );
    // One window's title: only its lastIpcObject changes.
    *TITLE.lock().unwrap() = "renamed".into();
    es.write_all(b"windowtitlev2>>a0,renamed\n").unwrap();
    sh.until(|sh| sh.str("title") == "renamed");
    assert_eq!(snapshots(), s0 + 2);
    assert_eq!(sh.num("ipcChanges"), c0 + 1.0);
}

#[test]
fn hook_is_set_again_after_a_reconnect() {
    if !own_process(concat!(
        module_path!(),
        "::hook_is_set_again_after_a_reconnect"
    )) {
        return;
    }
    let ev = compositor();
    let mut sh = Shell::new(ROOT);
    // Hyprland closes a client it cannot write to; a new Hyprland has a new Lua state. Either
    // way the hook must go in again once the shell is back.
    eprintln!("waiting for the first hook and snapshot");
    sh.until(|_| {
        let r = REQS.lock().unwrap();
        r.iter().any(|c| is_hook(c)) && r.iter().any(|c| c.starts_with("[[BATCH]]"))
    });
    drop(ev.recv().unwrap());
    let _second = ev.recv().unwrap();
    let at = REQS.lock().unwrap().len();
    eprintln!("waiting for the snapshot after the reconnect");
    sh.until(|_| {
        REQS.lock().unwrap()[at..]
            .iter()
            .any(|c| c.starts_with("[[BATCH]]"))
    });
    let reqs = REQS.lock().unwrap();
    assert_eq!(reqs.iter().filter(|c| is_hook(c)).count(), 2, "{reqs:?}");
    assert!(reqs[at..].iter().any(|c| is_hook(c)), "{reqs:?}");
}

/// The urgent windows' addresses and the urgent workspaces' ids, as QML reads them.
const URGENT: &str = "import QtQuick\nimport Rustshell.Hyprland\nItem {\n\
     property int events: 0\n\
     property int tops: Hyprland.toplevels.values.length\n\
     property string windows: Hyprland.toplevels.values.filter(t => t.urgent).map(t => t.address).join(' ')\n\
     property string workspaces: Hyprland.workspaces.values.filter(w => w.urgent).map(w => w.id).join(' ')\n\
     Connections { target: Hyprland; function onRawEvent(e) { events++ } }\n}";

#[test]
fn urgent_event_marks_the_window_and_its_workspace() {
    if !own_process(concat!(
        module_path!(),
        "::urgent_event_marks_the_window_and_its_workspace"
    )) {
        return;
    }
    let ev = compositor();
    let mut sh = Shell::new(URGENT);
    eprintln!("waiting for the first snapshot");
    sh.until(|sh| sh.num("tops") == WINDOWS as f64);
    let mut es = ev.recv().unwrap();
    let urgent = |sh: &Shell| (sh.str("windows"), sh.str("workspaces"));
    let is = |w: &str, ws: &str| (w.to_string(), ws.to_string());
    assert_eq!(urgent(&sh), is("", ""));
    // Windows a1 and ab are on workspace 2, a2 on workspace 3; workspace 1 has the focus.
    let mut event = |sh: &mut Shell, e: &str| {
        let n = sh.num("events");
        eprintln!("waiting for {e}");
        es.write_all(format!("{e}\n").as_bytes()).unwrap();
        sh.until(|sh| sh.num("events") == n + 1.0);
    };
    event(&mut sh, "urgent>>a1");
    assert_eq!(urgent(&sh), is("a1", "2"));
    event(&mut sh, "urgent>>a2");
    assert_eq!(urgent(&sh), is("a1 a2", "2 3"));
    // A window of the focused workspace does not stay urgent.
    event(&mut sh, "urgent>>a0");
    assert_eq!(urgent(&sh), is("a1 a2", "2 3"));
    // Workspace 3 gets the focus with none of its windows active: its windows stop being
    // urgent, the other workspace's stay.
    FOCUSED.store(3, SeqCst);
    event(&mut sh, "workspacev2>>3,3");
    assert_eq!(urgent(&sh), is("a1", "2"));
    // The urgent window becomes the active one while its workspace is not the focused one
    // (the snapshot is taken between Hyprland's `activewindowv2` and `workspacev2`).
    event(&mut sh, "urgent>>ab");
    assert_eq!(urgent(&sh), is("a1 ab", "2"));
    ACTIVE.store(1, SeqCst);
    event(&mut sh, "activewindowv2>>a1");
    assert_eq!(urgent(&sh), is("ab", "2"));
    // A closed window's urgency goes with it: the address may be given to another window.
    event(&mut sh, "closewindow>>ab");
    FOCUSED.store(2, SeqCst);
    event(&mut sh, "workspacev2>>2,2");
    FOCUSED.store(1, SeqCst);
    event(&mut sh, "workspacev2>>1,1");
    assert_eq!(urgent(&sh), is("", ""));
}

#[test]
fn using_lua_is_what_status_says() {
    if !own_process(concat!(module_path!(), "::using_lua_is_what_status_says")) {
        return;
    }
    // A Hyprland whose config is not Lua.
    *STATUS.lock().unwrap() = r#"{"configProvider":"hyprlang","backend":"aquamarine"}"#;
    let ev = compositor();
    let mut sh = Shell::new(
        "import QtQuick\nimport Rustshell.Hyprland\nItem {\n\
         property bool lua: Hyprland.usingLua\n\
         property int tops: Hyprland.toplevels.values.length\n}",
    );
    let lua = |sh: &Shell| sh.rt.get(sh.root, "lua").truthy();
    let asked = || {
        let r = REQS.lock().unwrap();
        r.iter().filter(|c| *c == "j/status").count()
    };
    eprintln!("waiting for the first snapshot");
    sh.until(|sh| sh.num("tops") == WINDOWS as f64);
    assert_eq!(asked(), 1);
    assert!(!lua(&sh), "usingLua with a config that is not Lua");
    // Another Hyprland takes the instance's place, this one with a Lua config.
    *STATUS.lock().unwrap() = r#"{"configProvider":"lua","backend":"aquamarine"}"#;
    drop(ev.recv().unwrap());
    eprintln!("waiting for usingLua after the reconnect");
    sh.until(lua);
    assert_eq!(asked(), 2);
    // A Hyprland from before `status`.
    *STATUS.lock().unwrap() = "unknown request";
    drop(ev.recv().unwrap());
    eprintln!("waiting for usingLua to go after the second reconnect");
    sh.until(|sh| !lua(sh));
    assert_eq!(asked(), 3);
}

#[test]
fn failed_dispatch_reports_hyprlands_error() {
    const TEST: &str = concat!(module_path!(), "::failed_dispatch_reports_hyprlands_error");
    if !testenv::in_own_process(TEST) {
        let rt_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(rt_dir.path().join("hypr").join(INSTANCE)).unwrap();
        let mut c = testenv::own_process_command(TEST);
        // libtest would keep what the test's threads print to itself.
        c.arg("--nocapture")
            .envs(env(rt_dir.path()))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = c.spawn().unwrap();
        let out = testenv::Log::capture(child.stdout.take().unwrap());
        let err = testenv::Log::capture(child.stderr.take().unwrap());
        let out = out.until_closed() + &err.until_closed();
        assert!(testenv::wait_exit(&mut child).success(), "{out}");
        assert!(
            out.contains(
                r#"hyprland dispatch "nosuch" failed with error "Invalid dispatcher nosuch""#
            ),
            "{out}"
        );
        return;
    }
    let _ev = compositor();
    let mut sh = Shell::new(
        "import QtQuick\nimport Rustshell.Hyprland\nItem { Component.onCompleted: Hyprland.dispatch(\"nosuch\") }",
    );
    sh.until(|_| REQS.lock().unwrap().iter().any(|c| c == "dispatch nosuch"));
}

/// Main-thread CPU, wall time and property changes per event, each event waiting for what it
/// asks of the compositor:
/// `cargo test --features live --release -p qml --test hypr_event_refresh bench -- --nocapture`
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn bench_hypr_events() {
    if !own_process(concat!(module_path!(), "::bench_hypr_events")) {
        return;
    }
    const N: usize = 300;
    let ev = compositor();
    let mut sh = Shell::new(ROOT);
    sh.until(|sh| sh.num("tops") == WINDOWS as f64);
    let mut es = ev.recv().unwrap();
    let len = reply_len();
    println!("snapshot reply: {len} bytes, {WINDOWS} windows, 10 workspaces, 2 monitors");
    println!(
        "| event | snapshots / {N} | main thread CPU / event | wall / event | changes / event |"
    );
    println!("|---|---|---|---|---|");
    for e in [
        "activelayout>>kb,us",
        "submap>>resize",
        "bell>>a0",
        "custom>>x",
        "openlayer>>rofi",
        "workspacev2>>1,1",
    ] {
        let (s0, rev0) = (snapshots(), sh.rt.revision());
        let (cpu0, t0) = (thread_cpu(), std::time::Instant::now());
        for _ in 0..N {
            let n = sh.num("events");
            es.write_all(format!("{e}\n").as_bytes()).unwrap();
            sh.until(|sh| sh.num("events") > n);
        }
        let (cpu, wall) = (thread_cpu() - cpu0, t0.elapsed());
        println!(
            "| `{}` | {} | {:.0} µs | {:.2} ms | {:.0} |",
            e.split(">>").next().unwrap(),
            snapshots() - s0,
            cpu.as_secs_f64() * 1e6 / N as f64,
            wall.as_secs_f64() * 1e3 / N as f64,
            (sh.rt.revision() - rev0) as f64 / N as f64
        );
    }
}

fn reply_len() -> usize {
    hypr_ipc::answer(
        "[[BATCH]]j/monitors;j/workspaces;j/activeworkspace;j/clients;j/activewindow",
        reply,
    )
    .len()
}

fn thread_cpu() -> std::time::Duration {
    // SAFETY: timespec is two integers, for which all zeros is valid.
    let mut t: libc::timespec = unsafe { std::mem::zeroed() };
    // SAFETY: `t` is a live timespec.
    let r = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    assert_eq!(r, 0);
    std::time::Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

#[test]
fn unparsed_snapshot_is_not_retaken_every_poll() {
    if !own_process("unparsed_snapshot_is_not_retaken_every_poll") {
        return;
    }
    BROKEN.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ev = compositor();
    let mut sh = Shell::new(
        "import QtQuick\nimport Rustshell.Hyprland\nimport Rustshell.Wayland\nItem {\n\
         property int tops: ToplevelManager.toplevels.values.length\n\
         property int events: 0\n\
         Connections { target: Hyprland; function onRawEvent(e) { events++ } }\n}",
    );
    let mut polls = |n| {
        for _ in 0..n {
            settle(&mut sh.rt);
            qml::hypr::drain();
        }
        snapshots()
    };
    // The first snapshot, and the one the event socket's connection asks for.
    let first = polls(20);
    assert!((1..=2).contains(&first), "{first}");
    assert_eq!(polls(100), first);
}
