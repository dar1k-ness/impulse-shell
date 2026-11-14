//! The system- and session-bus services (UPower, PowerProfiles, Network, Bluetooth, Mpris, the
//! system tray) live as long as their World: dropping it ends each one's subscription at once,
//! with the match rules it added, instead of when its daemon next signals (never, for an absent
//! or idle one). The last Bluetooth service also withdraws the pairing agent from bluetoothd and
//! the bus. Each test runs in a process of its own whose system and session bus is one private
//! `dbus-daemon`.
use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use testenv::Wakeups;
use zbus::blocking::Connection;

/// Closing it (test process exit) makes the wrapper shell kill the daemon.
struct Bus(#[allow(dead_code)] ChildStdin);

/// In `test`'s own process (see `testenv::run_in_own_process`): a private bus at the address the
/// environment names as both buses, a socket in a directory the parent made. `None` in the
/// process that ran the test there, or without dbus-daemon. No <servicedir>: nothing is
/// bus-activated.
fn bus(test: &str) -> Option<Bus> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(
            test,
            [
                ("DBUS_SESSION_BUS_ADDRESS", addr.clone()),
                ("DBUS_SYSTEM_BUS_ADDRESS", addr),
            ],
        );
        return None;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let cfg = std::env::temp_dir().join(format!("qml-svc-life-{}.conf", std::process::id()));
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
    c.stdin.take().map(Bus)
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

/// The unique names of this process's shared connections.
fn mine() -> Vec<String> {
    let mut v: Vec<String> = [qml::dbus::session(), qml::dbus::system()]
        .into_iter()
        .flatten()
        .map(|c| c.unique_name().unwrap().to_string())
        .collect();
    v.dedup();
    v
}

/// The senders of the method calls named `member` on the bus from now on, each also pinging
/// `wake`: a monitor (the bus driver's `BecomeMonitor`) sees calls addressed to anyone.
fn calls(member: &str, wake: &Wakeups) -> Receiver<String> {
    let mon = Connection::session().unwrap();
    mon.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus.Monitoring"),
        "BecomeMonitor",
        &(vec![format!("type='method_call',member='{member}'")], 0u32),
    )
    .unwrap();
    let it = zbus::blocking::MessageIterator::from(&mon);
    let (tx, rx) = channel();
    let ping = wake.ping();
    std::thread::spawn(move || {
        let _mon = mon;
        for m in it.flatten() {
            let sender = m.header().sender().map(|s| s.to_string());
            if tx.send(sender.unwrap_or_default()).is_err() {
                return;
            }
            ping();
        }
    });
    rx
}

/// The match rules naming `what` that this process's shared connections have on the bus, once
/// the bus has handled everything each sent so far (the bus handles one connection's messages in
/// order, a reply to a later call comes after).
fn rules(probe: &Connection, what: &str) -> Vec<String> {
    for c in [qml::dbus::session(), qml::dbus::system()]
        .into_iter()
        .flatten()
    {
        c.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Peer"),
            "Ping",
            &(),
        )
        .unwrap();
    }
    let mine = mine();
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
    mine.iter()
        .flat_map(|m| all.get(m).cloned().unwrap_or_default())
        .filter(|r| r.contains(what))
        .collect()
}

/// Runs `rt` until this process calls `first` (seen in `seen`): the call a service's worker makes
/// first once subscribed, so the subscription is complete and its stop armed.
fn until_called(rt: &mut Runtime, wake: &Wakeups, seen: &Receiver<String>, first: &str) {
    let mine = mine();
    until(
        rt,
        wake,
        &format!("the service's first {first} call"),
        |_| seen.try_iter().any(|s| mine.contains(&s)),
    );
}

/// Starts the service `qml` uses in a World, waits until it is subscribed (it made its `first`
/// call), drops the World and checks that none of its rules is left.
fn ends_with_world(
    test: &str,
    qml: &str,
    what: &str,
    first: &str,
    root: Option<std::path::PathBuf>,
    held: &[String],
) {
    let Some(_bus) = bus(test) else {
        return;
    };
    let probe = Connection::session().unwrap();
    for n in held {
        probe
            .request_name_with_flags(n.as_str(), zbus::fdo::RequestNameFlags::DoNotQueue.into())
            .unwrap();
    }
    let wake = Wakeups::new();
    let seen = calls(first, &wake);
    let mut rt = Runtime::new(root.clone());
    rt.world.borrow().waker.set_ping(wake.ping());
    if let Some(r) = &root {
        rt.load_singletons_where(&[r.join("services")], &|n| n == "Network");
    }
    rt.load_str(qml).unwrap();
    until_called(&mut rt, &wake, &seen, first);
    assert!(!rules(&probe, what).is_empty());
    let world = std::rc::Rc::downgrade(&rt.world);
    drop(rt);
    assert!(world.upgrade().is_none(), "the World outlived its Runtime");
    assert_eq!(
        rules(&probe, what),
        Vec::<String>::new(),
        "the subscription outlived its World"
    );
}

#[test]
fn upower_ends_with_its_world() {
    ends_with_world(
        concat!(module_path!(), "::upower_ends_with_its_world"),
        "import QtQuick\nimport Rustshell.Services.UPower\nItem { property bool a: UPower.onBattery }",
        "org.freedesktop.UPower'",
        "GetAll",
        None,
        &[],
    );
}

#[test]
fn power_profiles_end_with_their_world() {
    ends_with_world(
        concat!(module_path!(), "::power_profiles_end_with_their_world"),
        "import QtQuick\nimport Rustshell.Services.UPower\nItem { property int p: PowerProfiles.profile }",
        "PowerProfiles",
        "GetAll",
        None,
        &[],
    );
}

#[test]
fn mpris_ends_with_its_world() {
    ends_with_world(
        concat!(module_path!(), "::mpris_ends_with_its_world"),
        "import QtQuick\nimport Rustshell.Services.Mpris\nItem { property int c: Mpris.players.values.length }",
        "mpris",
        "ListNames",
        None,
        &[],
    );
}

#[test]
fn network_ends_with_its_world() {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"));
    ends_with_world(
        concat!(module_path!(), "::network_ends_with_its_world"),
        "import QtQuick\nimport ri.services\nItem { property bool e: Network.wifiEnabled }",
        "org.freedesktop.NetworkManager",
        "NameHasOwner",
        Some(root.canonicalize().unwrap_or(root)),
        &[],
    );
}

/// The tray's item and Watcher signals, on a bus where nothing signals the tray after its World
/// goes: another connection is the Watcher, and holds the tray's host name, so the tray only
/// queues for it and its release on stop changes no owner (no `NameOwnerChanged`). A
/// subscription that ended only when a matching signal found its loop gone would stay. The
/// Watcher the tray serves, with its own `NameOwnerChanged` watch, belongs to the connection and
/// stays for a later tray; zbus's `NameAcquired`/`NameLost` matches for the names it requests
/// are zbus's own.
#[test]
fn tray_ends_with_its_world() {
    ends_with_world(
        concat!(module_path!(), "::tray_ends_with_its_world"),
        "import QtQuick\nimport Rustshell.Services.SystemTray\nItem { property int n: SystemTray.items.values.length }",
        "interface='org.kde.StatusNotifier",
        "RequestName",
        None,
        &[
            "org.kde.StatusNotifierWatcher".into(),
            // The process's first tray's, in the spec's `org.freedesktop.StatusNotifierHost-PID-ID`.
            format!("org.kde.StatusNotifierHost-{}-1", std::process::id()),
        ],
    );
}

const AGENT: &str = "/org/rustshell/BluetoothAgent";
const BT_QML: &str = "import QtQuick\nimport Rustshell.Bluetooth\nItem { property int n: Bluetooth.devices.values.length }";

/// tests/fake_sysbus.py as bluetoothd; its output lines, each also pinging `wake`.
fn fake_bluez(wake: &Wakeups) -> Option<(Child, Receiver<String>)> {
    let python = ["/usr/bin/python3", "python3"].into_iter().find(|p| {
        Command::new(p)
            .args([
                "-c",
                "import dbus, dbus.mainloop.glib; from gi.repository import GLib",
            ])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    let Some(python) = python else {
        testenv::unavailable("python3 with dbus-python and gi not found");
        return None;
    };
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake_sysbus.py");
    let init = r#"{"objects": {"/org/bluez/hci0": {"org.bluez.Adapter1": {"Address": "00:11:22:33:44:55", "Alias": "hci0", "Powered": true, "PowerState": "on", "Discoverable": false, "Pairable": true, "Discovering": false, "DiscoverableTimeout": 180, "PairableTimeout": 0}}}}"#;
    let mut child = Command::new(python)
        .args([script, "bluez", init])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn fake bluetoothd");
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = channel();
    let ping = wake.ping();
    std::thread::spawn(move || {
        for l in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(l).is_err() {
                return;
            }
            ping();
        }
    });
    expect(&rx, "READY");
    Some((child, rx))
}

/// Blocks until the fake prints a line containing `s`; the lines before it.
fn expect(out: &Receiver<String>, s: &str) -> Vec<String> {
    eprintln!("waiting for the fake to print {s:?}");
    let mut before = vec![];
    for l in out.iter() {
        if l.contains(s) {
            return before;
        }
        before.push(l);
    }
    panic!("the fake ended before printing {s:?}");
}

/// Whether this process still serves the agent object: the ObjectServer answers a call to a path
/// it does not serve with `UnknownObject`.
fn agent_served(probe: &Connection) -> bool {
    let me = qml::dbus::system()
        .unwrap()
        .unique_name()
        .unwrap()
        .to_string();
    match probe.call_method(
        Some(me.as_str()),
        AGENT,
        Some("org.freedesktop.DBus.Introspectable"),
        "Introspect",
        &(),
    ) {
        Ok(_) => true,
        Err(zbus::Error::MethodError(n, _, _))
            if n.as_str() == "org.freedesktop.DBus.Error.UnknownObject" =>
        {
            false
        }
        Err(e) => panic!("Introspect {AGENT}: {e}"),
    }
}

#[test]
fn bluetooth_ends_with_its_world() {
    let Some(_bus) = bus(concat!(module_path!(), "::bluetooth_ends_with_its_world")) else {
        return;
    };
    let probe = Connection::session().unwrap();
    let wake = Wakeups::new();
    let Some((mut fake, out)) = fake_bluez(&wake) else {
        return;
    };
    let seen = calls("GetManagedObjects", &wake);
    let world = |wake: &Wakeups, out: &Receiver<String>| {
        let mut rt = Runtime::new(None);
        rt.world.borrow().waker.set_ping(wake.ping());
        rt.load_str(BT_QML).unwrap();
        until_called(&mut rt, wake, &seen, "GetManagedObjects");
        expect(out, &format!("AGENT RequestDefaultAgent {AGENT}"));
        rt
    };

    // Overlapping Worlds (a reload's successor starts before the old one goes): the survivor
    // keeps the agent and its subscription.
    let old = world(&wake, &out);
    let new = world(&wake, &out);
    let weak = std::rc::Rc::downgrade(&old.world);
    drop(old);
    assert!(weak.upgrade().is_none(), "the World outlived its Runtime");
    assert!(!rules(&probe, "org.bluez").is_empty());
    assert!(agent_served(&probe));

    let weak = std::rc::Rc::downgrade(&new.world);
    drop(new);
    assert!(weak.upgrade().is_none(), "the World outlived its Runtime");
    assert_eq!(
        rules(&probe, "org.bluez"),
        Vec::<String>::new(),
        "the subscription outlived its World"
    );
    // Taken off the bus before bluetoothd hears of it, and unregistered once.
    let before = expect(&out, &format!("AGENT UnregisterAgent {AGENT}"));
    assert!(
        !before.iter().any(|l| l.contains("UnregisterAgent")),
        "unregistered while a World was alive: {before:?}"
    );
    assert!(!agent_served(&probe), "the agent outlived the last World");
    let _ = fake.kill();
    let _ = testenv::wait_exit(&mut fake);
}
