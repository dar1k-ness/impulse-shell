//! "Clear all" of the original's notification list against the real Quickshell 0.3.2:
//! `notification_clear_all/shell.qml` is `Notifications.discardAllNotifications` of
//! `qml/services/Notifications.qml` in small. Quickshell's `dismiss()` takes the notification
//! out of `trackedNotifications` inside the call, so the `forEach` over that list skips every
//! second one; here the list changes after the script, and every notification is closed
//! (docs/quickshell-differences.md).
//!
//! Each test runs one shell on a session bus of its own and is the program that sends the
//! notifications: `first`, `second` and `third` (ids 1 to 3), which the shell clears after the
//! third, then `mark` (id 4). It asks the server to close 2 and then 4: the `NotificationClosed`
//! of 4 ends what the server sent, and the shell quits on it.
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use zbus::blocking::{Connection, MessageIterator};

const NAME: &str = "org.freedesktop.Notifications";
const OBJECT: &str = "/org/freedesktop/Notifications";
/// Desktop Notifications 1.3, the reasons of `NotificationClosed`.
const DISMISSED: u32 = 2;
const CLOSE_CALLED: u32 = 3;

fn conf() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/notification_clear_all")
}

/// A session bus with no service files, so that no notification daemon of the system is
/// started on it. Closing the wrapper's stdin (the test's end) kills the daemon.
struct Bus {
    address: String,
    _daemon: Child,
    _dir: tempfile::TempDir,
}

fn bus() -> Bus {
    let dir = tempfile::tempdir().unwrap();
    let address = format!("unix:path={}", dir.path().join("bus").display());
    let cfg = dir.path().join("bus.conf");
    std::fs::write(
        &cfg,
        "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>\
         <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
    )
    .unwrap();
    let mut daemon = Command::new("sh")
        .args(["-c", "dbus-daemon --config-file=\"$0\" --address=\"$1\" --nofork --nopidfile --print-address=1 & read _; kill $!"])
        .arg(&cfg)
        .arg(&address)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("dbus-daemon");
    let mut line = String::new();
    let _ = BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut line);
    assert!(!line.trim().is_empty(), "dbus-daemon did not start");
    Bus {
        address,
        _daemon: daemon,
        _dir: dir,
    }
}

/// The text of a console line: the native shell prints `console: <text>`, Quickshell
/// `DEBUG qml: <text>`.
fn console(line: &str) -> Option<&str> {
    line.strip_prefix("console: ")
        .or_else(|| line.find(" qml: ").map(|i| &line[i + 6..]))
}

fn notify(c: &Connection, summary: &str) -> u32 {
    let (actions, hints) = (
        Vec::<&str>::new(),
        HashMap::<&str, zbus::zvariant::Value>::new(),
    );
    c.call_method(
        Some(NAME),
        OBJECT,
        Some(NAME),
        "Notify",
        &("clear-all", 0u32, "", summary, "", actions, hints, 0i32),
    )
    .unwrap()
    .body()
    .deserialize()
    .unwrap()
}

/// `CloseNotification(id)`; the answer is an error when the server has no such notification.
fn close(c: &Connection, id: u32) {
    let _ = c.call_method(Some(NAME), OBJECT, Some(NAME), "CloseNotification", &(id,));
}

/// What one run of `notification_clear_all` gave.
#[derive(Debug, PartialEq)]
struct Cleared {
    /// (id, reason) of each `NotificationClosed` before the mark's.
    closed: Vec<(u32, u32)>,
    /// The console lines after `cleared`: a wrapper whose summary went empty.
    said: Vec<String>,
}

fn clear_all(shell: Command) -> Cleared {
    let bus = bus();
    let c = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&c).unwrap();
    let mut claims = dbus
        .receive_name_owner_changed_with_args(&[(0, NAME)])
        .unwrap();
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(NAME)
        .unwrap()
        .member("NotificationClosed")
        .unwrap()
        .build();
    let signals = MessageIterator::for_match_rule(rule, &c, None).unwrap();

    // Both shells' lines through one pipe, in the order they were written.
    let mut child = Command::new("sh")
        .args(["-c", "exec \"$@\" 2>&1", "sh"])
        .arg(shell.get_program())
        .args(shell.get_args())
        .envs(shell.get_envs().filter_map(|(k, v)| Some((k, v?))))
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    assert!(
        claims.any(|s| s.args().is_ok_and(|a| a.new_owner().is_some())),
        "the bus closed before the shell took {NAME}"
    );

    let ids: Vec<u32> = ["first", "second", "third"]
        .iter()
        .map(|s| notify(&c, s))
        .collect();
    assert_eq!(ids, [1, 2, 3]);
    let mut log = String::new();
    let cleared = lines.by_ref().map_while(Result::ok).any(|l| {
        log += &format!("{l}\n");
        console(&l) == Some("cleared")
    });
    assert!(cleared, "the shell ended before it cleared:\n{log}");

    let mark = notify(&c, "mark");
    close(&c, 2);
    close(&c, mark);
    let said: Vec<String> = lines
        .map_while(Result::ok)
        .filter_map(|l| {
            log += &format!("{l}\n");
            console(&l).map(str::to_string)
        })
        .collect();
    let status = testenv::wait_exit(&mut child);
    assert!(status.success(), "{status}:\n{log}");

    let closed = signals
        .map(|m| m.unwrap().body().deserialize::<(u32, u32)>().unwrap())
        .take_while(|s| *s != (mark, CLOSE_CALLED))
        .collect();
    Cleared { closed, said }
}

/// Quickshell and a compositor for it and for the native shell.
fn available() -> bool {
    if !testenv::require_all(&["qs", "dbus-daemon"]) {
        return false;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return false;
    }
    true
}

/// Quickshell dismisses the first and the third. The second stays on the server until the
/// sender closes it, and the wrappers of the two dismissed lose their text before they go.
#[test]
fn quickshell_clears_every_second_notification() {
    if !available() {
        return;
    }
    let mut qs = Command::new("qs");
    qs.arg("--no-color")
        .arg("-p")
        .arg(conf())
        .env("QS_NO_RELOAD_POPUP", "1");
    assert_eq!(
        clear_all(qs),
        Cleared {
            closed: vec![(1, DISMISSED), (3, DISMISSED), (2, CLOSE_CALLED)],
            said: vec![
                "the summary of 1 is empty".into(),
                "the summary of 3 is empty".into()
            ],
        }
    );
}

/// The native shell dismisses all three: three `NotificationClosed` of three, nothing left for
/// the sender to close, and no wrapper loses its text.
#[test]
fn clear_all_closes_every_notification() {
    if !available() {
        return;
    }
    let mut native = Command::new(env!("CARGO_BIN_EXE_impulse-shell"));
    native.arg("-p").arg(conf()).env("RI_INTERP", "1");
    assert_eq!(
        clear_all(native),
        Cleared {
            closed: vec![(1, DISMISSED), (2, DISMISSED), (3, DISMISSED)],
            said: vec![],
        }
    );
}
