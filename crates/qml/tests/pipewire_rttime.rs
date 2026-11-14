//! The shell's PipeWire clients leave the process's realtime budget (`RLIMIT_RTTIME`) alone.
//! `libpipewire-module-rt`, which `client.conf` loads into every client, asks the desktop
//! portal for `RTTimeUSecMax` and sets the answer as the limit of the whole process; a portal
//! with no RTKit behind it answers 0. With a limit of 0 the kernel sends the process SIGKILL
//! when a CPU-clock timer of one of its threads expires, and the JS watchdog kept such a
//! timer on every thread that evaluates: the shell was gone, with no line in its log and no
//! core dump, once its main thread had consumed the watchdog's timeout (5 s) of CPU time.
//! Runs a private session bus with such a portal and a private pipewire; fails when they are
//! not installed (`testenv`).
#![cfg(feature = "pipewire")]

use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use testenv::Wakeups;
use testenv::pipewire::socket_activated_pipewire;

struct Kill(Vec<Child>);

impl Drop for Kill {
    fn drop(&mut self) {
        for c in self.0.iter_mut().rev() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// `org.freedesktop.portal.Realtime` as xdg-desktop-portal 1.22 serves it on a machine without
/// RTKit: every property is 0. Counts the reads.
struct Realtime(Arc<AtomicUsize>);

#[zbus::interface(name = "org.freedesktop.portal.Realtime")]
impl Realtime {
    #[zbus(property, name = "MaxRealtimePriority")]
    fn max_realtime_priority(&self) -> i32 {
        self.0.fetch_add(1, Ordering::Relaxed);
        0
    }

    #[zbus(property, name = "MinNiceLevel")]
    fn min_nice_level(&self) -> i32 {
        self.0.fetch_add(1, Ordering::Relaxed);
        0
    }

    #[zbus(property, name = "RTTimeUSecMax")]
    fn rt_time_usec_max(&self) -> i64 {
        self.0.fetch_add(1, Ordering::Relaxed);
        0
    }
}

/// The kernel ends the child when this process ends: a process the kernel killed runs no
/// destructor, and a daemon left behind would keep the pipe the test's output is read from.
trait WithThisProcess {
    fn with_this_process(&mut self) -> &mut Self;
}

impl WithThisProcess for Command {
    fn with_this_process(&mut self) -> &mut Self {
        let die_with_parent = || {
            // SAFETY: PR_SET_PDEATHSIG changes only the calling process, the child.
            unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) };
            Ok(())
        };
        // SAFETY: the closure makes one prctl(2) call, which is async-signal-safe.
        unsafe { self.pre_exec(die_with_parent) }
    }
}

fn rttime() -> (libc::rlim_t, libc::rlim_t) {
    let mut l = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit(2) fills the rlimit it is given.
    let rc = unsafe { libc::getrlimit(libc::RLIMIT_RTTIME, &raw mut l) };
    assert_eq!(rc, 0);
    (l.rlim_cur, l.rlim_max)
}

/// The watchdog's timeout in this test.
const TIMEOUT: Duration = Duration::from_millis(100);

fn thread_cpu_time() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime(2) fills the timespec it is given.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &raw mut ts) };
    assert_eq!(rc, 0);
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// In its own process: libpipewire reads the session's directories with getenv(3), and the
/// limit is the process's.
#[test]
fn the_portal_realtime_budget_does_not_end_the_shell() {
    const TEST: &str = concat!(
        module_path!(),
        "::the_portal_realtime_budget_does_not_end_the_shell"
    );
    if !testenv::in_own_process(TEST) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for d in ["state", "config"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let mut bus = std::ffi::OsString::from("unix:path=");
        bus.push(dir.join("bus"));
        testenv::run_in_own_process_env(
            TEST,
            [
                ("XDG_RUNTIME_DIR", dir.into()),
                ("XDG_CONFIG_HOME", dir.join("config").into()),
                ("XDG_STATE_HOME", dir.join("state").into()),
                ("DBUS_SESSION_BUS_ADDRESS", bus),
            ],
            &["PIPEWIRE_REMOTE", "PIPEWIRE_RUNTIME_DIR"],
        );
        return;
    }
    if !testenv::require_all(&["dbus-daemon", "pipewire"]) {
        return;
    }
    let dir = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let mut kids = Kill(vec![]);
    // The bus libpipewire's module finds.
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let mut bus = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .arg(format!("--address={addr}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .with_this_process()
        .spawn()
        .unwrap();
    let out = bus.stdout.take().unwrap();
    kids.0.push(bus);
    // Printed once the bus listens.
    let mut line = String::new();
    BufReader::new(out).read_line(&mut line).unwrap();
    assert!(!line.is_empty(), "dbus-daemon printed no address");
    let asked = Arc::new(AtomicUsize::new(0));
    let _portal = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .name("org.freedesktop.portal.Desktop")
        .unwrap()
        .serve_at("/org/freedesktop/portal/desktop", Realtime(asked.clone()))
        .unwrap()
        .build()
        .unwrap();
    let before = rttime();
    assert_ne!(
        before.0, 0,
        "the test itself runs with a realtime budget of 0"
    );
    kids.0.push(
        socket_activated_pipewire(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .with_this_process()
            .spawn()
            .unwrap(),
    );

    // The watchdog's timer is armed by the first evaluation, before a client connects, as
    // in the shell.
    qml::watchdog::set_timeout(Some(TIMEOUT));
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str("import QtQuick\nItem {\n property int n: 0\n property int k: 0\n onNChanged: k = n\n}\n")
        .unwrap();
    rt.settle();
    // The shell's two clients: the `Pipewire` service and a visualizer's capture.
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let viz = {
        let mut w = rt.world.borrow_mut();
        let viz = qml::cava::new_item(&mut w, None);
        w.write(viz, "running", qml::value::Value::Bool(true));
        viz
    };
    assert!(qml::pw::wait_snapshot_until(|s| s.connected).connected);
    // The capture's first frame: its stream is set up on a live connection.
    loop {
        rt.poll_native();
        rt.settle();
        let points = rt.world.borrow_mut().read(viz, "points");
        if points.is_some_and(|p| !p.to_js_string().is_empty()) {
            break;
        }
        wake.wait(None, None);
    }
    // Short evaluations that take three timeouts of this thread's CPU time: the watchdog's
    // timer expires among them.
    let t0 = thread_cpu_time();
    let mut n = 0.0;
    while thread_cpu_time() - t0 < 3 * TIMEOUT {
        n += 1.0;
        rt.set(root, "n", qml::value::Value::Num(n));
    }
    assert_eq!(rt.get(root, "k").to_number(), n);
    assert_eq!(
        (rttime(), asked.load(Ordering::Relaxed)),
        (before, 0),
        "(RLIMIT_RTTIME (soft, hard), reads of the portal's realtime properties) with both \
         clients connected"
    );
}
