//! The notification server outlives its session-bus connection: when the bus drops it, the
//! server serves again on the next connection, takes the name back and keeps its live
//! notifications; with no bus at its start, it serves once the bus socket appears. Each test runs
//! in a process of its own whose session bus is a private `dbus-daemon`; the shell reaches it
//! through a relay socket, so the test can end just the shell's connection while the bus (and
//! the test's own clients, connected to it directly) stay up.

use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use testenv::Wakeups;
use zbus::blocking::Connection;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

const QML: &str = r#"import QtQuick
import Rustshell.Services.Notifications
Item {
    id: root
    property string log: ""
    property int count: server.trackedNotifications.values.length
    NotificationServer {
        id: server
        onNotification: { notification.tracked = true; root.log += notification.summary + ";" }
    }
}
"#;

/// A private `dbus-daemon` at `path`; closing the returned stdin makes its wrapper kill it.
/// No <servicedir>: a system notification daemon must not be bus-activated.
fn daemon(path: &str) -> Option<ChildStdin> {
    let cfg = std::env::temp_dir().join(format!("qml-notif-reconnect-{}.conf", std::process::id()));
    std::fs::write(
        &cfg,
        "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>\
         <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
    )
    .expect("the bus config in the temporary directory");
    let Ok(mut c) = Command::new("sh")
        .args(["-c", "dbus-daemon --config-file=\"$0\" --address=\"unix:path=$1\" --nofork --nopidfile --print-address=1 & read _; kill $!"])
        .arg(&cfg)
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    else {
        testenv::unavailable("no dbus-daemon");
        return None;
    };
    let mut line = String::new();
    let _ = BufReader::new(c.stdout.take().unwrap()).read_line(&mut line);
    if line.trim().is_empty() {
        testenv::unavailable("dbus-daemon not available");
        return None;
    }
    c.stdin.take()
}

/// Relays every connection to `listen` to the bus at `bus`; the returned list holds both ends
/// of each relayed connection, for [`cut`]. Listening at a scratch name, then renamed onto
/// `listen`: it accepts connections before it appears there, as a bus socket announces itself.
fn relay(listen: &str, bus: &str) -> Arc<Mutex<Vec<UnixStream>>> {
    let ends = Arc::new(Mutex::new(Vec::new()));
    let scratch = format!("{listen}.new");
    let l = UnixListener::bind(&scratch).unwrap();
    std::fs::rename(&scratch, listen).unwrap();
    let (bus, e) = (bus.to_string(), ends.clone());
    std::thread::spawn(move || {
        for client in l.incoming() {
            let Ok(client) = client else { return };
            let up = UnixStream::connect(&bus).unwrap();
            e.lock()
                .unwrap()
                .extend([client.try_clone().unwrap(), up.try_clone().unwrap()]);
            for (mut from, mut to) in [
                (client.try_clone().unwrap(), up.try_clone().unwrap()),
                (up, client),
            ] {
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut from, &mut to);
                    let _ = to.shutdown(std::net::Shutdown::Write);
                });
            }
        }
    });
    ends
}

/// Ends every connection relayed so far, as a bus dropping its client does.
fn cut(ends: &Mutex<Vec<UnixStream>>) {
    for s in ends.lock().unwrap().drain(..) {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
}

/// A client of the bus at `bus` itself, not through the relay.
fn direct(bus: &str) -> Connection {
    zbus::blocking::connection::Builder::address(format!("unix:path={bus}").as_str())
        .unwrap()
        .build()
        .unwrap()
}

/// Each new owner of the notifications name ("" once nobody owns it), from a match added
/// before this returns.
fn owners(c: Connection, ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<String> {
    let it = zbus::blocking::fdo::DBusProxy::new(&c)
        .unwrap()
        .receive_name_owner_changed_with_args(&[(0, NAME)])
        .unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _c = c;
        for s in it {
            let Ok(a) = s.args() else { continue };
            let new = a
                .new_owner()
                .as_ref()
                .map_or(String::new(), |n| n.to_string());
            if tx.send(new).is_err() {
                return;
            }
            ping();
        }
    });
    rx
}

/// `NotificationClosed (id, reason)` signals, from a match rule added before this returns.
fn closed(c: Connection, ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<(u32, u32)> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(NAME)
        .unwrap()
        .member("NotificationClosed")
        .unwrap()
        .build();
    let it = zbus::blocking::MessageIterator::for_match_rule(rule, &c, None).unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _c = c;
        for m in it.flatten() {
            let Ok(b) = m.body().deserialize::<(u32, u32)>() else {
                continue;
            };
            if tx.send(b).is_err() {
                return;
            }
            ping();
        }
    });
    rx
}

/// `Notify` from `c` on a thread: the reply comes only once the shell has handled the
/// notification, so the caller keeps running the loop meanwhile.
fn notify(c: Connection, summary: &str, ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<u32> {
    let (tx, rx) = channel();
    let summary = summary.to_string();
    std::thread::spawn(move || {
        let id = c
            .call_method(
                Some(NAME),
                PATH,
                Some(NAME),
                "Notify",
                &(
                    "app",
                    0u32,
                    "",
                    summary.as_str(),
                    "body",
                    Vec::<String>::new(),
                    std::collections::HashMap::<String, zbus::zvariant::Value>::new(),
                    -1i32,
                ),
            )
            .unwrap()
            .body()
            .deserialize::<u32>()
            .unwrap();
        let _ = tx.send(id);
        ping();
    });
    rx
}

/// Runs `rt`'s loop turns, sleeping on `wake` between them, until `done`.
fn until(rt: &mut Runtime, wake: &Wakeups, what: &str, mut done: impl FnMut(&mut Runtime) -> bool) {
    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for {what}");
    loop {
        rt.poll_native();
        rt.settle();
        if done(rt) {
            return;
        }
        wake.wait(None, None);
    }
}

/// The next value of `rx` that `pred` accepts, running the loop meanwhile.
fn recv_until<T>(
    rt: &mut Runtime,
    wake: &Wakeups,
    rx: &Receiver<T>,
    what: &str,
    mut pred: impl FnMut(&T) -> bool,
) -> T {
    let mut got = None;
    until(rt, wake, what, |_| {
        got = rx.try_iter().find(&mut pred);
        got.is_some()
    });
    got.unwrap()
}

fn prop(rt: &Runtime, root: usize, name: &str) -> String {
    rt.world
        .borrow_mut()
        .read(root, name)
        .map_or(String::new(), |v| v.to_js_string())
}

/// `test` in a process of its own, its shell's session bus a relay (`DBUS_SESSION_BUS_ADDRESS`)
/// to the bus socket named by `NOTIF_RECONNECT_BUS`. False in the process that ran it there.
fn own_process(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = |n: &str| dir.path().join(n).display().to_string();
    testenv::run_in_own_process(
        test,
        [
            (
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", path("relay")),
            ),
            ("NOTIF_RECONNECT_BUS", path("bus")),
        ],
    );
    false
}

#[test]
fn notifications_serve_again_after_reconnect() {
    if !own_process(concat!(
        module_path!(),
        "::notifications_serve_again_after_reconnect"
    )) {
        return;
    }
    let bus = std::env::var("NOTIF_RECONNECT_BUS").unwrap();
    let Some(_daemon) = daemon(&bus) else {
        return;
    };
    let relayed = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let ends = relay(relayed.strip_prefix("unix:path=").unwrap(), &bus);
    let wake = Wakeups::new();
    let ping: Arc<dyn Fn() + Send + Sync> = Arc::new(wake.ping());
    let owners = owners(direct(&bus), ping.clone());
    let closed = closed(direct(&bus), ping.clone());
    let client = direct(&bus);

    let mut rt = Runtime::new(None);
    rt.world.borrow().waker.set_ping(wake.ping());
    let root = rt.load_str(QML).unwrap();
    let first = recv_until(
        &mut rt,
        &wake,
        &owners,
        "the server to take the name",
        |o| !o.is_empty(),
    );
    let shown = notify(client.clone(), "before", ping.clone());
    let before = recv_until(&mut rt, &wake, &shown, "the first notification", |_| true);
    until(&mut rt, &wake, "the first notification tracked", |rt| {
        prop(rt, root, "count") == "1"
    });

    cut(&ends);
    recv_until(&mut rt, &wake, &owners, "the name released", |o| {
        o.is_empty()
    });
    let again = recv_until(&mut rt, &wake, &owners, "the name taken again", |o| {
        !o.is_empty()
    });
    assert_ne!(again, first, "served again on a new connection");
    assert_eq!(prop(&rt, root, "count"), "1", "the live notification stays");

    let shown = notify(client.clone(), "after", ping.clone());
    let after = recv_until(
        &mut rt,
        &wake,
        &shown,
        "a notification after the reconnect",
        |_| true,
    );
    assert_ne!(after, before, "ids stay unique across the reconnect");
    until(&mut rt, &wake, "both notifications tracked", |rt| {
        prop(rt, root, "log") == "before;after;" && prop(rt, root, "count") == "2"
    });
    // The notification from before the reconnect is still the server's: closing it through the
    // new connection closes it, with the signal sent there. The reply comes once the loop has
    // closed it, so the call is made on a thread of its own while the loop runs.
    let reply = {
        let (tx, rx) = channel();
        let (c, ping) = (client.clone(), ping.clone());
        std::thread::spawn(move || {
            let r = c
                .call_method(
                    Some(NAME),
                    PATH,
                    Some(NAME),
                    "CloseNotification",
                    &(before,),
                )
                .map(|_| ());
            let _ = tx.send(r);
            ping();
        });
        rx
    };
    let got = recv_until(&mut rt, &wake, &closed, "NotificationClosed", |c| {
        c.0 == before
    });
    assert_eq!(got, (before, 3), "closed by request");
    let r = recv_until(
        &mut rt,
        &wake,
        &reply,
        "the CloseNotification reply",
        |_| true,
    );
    assert!(r.is_ok(), "{r:?}");
    until(&mut rt, &wake, "one notification left", |rt| {
        prop(rt, root, "count") == "1"
    });
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn notifications_serve_once_the_bus_appears() {
    if !own_process(concat!(
        module_path!(),
        "::notifications_serve_once_the_bus_appears"
    )) {
        return;
    }
    let bus = std::env::var("NOTIF_RECONNECT_BUS").unwrap();
    let wake = Wakeups::new();
    let ping: Arc<dyn Fn() + Send + Sync> = Arc::new(wake.ping());

    // No bus at the address yet: the server's first connect fails.
    let mut rt = Runtime::new(None);
    rt.world.borrow().waker.set_ping(wake.ping());
    let root = rt.load_str(QML).unwrap();
    // The loop turn that starts the server, and its connect on the bus lane, both before the bus.
    rt.poll_native();
    rt.settle();
    workpool::wait_idle();
    assert_eq!(qml::dbus::opened(), 0, "connected with no bus");

    // The bus comes up at the address the shell was given (here through the relay).
    let relayed = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let Some(_daemon) = daemon(&bus) else {
        return;
    };
    let owners = owners(direct(&bus), ping.clone());
    let _ends = relay(relayed.strip_prefix("unix:path=").unwrap(), &bus);
    recv_until(
        &mut rt,
        &wake,
        &owners,
        "the server to take the name",
        |o| !o.is_empty(),
    );
    let shown = notify(direct(&bus), "late", ping.clone());
    recv_until(&mut rt, &wake, &shown, "a notification", |_| true);
    until(&mut rt, &wake, "the notification tracked", |rt| {
        prop(rt, root, "log") == "late;"
    });
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
