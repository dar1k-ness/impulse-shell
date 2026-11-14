//! Bus connections leave no helper thread behind: zbus connects unix sockets on the `blocking`
//! crate's pool, whose threads never exit and wake every 500 ms. Fails when dbus-daemon is
//! unavailable, unless RI_EQUIV_SKIP=1.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use zbus::MatchRule;
use zbus::message::Type as MsgType;

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

fn threads() -> Vec<String> {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .map(|e| {
            std::fs::read_to_string(e.unwrap().path().join("comm"))
                .unwrap()
                .trim()
                .to_owned()
        })
        .collect()
}

#[test]
fn connections_leave_no_blocking_pool_thread() {
    const TEST: &str = concat!(
        module_path!(),
        "::connections_leave_no_blocking_pool_thread"
    );
    // The shared connection finds the bus through the environment, so the test runs in a
    // process of its own with the session bus address set (a socket in a fresh directory).
    if !testenv::in_own_process(TEST) {
        let dir = std::env::temp_dir().join(format!("ri-bus-connect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a1 = format!("unix:path={}", dir.join("bus").display());
        testenv::run_in_own_process(TEST, [("DBUS_SESSION_BUS_ADDRESS", a1)]);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let a1 = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let Some((_d1, a1)) = daemon(&a1) else {
        return;
    };
    let Some((_d2, a2)) = daemon(&format!(
        "unix:abstract=ri-bus-connect-{}",
        std::process::id()
    )) else {
        return;
    };
    assert!(
        a1.starts_with("unix:path=") && a2.starts_with("unix:abstract="),
        "{a1} {a2}"
    );

    // The shared connection, through the bus address environment.
    let rule = MatchRule::builder()
        .msg_type(MsgType::Signal)
        .interface("org.example.I")
        .unwrap()
        .build();
    let (_c, rx) = bus::subscribe(bus::Bus::Session, vec![rule]).unwrap();
    let peer = bus::builder(&a1).unwrap().build().unwrap();
    peer.emit_signal(None::<&str>, "/t", "org.example.I", "Ping", &(7u32,))
        .unwrap();
    let m = rx.recv().unwrap();
    assert_eq!(m.body().deserialize::<u32>().unwrap(), 7);

    // An abstract socket, and a method call with its reply.
    let c = bus::builder(&a2).unwrap().build().unwrap();
    let id: String = c
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetId",
            &(),
        )
        .unwrap()
        .body()
        .deserialize()
        .unwrap();
    assert_eq!(id.len(), 32);

    // A dead socket is an error, not a hang.
    let none = std::env::temp_dir().join(format!("ri-bus-connect-none-{}", std::process::id()));
    assert!(bus::builder(&format!("unix:path={}", none.display())).is_err());

    let t = threads();
    assert!(!t.iter().any(|n| n.starts_with("blocking")), "{t:?}");
}

struct Late;

#[zbus::interface(name = "org.example.Late")]
impl Late {
    fn ping(&self) -> u32 {
        7
    }
}

#[test]
fn shared_connection_answers_method_calls_from_the_start() {
    const TEST: &str = concat!(
        module_path!(),
        "::shared_connection_answers_method_calls_from_the_start"
    );
    if !testenv::in_own_process(TEST) {
        let dir = std::env::temp_dir().join(format!("ri-bus-serve-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = format!("unix:path={}", dir.join("bus").display());
        testenv::run_in_own_process(TEST, [("DBUS_SESSION_BUS_ADDRESS", a)]);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let a = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let Some((_d, a)) = daemon(&a) else {
        return;
    };
    // Nothing in this process has served an object yet: a call that comes now is answered all
    // the same (a started-later object server would never see it, and its caller would wait
    // for good), and so is one to an object served afterwards (a Watcher's registrations).
    let conn = bus::session().unwrap();
    let me = conn.unique_name().unwrap().to_string();
    let peer = bus::builder(&a).unwrap().build().unwrap();
    let call = |path: &str| {
        peer.call_method(
            Some(me.as_str()),
            path,
            Some("org.example.Late"),
            "Ping",
            &(),
        )
        .map(|m| m.body().deserialize::<u32>().unwrap())
        .map_err(zbus::fdo::Error::from)
    };
    assert!(
        matches!(call("/Late"), Err(zbus::fdo::Error::UnknownObject(_))),
        "a call before anything is served"
    );
    conn.object_server().at("/Late", Late).unwrap();
    assert_eq!(call("/Late"), Ok(7));
}

/// A session bus that accepts the connection and never answers holds the thread that opens
/// it, and only that thread: a call on the system bus is written meanwhile.
#[test]
fn a_bus_being_opened_does_not_hold_calls_on_the_other() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_bus_being_opened_does_not_hold_calls_on_the_other"
    );
    if !testenv::in_own_process(TEST) {
        let dir = std::env::temp_dir().join(format!("ri-bus-other-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let at = |name: &str| format!("unix:path={}", dir.join(name).display());
        testenv::run_in_own_process(
            TEST,
            [
                ("DBUS_SESSION_BUS_ADDRESS", at("session")),
                ("DBUS_SYSTEM_BUS_ADDRESS", at("system")),
            ],
        );
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let a = std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap();
    let Some((_d, _)) = daemon(&a) else {
        return;
    };
    let session = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let silent =
        std::os::unix::net::UnixListener::bind(session.strip_prefix("unix:path=").unwrap())
            .unwrap();
    let (dialed, is_dialed) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut held = vec![];
        for s in silent.incoming().flatten() {
            held.push(s);
            let _ = dialed.send(());
        }
    });
    std::thread::spawn(bus::session);
    is_dialed.recv().unwrap();

    let (answered, answer) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let conn = bus::system().unwrap();
        let (dbus, path) = ("org.freedesktop.DBus", "/org/freedesktop/DBus");
        let limit = std::time::Duration::from_secs(60);
        bus::call_then_within(&conn, limit, dbus, path, dbus, "GetId", &(), move |r| {
            let _ = answered.send(r.map(|m| m.body().deserialize::<String>().unwrap()));
        })
        .unwrap();
    });
    // A call that waits for the session bus is never written, and the test never ends.
    let id = answer.recv().unwrap();
    assert!(!id.unwrap().is_empty());
}

/// The system refuses new threads (the limit on threads is reached) to the thread that opens
/// the session bus first in the process: the connection, its router and async-io each need
/// one. The open fails as it does for a bus that is not there, with one line in the log, and
/// the next one asks again; a thread that is given threads then opens the bus. Once async-io
/// runs, a connection refused its own threads is an error as well.
#[test]
fn an_open_refused_its_threads_is_an_error_and_the_next_asks_again() {
    const TEST: &str = concat!(
        module_path!(),
        "::an_open_refused_its_threads_is_an_error_and_the_next_asks_again"
    );
    if !testenv::in_own_process(TEST) {
        let dir = std::env::temp_dir().join(format!("ri-bus-refused-open-{}", std::process::id()));
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
    let refused = |log: &str| {
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), 1, "{log}");
        assert!(
            lines[0].starts_with("dbus: cannot connect to the Session bus: "),
            "{log}"
        );
    };
    std::thread::scope(|s| {
        let first = s.spawn(|| {
            testenv::refuse_new_threads();
            for _ in 0..2 {
                refused(&testenv::stderr_of(|| assert!(bus::session().is_none())));
            }
        });
        first.join().unwrap();
    });
    assert_eq!(bus::opened(), 0);

    let log = testenv::stderr_of(|| assert!(bus::session().is_some()));
    assert_eq!(log, "");
    assert_eq!(bus::opened(), 1);

    std::thread::scope(|s| {
        let later = s.spawn(|| {
            testenv::refuse_new_threads();
            let own = bus::builder(&addr).and_then(|b| b.build());
            assert!(own.is_err(), "a connection without the thread that runs it");
            assert!(
                bus::session().is_some(),
                "the open connection is not asked for again"
            );
        });
        later.join().unwrap();
    });
}
