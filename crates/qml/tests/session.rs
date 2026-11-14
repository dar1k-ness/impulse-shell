//! `qml::session` (A3) against what the original's `Session.qml` runs: the same logind calls
//! `systemctl`/`loginctl` make (recorded with a bus monitor on a private bus with a fake
//! logind; systemd >= 258, non-root, e.g. the Arch container), `kill(2)` vs `kill PID`, and
//! logout: the real `pkill -i Hyprland` vs the native IPC exit in a private PID namespace.
#[cfg(feature = "interp")]
use qml::runtime::Runtime;
use qml::session::{self, Action};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Flags;

struct Bus(Child, String);
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// In `test`'s own process (see `testenv::run_in_own_process`): a private bus used as the
/// system bus at the address the environment names (a socket in a directory the parent made),
/// as the native side, systemctl and loginctl all find it through the environment. `None` in
/// the process that ran the test there.
fn bus(test: &str) -> Option<Bus> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(test, [("DBUS_SYSTEM_BUS_ADDRESS", addr)]);
        return None;
    }
    let cfg = std::env::temp_dir().join(format!("qml-session-bus-{}.conf", std::process::id()));
    std::fs::write(
        &cfg,
        "<busconfig><type>system</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>\
         <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
    )
    .ok()?;
    let mut c = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", cfg.display()))
        .arg(format!(
            "--address={}",
            std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap()
        ))
        .args(["--nofork", "--nopidfile", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| testenv::unavailable(format!("no dbus-daemon ({e})")))
        .ok()?;
    let mut addr = String::new();
    BufReader::new(c.stdout.take().unwrap())
        .read_line(&mut addr)
        .ok()?;
    Some(Bus(c, addr.trim().to_string()))
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Ok,
    /// `*WithFlags` and `Lock` fail with AccessDenied (polkit said no).
    Deny,
    /// A logind older than v248: no `*WithFlags`.
    Old,
    /// `SetWallMessage` is never answered (as long as a polkit dialog stays open); counted in
    /// [`HELD`].
    Hold,
}

/// Calls the fake left unanswered ([`Mode::Hold`]).
static HELD: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

struct Mgr(Mode);

fn deny(m: Mode) -> zbus::fdo::Result<()> {
    match m {
        Mode::Ok | Mode::Hold => Ok(()),
        Mode::Deny => Err(zbus::fdo::Error::AccessDenied("no".into())),
        Mode::Old => Err(zbus::fdo::Error::UnknownMethod("no".into())),
    }
}

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Mgr {
    fn power_off_with_flags(&self, _f: u64) -> zbus::fdo::Result<()> {
        deny(self.0)
    }
    fn reboot_with_flags(&self, _f: u64) -> zbus::fdo::Result<()> {
        deny(self.0)
    }
    fn suspend_with_flags(&self, _f: u64) -> zbus::fdo::Result<()> {
        deny(self.0)
    }
    fn hibernate_with_flags(&self, _f: u64) -> zbus::fdo::Result<()> {
        deny(self.0)
    }
    fn power_off(&self, _i: bool) {}
    fn reboot(&self, _i: bool) {}
    fn suspend(&self, _i: bool) {}
    fn hibernate(&self, _i: bool) {}
    async fn set_wall_message(&self, _m: &str, _e: bool) {
        if self.0 == Mode::Hold {
            *HELD.0.lock().unwrap() += 1;
            HELD.1.notify_all();
            std::future::pending::<()>().await;
        }
    }
    fn set_reboot_to_firmware_setup(&self, _e: bool) {}
}

struct Sess(Mode);

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Sess {
    fn lock(&self) -> zbus::fdo::Result<()> {
        if self.0 == Mode::Deny {
            return Err(zbus::fdo::Error::AccessDenied("no".into()));
        }
        Ok(())
    }
}

/// A fake logind plus a monitor recording every call to it as
/// `path iface.Member(args) [interactive]`. A call to `MARK` is not recorded but counted in
/// `marks`: the monitor sees calls in the order the bus routes them, so once it saw a mark
/// sent after a call returned, it has recorded that call.
struct Fake {
    _bus: Bus,
    _srv: Connection,
    log: Arc<Mutex<Vec<String>>>,
    probe: Connection,
    marks: Arc<(Mutex<u64>, Condvar)>,
}

const MARK: &str = "/org/end4/test/mark";

fn fake(test: &str, mode: Mode) -> Option<Fake> {
    let b = bus(test)?;
    let srv = zbus::blocking::connection::Builder::address(b.1.as_str())
        .unwrap()
        .name("org.freedesktop.login1")
        .unwrap()
        .serve_at("/org/freedesktop/login1", Mgr(mode))
        .unwrap()
        .serve_at("/org/freedesktop/login1/session/auto", Sess(mode))
        .unwrap()
        .build()
        .unwrap();
    let mon = zbus::blocking::connection::Builder::address(b.1.as_str())
        .unwrap()
        .build()
        .unwrap();
    mon.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus.Monitoring"),
        "BecomeMonitor",
        &(
            vec!["type='method_call',destination='org.freedesktop.login1'"],
            0u32,
        ),
    )
    .unwrap();
    let probe = zbus::blocking::connection::Builder::address(b.1.as_str())
        .unwrap()
        .build()
        .unwrap();
    let log = Arc::new(Mutex::new(vec![]));
    let l = log.clone();
    let marks = Arc::new((Mutex::new(0), Condvar::new()));
    let seen = marks.clone();
    let it = MessageIterator::from(&mon);
    std::thread::spawn(move || {
        let _mon = mon;
        for m in it.flatten() {
            let h = m.header();
            if h.message_type() != zbus::message::Type::MethodCall {
                continue;
            }
            let member = h.member().map(|m| m.to_string()).unwrap_or_default();
            let iface = h.interface().map(|m| m.to_string()).unwrap_or_default();
            let path = h.path().map(|m| m.to_string()).unwrap_or_default();
            if path == MARK {
                *seen.0.lock().unwrap() += 1;
                seen.1.notify_all();
                continue;
            }
            let sig = m.body().signature().to_string();
            let sig = sig.trim_start_matches('(').trim_end_matches(')');
            let args = match sig {
                "" => String::new(),
                "t" => m.body().deserialize::<u64>().unwrap().to_string(),
                "b" => m.body().deserialize::<bool>().unwrap().to_string(),
                "sb" => format!("{:?}", m.body().deserialize::<(String, bool)>().unwrap()),
                s => format!("<{s}>"),
            };
            let ia = if h.primary().flags().contains(Flags::AllowInteractiveAuth) {
                " [interactive]"
            } else {
                ""
            };
            l.lock()
                .unwrap()
                .push(format!("{path} {iface}.{member}({args}){ia}"));
        }
    });
    Some(Fake {
        _bus: b,
        _srv: srv,
        log,
        probe,
        marks,
    })
}

impl Fake {
    /// Blocks until the monitor recorded every call made before this one.
    fn sync(&self) {
        let n = *self.marks.0.lock().unwrap();
        // The fake answers UnknownObject: only the call itself, routed after the earlier ones,
        // matters.
        let _ = self.probe.call_method(
            Some("org.freedesktop.login1"),
            MARK,
            Some("org.freedesktop.DBus.Peer"),
            "Ping",
            &(),
        );
        let g = self.marks.0.lock().unwrap();
        drop(self.marks.1.wait_while(g, |m| *m == n).unwrap());
    }

    /// The calls `f` made (it returns once they were answered).
    fn record(&self, f: impl FnOnce()) -> Vec<String> {
        self.sync();
        self.log.lock().unwrap().clear();
        f();
        self.sync();
        std::mem::take(&mut *self.log.lock().unwrap())
    }
}

/// The command the original's Session.qml runs for `a`.
fn original(a: Action) -> &'static [&'static str] {
    match a {
        Action::PowerOff => &["bash", "-c", "systemctl poweroff || loginctl poweroff"],
        Action::Reboot => &["bash", "-c", "reboot || loginctl reboot"],
        Action::RebootToFirmware => &[
            "bash",
            "-c",
            "systemctl reboot --firmware-setup || loginctl reboot --firmware-setup",
        ],
        Action::Suspend => &["bash", "-c", "systemctl suspend || loginctl suspend"],
        Action::Hibernate => &["bash", "-c", "systemctl hibernate || loginctl hibernate"],
        Action::Lock => &["loginctl", "lock-session"],
    }
}

const M: &str = "/org/freedesktop/login1 org.freedesktop.login1.Manager";

fn expected(a: Action, mode: Mode) -> Vec<String> {
    if a == Action::Lock {
        return vec![
            "/org/freedesktop/login1/session/auto org.freedesktop.login1.Session.Lock() [interactive]"
                .into(),
        ];
    }
    let (m, f) = match a {
        Action::PowerOff => ("PowerOff", 0),
        Action::Reboot | Action::RebootToFirmware => ("Reboot", session::flags(Action::Reboot)),
        Action::Suspend => ("Suspend", 0),
        Action::Hibernate => ("Hibernate", 0),
        Action::Lock => unreachable!(),
    };
    // `reboot` is systemctl's halt mode, which never asks polkit interactively.
    let ask = a != Action::Reboot;
    let ia = if ask { " [interactive]" } else { "" };
    let mut v = vec![];
    if a == Action::RebootToFirmware {
        v.push(format!("{M}.SetRebootToFirmwareSetup(true) [interactive]"));
    }
    v.push(format!("{M}.SetWallMessage((\"\", true)){ia}"));
    v.push(format!("{M}.{m}WithFlags({f}){ia}"));
    if mode == Mode::Old {
        v.push(format!("{M}.{m}({ask}){ia}"));
    }
    v
}

const ACTIONS: [Action; 6] = [
    Action::PowerOff,
    Action::Reboot,
    Action::RebootToFirmware,
    Action::Suspend,
    Action::Hibernate,
    Action::Lock,
];

fn check(test: &str, mode: Mode) {
    for tool in ["bash", "systemctl", "loginctl", "reboot"] {
        if !testenv::require(tool) {
            return;
        }
    }
    if !std::path::Path::new("/run/systemd/system").is_dir() {
        testenv::unavailable(
            "/run/systemd/system: systemctl/loginctl refuse the system bus without it",
        );
        return;
    }
    let Some(f) = fake(test, mode) else { return };
    for a in ACTIONS {
        let native = f.record(|| {
            let c = Connection::system().unwrap();
            let r = session::logind(&c, a);
            assert_eq!(r.is_ok(), mode != Mode::Deny, "{a:?}: {r:?}");
        });
        assert_eq!(native, expected(a, mode), "native {a:?}");
        let o = original(a);
        let got = f.record(|| {
            let out = Command::new(o[0]).args(&o[1..]).output().unwrap();
            assert_eq!(out.status.success(), mode != Mode::Deny, "{o:?}: {out:?}");
        });
        assert_eq!(got, native, "original {o:?}");
    }
}

#[test]
fn logind_calls_match_systemctl() {
    check(
        concat!(module_path!(), "::logind_calls_match_systemctl"),
        Mode::Ok,
    );
}

#[test]
fn logind_denied_matches_systemctl() {
    check(
        concat!(module_path!(), "::logind_denied_matches_systemctl"),
        Mode::Deny,
    );
}

#[test]
fn logind_without_with_flags_matches_systemctl() {
    check(
        concat!(
            module_path!(),
            "::logind_without_with_flags_matches_systemctl"
        ),
        Mode::Old,
    );
}

/// Blocks until killed.
fn sleeper() -> Child {
    Command::new("sleep").arg("infinity").spawn().unwrap()
}

fn signal_of(mut c: Child) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    testenv::wait_exit(&mut c).signal()
}

#[test]
fn kill_matches_kill_command() {
    let a = sleeper();
    let b = sleeper();
    let ok = Command::new("kill")
        .arg(a.id().to_string())
        .status()
        .unwrap()
        .success();
    // Both are waited for until they exit: each must have been signalled.
    assert!(ok);
    assert_eq!(session::kill(&b.id().to_string()), ok);
    assert_eq!(signal_of(b), signal_of(a));
    for bad in ["", "abc", "undefined", "12x"] {
        let k = Command::new("kill")
            .arg(bad)
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert_eq!(session::kill(bad), k, "{bad:?}");
    }
    // Refused although `kill 0` signals the caller's process group.
    assert!(!session::kill("0"));
    assert!(!session::kill("-1"));
}

/// Process names next to the original's `logout`: the compositor, its helpers (comm is cut at 15
/// bytes, so `xdg-desktop-portal-hyprland` is `xdg-desktop-por`), and near misses.
/// A request socket answering each command with `replies[i]`; returns what it got.
fn hypr_socket(
    replies: &'static [&'static str],
) -> (std::path::PathBuf, std::thread::JoinHandle<Vec<String>>) {
    let d = std::env::temp_dir().join(format!(
        "qml-session-hypr-{}-{}",
        std::process::id(),
        replies.len()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let l = std::os::unix::net::UnixListener::bind(d.join(".socket.sock")).unwrap();
    // Hyprland writes its lock once its sockets listen.
    std::fs::write(d.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
    let h = std::thread::spawn(move || {
        let mut got = vec![];
        for r in replies {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 256];
            let n = s.read(&mut buf).unwrap();
            got.push(String::from_utf8_lossy(&buf[..n]).to_string());
            s.write_all(r.as_bytes()).unwrap();
        }
        got
    });
    (d, h)
}

#[test]
fn hyprland_exit_over_socket() {
    let (d, h) = hypr_socket(&["ok"]);
    assert!(session::exit_hyprland(&d));
    assert_eq!(
        h.join().unwrap(),
        ["eval return hl.dispatch(hl.dsp.exit())"]
    );
    let (d, h) = hypr_socket(&["unknown request", "ok"]);
    assert!(session::exit_hyprland(&d));
    assert_eq!(
        h.join().unwrap(),
        ["eval return hl.dispatch(hl.dsp.exit())", "dispatch exit"]
    );
}

const LOGOUT_NAMES: &[&str] = &[
    "Hyprland",
    "hyprland-dialog",
    "hyprland-share-picker",
    "xdg-desktop-portal-hyprland",
    "HYPRLANDx",
    "hypr-land",
    "Hypr",
    "hyprpaper",
];

/// The lowest signal pending for `pid` (`/proc/PID/status` `ShdPnd`/`SigPnd`).
fn pending_signal(pid: u32) -> Option<i32> {
    let st = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    let set = st
        .lines()
        .filter_map(|l| {
            l.strip_prefix("ShdPnd:")
                .or_else(|| l.strip_prefix("SigPnd:"))
        })
        .map(|v| u64::from_str_radix(v.trim(), 16).unwrap())
        .fold(0, |a, b| a | b);
    (set != 0).then(|| set.trailing_zeros() as i32 + 1)
}

/// Blocks every signal that can be blocked (in a child before it execs).
fn block_signals() -> std::io::Result<()> {
    // SAFETY: sigset_t is plain integers, for which all-zero bytes are a valid value.
    let mut all: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: `all` is a valid sigset_t to fill.
    unsafe { libc::sigfillset(&mut all) };
    // SAFETY: `all` is a valid, filled set; the old mask is not asked for.
    if unsafe { libc::sigprocmask(libc::SIG_BLOCK, &all, std::ptr::null_mut()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Inside the PID namespace: the `LOGOUT_NAMES` that `kill` (given the `Hyprland` pid)
/// signalled, with the signal, and whether `kill` reported success. Each name blocks every
/// signal, so a signal sent stays pending (kill(2) queues it before it returns) and one not
/// sent is not waited for.
fn logout_victims(dir: &std::path::Path, kill: impl FnOnce(u32) -> bool) -> (bool, Vec<String>) {
    use std::os::unix::process::CommandExt;
    let sleep = qml::osutil::command_v("sleep").unwrap();
    let mut kids: Vec<(String, Child)> = LOGOUT_NAMES
        .iter()
        .map(|n| {
            let p = dir.join(n);
            let _ = std::fs::remove_file(&p);
            std::os::unix::fs::symlink(&sleep, &p).unwrap();
            let mut c = Command::new(&p);
            c.arg("infinity");
            // SAFETY: the hook only calls the async-signal-safe sigfillset(3)/sigprocmask(2).
            unsafe { c.pre_exec(block_signals) };
            // spawn returns once the child exec'd, under its name.
            (n.to_string(), c.spawn().unwrap())
        })
        .collect();
    let ok = kill(kids[0].1.id());
    let mut signalled = vec![];
    for (n, c) in &mut kids {
        if let Some(s) = pending_signal(c.id()) {
            signalled.push(format!("{n}: {:?}", Some(s)));
        }
        let _ = c.kill();
        testenv::wait_exit(c);
    }
    (ok, signalled)
}

/// The native side's compositor: a `.socket.sock` under the `XDG_RUNTIME_DIR` and
/// `HYPRLAND_INSTANCE_SIGNATURE` the process was started with that, on Hyprland's exit
/// request, answers `ok` and ends the `Hyprland` process the way the compositor ends itself.
/// The receiver gets a message once it was signalled.
fn exiting_hyprland(pid: u32) -> std::sync::mpsc::Receiver<()> {
    let rt = std::env::var_os("XDG_RUNTIME_DIR").unwrap();
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").unwrap();
    let d = std::path::Path::new(&rt).join("hypr").join(sig);
    std::fs::create_dir_all(&d).unwrap();
    let l = std::os::unix::net::UnixListener::bind(d.join(".socket.sock")).unwrap();
    // Hyprland writes its lock once its sockets listen.
    std::fs::write(d.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok((mut s, _)) = l.accept() {
            let mut buf = [0u8; 256];
            let n = s.read(&mut buf).unwrap_or(0);
            let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
            if cmd == "eval return hl.dispatch(hl.dsp.exit())" || cmd == "dispatch exit" {
                let _ = s.write_all(b"ok");
                // SAFETY: kill(2) takes no pointers.
                unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                let _ = tx.send(());
            } else {
                let _ = s.write_all(b"unknown request");
            }
        }
    });
    rx
}

/// `Session.logout()` ends the Hyprland session. The original does it with
/// `pkill -i Hyprland`, a case-insensitive substring match that also SIGTERMs every helper
/// whose 15-byte comm contains "hyprland" (hyprland-dialog, hyprland-share-picker, ...). The native side asks the compositor to exit over its IPC
/// socket instead, so the asserted result is the intended one: Hyprland ends, nothing else
/// is signalled. Each side runs in its own PID namespace (`unshare -Urpf --mount-proc`), so
/// neither can reach a real compositor.
#[test]
fn logout_ends_hyprland_only() {
    if let Ok(side) = std::env::var("QML_SESSION_PKILL_SIDE") {
        let dir = tempfile::tempdir().unwrap();
        let (ok, dead) = logout_victims(dir.path(), |pid| {
            if side == "original" {
                Command::new("pkill")
                    .args(["-i", "Hyprland"])
                    .status()
                    .unwrap()
                    .success()
            } else {
                let exited = exiting_hyprland(pid);
                // The logout request runs on the IPC worker; the compositor ends Hyprland
                // when it gets it.
                session::run_effect(&qml::svc::Call::new("logout", vec![]));
                exited.recv().unwrap();
                true
            }
        });
        println!("RESULT {ok} {dead:?}");
        return;
    }
    for tool in ["unshare", "pkill"] {
        if !testenv::require(tool) {
            return;
        }
    }
    // The native side's compositor lives under a runtime directory of its own.
    let rt = tempfile::tempdir().unwrap();
    let side = |s: &str| {
        let out = Command::new("unshare")
            .args(["-Urpf", "--mount-proc"])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "logout_ends_hyprland_only",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("QML_SESSION_PKILL_SIDE", s)
            .env("XDG_RUNTIME_DIR", rt.path())
            .env("HYPRLAND_INSTANCE_SIGNATURE", "sig")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        stdout
            .lines()
            .find_map(|l| l.split_once("RESULT ").map(|(_, r)| r.to_string()))
            .unwrap_or_else(|| {
                testenv::unavailable(format!(
                    "no PID namespace (docker run --security-opt seccomp=unconfined --security-opt systempaths=unconfined --security-opt apparmor=unconfined): {stdout}{}",
                    String::from_utf8_lossy(&out.stderr)
                ));
                String::new()
            })
    };
    let orig = side("original");
    if orig.is_empty() {
        return;
    }
    assert!(orig.starts_with("true [\"Hyprland: Some(15)\""), "{orig}");
    assert!(orig.contains("hyprland-dialog"), "{orig}");
    assert_eq!(side("native"), "true [\"Hyprland: Some(15)\"]");
}

#[cfg(feature = "interp")]
/// Session.qml's functions (imports of the tree dropped: everything sits in one directory).
const SESSION_QML: &str = include_str!("../../../qml/modules/common/functions/Session.qml");

#[cfg(feature = "interp")]
fn session_tree() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("qml-session-tree-{}", std::process::id()));
    let d = root.join("modules/common/functions");
    std::fs::create_dir_all(&d).unwrap();
    let body: String = SESSION_QML
        .lines()
        .filter(|l| !l.starts_with("import ri"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(d.join("Session.qml"), body).unwrap();
    std::fs::write(
        d.join("HyprlandData.qml"),
        "pragma Singleton\nimport Quickshell\nSingleton { property var windowList: [{pid: 4242}, {pid: 4343}] }\n",
    )
    .unwrap();
    std::fs::write(
        d.join("Config.qml"),
        "pragma Singleton\nimport Quickshell\nSingleton { property var options: ({apps: {changePassword: 'pw', taskManager: 'tm'}}) }\n",
    )
    .unwrap();
    d
}

#[cfg(feature = "interp")]
/// What each Session method starts, through the spawn hook: `["SessionControl", action]` and
/// `["SessionControl", "terminate", pid]` for `SessionControl`'s actions, the argv for a
/// command.
fn session_calls() -> Vec<(String, Vec<Vec<String>>)> {
    let d = session_tree();
    let mut out = vec![];
    for m in [
        "closeAllWindows",
        "lock",
        "suspend",
        "logout",
        "hibernate",
        "poweroff",
        "reboot",
        "rebootToFirmware",
        "changePassword",
        "launchTaskManager",
    ] {
        let tape = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
        let t = tape.clone();
        qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
            t.borrow_mut().push(argv.to_vec());
            Some(String::new())
        })));
        let mut rt = Runtime::new(None);
        rt.load_singletons(std::slice::from_ref(&d));
        rt.load_str(&format!(
            "import QtQuick\nItem {{ Component.onCompleted: Session.{m}() }}"
        ))
        .unwrap();
        rt.settle();
        rt.poll_native();
        qml::native::set_spawn_hook(None);
        out.push((m.to_string(), tape.borrow().clone()));
    }
    out
}

#[cfg(feature = "interp")]
/// Session.qml's methods run `SessionControl`'s actions (checked against logind, `kill` and
/// Hyprland above), the windows closed first where the original closed them; the two user-configured
/// commands stay commands.
#[test]
fn session_methods_call_session_control() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let kills = || {
        vec![
            s(&["SessionControl", "terminate", "4242"]),
            s(&["SessionControl", "terminate", "4343"]),
        ]
    };
    for (m, got) in session_calls() {
        let closes = matches!(
            m.as_str(),
            "closeAllWindows" | "logout" | "poweroff" | "reboot" | "rebootToFirmware"
        );
        let mut want = if closes { kills() } else { vec![] };
        match m.as_str() {
            "closeAllWindows" => {}
            "changePassword" => want.push(s(&["bash", "-c", "pw"])),
            "launchTaskManager" => want.push(s(&["bash", "-c", "tm"])),
            m => want.push(s(&["SessionControl", m])),
        }
        assert_eq!(got, want, "{m}");
    }
}

/// `cargo test --features live -p qml --test session -- --nocapture`: wall time, CPU and processes
/// per action, original commands vs native calls, against the fake logind.
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn measure() {
    let Some(f) = fake(concat!(module_path!(), "::measure"), Mode::Ok) else {
        return;
    };
    fn cpu(who: i32) -> Duration {
        // SAFETY: rusage is plain integers, for which all-zero bytes are a valid value.
        let mut u: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: `u` is a valid out pointer.
        unsafe { libc::getrusage(who, &mut u) };
        let t = |v: libc::timeval| Duration::new(v.tv_sec as u64, v.tv_usec as u32 * 1000);
        t(u.ru_utime) + t(u.ru_stime)
    }
    const N: u32 = 20;
    println!("action            orig wall  orig cpu  procs | native wall  native cpu  procs");
    for a in ACTIONS {
        let o = original(a);
        let (c0, s0, t) = (
            cpu(libc::RUSAGE_CHILDREN),
            cpu(libc::RUSAGE_SELF),
            Instant::now(),
        );
        for _ in 0..N {
            f.log.lock().unwrap().clear();
            Command::new(o[0]).args(&o[1..]).status().unwrap();
        }
        let ow = t.elapsed() / N;
        let oc = (cpu(libc::RUSAGE_CHILDREN) - c0 + cpu(libc::RUSAGE_SELF) - s0) / N;
        // bash + systemctl (`reboot` is systemctl too); `loginctl lock-session` alone.
        let procs = if a == Action::Lock { 1 } else { 2 };
        let (s0, t) = (cpu(libc::RUSAGE_SELF), Instant::now());
        for _ in 0..N {
            // A fresh connection per action, as `run_effect` makes.
            session::logind(&Connection::system().unwrap(), a).unwrap();
        }
        let nw = t.elapsed() / N;
        let nc = (cpu(libc::RUSAGE_SELF) - s0) / N;
        println!(
            "{:<16} {:>9.2?} {:>9.2?} {:>6} | {:>11.2?} {:>11.2?} {:>6}",
            format!("{a:?}"),
            ow,
            oc,
            procs,
            nw,
            nc,
            0
        );
    }
}

#[cfg(feature = "interp")]
const BATTERY_QML: &str = include_str!("../../../qml/services/Battery.qml");

#[cfg(feature = "interp")]
/// Battery.qml's auto-suspend handler, verbatim, on a singleton whose inputs are plain
/// properties.
fn battery_tree() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("qml-session-battery-{}", std::process::id()));
    let d = root.join("services");
    std::fs::create_dir_all(&d).unwrap();
    let start = BATTERY_QML
        .find("    onIsSuspendingAndNotChargingChanged:")
        .unwrap();
    let end = start + BATTERY_QML[start..].find("\n    }\n").unwrap() + 7;
    std::fs::write(
        d.join("Battery.qml"),
        format!(
            "pragma Singleton\nimport Impulse\nimport Quickshell\nSingleton {{\n    id: root\n    \
             property bool available: false\n    property bool isSuspendingAndNotCharging: false\n{}}}\n",
            &BATTERY_QML[start..end]
        ),
    )
    .unwrap();
    d
}

#[cfg(feature = "interp")]
/// What starts after each `(available, isSuspendingAndNotCharging)` step.
fn battery_calls(steps: &[(bool, bool)]) -> Vec<Vec<Vec<String>>> {
    let d = battery_tree();
    let tape = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
    let t = tape.clone();
    qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
        t.borrow_mut().push(argv.to_vec());
        Some(String::new())
    })));
    let mut rt = Runtime::new(None);
    rt.load_singletons(std::slice::from_ref(&d));
    rt.load_str("import QtQuick\nItem { property bool available: Battery.available }")
        .unwrap();
    rt.settle();
    let b = rt.id("root").unwrap();
    rt.poll_native();
    let mut out = vec![];
    for &(a, s) in steps {
        tape.borrow_mut().clear();
        rt.set(b, "available", qml::value::Value::Bool(a));
        rt.set(b, "isSuspendingAndNotCharging", qml::value::Value::Bool(s));
        rt.poll_native();
        out.push(tape.borrow().clone());
    }
    qml::native::set_spawn_hook(None);
    out
}

#[cfg(feature = "interp")]
/// Battery.qml suspends through `SessionControl` each time the level becomes critical on an
/// available battery, and at no other step.
#[test]
fn battery_auto_suspend_calls_session_control() {
    let steps = [
        (false, true),
        (false, false),
        (true, false),
        (true, true),
        (true, false),
        (true, true),
        (false, true),
    ];
    let act = vec![vec!["SessionControl".to_string(), "suspend".into()]];
    let want: Vec<Vec<Vec<String>>> =
        vec![vec![], vec![], vec![], act.clone(), vec![], act, vec![]];
    assert_eq!(battery_calls(&steps), want);
}

/// A logind that never answers holds only its own calls, never a worker of `workpool::run`
/// (settings saves, notification images and file jobs run there): once every worker runs a job
/// of this test at the same time, none is held.
#[test]
fn held_logind_call_leaves_the_pool_free() {
    let Some(_f) = fake(
        concat!(module_path!(), "::held_logind_call_leaves_the_pool_free"),
        Mode::Hold,
    ) else {
        return;
    };
    session::run_effect(&qml::svc::Call::new("suspend", vec![]));
    eprintln!("waiting for the held SetWallMessage");
    drop(
        HELD.1
            .wait_while(HELD.0.lock().unwrap(), |n| *n == 0)
            .unwrap(),
    );
    eprintln!("waiting for every pool worker");
    let all = Arc::new(std::sync::Barrier::new(workpool::WORKERS + 1));
    for _ in 0..workpool::WORKERS {
        let all = all.clone();
        workpool::run(move || {
            all.wait();
        });
    }
    all.wait();
}
