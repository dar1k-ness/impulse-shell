//! Audit item 8: the tree's LockContext fingerprint unlock. `fingerPam` runs a pam_exec fixture
//! that logs each conversation: one that fails (`PamResult.Error`) and one that waits like
//! fprintd.
//! - Б-1: the 10 s password-clear timer does not abort the fingerprint conversation.
//! - Б-2/B7-3: after `PamResult.Error` a conversation that ran a verify (it delivered a PAM
//!   message) restarts at once; one that did not waits for fprintd (tests/fake_fprintd.py,
//!   bus-activatable on a private `dbus-daemon` used as the system bus) to appear configured.
//!
//! The test runs the runtime's host loop: it sleeps until the runtime's waker (PAM replies)
//! or its epoll set (process exits) wakes it, and drives timers with `tick` on a virtual
//! clock.

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use testenv::Wakeups;

fn ri() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"))
}

fn pam_exec() -> bool {
    [
        "/usr/lib/security",
        "/usr/lib/x86_64-linux-gnu/security",
        "/usr/lib64/security",
    ]
    .iter()
    .any(|d| Path::new(d).join("pam_exec.so").exists())
}

struct Lock {
    rt: Runtime,
    wake: Wakeups,
    ep: Option<i32>,
    root: usize,
    finger: usize,
    log: PathBuf,
    ready: PathBuf,
}

/// Makes the named pipe `p`.
fn mkfifo(p: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
    // SAFETY: mkfifo(3) reads the NUL-terminated path, which outlives the call.
    let r = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
    assert_eq!(r, 0, "mkfifo {p:?}: {}", std::io::Error::last_os_error());
}

/// What a conversation that waits like fprintd runs: tells the test it is up through the
/// `READY` pipe, then blocks until `abort()` kills its process group.
const WAITS: &str = "echo up > READY\nexec sleep infinity";

const HELPER: &str = concat!(module_path!(), "::conversation_helper");

/// The PamContext conversation helper (`qml::pam::serve`), run by this test binary in a
/// process of its own; a no-op test otherwise.
#[test]
fn conversation_helper() {
    if testenv::in_own_process(HELPER) {
        qml::pam::serve();
    }
}

fn helper_command() -> std::process::Command {
    let mut c = testenv::own_process_command(HELPER);
    c.stdout(std::process::Stdio::null());
    c
}

/// The tree's LockContext.qml copied under a temp `panels/lock/` with `pam/fprintd.conf`
/// running `script` (after logging a line) through pam_exec; `READY` in `script` names a pipe
/// the test reads (`Lock::wait_ready`).
fn lock(name: &str, script: &str) -> Option<Lock> {
    lock_with(name, "", script)
}

/// The tree's LockContext.qml copied under a temp `panels/lock/` with `pam/fprintd.conf`
/// running `script` (after logging a line) through pam_exec with `opts`.
fn lock_with(name: &str, opts: &str, script: &str) -> Option<Lock> {
    if !pam_exec() {
        testenv::unavailable("needs pam_exec.so");
        return None;
    }
    let base = std::env::temp_dir().join(format!("ri-lockctx-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("modules/common/panels/lock");
    std::fs::create_dir_all(dir.join("pam")).unwrap();
    let file = dir.join("LockContext.qml");
    std::fs::copy(
        ri().join("modules/common/panels/lock/LockContext.qml"),
        &file,
    )
    .unwrap();
    let log = base.join("runs.log");
    let ready = base.join("ready");
    mkfifo(&ready);
    let sh = base.join("finger.sh");
    let script = script.replace("READY", &format!("'{}'", ready.display()));
    testenv::install_script(
        &sh,
        &format!("#!/bin/sh\necho run >> '{}'\n{script}\n", log.display()),
    );
    std::fs::write(
        dir.join("pam/fprintd.conf"),
        format!("auth required pam_exec.so {opts} {}\n", sh.display()),
    )
    .unwrap();
    let wrap = dir.join("Wrap.qml");
    std::fs::write(
        &wrap,
        "LockContext { property var got: null; onUnlocked: a => got = a }\n",
    )
    .unwrap();
    qml::pam::enable(helper_command);
    let mut rt = Runtime::new(Some(ri()));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let dirs = ["", "modules/common", "modules/common/functions", "services"].map(|d| ri().join(d));
    rt.load_singletons_lazy(&dirs);
    let root = rt.load_file(&wrap).unwrap();
    rt.settle();
    let finger = rt.id("fingerPam").unwrap();
    let mut l = Lock {
        rt,
        wake,
        ep,
        root,
        finger,
        log,
        ready,
    };
    l.until(|rt| rt.world.borrow().fprint.as_ref().is_some_and(|f| f.idle()));
    l.set_configured(true);
    l.rt.tick(0.0);
    Some(l)
}

impl Lock {
    /// Runs the native services until nothing is ready: polls again while a poll wrote
    /// something (that counts for the next one) or a wakeup is already pending.
    fn drain(&mut self) {
        loop {
            let rev = self.rt.revision();
            self.rt.poll_native();
            self.rt.settle();
            let woke = self.wake.wait(self.ep, Some(Duration::ZERO));
            if woke.fd {
                self.rt.process_fd_ready();
            }
            if !woke.any() && self.rt.revision() == rev {
                return;
            }
        }
    }

    /// Runs the host loop until `done`, sleeping until the runtime is woken between turns.
    fn until(&mut self, mut done: impl FnMut(&mut Runtime) -> bool) {
        loop {
            self.drain();
            if done(&mut self.rt) {
                return;
            }
            if self.wake.wait(self.ep, None).fd {
                self.rt.process_fd_ready();
            }
        }
    }

    /// Blocks until the running conversation's script has reported it is up ([`WAITS`]).
    fn wait_ready(&self) {
        let mut s = String::new();
        std::fs::File::open(&self.ready)
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!(s, "up\n");
    }

    fn runs(&self) -> usize {
        std::fs::read_to_string(&self.log)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    fn active(&self) -> bool {
        self.rt.get(self.finger, "active").truthy()
    }

    fn call(&mut self, f: &str) {
        let r = self.rt.call_method(self.root, f, vec![]);
        assert!(matches!(r, Some(Ok(_))), "{f}: {r:?}");
        self.rt.settle();
    }

    /// Wait until `n` conversations ran and the last one ended (its script logs before it
    /// exits, and the end arrives as a PAM reply that wakes the loop).
    fn ran(&mut self, n: usize, what: &str) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        let log = self.log.clone();
        let finger = self.finger;
        self.until(|rt| {
            let runs = std::fs::read_to_string(&log)
                .map(|s| s.lines().count())
                .unwrap_or(0);
            runs >= n && !rt.get(finger, "active").truthy()
        });
    }

    /// At least `n` conversations started, whatever state the current one is in. Each one
    /// that ends (the original restarts at once on Error) wakes the loop after its script logged.
    fn started(&mut self, n: usize, what: &str) {
        eprintln!("waiting for {what}");
        let log = self.log.clone();
        self.until(|_| std::fs::read_to_string(&log).map_or(0, |s| s.lines().count()) >= n);
    }

    /// Runs the host loop until `done`, naming the wait in the output nextest shows when its
    /// slow-timeout ends a hang.
    fn wait(&mut self, what: &str, done: impl FnMut(&mut Runtime) -> bool) {
        eprintln!("waiting for {what}");
        self.until(done);
    }

    fn configured(&self) -> bool {
        self.rt.get(self.root, "fingerprintsConfigured").truthy()
    }

    /// Sets `Fingerprint.enrolled`, which LockContext's `fingerprintsConfigured` follows.
    fn set_configured(&mut self, v: bool) {
        let f = self.rt.world.borrow().singletons["Fingerprint"];
        self.rt.set(f, "enrolled", Value::Bool(v));
        self.rt.settle();
    }

    /// `Fingerprint.waitForAvailable()` is waiting.
    fn waiting(&self) -> bool {
        self.rt
            .world
            .borrow()
            .fprint
            .as_ref()
            .is_some_and(|f| f.waiting())
    }

    /// Let the `n`th conversation, blocked reading `gate`, go on to fail: the open returns once
    /// its script opened the FIFO, so it waits for that conversation to start.
    fn release(&mut self, gate: &Path, n: usize) {
        eprintln!("waiting for conversation {n} to open the gate");
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).open(gate).unwrap();
        f.write_all(b"\n").unwrap();
    }
}

/// A conversation that ran a verify (it delivered a PAM message, as pam_fprintd's "Place your
/// finger" after VerifyStart) and ended in `PamResult.Error` (fprintd's timeout) restarts at
/// once, with the animation clock never advanced.
#[test]
fn verified_error_restarts_at_once() {
    let Some(mut l) = lock_with(
        "verified",
        "stdout",
        "echo 'Place your finger on the fingerprint reader'\nexit 1",
    ) else {
        return;
    };
    l.call("tryFingerUnlock");
    l.started(4, "a restart after a verify that ended in Error");
    assert!(l.configured());
    assert!(!l.waiting());
    l.call("reset");
    assert!(!l.active());
}

const PY: &str = "/usr/bin/python3";

struct Bus {
    _daemon: std::process::Child,
    dir: PathBuf,
}

/// In `test`'s own process: a private `dbus-daemon` at the address the environment names as the
/// system bus, with an (empty) activation directory. `None` in the process that ran the test
/// there, or without the tools.
fn private_bus(test: &str) -> Option<Bus> {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
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
    ])) || !ok(Command::new("sh").args(["-c", "command -v dbus-daemon"]))
    {
        testenv::unavailable("needs python3 dbus+gi and dbus-daemon");
        return None;
    }
    let addr = std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap();
    let sock = PathBuf::from(addr.strip_prefix("unix:path=").unwrap());
    let dir = sock.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(dir.join("services")).unwrap();
    let conf = dir.join("bus.conf");
    std::fs::write(
        &conf,
        format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\"\n \
             \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig><type>session</type><listen>{addr}</listen>\
             <auth>EXTERNAL</auth><servicedir>{}</servicedir>\
             <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
             <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n",
            dir.join("services").display()
        ),
    )
    .unwrap();
    // Closing stdin (test process exit) makes the wrapper shell kill the daemon.
    let mut d = Command::new("sh")
        .args([
            "-c",
            "dbus-daemon --config-file=\"$0\" --nofork --print-address=1 & read _; kill $!",
        ])
        .arg(&conf)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    let _ = std::io::BufReader::new(d.stdout.take().unwrap()).read_line(&mut line);
    assert!(!line.is_empty(), "dbus-daemon did not start");
    Some(Bus { _daemon: d, dir })
}

impl Bus {
    fn call(&self, method: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) {
        let c = zbus::blocking::Connection::system().unwrap();
        c.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            method,
            body,
        )
        .unwrap();
    }

    /// fprintd (the fake with `cfg`) becomes bus-activatable; each start logs to `started`.
    fn install_fprintd(&self, cfg: &str) {
        let sh = self.dir.join("fprintd.sh");
        testenv::install_script(
            &sh,
            &format!(
                "#!/bin/sh\necho started >> '{}'\nexec {PY} '{}' '{cfg}' >/dev/null 2>&1\n",
                self.started_log().display(),
                concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake_fprintd.py"),
            ),
        );
        std::fs::write(
            self.dir.join("services/net.reactivated.Fprint.service"),
            format!(
                "[D-BUS Service]\nName=net.reactivated.Fprint\nExec={}\n",
                sh.display()
            ),
        )
        .unwrap();
        // Answered once the daemon reread its activation directories.
        self.call("ReloadConfig", &());
    }

    fn started_log(&self) -> PathBuf {
        self.dir.join("started")
    }

    /// Another client activates fprintd (as fprintd-list would); returns once it owns its name.
    fn activate(&self) {
        self.call("StartServiceByName", &("net.reactivated.Fprint", 0u32));
    }
}

/// A conversation that failed before any PAM message (no verify: pam_fprintd found no fprintd)
/// does not restart by itself. `fingerprintsConfigured` follows fprintd without starting it,
/// and the conversation restarts when a client activating fprintd makes it configured; a
/// configured fprintd already there when a conversation fails is not such an event.
fn waits_for_fprintd(test: &str, name: &str, cfg: &str, prints: bool) {
    let Some(bus) = private_bus(test) else {
        return;
    };
    let gate = bus.dir.join("gate");
    mkfifo(&gate);
    let Some(mut l) = lock_with(
        name,
        // quiet: pam_exec reports a failing script as a PAM_ERROR_MSG otherwise.
        "quiet",
        &format!("read _ < '{}'\nexit 1", gate.display()),
    ) else {
        return;
    };
    // fprintd becomes activatable only now: the lock's own check found none.
    bus.install_fprintd(cfg);
    l.call("tryFingerUnlock");
    l.release(&gate, 1);
    let root = l.root;
    l.wait(
        "fingerprintsConfigured taken from fprintd after the failure",
        |rt| !rt.get(root, "fingerprintsConfigured").truthy(),
    );
    assert!(
        !bus.started_log().exists(),
        "the check after the failure started fprintd"
    );
    assert_eq!((l.runs(), l.active()), (1, false));
    if !prints {
        l.set_configured(true);
    }
    bus.activate();
    let finger = l.finger;
    if prints {
        l.wait(
            "fprintd appearing configured to restart the conversation",
            |rt| rt.get(finger, "active").truthy(),
        );
        assert!(l.configured());
        // The next failure's check finds fprintd configured: reported, but no restart.
        l.set_configured(false);
        l.release(&gate, 2);
        l.wait(
            "fingerprintsConfigured taken from fprintd after the second failure",
            |rt| rt.get(root, "fingerprintsConfigured").truthy(),
        );
        assert_eq!((l.runs(), l.active()), (2, false));
    } else {
        l.wait(
            "fingerprintsConfigured taken from the fprintd that appeared",
            |rt| !rt.get(root, "fingerprintsConfigured").truthy(),
        );
        assert_eq!((l.runs(), l.active()), (1, false));
    }
    l.call("reset");
    assert!(!l.waiting());
}

#[test]
fn unverified_error_waits_for_fprintd() {
    waits_for_fprintd(
        concat!(module_path!(), "::unverified_error_waits_for_fprintd"),
        "wait",
        r#"{"devices": [{"fingers": ["right-index-finger"]}]}"#,
        true,
    );
}

#[test]
fn fprintd_without_prints_keeps_waiting() {
    waits_for_fprintd(
        concat!(module_path!(), "::fprintd_without_prints_keeps_waiting"),
        "noprints",
        r#"{"devices": [{"fingers": []}]}"#,
        false,
    );
}

fn type_and_wait_clear(l: &mut Lock) {
    l.wait_ready();
    l.drain();
    assert!(l.active());
    assert_eq!(l.runs(), 1);
    l.rt.set(l.root, "currentText", Value::str("hunter2"));
    l.rt.settle();
    l.rt.tick(100.0);
    l.rt.tick(10_100.0);
    l.drain();
}

#[test]
fn password_clear_keeps_fingerprint() {
    let Some(mut l) = lock("clear", WAITS) else {
        return;
    };
    l.call("tryFingerUnlock");
    type_and_wait_clear(&mut l);
    assert_eq!(l.rt.get(l.root, "currentText").to_js_string(), "");
    assert!(l.active(), "the password-clear timer aborted fingerPam");
    assert_eq!(l.runs(), 1);
    // reset() (unlock, next lock) still stops it.
    l.call("reset");
    assert!(!l.active());
}

#[test]
fn success_unlocks() {
    let Some(mut l) = lock("success", "exit 0") else {
        return;
    };
    l.rt.set(l.root, "targetAction", Value::Num(2.0));
    l.rt.settle();
    l.call("tryFingerUnlock");
    l.ran(1, "conversation 1 to end");
    assert_eq!(l.rt.get(l.root, "got").to_number(), 2.0);
    assert!(!l.waiting());
}
