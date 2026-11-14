//! `bus::call_then`: a method call on the shared connection is written and the caller goes on;
//! the answer (a reply, an error of the peer or the bus, the connection closing) reaches the
//! call's closure when it comes. A peer that never answers holds nothing back.
//! `bus::call_then_within`: the same call with a limit, at which it is given up.
//! Fails when dbus-daemon is unavailable, unless RI_EQUIV_SKIP=1.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use zbus::blocking::Connection;

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

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

/// The method calls `c` receives, by member.
fn calls(c: &Connection) -> impl Iterator<Item = String> + use<> {
    zbus::blocking::MessageIterator::from(c).filter_map(|m| {
        let m = m.ok()?;
        (m.message_type() == zbus::message::Type::MethodCall)
            .then(|| m.header().member().unwrap().to_string())
    })
}

#[test]
fn a_call_is_written_and_answered_later() {
    const TEST: &str = concat!(module_path!(), "::a_call_is_written_and_answered_later");
    // The shared connection finds the bus through the environment, so the test runs in a
    // process of its own with the session bus address set (a socket in a fresh directory).
    if !testenv::in_own_process(TEST) {
        let dir = std::env::temp_dir().join(format!("ri-bus-then-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let addr = format!("unix:path={}", dir.join("bus").display());
        testenv::run_in_own_process(TEST, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let Some((daemon, addr)) = daemon(&addr) else {
        return;
    };
    let c = bus::session().expect("session");
    // Peers that answer no call: a connection with no object server replies to nothing.
    let (silent, quiet) = (peer(&addr), peer(&addr));
    let (silent_name, quiet_name) = (
        silent.unique_name().unwrap().to_string(),
        quiet.unique_name().unwrap().to_string(),
    );
    let mut got = calls(&silent);
    let hold = |to: &str, method: &str| {
        let (tx, rx) = channel();
        bus::call_then(
            &c,
            to,
            "/held",
            "org.example.Silent",
            method,
            &(),
            move |r| {
                let _ = tx.send(r);
            },
        )
        .unwrap();
        rx
    };

    // Written in order, each without an answer to the one before.
    let first = hold(&silent_name, "First");
    let second = hold(&silent_name, "Second");
    assert_eq!(got.next().as_deref(), Some("First"));
    assert_eq!(got.next().as_deref(), Some("Second"));
    assert!(first.try_recv().is_err() && second.try_recv().is_err());

    // A reply reaches its own call, behind the unanswered ones.
    let (tx, rx) = channel();
    bus::call_then(
        &c,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetId",
        &(),
        move |r| {
            let _ = tx.send(r);
        },
    )
    .unwrap();
    let id: String = rx.recv().unwrap().unwrap().body().deserialize().unwrap();
    assert!(!id.is_empty());

    // An error reply too: the bus knows no such name.
    let unknown = hold("org.example.Nobody", "Hello");
    match unknown.recv().unwrap() {
        Err(zbus::Error::MethodError(name, _, _)) => {
            assert_eq!(name.as_str(), "org.freedesktop.DBus.Error.ServiceUnknown")
        }
        other => panic!("a call to nobody ended with {other:?}"),
    }

    // The peer leaves without answering: the bus answers for it.
    drop(got);
    drop(silent);
    for rx in [first, second] {
        match rx.recv().unwrap() {
            Err(zbus::Error::MethodError(name, _, _)) => {
                assert_eq!(name.as_str(), "org.freedesktop.DBus.Error.NoReply")
            }
            other => panic!("a call to a peer that left ended with {other:?}"),
        }
    }

    // The connection closes with a call out: the call is told, and later ones are not sent.
    let last = hold(&quiet_name, "Last");
    drop(daemon);
    match last.recv().unwrap() {
        Err(zbus::Error::InputOutput(_)) => {}
        other => panic!("a call on a closed connection ended with {other:?}"),
    }
    let (tx, rx) = channel();
    let unsent = bus::call_then(
        &c,
        &quiet_name,
        "/held",
        "org.example.Silent",
        "Unsent",
        &(),
        move |r| {
            let _ = tx.send(r);
        },
    );
    assert!(unsent.is_err(), "a call was sent on a closed connection");
    assert!(rx.recv().is_err(), "an unsent call was answered");
}

fn timed_out(r: zbus::Result<zbus::Message>) -> bool {
    matches!(r, Err(zbus::Error::InputOutput(e)) if e.kind() == std::io::ErrorKind::TimedOut)
}

/// A peer that stays on the bus and answers nothing sends no event to wait on, so the call's
/// limit is a clock (docs/clocks.md): the call ends with `TimedOut` at its limit and is
/// forgotten, a nearer limit is not slept through, an answer ends the call before any limit,
/// and an answer after the limit reaches nobody.
#[test]
fn a_held_call_ends_at_its_limit_and_is_forgotten() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_held_call_ends_at_its_limit_and_is_forgotten"
    );
    if !testenv::in_own_process(TEST) {
        let dir = std::env::temp_dir().join(format!("ri-bus-within-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let addr = format!("unix:path={}", dir.join("bus").display());
        testenv::run_in_own_process(TEST, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let Some((_daemon, addr)) = daemon(&addr) else {
        return;
    };
    let c = bus::session().expect("session");
    // A peer that answers no call until the test makes it.
    let silent = peer(&addr);
    let silent_name = silent.unique_name().unwrap().to_string();
    let mut got = zbus::blocking::MessageIterator::from(&silent)
        .filter_map(|m| m.ok())
        .filter(|m| m.message_type() == zbus::message::Type::MethodCall);
    let within = |limit: Duration, to: &str, path: &str, iface: &str, method: &str| {
        let (tx, rx) = channel();
        bus::call_then_within(&c, limit, to, path, iface, method, &(), move |r| {
            let _ = tx.send(r);
        })
        .unwrap();
        rx
    };
    let hold = |limit, method| within(limit, &silent_name, "/held", "org.example.Silent", method);
    let get_id = || {
        let dbus = "org.freedesktop.DBus";
        within(DAY, dbus, "/org/freedesktop/DBus", dbus, "GetId")
    };
    // A limit no test outlives.
    const DAY: Duration = Duration::from_secs(86_400);
    const HOUR: Duration = Duration::from_secs(3600);

    // The limiter sleeps towards the long call's limit when the short one is made: it ends
    // the short one at its own, and only that one.
    let t0 = Instant::now();
    let long = hold(HOUR, "Long");
    let short = hold(Duration::from_millis(200), "Short");
    assert!(timed_out(short.recv().unwrap()), "the short call's end");
    assert!(long.try_recv().is_err(), "the long call ended with it");
    let call = got.next().unwrap();
    assert_eq!(call.header().member().unwrap().as_str(), "Long");
    let call = got.next().unwrap();
    assert_eq!(call.header().member().unwrap().as_str(), "Short");

    // The peer answers after the limit: nobody is told, and the calls after it are answered
    // as ever. An answer ends its call whatever the limit: returning proves it.
    silent.reply(&call.header(), &()).unwrap();
    let id: String = get_id()
        .recv()
        .unwrap()
        .unwrap()
        .body()
        .deserialize()
        .unwrap();
    assert!(!id.is_empty());
    assert!(short.recv().is_err(), "a call that had ended was answered");

    // The moment named instead of waited for: nothing ends before the long call's limit, the
    // call ends at it, and the calls that were answered are not there to end.
    assert_eq!(
        bus::limits_reached_at(&c, t0 + HOUR - Duration::from_secs(1)),
        0
    );
    assert!(long.try_recv().is_err());
    assert_eq!(bus::limits_reached_at(&c, Instant::now() + HOUR), 1);
    assert!(timed_out(long.recv().unwrap()), "the long call's end");
    assert_eq!(bus::limits_reached_at(&c, Instant::now() + DAY + HOUR), 0);
}

/// The system refuses the thread that ends calls at their limits (the limit on threads is
/// reached). The refusal is logged in one line and the call is written and answered as any
/// other; the next call with a limit asks for the thread again, and once one is given, the
/// thread ends the earlier call too, whose limit has come meanwhile.
#[test]
fn a_call_whose_limiter_thread_is_refused_is_written_and_the_next_asks_again() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_call_whose_limiter_thread_is_refused_is_written_and_the_next_asks_again"
    );
    if !testenv::in_own_process(TEST) {
        let dir = std::env::temp_dir().join(format!("ri-bus-refused-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let addr = format!("unix:path={}", dir.join("bus").display());
        testenv::run_in_own_process(TEST, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let Some((_daemon, addr)) = daemon(&addr) else {
        return;
    };
    // The connection's router thread runs from here on.
    let c = bus::session().expect("session");
    let silent = peer(&addr);
    let silent_name = silent.unique_name().unwrap().to_string();
    let mut got = calls(&silent);
    let within = |limit: Duration, to: &str, path: &str, iface: &str, method: &str| {
        let (tx, rx) = channel();
        bus::call_then_within(&c, limit, to, path, iface, method, &(), move |r| {
            let _ = tx.send(r);
        })
        .expect("the call was not written");
        rx
    };
    let hold = |limit, method| within(limit, &silent_name, "/held", "org.example.Silent", method);
    let get_id = |limit| {
        let dbus = "org.freedesktop.DBus";
        within(limit, dbus, "/org/freedesktop/DBus", dbus, "GetId")
    };
    const HOUR: Duration = Duration::from_secs(3600);
    let refused = |log: &str| {
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), 1, "{log}");
        assert!(lines[0].starts_with("dbus: no thread to end calls at their limits"));
    };

    // The thread that makes the first two calls with a limit gets no new thread.
    let held = std::thread::scope(|s| {
        let calls = s.spawn(|| {
            testenv::refuse_new_threads();
            let mut held = None;
            refused(&testenv::stderr_of(|| {
                held = Some(hold(Duration::from_millis(1), "Held"));
            }));
            // Asked for again, and refused again; the call is answered.
            let mut id = None;
            refused(&testenv::stderr_of(|| id = Some(get_id(HOUR))));
            let id: String = id
                .unwrap()
                .recv()
                .unwrap()
                .expect("GetId")
                .body()
                .deserialize()
                .unwrap();
            assert!(!id.is_empty());
            held.unwrap()
        });
        calls.join().unwrap()
    });
    assert_eq!(got.next().as_deref(), Some("Held"));

    // A call from a thread that may start one: the limiter runs, and ends the held call.
    let log = testenv::stderr_of(|| {
        let long = hold(HOUR, "Long");
        assert!(timed_out(held.recv().unwrap()), "the held call's end");
        assert!(long.try_recv().is_err(), "the long call ended with it");
    });
    assert_eq!(log, "");
    assert_eq!(got.next().as_deref(), Some("Long"));
}
