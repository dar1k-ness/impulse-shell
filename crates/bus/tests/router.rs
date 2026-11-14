//! The shared-connection router against private `dbus-daemon`s: one connection per bus,
//! well-known sender rules follow the name's current owner, a subscriber that never reads does
//! not stall the others, `sync` returns only once the sinks have every earlier signal, and a bus
//! that goes away ends the old subscriptions and the next call reconnects.
//! Fails when dbus-daemon is unavailable, unless RI_EQUIV_SKIP=1.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
use zbus::MatchRule;
use zbus::blocking::Connection;
use zbus::message::{Message, Type as MsgType};

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A daemon listening on `address`.
fn daemon(address: &str) -> Option<(Daemon, String)> {
    let mut c = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .arg(format!("--address={address}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| testenv::equiv_missing(format_args!("dbus-daemon ({e})")))
        .ok()?;
    let mut addr = String::new();
    BufReader::new(c.stdout.take()?).read_line(&mut addr).ok()?;
    Some((Daemon(c), addr.trim().to_string()))
}

fn peer(addr: &str) -> Connection {
    zbus::blocking::connection::Builder::address(addr)
        .unwrap()
        .build()
        .unwrap()
}

fn ping(c: &Connection, n: u32) {
    c.emit_signal(None::<&str>, "/t", "org.example.I", "Ping", &(n,))
        .unwrap();
}

fn rule(sender: Option<&'static str>) -> MatchRule<'static> {
    let b = MatchRule::builder()
        .msg_type(MsgType::Signal)
        .interface("org.example.I")
        .unwrap();
    match sender {
        Some(s) => b.sender(s).unwrap().build(),
        None => b.build(),
    }
}

fn n(m: &Message) -> u32 {
    m.body().deserialize().unwrap()
}

fn recv(rx: &Receiver<Message>) -> u32 {
    n(&rx.recv().expect("signal"))
}

#[test]
fn shared_router() {
    const TEST: &str = concat!(module_path!(), "::shared_router");
    // The router finds the bus through the environment, so the test runs in a process of its
    // own with the session bus address set (a socket in a fresh directory).
    if !testenv::in_own_process(TEST) {
        let dir = std::env::temp_dir().join(format!("ri-bus-router-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let addr = format!("unix:path={}", dir.join("bus").display());
        testenv::run_in_own_process(TEST, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let Some((d1, a1)) = daemon(&addr) else {
        return;
    };

    let c = bus::session().expect("session");
    let again = bus::session().unwrap();
    assert_eq!(c.unique_name(), again.unique_name());
    assert_eq!(bus::opened(), 1);

    let (owner, other) = (peer(&a1), peer(&a1));
    owner.request_name("org.example.A").unwrap();
    let (_, from_a) = bus::subscribe(bus::Bus::Session, vec![rule(Some("org.example.A"))]).unwrap();
    let (_, all) = bus::subscribe(bus::Bus::Session, vec![rule(None)]).unwrap();
    // Never read: must not hold back the others.
    let (_, stuck) = bus::subscribe(bus::Bus::Session, vec![rule(None)]).unwrap();
    assert_eq!(bus::opened(), 1, "subscriptions share the connection");

    // Two connections' signals have no order on the bus: each ping waits until the router
    // handed the previous one to `all`.
    ping(&other, 1);
    assert_eq!(recv(&all), 1);
    ping(&owner, 2);
    assert_eq!(recv(&all), 2);
    assert_eq!(recv(&from_a), 2, "only the name's owner matches");

    // The name moves: the rule follows the new owner. The router updates the owner before it
    // hands a NameOwnerChanged to any sink, so once a subscriber has the one naming `other`,
    // the router follows it.
    let (_, moved) = bus::subscribe(
        bus::Bus::Session,
        vec![
            MatchRule::builder()
                .msg_type(MsgType::Signal)
                .sender("org.freedesktop.DBus")
                .unwrap()
                .interface("org.freedesktop.DBus")
                .unwrap()
                .member("NameOwnerChanged")
                .unwrap()
                .arg(0, "org.example.A")
                .unwrap()
                .build(),
        ],
    )
    .unwrap();
    owner.release_name("org.example.A").unwrap();
    other.request_name("org.example.A").unwrap();
    let new_owner = other.unique_name().unwrap().to_string();
    assert!(
        moved.iter().any(|m| {
            m.body()
                .deserialize::<(String, String, String)>()
                .is_ok_and(|(_, _, new)| new == new_owner)
        }),
        "NameOwnerChanged to the new owner"
    );
    drop(moved);
    ping(&owner, 3);
    assert_eq!(recv(&all), 3);
    ping(&other, 4);
    assert_eq!(recv(&all), 4);
    assert_eq!(recv(&from_a), 4);

    for i in 0..2000 {
        ping(&owner, 100 + i);
    }
    for i in 0..2000 {
        assert_eq!(recv(&all), 100 + i, "bus order");
    }
    drop(stuck);

    // `sync`: the router has handed the sinks every signal the bus sent before its reply.
    let sync_rule = MatchRule::builder()
        .msg_type(MsgType::Signal)
        .interface("org.example.Sync")
        .unwrap()
        .build();
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let s = seen.clone();
    let sink: bus::Sink = Box::new(move |_| {
        s.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    });
    let _ = bus::subscribe_to(bus::Bus::Session, vec![sync_rule], sink).unwrap();
    for i in 0..500u32 {
        owner
            .emit_signal(None::<&str>, "/t", "org.example.Sync", "Ping", &(i,))
            .unwrap();
    }
    // The bus has queued them to us once it answers their sender.
    zbus::blocking::fdo::DBusProxy::new(&owner)
        .unwrap()
        .get_id()
        .unwrap();
    assert!(bus::sync(bus::Bus::Session));
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 500);

    // The last subscriber of a rule gone: the router removes the rule from the bus (from a
    // task on the connection's executor) once a match finds the subscription closed.
    let gone_rule = MatchRule::builder()
        .msg_type(MsgType::Signal)
        .interface("org.example.Gone")
        .unwrap()
        .build();
    let (_, gone) = bus::subscribe(bus::Bus::Session, vec![gone_rule.clone()]).unwrap();
    let on_bus = || {
        let reply = other
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus.Debug.Stats"),
                "GetAllMatchRules",
                &(),
            )
            .unwrap();
        let all: std::collections::HashMap<String, Vec<String>> =
            reply.body().deserialize().unwrap();
        all.get(c.unique_name().unwrap().as_str())
            .is_some_and(|rs| rs.iter().any(|r| r.contains("org.example.Gone")))
    };
    assert!(on_bus(), "AddMatch for the new rule");
    // A monitor sees the router's RemoveMatch: dbus-daemon sends monitors their copy of a call
    // to the bus with the transaction that handled it, so the match is gone by then.
    let monitor = peer(&a1);
    let watched = zbus::blocking::MessageIterator::from(&monitor);
    zbus::blocking::fdo::MonitoringProxy::new(&monitor)
        .unwrap()
        .become_monitor(
            &[MatchRule::builder()
                .msg_type(MsgType::MethodCall)
                .sender(c.unique_name().unwrap().to_owned())
                .unwrap()
                .interface("org.freedesktop.DBus")
                .unwrap()
                .member("RemoveMatch")
                .unwrap()
                .build()],
            0,
        )
        .unwrap();
    drop(gone);
    other
        .emit_signal(None::<&str>, "/t", "org.example.Gone", "Ping", &(0u32,))
        .unwrap();
    let remove_match = watched.map(|m| m.expect("monitor connection")).find(|m| {
        m.message_type() == MsgType::MethodCall
            && m.header().member().is_some_and(|n| n == "RemoveMatch")
            && m.body()
                .deserialize::<&str>()
                .is_ok_and(|r| r.contains("org.example.Gone"))
    });
    assert!(remove_match.is_some(), "RemoveMatch never sent");
    assert!(!on_bus(), "RemoveMatch handled");
    drop(monitor);
    ping(&owner, 5);
    assert_eq!(recv(&all), 5, "the router keeps routing");

    // An owned subscription ends when dropped, with no message needed to find it gone: its
    // sink is dropped and its rule's RemoveMatch is handled before anything sent after it.
    let owned_rule = MatchRule::builder()
        .msg_type(MsgType::Signal)
        .interface("org.example.Owned")
        .unwrap()
        .build();
    let (tx, owned_rx) = std::sync::mpsc::channel::<Message>();
    let owned = bus::subscription(
        bus::Bus::Session,
        vec![owned_rule],
        Box::new(move |m| tx.send(m.clone()).is_ok()),
    )
    .unwrap();
    assert_eq!(owned.connection().unique_name(), c.unique_name());
    let owned_on_bus = || {
        let reply = other
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus.Debug.Stats"),
                "GetAllMatchRules",
                &(),
            )
            .unwrap();
        let all: std::collections::HashMap<String, Vec<String>> =
            reply.body().deserialize().unwrap();
        all.get(c.unique_name().unwrap().as_str())
            .is_some_and(|rs| rs.iter().any(|r| r.contains("org.example.Owned")))
    };
    assert!(owned_on_bus(), "AddMatch for the owned rule");
    drop(owned);
    assert!(
        owned_rx.recv().is_err(),
        "dropping the subscription drops its sink"
    );
    assert!(bus::sync(bus::Bus::Session));
    assert!(
        !owned_on_bus(),
        "RemoveMatch handled before the next round trip"
    );

    // The bus goes away (its daemon exits): the old subscriptions end, and once a daemon
    // listens at the address again the next call reconnects.
    drop(d1);
    assert!(all.recv().is_err(), "closed bus ends its subscriptions");
    let (d2, a2) = daemon(&addr).unwrap();
    let (_, fresh) = bus::subscribe(bus::Bus::Session, vec![rule(None)]).unwrap();
    assert_eq!(bus::opened(), 2);
    ping(&peer(&a2), 7);
    assert_eq!(recv(&fresh), 7, "subscribed on the new bus");
    drop(d2);
}

/// The match rules `c` has on the bus, as `org.freedesktop.DBus.Debug.Stats` lists them.
fn rules_of(other: &Connection, c: &Connection) -> Vec<String> {
    let reply = other
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Debug.Stats"),
            "GetAllMatchRules",
            &(),
        )
        .unwrap();
    let all: std::collections::HashMap<String, Vec<String>> = reply.body().deserialize().unwrap();
    all.get(c.unique_name().unwrap().as_str())
        .cloned()
        .unwrap_or_default()
}

fn owned_sub(name: &'static str) -> (bus::Subscription, Receiver<Message>) {
    let (tx, rx) = std::sync::mpsc::channel::<Message>();
    let sub = bus::subscription(
        bus::Bus::Session,
        vec![rule(Some(name))],
        Box::new(move |m| tx.send(m.clone()).is_ok()),
    )
    .unwrap();
    (sub, rx)
}

/// A well-known sender's owner watch (`NameOwnerChanged arg0=<name>`) stays on the bus while a
/// subscription names it, and leaves with the last one, as a tray menu of an item named
/// `org.kde.StatusNotifierItem-PID-N` does when it closes.
#[test]
fn owner_watch_ends_with_the_last_subscription_naming_it() {
    const TEST: &str = concat!(
        module_path!(),
        "::owner_watch_ends_with_the_last_subscription_naming_it"
    );
    if !testenv::in_own_process(TEST) {
        let dir = std::env::temp_dir().join(format!("ri-bus-owner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let addr = format!("unix:path={}", dir.join("bus").display());
        testenv::run_in_own_process(TEST, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let Some((_d, a)) = daemon(&addr) else {
        return;
    };
    let c = bus::session().expect("session");
    let (other, owner) = (peer(&a), peer(&a));
    let watches = || -> Vec<String> {
        // Everything the router sent before (AddMatch, RemoveMatch) is handled first.
        assert!(bus::sync(bus::Bus::Session));
        rules_of(&other, &c)
            .into_iter()
            .filter(|r| r.contains("NameOwnerChanged"))
            .collect()
    };
    assert_eq!(watches(), Vec::<String>::new());

    for name in ["org.example.N0", "org.example.N1", "org.example.N2"] {
        let (sub, _rx) = owned_sub(name);
        let w = watches();
        assert!(
            w.len() == 1 && w[0].contains(&format!("arg0='{name}'")),
            "one owner watch while subscribed: {w:?}"
        );
        drop(sub);
        assert_eq!(watches(), Vec::<String>::new(), "{name}'s watch stayed");
    }

    // Two subscriptions name one sender: the first one ending keeps the watch the other routes
    // by, so a later owner change still reaches it.
    let (first, _rx1) = owned_sub("org.example.A");
    let (second, rx2) = owned_sub("org.example.A");
    drop(first);
    assert_eq!(
        watches().len(),
        1,
        "the remaining subscription lost its watch"
    );
    // Subscribed before the change, so the claim is seen.
    let (_, claims, stop_claims) = bus::subscribe_cancellable(
        bus::Bus::Session,
        vec![
            MatchRule::builder()
                .msg_type(MsgType::Signal)
                .sender("org.freedesktop.DBus")
                .unwrap()
                .member("NameOwnerChanged")
                .unwrap()
                .arg(0, "org.example.A")
                .unwrap()
                .build(),
        ],
    )
    .unwrap();
    owner.request_name("org.example.A").unwrap();
    claims.recv().expect("NameOwnerChanged for the claim");
    ping(&owner, 1);
    assert_eq!(recv(&rx2), 1, "routed to the name's new owner");
    drop(second);
    stop_claims.cancel();
    drop(claims);
    assert_eq!(
        watches(),
        Vec::<String>::new(),
        "the last subscription naming it left its watch"
    );
}
