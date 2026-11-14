//! `Qt.openUrlExternally` never blocks the GUI thread: the portal call ran synchronously
//! (a slow portal froze the shell for up to the D-Bus timeout) and a local FIFO was opened for
//! reading before the call (blocking forever with no writer).
use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};

/// Closed until the test opens it.
type Gate = Arc<(Mutex<bool>, Condvar)>;

/// What the portal was asked, notifying the test of each call.
type Log = Arc<(Mutex<Vec<String>>, Condvar)>;

struct OpenUri(Log, Gate);

#[zbus::interface(name = "org.freedesktop.portal.OpenURI")]
impl OpenUri {
    #[zbus(name = "OpenURI")]
    fn open_uri(
        &self,
        _parent: &str,
        uri: &str,
        _o: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> zbus::zvariant::OwnedObjectPath {
        self.0.0.lock().unwrap().push(format!("uri {uri}"));
        self.0.1.notify_all();
        // A portal that takes its time (a chooser dialog, a slow handler): it answers only once
        // the test has seen the call return.
        let (open, cv) = &*self.1;
        drop(cv.wait_while(open.lock().unwrap(), |open| !*open).unwrap());
        zbus::zvariant::OwnedObjectPath::try_from("/r/1").unwrap()
    }
    fn open_file(
        &self,
        _parent: &str,
        fd: zbus::zvariant::OwnedFd,
        _o: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> zbus::zvariant::OwnedObjectPath {
        use std::os::fd::AsRawFd;
        let p = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())).unwrap();
        self.0
            .0
            .lock()
            .unwrap()
            .push(format!("file {}", p.to_string_lossy()));
        self.0.1.notify_all();
        zbus::zvariant::OwnedObjectPath::try_from("/r/2").unwrap()
    }
}

/// A session bus at the address the environment names, with the fake portal on it.
fn private_bus(log: Log, gate: Gate) {
    let mut c = Command::new("dbus-daemon");
    c.args(["--session", "--nofork", "--print-address=1", "--nopidfile"])
        .arg(format!(
            "--address={}",
            std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap()
        ))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let die_with_parent = || {
        // SAFETY: prctl(2) PR_SET_PDEATHSIG takes a signal number, no pointers.
        unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) };
        Ok(())
    };
    // SAFETY: the hook runs between fork and exec; it only makes the prctl(2) syscall, which
    // is async-signal-safe, and allocates nothing.
    unsafe { std::os::unix::process::CommandExt::pre_exec(&mut c, die_with_parent) };
    let mut d = c.spawn().expect("dbus-daemon");
    let mut addr = String::new();
    BufReader::new(d.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    let addr = addr.trim().to_string();
    std::mem::forget(d);
    let c = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .name("org.freedesktop.portal.Desktop")
        .unwrap()
        .serve_at("/org/freedesktop/portal/desktop", OpenUri(log, gate))
        .unwrap()
        .build()
        .unwrap();
    std::mem::forget(c);
}

#[test]
fn open_url_externally_does_not_block() {
    const TEST: &str = concat!(module_path!(), "::open_url_externally_does_not_block");
    // The portal is found through the session bus address: run in a process of its own with
    // it naming a socket in a fresh directory.
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(TEST, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        return;
    }
    // A call that blocks (the FIFO open never returns) hangs the test until nextest's
    // slow-timeout ends it.
    let log = Log::default();
    let gate = Gate::default();
    private_bus(log.clone(), gate.clone());
    let d = std::env::temp_dir().join(format!("ri-openurl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let fifo = d.join("pipe");
    let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: `c` is a NUL-terminated path that outlives the call.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);

    let mut rt = Runtime::new(None);
    let root = rt
        .load_str("import QtQuick\nItem { function go(u) { return Qt.openUrlExternally(u) } }\n")
        .unwrap();
    let mut go = |u: &str| {
        let r = rt
            .call_method(root, "go", vec![qml::value::Value::str(u)])
            .unwrap()
            .unwrap();
        assert!(r.truthy(), "{u}");
    };
    let wait_for = |what: &str| {
        let (calls, cv) = &*log;
        drop(
            cv.wait_while(calls.lock().unwrap(), |c| !c.iter().any(|l| l == what))
                .unwrap(),
        );
    };

    // Returned while the portal still holds its answer to OpenURI.
    go("https://example.org/a");
    wait_for("uri https://example.org/a");
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();

    // Returned although the FIFO has no writer, so opening it for reading never completes.
    go(&fifo.to_string_lossy());
    wait_for(&format!("file {}", fifo.to_string_lossy()));
    let _ = std::fs::remove_dir_all(&d);
}
