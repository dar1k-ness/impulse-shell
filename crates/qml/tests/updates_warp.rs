//! The tree's Updates service and Cloudflare WARP toggle run against stub `pacman` and
//! `warp-cli` on PATH and a fake notification server on a private session bus: the count,
//! availability and toggle state the shell shows, and the failure notifications it sends.
//! And SongRec.qml against the same server: its notifications and what the Shazam and YouTube
//! buttons open.
use qml::runtime::Runtime;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use testenv::Wakeups;

/// The test's host loop on a virtual clock (as `event_loop::Loop`), which can also block until
/// the runtime is woken: by its waker's ping (a worker's reply) or its epoll set (process I/O).
struct Host {
    wake: Wakeups,
    ep: Option<i32>,
    /// The virtual time in ms, what the runtime's timers see.
    now: f64,
}

impl Host {
    fn new(rt: &mut Runtime) -> Host {
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Host { wake, ep, now: 0.0 }
    }

    /// Runs loop turns until nothing is ready at `now`: no pool work in flight, no wakeup,
    /// no event on the epoll set and no timer due.
    fn drain(&mut self, rt: &mut Runtime) {
        loop {
            workpool::wait_idle();
            rt.poll_native();
            rt.tick(self.now);
            rt.poll_native();
            rt.settle();
            let woke = self.wake.wait(self.ep, Some(Duration::ZERO));
            if woke.fd {
                rt.process_fd_ready();
            }
            if !woke.any() && rt.next_deadline(self.now) > 0.0 {
                return;
            }
        }
    }

    /// Blocks until the runtime is woken, the clock standing still.
    fn block(&mut self, rt: &mut Runtime) {
        if self.wake.wait(self.ep, None).fd {
            rt.process_fd_ready();
        }
    }
}

fn ri() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"))
}

const MAIN: &str = "import QtQuick
import ri.services
import ri.modules.common
import ri.modules.common.models.quickToggles
Item {
    property bool ready: Config.ready
    property bool available: Updates.available
    property int count: Updates.count
    property var warp: CloudflareWarpToggle {}
    property bool warpAvailable: warp.available
    property bool warpToggled: warp.toggled
    function recheck() { warp.destroy(); warp = Qt.createQmlObject('import ri.modules.common.models.quickToggles; CloudflareWarpToggle {}', this) }
}
";

/// What the fake notification server was sent, and the signal that one more arrived.
type Log = Arc<(Mutex<Vec<String>>, Condvar)>;

/// The fake server: the log and the id of the last notification (they count from 1).
struct Srv(Log, AtomicU32);

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Srv {
    fn get_capabilities(&self) -> Vec<String> {
        vec!["actions".into(), "body".into()]
    }
    /// Closes as a real server does: `NotificationClosed(id, 3)`, which ends the sender's wait.
    async fn close_notification(&self, #[zbus(connection)] conn: &zbus::Connection, id: u32) {
        conn.emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "NotificationClosed",
            &(id, 3u32),
        )
        .await
        .unwrap();
    }
    fn get_server_information(&self) -> (String, String, String, String) {
        ("test".into(), "test".into(), "1".into(), "1.2".into())
    }
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app: String,
        _replaces: u32,
        _icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        _hints: HashMap<String, zbus::zvariant::OwnedValue>,
        _timeout: i32,
    ) -> u32 {
        let (log, more) = &*self.0;
        // With buttons, `|key=label,...` follows.
        let mut line = format!("{app}|{summary}|{body}");
        if !actions.is_empty() {
            let pairs: Vec<String> = actions.chunks(2).map(|p| p.join("=")).collect();
            line += &format!("|{}", pairs.join(","));
        }
        log.lock().unwrap().push(line);
        more.notify_all();
        self.1.fetch_add(1, Ordering::SeqCst) + 1
    }
}

fn stub(dir: &Path, name: &str, script: &str) {
    let p = dir.join(name);
    testenv::install_script(&p, &format!("#!/bin/sh\n{script}\n"));
}

/// Runs the loop until `f` holds, on a clock that stands still: it waits for the stubs' output
/// and exits, never for a stretch of time (nextest's slow-timeout ends a hang).
fn wait(lp: &mut Host, rt: &mut Runtime, mut f: impl FnMut(&mut Runtime) -> bool) {
    loop {
        lp.drain(rt);
        if f(rt) {
            return;
        }
        lp.block(rt);
    }
}

/// Runs the loop until no `Process` is left running and the pool is idle (`warp-cli` has
/// answered and the toggle has handled it), then takes what the notification server got.
/// `Notify.send` puts a notification on the bus without waiting for the server, so this
/// waits for the server to have received `n` of them.
fn notes(lp: &mut Host, rt: &mut Runtime, log: &Log, n: usize) -> Vec<String> {
    wait(lp, rt, |rt| rt.world.borrow().procs.is_empty());
    received(log, n)
}

/// Waits for the server to have received `n` notifications and takes them.
fn received(log: &Log, n: usize) -> Vec<String> {
    let (log, more) = &**log;
    let mut got = more
        .wait_while(log.lock().unwrap(), |l| l.len() < n)
        .unwrap();
    std::mem::take(&mut *got)
}

/// A private session bus: the daemon and its address.
fn bus() -> (Daemon, String) {
    let mut d = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut addr = String::new();
    std::io::BufReader::new(d.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    (Daemon(d), addr.trim().to_string())
}

/// The fake notification server on the bus of DBUS_SESSION_BUS_ADDRESS.
fn server() -> (zbus::blocking::Connection, Log) {
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let log: Log = Arc::default();
    let srv = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .serve_at(
            "/org/freedesktop/Notifications",
            Srv(log.clone(), AtomicU32::new(0)),
        )
        .unwrap()
        .build()
        .unwrap();
    (srv, log)
}

#[test]
fn updates_and_warp_toggle() {
    let test = concat!(module_path!(), "::updates_and_warp_toggle");
    if !testenv::in_own_process(test) {
        if !testenv::require("dbus-daemon") {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(home.join("qml")).unwrap();
        std::fs::create_dir_all(home.join(".config/rusty-impulse")).unwrap();
        std::fs::write(home.join(".config/rusty-impulse/config.json"), "{}").unwrap();
        std::fs::write(home.join("qml/main.qml"), MAIN).unwrap();
        // `which checkupdates` finds the script; the count comes from checkupdates' own steps
        // over the pacman it finds (a native check does not run the script itself).
        stub(&bin, "checkupdates", "exit 99");
        stub(&bin, "fakeroot", "[ \"$1\" = -- ] && shift; exec \"$@\"");
        stub(&bin, "pacman-conf", "true");
        stub(
            &bin,
            "pacman",
            "case \"$1\" in -Qu) printf 'linux 1 -> 2\\nmesa 1 -> 2\\nqt6-base 1 -> 2\\nfoo 1 -> 2 [ignored]\\n';; esac",
        );
        // `status` answers from the `mode` file; registration fails, connect fails.
        stub(
            &bin,
            "warp-cli",
            &format!(
                "case \"$1\" in status) cat '{}';; registration) exit 0;; connect) exit 1;; esac",
                home.join("mode").display()
            ),
        );
        std::fs::write(home.join("mode"), "Status update: Connected\n").unwrap();
        let (_d, addr) = bus();
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let vars = testenv::home_vars(home).into_iter().chain([
            ("PATH", path.into()),
            ("DBUS_SESSION_BUS_ADDRESS", addr.into()),
            ("CHECKUPDATES_DB", home.join("checkup-db").into()),
        ]);
        testenv::run_in_own_process(test, vars);
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let bin = home.join("bin");
    let dir = home.join("qml");
    let mode = home.join("mode");
    let (_srv, log) = server();

    let mut rt = Runtime::new(Some(ri()));
    let mut lp = Host::new(&mut rt);
    let dirs = ["modules/common", "modules/common/functions", "services"].map(|d| ri().join(d));
    rt.load_singletons_lazy(&dirs);
    rt.set_screens(&[("Virtual-1".into(), 0, 0, 1920, 1080)]);
    let r = rt.load_file(&dir.join("main.qml")).unwrap();
    rt.settle();

    wait(&mut lp, &mut rt, |rt| rt.get(r, "count").to_number() == 3.0);
    assert!(rt.get(r, "available").truthy());
    wait(&mut lp, &mut rt, |rt| rt.get(r, "warpToggled").truthy());
    assert!(rt.get(r, "warpAvailable").truthy());

    // Not registered: registration succeeds, connect fails -> "Connection failed".
    std::fs::write(&mode, "Unable to connect to the daemon\n").unwrap();
    rt.call_method(r, "recheck", vec![]).unwrap().unwrap();
    assert_eq!(
        notes(&mut lp, &mut rt, &log, 1),
        [
            "Shell|Cloudflare WARP|Connection failed. Please inspect manually with the <tt>warp-cli</tt> command"
        ]
    );

    // Registration fails -> "Registration failed".
    stub(
        &bin,
        "warp-cli",
        &format!(
            "case \"$1\" in status) cat '{}';; *) exit 1;; esac",
            mode.display()
        ),
    );
    rt.call_method(r, "recheck", vec![]).unwrap().unwrap();
    assert_eq!(
        notes(&mut lp, &mut rt, &log, 1),
        [
            "Shell|Cloudflare WARP|Registration failed. Please inspect manually with the <tt>warp-cli</tt> command"
        ]
    );
    assert!(!rt.get(r, "warpToggled").truthy());
}

const WARP: &str = "import QtQuick
import Impulse
Item {
    property var exits: []
    function connect() { return Warp.connect() }
    Connections {
        target: Warp
        function onConnectExited(request, exitCode) { exits = exits.concat([`${request}:${exitCode}`]) }
    }
}
";

/// A `warp-cli` that does not return (its daemon does not answer) holds no worker of the
/// shared pool, however many calls wait for it: other pool work runs meanwhile. The calls
/// run one after another and report in the order made; one still running when the shell's
/// objects go is ended with them.
#[test]
fn hung_warp_cli_holds_no_shared_worker() {
    use std::io::{Read, Write};
    let test = concat!(module_path!(), "::hung_warp_cli_holds_no_shared_worker");
    if !testenv::in_own_process(test) {
        if !testenv::require("mkfifo") {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for fifo in ["started", "release"] {
            let made = Command::new("mkfifo").arg(home.join(fifo)).status();
            assert!(made.unwrap().success());
        }
        // Says it started, then waits for a line that only the test writes.
        stub(
            &bin,
            "warp-cli",
            &format!(
                "echo s > '{}'\nread go < '{}'",
                home.join("started").display(),
                home.join("release").display()
            ),
        );
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let vars = testenv::home_vars(home)
            .into_iter()
            .chain([("PATH", path.into())]);
        testenv::run_in_own_process(test, vars);
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    // Both ends of each FIFO: no open or write of the stub waits for the test.
    let both = |name: &str| {
        let mut o = std::fs::OpenOptions::new();
        o.read(true).write(true).open(home.join(name)).unwrap()
    };
    let (mut started, mut release) = (both("started"), both("release"));
    let mut wait_started = || {
        let mut line = [0u8; 2];
        started.read_exact(&mut line).unwrap();
        assert_eq!(&line, b"s\n");
    };
    let mut rt = Runtime::new(None);
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let r = rt.load_str(WARP).unwrap();
    rt.settle();
    // More calls than the pool has workers.
    let calls = workpool::WORKERS + 1;
    for n in 1..=calls {
        let request = rt.call_method(r, "connect", vec![]).unwrap().unwrap();
        assert_eq!(request.to_number(), n as f64);
    }
    wait_started();
    let (tx, ran) = std::sync::mpsc::channel();
    workpool::run(move || {
        let _ = tx.send(());
    });
    ran.recv().expect("the pool ran a job while warp-cli hung");
    // Let every call end: they report in the order made.
    for n in 1..=calls {
        if n > 1 {
            wait_started();
        }
        release.write_all(b"go\n").unwrap();
    }
    let exits = |rt: &Runtime| rt.get(r, "exits").to_js_string();
    let want: Vec<String> = (1..=calls).map(|n| format!("{n}:0")).collect();
    while exits(&rt).split(',').count() < calls {
        wake.wait(None, None);
        rt.poll_native();
        rt.settle();
    }
    assert_eq!(exits(&rt), want.join(","));
    // One that hangs when the shell's objects go is ended with them.
    rt.call_method(r, "connect", vec![]).unwrap().unwrap();
    wait_started();
    drop(rt);
    workpool::wait_idle();
}

const SONGREC: &str = "import QtQuick
import Impulse
import ri.services
Item {
    property int finished: 0
    property int pressed: 0
    function listen() { SongRec.toggleRunning(true) }
    function recognized(json) { SongRec.handleRecognition(json) }
    Connections {
        target: MusicRecognizer
        function onFinished(text, exitCode) { finished++ }
    }
    Connections {
        target: Notify
        function onActionInvoked(id, index) { pressed++ }
    }
}
";

/// SongRec.qml's notifications go out through `Notify`: without `songrec` installed a run
/// ends with "Make sure you have songrec installed" alone (nothing listened, so it is not
/// "too niche", which came first), a recognized track is shown with the Shazam and YouTube
/// buttons, and the buttons open the track's page and the YouTube search (Shazam is button 0).
#[test]
fn songrec_notifications() {
    let test = concat!(module_path!(), "::songrec_notifications");
    if !testenv::in_own_process(test) {
        if !testenv::require("dbus-daemon") {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(home.join("qml")).unwrap();
        std::fs::create_dir_all(home.join(".config/rusty-impulse")).unwrap();
        std::fs::write(home.join(".config/rusty-impulse/config.json"), "{}").unwrap();
        std::fs::write(home.join("qml/main.qml"), SONGREC).unwrap();
        // `xdg-open` reports its argument through a FIFO; no `songrec` on PATH.
        let opened = home.join("opened");
        assert!(
            Command::new("mkfifo")
                .arg(&opened)
                .status()
                .unwrap()
                .success()
        );
        stub(
            &bin,
            "xdg-open",
            &format!("echo \"$1\" > '{}'", opened.display()),
        );
        let (_d, addr) = bus();
        let vars = testenv::home_vars(home).into_iter().chain([
            ("PATH", bin.into()),
            ("DBUS_SESSION_BUS_ADDRESS", addr.into()),
        ]);
        testenv::run_in_own_process(test, vars);
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    // Open for writing too, so the FIFO never reads end-of-file between calls: each read
    // blocks until the next `xdg-open` writes its line.
    let mut opened = std::io::BufReader::new(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(home.join("opened"))
            .unwrap(),
    );
    let mut opened = move || {
        let mut line = String::new();
        opened.read_line(&mut line).unwrap();
        line.trim_end().to_string()
    };
    let (srv, log) = server();
    let press = |id: u32, key: &str| {
        srv.emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "ActionInvoked",
            &(id, key),
        )
        .unwrap();
    };

    let mut rt = Runtime::new(Some(ri()));
    let mut lp = Host::new(&mut rt);
    let dirs = ["modules/common", "modules/common/functions", "services"].map(|d| ri().join(d));
    rt.load_singletons_lazy(&dirs);
    let r = rt.load_file(&home.join("qml/main.qml")).unwrap();
    rt.settle();

    // No songrec: the run ends at once with exit code 1 and no text.
    rt.call_method(r, "listen", vec![]).unwrap().unwrap();
    wait(&mut lp, &mut rt, |rt| {
        rt.get(r, "finished").to_number() == 1.0
    });
    wait(&mut lp, &mut rt, |rt| rt.world.borrow().procs.is_empty());

    // A notification with buttons is waited on by a pool job until it is closed, and
    // `Host::drain` waits for the pool: the loop runs again only after a press, which closes.
    let track = |title: &str| {
        format!(
            r#"{{"track":{{"title":"{title}","subtitle":"Artist","url":"https://www.shazam.com/{title}"}}}}"#
        )
    };
    let shown = |title: &str| {
        [format!(
            "Shell|Music Recognized|{title} - Artist|0=Shazam,1=YouTube"
        )]
    };
    let recognized = |rt: &mut Runtime, title: &str| {
        rt.call_method(r, "recognized", vec![qml::value::Value::str(&track(title))])
            .unwrap()
            .unwrap();
    };
    // The server gets notifications in the order they were sent: with the recognized track
    // there, the run without songrec has sent all it sends, the one notification before it.
    // The track's is the second of this server; Shazam is its button 0.
    recognized(&mut rt, "One");
    let [one] = shown("One");
    assert_eq!(
        received(&log, 2),
        [
            "Shell|Couldn't recognize music|Make sure you have songrec installed".to_string(),
            one
        ]
    );
    press(2, "0");
    wait(&mut lp, &mut rt, |rt| {
        rt.get(r, "pressed").to_number() == 1.0
    });
    assert_eq!(opened(), "https://www.shazam.com/One");

    // The third and fourth are open together. A button of the third, which is no longer the
    // shown track's, opens nothing: the next thing opened is the fourth's YouTube search.
    recognized(&mut rt, "Two");
    assert_eq!(received(&log, 1), shown("Two"));
    recognized(&mut rt, "Three");
    assert_eq!(received(&log, 1), shown("Three"));
    press(3, "0");
    press(4, "1");
    wait(&mut lp, &mut rt, |rt| {
        rt.get(r, "pressed").to_number() == 3.0
    });
    // xdg-open is given QUrl::toEncoded(), as Qt 6.12 gives it (tests/openurl_relative).
    assert_eq!(
        opened(),
        "https://www.youtube.com/results?search_query=Three%20-%20Artist"
    );
}

struct Daemon(std::process::Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
