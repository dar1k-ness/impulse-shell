//! DesktopEntries scans run off the main loop: a `.desktop` file that blocks on read (a FIFO
//! with no writer yet) must stall neither the first scan nor a rescan after a directory
//! change, and the previous entries stay until the rescan lands.
use qml::runtime::Runtime;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;
use testenv::Wakeups;

const SHELL: &str = r#"import QtQuick
import Rustshell

ShellRoot {
    readonly property string apps: DesktopEntries.applications.values.map(e => e.id).sort().join(",") || "-"
}
"#;

fn entry(name: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec=true\n")
}

/// A `.desktop` FIFO that holds its reader until the test lets it go.
struct SlowEntry {
    /// Fires (and pings the loop) once a reader opened the FIFO: the writer's blocking open(2)
    /// returns only then.
    reading: Receiver<()>,
    go: Sender<()>,
    /// Whether the test let it go. A scan on the main loop would never get there: it blocks
    /// the test, which never sends `go`, until nextest's slow-timeout ends it.
    released: std::thread::JoinHandle<bool>,
}

/// The FIFO `home/applications/{file}` under `base`, for an entry named `name`. Later rescans
/// read a regular file there, which takes the FIFO's name in one rename(2) with its text
/// already in it: a rescan started by that rename finds the entry whole. Removing the FIFO
/// and writing the file in place would show such a rescan the name missing, then the file
/// empty, and its list without the entry replaces the blocked scan's.
fn slow_entry(
    base: &Path,
    file: &str,
    name: &'static str,
    ping: impl Fn() + Send + 'static,
) -> SlowEntry {
    let p = base.join("home/applications").join(file);
    assert!(Command::new("mkfifo").arg(&p).status().unwrap().success());
    // Not a directory DesktopEntries watches: staging the file starts no rescan.
    let staged = base.join("staged").join(file);
    std::fs::write(&staged, entry(name)).unwrap();
    let (reading_tx, reading) = channel();
    let (go, go_rx) = channel();
    let released = std::thread::spawn(move || {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&p)
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        // The open write end keeps the reader blocked in read(2) until the entry is written.
        let _ = reading_tx.send(());
        ping();
        let released = go_rx.recv().is_ok();
        // First the name, while the write end is still open: no scan opens a FIFO that
        // nobody writes to any more.
        std::fs::rename(&staged, &p).unwrap();
        f.write_all(entry(name).as_bytes()).unwrap();
        drop(f);
        released
    });
    SlowEntry {
        reading,
        go,
        released,
    }
}

/// The main loop on a clock of its own. It never waits for the pool: a scan blocked on a FIFO
/// is what the test holds.
struct Turns {
    wake: Wakeups,
    ep: Option<i32>,
    now: f64,
}

impl Turns {
    fn new(wake: Wakeups, rt: &mut Runtime) -> Turns {
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Turns { wake, ep, now: 0.0 }
    }

    /// Turns the loop until `done`, checked after every turn. With nothing ready the clock
    /// jumps to the next timer, or with none pending the loop blocks for the next event (a
    /// ping, the epoll set).
    fn until(&mut self, rt: &mut Runtime, mut done: impl FnMut(&mut Runtime) -> bool) {
        loop {
            rt.poll_native();
            rt.tick(self.now);
            rt.poll_native();
            rt.settle();
            if done(rt) {
                return;
            }
            let d = rt.next_deadline(self.now);
            let woke = self
                .wake
                .wait(self.ep, d.is_finite().then_some(Duration::ZERO));
            if woke.fd {
                rt.process_fd_ready();
            }
            if !woke.any() && d.is_finite() {
                self.now += d;
            }
        }
    }
}

#[test]
fn scans_never_block_the_main_loop() {
    const TEST: &str = concat!(module_path!(), "::scans_never_block_the_main_loop");
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::Builder::new()
            .prefix("desktop-async-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .unwrap();
        let base = dir.path();
        std::fs::create_dir_all(base.join("home/applications")).unwrap();
        std::fs::create_dir_all(base.join("sys/applications")).unwrap();
        // Made before the shell watches `base`.
        std::fs::create_dir_all(base.join("staged")).unwrap();
        std::fs::write(base.join("home/applications/a.desktop"), entry("A")).unwrap();
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_DATA_HOME", base.join("home").into_os_string()),
                ("XDG_DATA_DIRS", base.join("sys").into_os_string()),
                ("LANG", "C".into()),
                ("LC_ALL", "C".into()),
            ],
        );
        return;
    }
    let base = Path::new(&std::env::var_os("XDG_DATA_HOME").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let cfg = base.join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("shell.qml"), SHELL).unwrap();

    let wake = Wakeups::new();
    let first = slow_entry(&base, "slow.desktop", "Slow", wake.ping());
    let mut rt = Runtime::new(Some(cfg.clone()));
    let mut turns = Turns::new(wake, &mut rt);
    let ping = turns.wake.ping();
    let root = rt.load_file(&cfg.join("shell.qml")).unwrap();
    rt.settle();
    let apps = |rt: &mut Runtime| {
        rt.world
            .borrow_mut()
            .read(root, "apps")
            .map(|v| v.to_js_string())
            .unwrap_or_default()
    };
    // The main loop keeps turning while a scan is blocked on `slow`, the list stays `keep`,
    // and once `slow` is let go the scan lands with `want`. A scan on the main loop would
    // hold it until the entry's fallback, which `released` reports.
    let mut scan_around = |rt: &mut Runtime, slow: SlowEntry, want: &str, keep: &str| {
        turns.until(rt, |rt| {
            let reached = slow.reading.try_recv().is_ok();
            assert_eq!(
                apps(rt),
                keep,
                "list before the scan reached the slow entry"
            );
            reached
        });
        turns.until(rt, |_| true);
        assert_eq!(apps(rt), keep, "list while the scan is blocked");
        let _ = slow.go.send(());
        turns.until(rt, |rt| {
            let got = apps(rt);
            if got == want {
                return true;
            }
            assert_eq!(got, keep, "list while the scan is running");
            false
        });
        assert!(
            slow.released.join().unwrap(),
            "a scan blocked the main loop"
        );
    };
    scan_around(&mut rt, first, "a,slow", "-");

    // A package install: the rescan blocks on the new entry, the old list stays meanwhile.
    let second = slow_entry(&base, "b.desktop", "B", ping);
    scan_around(&mut rt, second, "a,b,slow", "a,slow");
}
