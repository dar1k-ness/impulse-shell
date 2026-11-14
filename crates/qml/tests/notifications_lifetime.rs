//! The notification server's name watch lives as long as its World: each start (every World,
//! every reload) ends it when the World goes, instead of leaving a thread and a
//! `NameOwnerChanged` match behind for the rest of the process. The test runs in a process of
//! its own whose session bus is a private `dbus-daemon`.
use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use testenv::Wakeups;
use zbus::blocking::Connection;

const NAME: &str = "org.freedesktop.Notifications";

const QML: &str = r#"import QtQuick
import Rustshell.Services.Notifications
Item { NotificationServer {} }
"#;

/// In `test`'s own process (see `testenv::run_in_own_process`): a private session bus at the
/// address the environment names, a socket in a directory the parent made. `None` in the
/// process that ran the test there, or without dbus-daemon. Closing the returned stdin (test
/// exit) makes the wrapper shell kill the daemon. No <servicedir>: a system notification daemon
/// must not be bus-activated.
fn bus(test: &str) -> Option<ChildStdin> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(test, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        return None;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let cfg = std::env::temp_dir().join(format!("qml-notif-life-{}.conf", std::process::id()));
    std::fs::write(
        &cfg,
        "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>\
         <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
    )
    .ok()?;
    let Ok(mut c) = Command::new("sh")
        .args(["-c", "dbus-daemon --config-file=\"$0\" --address=\"$1\" --nofork --nopidfile --print-address=1 & read _; kill $!"])
        .arg(&cfg)
        .arg(&addr)
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

/// Each new owner of the notifications name ("" once nobody owns it), from a match rule added
/// before this returns.
fn owners(ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<String> {
    let c = Connection::session().unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&c).unwrap();
    let it = dbus
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

/// The `NameOwnerChanged` matches for the notifications name that this process's shared
/// session connection has on the bus, once the bus has handled everything it sent so far
/// (the bus handles one connection's messages in order, a reply to a later call comes after).
fn name_watches(probe: &Connection) -> usize {
    let shared = qml::dbus::session().unwrap();
    shared
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Peer"),
            "Ping",
            &(),
        )
        .unwrap();
    let me = shared.unique_name().unwrap().to_string();
    let all: std::collections::HashMap<String, Vec<String>> = probe
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Debug.Stats"),
            "GetAllMatchRules",
            &(),
        )
        .unwrap()
        .body()
        .deserialize()
        .unwrap();
    all.get(&me).map_or(0, |rules| {
        rules
            .iter()
            .filter(|r| {
                r.contains("member='NameOwnerChanged'") && r.contains(&format!("arg0='{NAME}'"))
            })
            .count()
    })
}

/// A World waiting for another notification server to leave, so its watch is what would take
/// the name over.
fn watching_world(wake: &Wakeups) -> Runtime {
    let mut rt = Runtime::new(None);
    rt.world.borrow().waker.set_ping(wake.ping());
    rt.load_str(QML).unwrap();
    until(&mut rt, wake, "the server to watch its name", |rt| {
        rt.world
            .borrow()
            .notifications
            .as_ref()
            .is_some_and(|n| n.watching())
    });
    rt
}

#[test]
fn name_watch_ends_with_its_world() {
    let Some(_bus) = bus(concat!(module_path!(), "::name_watch_ends_with_its_world")) else {
        return;
    };
    let probe = Connection::session().unwrap();
    let other = Connection::session().unwrap();
    other.request_name(NAME).unwrap();
    let wake = Wakeups::new();

    // Reloads: each World watches once while it lives and leaves nothing behind.
    for round in 0..3 {
        let rt = watching_world(&wake);
        assert_eq!(
            name_watches(&probe),
            1,
            "round {round}: one watch per live World"
        );
        drop(rt);
        assert_eq!(
            name_watches(&probe),
            0,
            "round {round}: the watch outlived its World"
        );
    }

    // Overlapping Worlds (the new one starts before the old one goes) share the one bus rule;
    // the survivor keeps it and still takes the name over.
    let old = watching_world(&wake);
    let mut new = watching_world(&wake);
    assert_eq!(name_watches(&probe), 1);
    drop(old);
    assert_eq!(
        name_watches(&probe),
        1,
        "the old World took the new one's watch"
    );

    let ping: Arc<dyn Fn() + Send + Sync> = Arc::new(wake.ping());
    let owners = owners(ping);
    other.release_name(NAME).unwrap();
    let me = qml::dbus::session()
        .unwrap()
        .unique_name()
        .unwrap()
        .to_string();
    until(&mut new, &wake, "the live World to take the name", |_| {
        owners.try_iter().any(|o| o == me)
    });
    drop(new);
    assert_eq!(name_watches(&probe), 0);
}

const LOADED: &str = r#"import QtQuick
import Rustshell.Services.Notifications
Item {
    id: root
    property bool on: true
    property string log: ""
    Loader {
        active: root.on
        sourceComponent: Component {
            NotificationServer {
                onNotification: { notification.tracked = true; root.log += notification.summary + ";" }
            }
        }
    }
}
"#;

/// `NotificationClosed (id, reason)` signals, from a match rule added before this returns.
fn closed(ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<(u32, u32)> {
    let c = Connection::session().unwrap();
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

/// `Notify` from a client of its own, on a thread: the reply comes only once the shell has
/// handled the notification, so the caller keeps running the loop meanwhile.
fn notify(summary: &str, ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<zbus::Result<u32>> {
    let (tx, rx) = channel();
    let summary = summary.to_string();
    std::thread::spawn(move || {
        let r = Connection::session().and_then(|c| {
            c.call_method(
                Some(NAME),
                "/org/freedesktop/Notifications",
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
            )?
            .body()
            .deserialize::<u32>()
        });
        let _ = tx.send(r);
        ping();
    });
    rx
}

/// The reply to a [`notify`], running `rt`'s loop until it comes.
fn reply(rt: &mut Runtime, wake: &Wakeups, what: &str, rx: &Receiver<zbus::Result<u32>>) -> u32 {
    let mut r = None;
    until(rt, wake, what, |_| {
        r = r.take().or_else(|| rx.try_recv().ok());
        r.is_some()
    });
    r.unwrap().unwrap()
}

fn log(rt: &Runtime, root: usize) -> String {
    rt.world
        .borrow_mut()
        .read(root, "log")
        .map_or(String::new(), |v| v.to_js_string())
}

/// With no `NotificationServer` left (its Loader inactive) the World keeps nothing on the bus:
/// the tracked notifications close, the name is released for another daemon to take, and a new
/// server takes it back once that daemon leaves and shows notifications again.
#[test]
fn name_released_with_the_last_server() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::name_released_with_the_last_server"
    )) else {
        return;
    };
    let probe = Connection::session().unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&probe).unwrap();
    let wake = Wakeups::new();
    let ping: Arc<dyn Fn() + Send + Sync> = Arc::new(wake.ping());
    let owners = owners(ping.clone());
    let closed = closed(ping.clone());
    let me = qml::dbus::session()
        .unwrap()
        .unique_name()
        .unwrap()
        .to_string();

    let mut rt = Runtime::new(None);
    rt.world.borrow().waker.set_ping(wake.ping());
    let root = rt.load_str(LOADED).unwrap();
    until(&mut rt, &wake, "the server to take the name", |_| {
        owners.try_iter().any(|o| o == me)
    });
    let shown = notify("first", ping.clone());
    let id = reply(&mut rt, &wake, "the first notification", &shown);
    assert_eq!(log(&rt, root), "first;");

    rt.world
        .borrow_mut()
        .write(root, "on", qml::value::Value::Bool(false));
    until(&mut rt, &wake, "the server item to go", |rt| {
        rt.world
            .borrow_mut()
            .of_type("NotificationServer")
            .is_empty()
    });
    rt.poll_native();
    // The release runs on a pool lane: once the pool is idle it has been answered.
    workpool::wait_idle();
    assert!(
        dbus.get_name_owner(NAME.try_into().unwrap()).is_err(),
        "the name is still owned with no NotificationServer left"
    );
    assert_eq!(
        closed.recv().unwrap(),
        (id, 4),
        "the shown notification closes"
    );

    let other = Connection::session().unwrap();
    assert_eq!(
        other
            .request_name_with_flags(NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into())
            .unwrap(),
        zbus::fdo::RequestNameReply::PrimaryOwner,
        "another daemon cannot take the released name"
    );

    // A server made again waits for that daemon to leave, then takes the name back.
    rt.world
        .borrow_mut()
        .write(root, "on", qml::value::Value::Bool(true));
    until(&mut rt, &wake, "the new server to watch its name", |rt| {
        rt.world
            .borrow()
            .notifications
            .as_ref()
            .is_some_and(|n| n.watching())
    });
    other.release_name(NAME).unwrap();
    until(
        &mut rt,
        &wake,
        "the new server to take the name back",
        |_| owners.try_iter().any(|o| o == me),
    );
    let shown = notify("second", ping.clone());
    reply(&mut rt, &wake, "the second notification", &shown);
    assert_eq!(log(&rt, root), "first;second;");
}
