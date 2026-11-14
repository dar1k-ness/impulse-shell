//! A7: `qml::fprint` against the original `fprintd-list $(whoami)` (LockContext's
//! fingerprintCheckProc + its handlers), both talking to tests/fake_fprintd.py on a private
//! `dbus-daemon` used as the system bus. Skipped without dbus-daemon, fprintd-list or
//! python3 with dbus-python + gi.

use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

const PY: &str = "/usr/bin/python3";

struct Bus(#[allow(dead_code)] Child);

/// In `test`'s own process (see `testenv::run_in_own_process`): a private bus at the address
/// the environment names as the system bus (a socket in a directory the parent made), as both
/// sides find it through the environment. `None` in the process that ran the test there, or
/// without the tools.
fn bus(test: &str) -> Option<Bus> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(test, [("DBUS_SYSTEM_BUS_ADDRESS", addr)]);
        return None;
    }
    let ok = |c: &mut Command| {
        c.stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    if !ok(Command::new(PY).args([
        "-c",
        "import dbus, dbus.mainloop.glib; from gi.repository import GLib",
    ])) || !ok(Command::new("sh").args(["-c", "command -v fprintd-list"]))
    {
        testenv::unavailable("needs python3 dbus+gi and fprintd-list");
        return None;
    }
    // Closing stdin (test process exit) makes the wrapper shell kill the daemon.
    let Ok(mut d) = Command::new("sh")
        .args([
            "-c",
            "dbus-daemon --session --address=\"$0\" --nofork --print-address=1 & read _; kill $!",
        ])
        .arg(std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        testenv::unavailable("no dbus-daemon");
        return None;
    };
    let mut line = String::new();
    let _ = BufReader::new(d.stdout.take().unwrap()).read_line(&mut line);
    Some(Bus(d))
}

struct Fake(Child, Receiver<String>);

impl Fake {
    fn start(cfg: &str) -> Fake {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake_fprintd.py");
        let mut c = Command::new(PY)
            .args([script, cfg])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let (tx, rx) = channel();
        let out = c.stdout.take().unwrap();
        std::thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
        assert_eq!(rx.recv().unwrap(), "READY");
        Fake(c, rx)
    }
    /// The calls made since the last time: the fake prints its lines in the order it handles
    /// calls, so the `SYNC` it prints for this call follows the lines of every call before it.
    fn calls(&self) -> Vec<String> {
        zbus::blocking::Connection::system()
            .unwrap()
            .call_method(
                Some("net.reactivated.Fprint"),
                "/net/reactivated/Fprint/Manager",
                Some("net.reactivated.Fprint.Test"),
                "Sync",
                &(),
            )
            .unwrap();
        self.1.iter().take_while(|l| l != "SYNC").collect()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// LockContext.qml: `fingerprintsConfigured = text.includes("Fingerprints for user")`, then
/// false when fprintd-list exits non-zero.
fn original() -> bool {
    let o = Command::new("bash")
        .args(["-c", "fprintd-list $(whoami)"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).contains("Fingerprints for user") && o.status.success()
}

fn native() -> bool {
    qml::fprint::fingerprints_configured(&qml::fprint::user_name())
}

const CASES: &[(&str, bool)] = &[
    (
        r#"{"devices": [{"fingers": ["right-index-finger"]}]}"#,
        true,
    ),
    (
        r#"{"devices": [{"fingers": ["left-thumb", "right-index-finger"]}]}"#,
        true,
    ),
    (r#"{"devices": [{"fingers": []}]}"#, false),
    (r#"{"devices": [{"error": "NoEnrolledPrints"}]}"#, false),
    (r#"{"devices": [{"error": "PermissionDenied"}]}"#, false),
    (r#"{"devices": []}"#, false),
    (
        r#"{"manager_error": true, "devices": [{"fingers": ["left-thumb"]}]}"#,
        false,
    ),
    (
        r#"{"devices": [{"error": "NoEnrolledPrints"}, {"fingers": ["left-thumb"]}]}"#,
        true,
    ),
    (
        r#"{"devices": [{"fingers": ["left-thumb"]}, {"error": "NoEnrolledPrints"}]}"#,
        true,
    ),
    (
        r#"{"devices": [{"fingers": ["left-thumb"]}, {"error": "Internal"}]}"#,
        false,
    ),
];

#[test]
fn matches_fprintd_list() {
    let Some(_b) = bus(concat!(module_path!(), "::matches_fprintd_list")) else {
        return;
    };
    for (cfg, want) in CASES {
        let f = Fake::start(cfg);
        let o = original();
        let ocalls = f.calls();
        let n = native();
        let ncalls = f.calls();
        assert_eq!((o, n), (*want, *want), "{cfg}");
        // Same ListEnrolledFingers calls (devices in order, user = whoami).
        assert_eq!(ocalls, ncalls, "{cfg}");
    }
}

#[test]
fn no_fprintd() {
    let Some(_b) = bus(concat!(module_path!(), "::no_fprintd")) else {
        return;
    };
    assert!(!original());
    assert!(!native());
}

/// The shell's host loop for one runtime: poll its services until nothing changes, then block
/// until one of them wakes it (a service channel's ping, or its Process/inotify fd readable).
struct Host {
    wake: testenv::Wakeups,
    fd: Option<i32>,
}

impl Host {
    fn new(rt: &mut Runtime) -> Host {
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        Host {
            wake,
            fd: rt.process_fd(),
        }
    }

    fn until(&self, rt: &mut Runtime, what: &str, f: &dyn Fn(&mut Runtime) -> bool) {
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
            if f(rt) {
                return;
            }
            if self.wake.wait(self.fd, None).fd {
                rt.process_fd_ready();
            }
        }
    }
}

fn ri() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"))
}

/// The tree's LockContext: `fingerprintsConfigured` is `Fingerprint.enrolled`, read from
/// fprintd with no process.
#[test]
fn lock_context() {
    let Some(_b) = bus(concat!(module_path!(), "::lock_context")) else {
        return;
    };
    for (cfg, want) in [(CASES[0].0, true), (CASES[3].0, false)] {
        let _f = Fake::start(cfg);
        let mut rt = Runtime::new(Some(ri()));
        let f = std::env::temp_dir().join(format!("qml-fprint-{}.qml", std::process::id()));
        std::fs::write(&f, "import ri.modules.common.panels.lock\nLockContext {}\n").unwrap();
        let r = rt.load_file(&f).unwrap();
        rt.settle();
        Host::new(&mut rt).until(&mut rt, "the fingerprint check", &|rt| {
            rt.world.borrow().fprint.as_ref().is_some_and(|f| f.idle())
        });
        assert_eq!(
            rt.get(r, "fingerprintsConfigured").to_js_string(),
            want.to_string(),
            "{cfg}"
        );
        assert!(rt.world.borrow().procs.is_empty());
    }
}

/// `cargo test --features live -p qml --test fprint -- --nocapture`: wall time and child processes of
/// one check, original vs native.
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn measure() {
    let Some(_b) = bus(concat!(module_path!(), "::measure")) else {
        return;
    };
    let _f = Fake::start(CASES[0].0);
    let n = 50;
    let t = Instant::now();
    for _ in 0..n {
        assert!(original());
    }
    let o = t.elapsed() / n;
    let t = Instant::now();
    for _ in 0..n {
        assert!(native());
    }
    let nt = t.elapsed() / n;
    println!("fprint check: original {o:?}/check, native {nt:?}/check");
}
