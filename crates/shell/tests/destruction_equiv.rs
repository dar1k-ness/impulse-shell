//! `Component.onDestruction` when the shell ends, against the real Quickshell 0.3.2 and Qt 6.12.
//! Quickshell deletes the root object at `Qt.quit()`, `Qt.exit()` and `qs kill`, and the
//! handler of every live object under it runs before the process ends; a termination signal
//! ends Quickshell with no handler run.
//!
//! Each directory of `destruction/` is a configuration. `<name>.qs.txt` is what Quickshell
//! printed for it (`QS_NO_RELOAD_POPUP=1 qs --no-color -p <name>`): its console lines as
//! `qml: <text>` and its exit status as `status <n>`. `<name>.qt612.txt` is the same from
//! `qml6` of Qt 6.12.0 (`QT_FORCE_STDERR_LOGGING=1 QT_QPA_PLATFORM=offscreen qml6
//! <name>/shell.qml`). Each test runs the real programs, which must print what is recorded,
//! and the native shell (`impulse-shell -p`), which must print what Quickshell does.
//!
//! - `quit`: a child, the object of a LazyLoader and the root.
//! - `order`: the order of the handlers, in QML that Qt runs too: an object and its children,
//!   a Loader and its item, a Repeater and its delegates, the instances of an inline component,
//!   objects made by `createObject` with a parent and without, a singleton.
//! - `orderqs`: the same with Quickshell's `Scope`, `LazyLoader` and `Variants`.
//! - `requit`: `Qt.exit(3)` then `Qt.quit()`, then `Qt.quit()` and `Qt.exit(5)` from handlers.
//! - `itemroot`: a root that Quickshell wraps in a window.
//! - `effects`: what a handler can still start: `execDetached`, a `Process`, a write of a
//!   `FileView`, a `Timer`.
//! - `bluetooth`: a handler that stops a Bluetooth discovery, with a bluetoothd of the test
//!   on a bus of its own.
//! - `silent`: a handler that leaves a Bluetooth write and a notification queued for services
//!   that do not answer (the native shell only).
//! - `clipimage`: a handler that removes a decoded clipboard image (the native shell only).
//! - `idle`: a shell that stays up, ended by `qs kill` and by SIGTERM, SIGINT and SIGHUP.
//! - `lost`: a shell that stays up with a `Process` of its own and one of a singleton, whose
//!   compositor goes away.
//! - `long`: a handler that works for a second of CPU time and writes 8 MiB with a `FileView`,
//!   then `Qt.exit(3)`'s other handlers.
//! - `watchdog`: a handler that never returns (the native shell only: Quickshell has no limit
//!   and stays in the handler, docs/quickshell-differences.md), then the same other handlers.
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;

use testenv::Log;

fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/destruction")
        .join(name)
}

/// The lines of `destruction/<name>.<who>.txt`.
fn recorded(name: &str, who: &str) -> Vec<String> {
    let file = dir(&format!("{name}.{who}.txt"));
    std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{}: {e}", file.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

/// The console lines of `out` as `qml: <text>`: the native shell prints `console: <text>`,
/// Quickshell `DEBUG qml: <text>`, qml6 `qml: <text>`.
fn console(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| {
            let t = l.strip_prefix("console: ").or_else(|| {
                l.strip_prefix("qml: ")
                    .or_else(|| l.find(" qml: ").map(|i| &l[i + 6..]))
            })?;
            Some(format!("qml: {t}"))
        })
        .collect()
}

/// The console lines and then how the program ended: `status <n>`, or `signal <n>`.
fn told(out: &str, status: ExitStatus) -> Vec<String> {
    let mut t = console(out);
    t.push(match (status.code(), status.signal()) {
        (Some(c), _) => format!("status {c}"),
        (None, Some(s)) => format!("signal {s}"),
        (None, None) => format!("{status}"),
    });
    t
}

fn quickshell(conf: &Path) -> Command {
    let mut c = Command::new("qs");
    c.arg("--no-color")
        .arg("-p")
        .arg(conf)
        .env("QS_NO_RELOAD_POPUP", "1");
    c
}

fn native(conf: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_impulse-shell"));
    c.arg("-p").arg(conf).env("RI_INTERP", "1");
    c
}

fn qml6(conf: &Path) -> Command {
    let mut c = Command::new("qml6");
    c.arg(conf.join("shell.qml"))
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_QPA_PLATFORM", "offscreen");
    c
}

/// Runs `c` to its end.
fn run(mut c: Command) -> Vec<String> {
    let out = c.stdin(Stdio::null()).output().unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text += &String::from_utf8_lossy(&out.stderr);
    eprintln!("{c:?}:\n{text}");
    told(&text, out.status)
}

/// Quickshell and a compositor for it and for the native shell.
fn available() -> bool {
    if !testenv::require("qs") {
        return false;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return false;
    }
    true
}

/// Quickshell prints `<name>.qs.txt` for `destruction/<name>` and the native shell the same.
fn same_as_quickshell(name: &str) {
    if !available() {
        return;
    }
    let want = recorded(name, "qs");
    assert_eq!(run(quickshell(&dir(name))), want, "Quickshell on {name}");
    assert_eq!(run(native(&dir(name))), want, "the native shell on {name}");
}

/// qml6 of Qt 6.12 prints `<name>.qt612.txt` for `destruction/<name>/shell.qml`.
fn qt_prints_its_record(name: &str) -> Option<Vec<String>> {
    if !testenv::require("qml6") {
        return None;
    }
    let want = recorded(name, "qt612");
    assert_eq!(run(qml6(&dir(name))), want, "qml6 on {name}");
    Some(want)
}

#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn handlers_run_at_qt_quit() {
    same_as_quickshell("quit");
}

/// Qt and Quickshell run the handlers in one order. The difference is the singleton: Qt runs
/// its handlers last, Quickshell stops JavaScript once the root is deleted, before the engine
/// deletes its singletons, so theirs never run (docs/qt-differences.md).
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn handlers_run_in_the_order_of_qt_except_those_of_singletons() {
    let Some(qt) = qt_prints_its_record("order") else {
        return;
    };
    let qs = recorded("order", "qs");
    let mut with_singleton = qs.clone();
    let status = with_singleton.pop().unwrap();
    with_singleton.extend(
        [
            "qml: destroyed singleton's object",
            "qml: destroyed singleton",
        ]
        .map(str::to_string),
    );
    with_singleton.push(status);
    assert_eq!(qt, with_singleton, "Qt 6.12 against Quickshell on order");
    same_as_quickshell("order");
}

#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn handlers_of_quickshell_objects_run_in_the_order_of_quickshell() {
    same_as_quickshell("orderqs");
}

/// Asking to end again while the shell ends changes nothing but the status: every handler
/// runs once, the code after `Qt.quit()` in a handler runs, and the last `Qt.exit(code)` is
/// the status. qml6 runs the same handlers; its status is that of the `qml` tool.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn a_handler_that_asks_to_exit_sets_the_status() {
    let Some(qt) = qt_prints_its_record("requit") else {
        return;
    };
    let qs = recorded("requit", "qs");
    assert_eq!(qt[..qt.len() - 1], qs[..qs.len() - 1], "console of requit");
    assert_eq!(qs.last().unwrap(), "status 5");
    same_as_quickshell("requit");
}

/// A root that is not a Quickshell object is made the child of a window Quickshell creates.
/// That window is what Quickshell deletes, and JavaScript stops before its children go: no
/// handler runs. Qt runs them (docs/qt-differences.md).
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn a_root_quickshell_wraps_in_a_window_runs_no_handler() {
    let Some(qt) = qt_prints_its_record("itemroot") else {
        return;
    };
    assert!(qt.contains(&"qml: destroyed root".to_string()), "{qt:?}");
    let qs = recorded("itemroot", "qs");
    assert_eq!(qs, ["qml: quit", "status 0"]);
    same_as_quickshell("itemroot");
}

/// Whether the process whose pid a `Process` wrote has terminated, by its pidfd, which is what
/// [`testenv::wait_gone`] waits on. The state letter of `/proc/<pid>/stat` does not tell: a
/// `Process` Quickshell killed and did not wait for is reaped by the init of the container, and
/// from that until the pid is released the letter is `X`, neither `Z` nor a missing file. A file
/// left empty is of one signalled before its `echo`: ended.
fn ended(pid: &str) -> bool {
    pid.parse().map_or(true, testenv::terminated)
}

/// What the root's handler of `effects` did, by what is left when the shell has ended.
fn effects(shell: fn(&Path) -> Command) -> Vec<String> {
    let tmp = tempfile::tempdir().unwrap();
    let at = |f: &str| tmp.path().join(f);
    let read = |f: &str| std::fs::read_to_string(at(f)).map(|t| t.trim().to_string());
    // A FIFO: the detached command is still starting when the shell has ended, and its
    // write ends when this test has read it.
    let made = Command::new("mkfifo").arg(at("detached")).status().unwrap();
    assert!(made.success(), "mkfifo: {made}");
    // To a file: a process the handler started and left behind would keep a pipe open.
    let log = std::fs::File::create(at("log")).unwrap();
    let status = shell(&dir("effects"))
        .env("DESTR_DIR", tmp.path())
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .status()
        .unwrap();
    let mut t = told(&read("log").unwrap(), status);
    eprintln!("{t:?}");
    if !t.contains(&"qml: destroyed root".to_string()) {
        return t;
    }
    t.push(format!("execDetached: {}", read("detached").unwrap()));
    // Both shells signal these and do not wait for them: their end is waited for here, and
    // one left running stops the test until nextest ends it. One signalled between its
    // redirection and its `echo` left its file empty: it has ended.
    for pid in ["started.pid", "stopped.pid", "single.pid"] {
        if let Some(pid) = read(pid).ok().and_then(|pid| pid.parse().ok()) {
            testenv::wait_gone(pid);
        }
    }
    t.push(format!(
        "Process started by the handler runs: {}",
        read("started.pid").is_ok_and(|pid| !ended(&pid))
    ));
    t.push(format!(
        "Process stopped by the handler runs: {}",
        !ended(&read("stopped.pid").unwrap())
    ));
    t.push(format!("FileView: {}", read("file").unwrap_or_default()));
    t.push(format!(
        "Process of the singleton runs: {}",
        !ended(&read("single.pid").unwrap())
    ));
    t.push(format!(
        "execDetached of the singleton: {}",
        read("singleton").unwrap_or_default()
    ));
    t
}

/// From a handler at exit, as in Quickshell: `execDetached` runs its command and the text
/// given to a `FileView` is written; a `Process` set running does not run (Quickshell kills
/// it as it deletes the `Process`), a `Timer` started does not fire, and the handler of a
/// singleton does not run, but the `Process` of the singleton ends with the shell (the original's
/// `AcpSession` leaves no agent behind).
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn what_a_handler_starts_at_exit() {
    if !available() {
        return;
    }
    let want = [
        "qml: quit single",
        "qml: destroyed root",
        "status 0",
        "execDetached: detached",
        "Process started by the handler runs: false",
        "Process stopped by the handler runs: false",
        "FileView: written",
        "Process of the singleton runs: false",
        "execDetached of the singleton: ",
    ];
    assert_eq!(effects(quickshell), want, "Quickshell");
    assert_eq!(effects(native), want, "the native shell");
}

const HCI0: &str = "/org/bluez/hci0";

struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The fake bluetoothd (`crates/qml/tests/fake_sysbus.py`) with one adapter, on a bus daemon
/// of its own.
struct Bluez {
    tmp: tempfile::TempDir,
    addr: String,
    daemon: Kill,
    fake: Kill,
    to_fake: std::process::ChildStdin,
    from_fake: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
    /// What the fake has printed so far.
    lines: Vec<String>,
}

impl Bluez {
    /// None without dbus-daemon, dbus-send or python3 with dbus-python and gi.
    fn start() -> Option<Bluez> {
        use std::io::{BufRead, BufReader};
        let python = "/usr/bin/python3";
        let has = Command::new(python)
            .args([
                "-c",
                "import dbus, dbus.mainloop.glib; from gi.repository import GLib",
            ])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !has {
            testenv::unavailable("python3 with dbus-python and gi not found");
            return None;
        }
        if !testenv::require_all(&["dbus-daemon", "dbus-send"]) {
            return None;
        }
        let tmp = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", tmp.path().join("bus").display());
        let mut daemon = Kill(
            Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
                .arg(format!("--address={addr}"))
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        // The address is printed once the daemon listens.
        let mut line = String::new();
        BufReader::new(daemon.0.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(!line.trim().is_empty(), "dbus-daemon printed no address");

        let init = format!(
            r#"{{"objects": {{"{HCI0}": {{"org.bluez.Adapter1": {{"Address": "00:1A:7D:DA:71:13",
            "Alias": "arch", "Name": "arch", "Powered": true, "PowerState": "on",
            "Discoverable": false, "DiscoverableTimeout": 180, "Discovering": false,
            "Pairable": true, "PairableTimeout": 0}}}}}}}}"#
        );
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../qml/tests/fake_sysbus.py");
        let mut fake = Kill(
            Command::new(python)
                .args([script, "bluez", &init])
                .env("DBUS_SYSTEM_BUS_ADDRESS", &addr)
                .env("DBUS_SESSION_BUS_ADDRESS", &addr)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let to_fake = fake.0.stdin.take().unwrap();
        let from_fake = BufReader::new(fake.0.stdout.take().unwrap()).lines();
        let mut b = Bluez {
            tmp,
            addr,
            daemon,
            fake,
            to_fake,
            from_fake,
            lines: vec![],
        };
        b.until("READY");
        Some(b)
    }

    /// Reads what the fake prints up to a line with `what`.
    fn until(&mut self, what: &str) {
        for l in self.from_fake.by_ref() {
            let l = l.unwrap();
            let found = l.contains(what);
            self.lines.push(l);
            if found {
                return;
            }
        }
        panic!(
            "the fake bluetoothd ended without printing {what:?}; it printed: {:?}",
            self.lines
        );
    }

    /// Sends the fake a command.
    fn cmd(&mut self, json: &str) {
        use std::io::Write;
        writeln!(self.to_fake, "{json}").unwrap();
    }

    /// Everything the fake was sent through the daemon so far has been printed when this
    /// returns: a call of the test's own through the same daemon comes after it, then the
    /// fake is told to end.
    fn finish(&mut self) {
        let ping = Command::new("dbus-send")
            .arg(format!("--bus={}", self.addr))
            .args(["--print-reply", "--dest=org.bluez", "/"])
            .arg("org.freedesktop.DBus.ObjectManager.GetManagedObjects")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(ping.status.success(), "dbus-send: {ping:?}");
        self.cmd(r#"{"op":"quit"}"#);
        self.until("DONE quit");
    }

    /// The method calls the fake printed.
    fn calls(&self) -> Vec<String> {
        let calls = self.lines.iter().filter(|l| l.starts_with("CALL "));
        calls.cloned().collect()
    }
}

/// What the fake bluetoothd was called with by a shell on `bluetooth`, after the shell's
/// console lines and status. With `lost` the bus daemon is killed once discovery was asked
/// for and the shell is then ended by its `kill` request. None without what [`Bluez`] needs.
fn bluetooth(shell: fn(&Path) -> Command, lost: bool) -> Option<Vec<String>> {
    let mut b = Bluez::start()?;
    let log = std::fs::File::create(b.tmp.path().join("log")).unwrap();
    // A copy of its own: `qs kill -p` below would end another test's shell on the same path.
    let conf = b.tmp.path().join("conf");
    std::fs::create_dir(&conf).unwrap();
    std::fs::copy(dir("bluetooth").join("shell.qml"), conf.join("shell.qml")).unwrap();
    let mut child = shell(&conf)
        .env("DBUS_SYSTEM_BUS_ADDRESS", &b.addr)
        .env("DBUS_SESSION_BUS_ADDRESS", &b.addr)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    b.until("StartDiscovery");
    if lost {
        let Bluez {
            tmp,
            daemon,
            fake,
            from_fake,
            mut lines,
            ..
        } = b;
        drop(daemon);
        // Each shell is asked through its own program: it finds the instances of its name.
        let o = Command::new(shell(&conf).get_program())
            .arg("-p")
            .arg(&conf)
            .arg("kill")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(o.status.success(), "qs kill: {o:?}");
        let status = testenv::wait_exit(&mut child);
        // Nothing reaches the fake any more: what it has printed is all there is.
        drop(fake);
        lines.extend(from_fake.map(|l| l.unwrap()));
        let text = std::fs::read_to_string(tmp.path().join("log")).unwrap();
        eprintln!("{text}");
        let mut t = told(&text, status);
        t.extend(lines.into_iter().filter(|l| l.starts_with("CALL ")));
        return Some(t);
    }
    b.cmd(&format!(
        r#"{{"op":"set","path":"{HCI0}","iface":"org.bluez.Adapter1","props":{{"Discoverable":true}}}}"#
    ));
    let status = testenv::wait_exit(&mut child);
    b.finish();
    let text = std::fs::read_to_string(b.tmp.path().join("log")).unwrap();
    eprintln!("{text}");
    let mut t = told(&text, status);
    t.extend(b.calls());
    Some(t)
}

/// `discovering = false` in a handler at exit (the original's `BluetoothControl.qml`) reaches
/// bluetoothd before the process ends: Quickshell sends `StopDiscovery` and so does the
/// native shell.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn the_discovery_a_handler_stops_at_exit_is_stopped_on_the_bus() {
    if !available() {
        return;
    }
    let want = [
        "qml: start discovery".to_string(),
        "qml: quit".to_string(),
        "qml: destroyed root".to_string(),
        "status 0".to_string(),
        format!("CALL {HCI0} StartDiscovery"),
        format!("CALL {HCI0} StopDiscovery"),
    ];
    let Some(qs) = bluetooth(quickshell, false) else {
        return;
    };
    assert_eq!(qs, want, "Quickshell");
    assert_eq!(bluetooth(native, false).unwrap(), want, "the native shell");
}

/// The system bus is gone when the shell ends: the handler runs, `StopDiscovery` reaches
/// nobody, and the shell ends with status 0 all the same, Quickshell and the native shell.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn a_handler_runs_at_exit_with_the_system_bus_gone() {
    if !available() {
        return;
    }
    let want = [
        "qml: start discovery".to_string(),
        "qml: destroyed root".to_string(),
        "status 0".to_string(),
        format!("CALL {HCI0} StartDiscovery"),
    ];
    let Some(qs) = bluetooth(quickshell, true) else {
        return;
    };
    assert_eq!(qs, want, "Quickshell");
    assert_eq!(bluetooth(native, true).unwrap(), want, "the native shell");
}

/// The exit waits once for the services that do not answer, not once for each: a handler
/// leaves a property write to bluetoothd queued behind one bluetoothd holds, and a
/// notification for a session bus that never answers. The shell ends
/// `qml::notifier::FLUSH` (2 s) after its handlers, without either, and not after twice
/// that. The native shell only: `Notify` is of its own `Impulse` module, and Quickshell
/// waits for neither.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn the_exit_waits_once_for_two_services_that_do_not_answer() {
    if !available() {
        return;
    }
    let Some(mut b) = Bluez::start() else {
        return;
    };
    // A session bus that accepts connections and never answers.
    let session = b.tmp.path().join("session");
    let silent = std::os::unix::net::UnixListener::bind(&session).unwrap();
    std::thread::spawn(move || {
        let mut held = vec![];
        for s in silent.incoming().flatten() {
            held.push(s);
        }
    });
    let (rd, wr) = std::io::pipe().unwrap();
    let mut c = native(&dir("silent"));
    let mut child = c
        .env("DBUS_SYSTEM_BUS_ADDRESS", &b.addr)
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", session.display()),
        )
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    drop(c);
    let log = Log::capture(rd);
    log.wait(|l| l.contains("ready"))
        .unwrap_or_else(|l| panic!("the shell never loaded silent:\n{l}"));
    b.cmd(r#"{"op":"flag","name":"hold_set","value":true}"#);
    b.until("DONE flag");
    b.cmd(&format!(
        r#"{{"op":"set","path":"{HCI0}","iface":"org.bluez.Adapter1","props":{{"Pairable":false}}}}"#
    ));
    log.wait(|l| l.contains("destroyed root"))
        .unwrap_or_else(|l| panic!("the handler never ran:\n{l}"));
    let handlers_done = std::time::Instant::now();
    let status = testenv::wait_exit(&mut child);
    let took = handlers_done.elapsed();
    let text = log.until_closed();
    eprintln!("{text}\nfrom the handler to the end of the process: {took:?}");
    assert_eq!(
        told(&text, status),
        ["qml: ready", "qml: quit", "qml: destroyed root", "status 0"]
    );
    b.finish();
    let sets = b.lines.iter().filter(|l| l.contains("SET "));
    assert_eq!(
        sets.collect::<Vec<_>>(),
        [
            &format!("SET {HCI0} Discoverable true"),
            "HELD SET Discoverable"
        ],
        "the first write is held by bluetoothd, the second never left the shell"
    );
    let limit = std::time::Duration::from_secs(2);
    assert!(
        took >= limit - std::time::Duration::from_millis(100),
        "the exit did not wait for what is queued: {took:?}"
    );
    assert!(
        took < limit + limit / 2,
        "the exit waited longer than one limit for both services: {took:?}"
    );
}

/// `ClipboardHistory.removeImage` in a handler at exit (the original's `CliphistImage.qml`) removes
/// the decoded image. The native shell only: `Impulse` is its own module.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn the_image_a_handler_removes_at_exit_is_removed() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let image = tmp.path().join("image.png");
    std::fs::write(&image, "png").unwrap();
    let mut c = native(&dir("clipimage"));
    c.env("DESTR_DIR", tmp.path());
    assert_eq!(run(c), ["qml: quit", "qml: destroyed root", "status 0"]);
    assert!(!image.exists(), "the image is still there");
}

/// A shell on a copy of `idle` in `at` that has printed `ready`, and its output as it arrives.
/// The copy is the caller's own: `qs kill -p` ends an instance of the path it is given, and
/// two tests on one path would end each other's.
fn idle(shell: fn(&Path) -> Command, at: &Path) -> (Child, Arc<Log>) {
    std::fs::copy(dir("idle").join("shell.qml"), at.join("shell.qml")).unwrap();
    let (rd, wr) = std::io::pipe().unwrap();
    let mut c = shell(at);
    let child = c
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    drop(c);
    let log = Log::capture(rd);
    log.wait(|l| l.contains("ready"))
        .unwrap_or_else(|l| panic!("the shell never loaded idle:\n{l}"));
    (child, log)
}

/// What a shell on its own copy of `idle` prints when `end`, given the shell and the directory
/// of the copy, ends it.
fn ended_by(shell: fn(&Path) -> Command, end: impl FnOnce(&Child, &Path)) -> Vec<String> {
    let tmp = tempfile::tempdir().unwrap();
    let (mut child, log) = idle(shell, tmp.path());
    end(&child, tmp.path());
    let status = testenv::wait_exit(&mut child);
    let text = log.until_closed();
    eprintln!("{text}");
    told(&text, status)
}

#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn handlers_run_at_qs_kill() {
    if !available() {
        return;
    }
    let kill = |shell: &Child, conf: &Path| {
        // Each shell is asked through its own program: it finds the instances of its name.
        let own = std::fs::read_link(format!("/proc/{}/exe", shell.id())).unwrap();
        let o = Command::new(own)
            .arg("-p")
            .arg(conf)
            .arg("kill")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(o.status.success(), "qs kill: {o:?}");
    };
    let want = [
        "qml: ready",
        "qml: destroyed child",
        "qml: destroyed root",
        "status 0",
    ];
    assert_eq!(ended_by(quickshell, kill), want, "Quickshell");
    assert_eq!(ended_by(native, kill), want, "the native shell");
}

/// SIGTERM, SIGINT and SIGHUP end Quickshell at once (it handles none of them): no handler
/// runs. The native shell saves what it holds and ends its children first, but runs no
/// handler either.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn handlers_do_not_run_at_a_termination_signal() {
    if !available() {
        return;
    }
    for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let signal = |c: &Child, _: &Path| {
            // SAFETY: kill(2) of the child this test started and has not reaped.
            let r = unsafe { libc::kill(c.id() as libc::pid_t, sig) };
            assert_eq!(r, 0, "kill: {}", std::io::Error::last_os_error());
        };
        let want = ["qml: ready".to_string(), format!("signal {sig}")];
        assert_eq!(ended_by(quickshell, signal), want, "Quickshell");
        assert_eq!(ended_by(native, signal), want, "the native shell");
    }
}

/// What is left of `shell` on `lost` once its compositor, a sway of its own, went away by
/// `leave`: its console lines and status, whether the image its handler would remove is still
/// there, and whether its two `Process`es still run (they are ended here). With `ends` the
/// shell is one that ends them: it signals them and does not wait, so their end is waited
/// for here, and one left running stops the test until nextest ends it.
fn lost(shell: fn(&Path) -> Command, leave: fn(&testenv::Sway), ends: bool) -> Vec<String> {
    let sway = testenv::Sway::start();
    let tmp = tempfile::tempdir().unwrap();
    let at = |f: &str| tmp.path().join(f);
    std::fs::write(at("image.png"), "png").unwrap();
    let (rd, wr) = std::io::pipe().unwrap();
    let mut c = shell(&dir("lost"));
    let mut child = sway
        .env(&mut c)
        .env("DESTR_DIR", tmp.path())
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    drop(c);
    let log = Log::capture(rd);
    log.wait(|l| l.contains("ready single"))
        .unwrap_or_else(|l| panic!("the shell never loaded lost:\n{l}"));
    leave(&sway);
    let status = testenv::wait_exit(&mut child);
    drop(sway);
    // Not to its end: a `Process` left behind keeps the pipe open.
    let text = log.text();
    eprintln!("{text}");
    let mut t = told(&text, status);
    let image = if at("image.png").exists() {
        "left"
    } else {
        "removed"
    };
    t.push(format!("image: {image}"));
    for (what, file) in [("scope", "scope.pid"), ("singleton", "single.pid")] {
        let pid = std::fs::read_to_string(at(file))
            .unwrap()
            .trim()
            .to_string();
        if ends {
            testenv::wait_gone(pid.parse().unwrap());
        }
        t.push(format!("Process of the {what} runs: {}", !ended(&pid)));
        testenv::kill_pid_and_wait(pid.parse().unwrap());
    }
    t
}

/// The compositor goes away under the shell: killed, or left in order (`swaymsg exit`, as
/// `hyprctl dispatch exit` ends a Hyprland session). Quickshell ends where it stands with
/// Qt's warning and status 255: no handler runs, so the image a handler would remove stays,
/// and its `Process`es are left running without a parent. The native shell ends the same
/// way but for the `Process`es, which it ends first (docs/quickshell-differences.md).
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn no_handler_runs_when_the_compositor_goes_away() {
    if !testenv::require_all(&["qs", "sway", "swaymsg"]) {
        return;
    }
    let killed: fn(&testenv::Sway) = |sway| {
        // SAFETY: kill(2) of the sway this test started and has not reaped.
        let r = unsafe { libc::kill(sway.pid() as libc::pid_t, libc::SIGKILL) };
        assert_eq!(r, 0, "kill: {}", std::io::Error::last_os_error());
    };
    let exit: fn(&testenv::Sway) = |sway| {
        let o = Command::new("swaymsg")
            .arg("-s")
            .arg(sway.ipc_socket())
            .arg("exit")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        // sway may be gone before it answered.
        let unanswered = String::from_utf8_lossy(&o.stderr).contains("Unable to receive IPC");
        assert!(o.status.success() || unanswered, "swaymsg exit: {o:?}");
    };
    for (leave, how) in [(killed, "sway killed"), (exit, "swaymsg exit")] {
        let left = |runs: bool| {
            [
                "qml: ready single".to_string(),
                "status 255".to_string(),
                "image: left".to_string(),
                format!("Process of the scope runs: {runs}"),
                format!("Process of the singleton runs: {runs}"),
            ]
        };
        assert_eq!(
            lost(quickshell, leave, false),
            left(true),
            "Quickshell, {how}"
        );
        assert_eq!(
            lost(native, leave, true),
            left(false),
            "the native shell, {how}"
        );
    }
}

/// The console lines and status of `shell` on `name`, then the size of the file its handlers
/// wrote with a `FileView`.
fn written(shell: fn(&Path) -> Command, name: &str) -> Vec<String> {
    written_as(shell, name, told)
}

/// [`written`] with the lines of the output read by `lines`.
fn written_as(
    shell: fn(&Path) -> Command,
    name: &str,
    lines: fn(&str, ExitStatus) -> Vec<String>,
) -> Vec<String> {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = shell(&dir(name));
    c.env("DESTR_DIR", tmp.path());
    let out = c.stdin(Stdio::null()).output().unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text += &String::from_utf8_lossy(&out.stderr);
    eprintln!("{c:?}:\n{text}");
    let mut t = lines(&text, out.status);
    let size = std::fs::metadata(tmp.path().join("file")).map(|m| m.len());
    t.push(format!("file: {} bytes", size.unwrap_or(0)));
    t
}

/// [`told`], and the error a handler ended with as `warning: Interrupted` where it was
/// printed. It is a warning of the engine and not a console line: the native shell prints
/// `qml: <where>: Interrupted` (which [`console`] alone takes for a line of qml6), Quickshell
/// ` WARN scene: <where>: Error: Interrupted`.
fn told_with_the_error(out: &str, status: ExitStatus) -> Vec<String> {
    let mut t = Vec::new();
    for l in out.lines() {
        if l.ends_with(": Interrupted") {
            t.push("warning: Interrupted".to_string());
        } else {
            t.extend(console(l));
        }
    }
    t.extend(told("", status));
    t
}

/// A handler that works for long without hanging is left to end, as in Quickshell: a second
/// of CPU time and 8 MiB written with a `FileView` are within the watchdog's 5 s. The whole
/// text is in the file, the handlers after it run and the status is the one asked for.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn a_handler_that_works_for_a_second_is_not_interrupted() {
    if !available() {
        return;
    }
    let mut want = recorded("long", "qs");
    want.push("file: 8388608 bytes".to_string());
    assert_eq!(written(quickshell, "long"), want, "Quickshell");
    assert_eq!(written(native, "long"), want, "the native shell");
}

/// A handler that never returns is interrupted by the watchdog (5 s of the thread's CPU
/// time, `qml::watchdog::DEFAULT_TIMEOUT`). The rest is as if it had returned: the handlers
/// after it run, what they write with a `FileView` is in the file, and the shell ends with
/// the status asked for (the lines of `long` but for the first handler's).
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn a_handler_that_never_returns_is_interrupted() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let mut want = recorded("long", "qs");
    want[1] = "qml: destroyed child, never returns".to_string();
    want.insert(2, "warning: Interrupted".to_string());
    want.push("file: 7 bytes".to_string());
    assert_eq!(written_as(native, "watchdog", told_with_the_error), want);
}

/// The error a destruction handler ends with is printed inside that handler, before the
/// handlers after it run, in Quickshell and in the native shell: the watchdog's `Interrupted`
/// of the test above stands where the error of a handler that throws it itself stands.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn the_error_of_a_handler_is_printed_before_the_next_handler_runs() {
    if !available() {
        return;
    }
    let mut want = recorded("long", "qs");
    want[1] = "qml: destroyed child, throws".to_string();
    want.insert(2, "warning: Interrupted".to_string());
    want.push("file: 7 bytes".to_string());
    assert_eq!(
        written_as(quickshell, "thrown", told_with_the_error),
        want,
        "Quickshell"
    );
    assert_eq!(
        written_as(native, "thrown", told_with_the_error),
        want,
        "the native shell"
    );
}
