//! `bus::call_timeout`: a method call ends with the reply, or with `TimedOut` once the peer
//! has not answered for the limit. A D-Bus peer that holds a call sends nothing at all (no
//! event to wait on), so the limit is a clock: `docs/clocks.md`. `bus::call_until` also ends
//! when its caller stops wanting the answer.
//! Fails when dbus-daemon is unavailable, unless RI_EQUIV_SKIP=1.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
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

#[test]
fn a_call_ends_with_the_reply_or_at_the_limit() {
    let dir = std::env::temp_dir().join(format!("ri-bus-timeout-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let Some((_daemon, addr)) = daemon(&format!("unix:path={}", dir.join("bus").display())) else {
        return;
    };
    let c = peer(&addr);
    // A peer that answers no call: a connection with no object server replies to nothing.
    let silent = peer(&addr);
    let silent_name = silent.unique_name().unwrap().to_owned();
    let held: zbus::Result<zbus::Message> = bus::call_timeout(
        &c,
        Duration::from_millis(200),
        Some(silent_name.as_str()),
        "/held",
        Some("org.example.Silent"),
        "Hold",
        &(),
    );
    match held {
        Err(zbus::Error::InputOutput(e)) => {
            assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}")
        }
        other => panic!("a held call ended with {other:?}"),
    }
    // The reply ends the wait: with a limit no test outlives, returning proves it.
    let id: String = bus::call_timeout(
        &c,
        Duration::from_secs(86_400),
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
    assert!(!id.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Completes once [`Flag::set`] has been called.
#[derive(Clone, Default)]
struct Flag(std::sync::Arc<std::sync::Mutex<(bool, Option<std::task::Waker>)>>);

impl Flag {
    fn set(&self) {
        let mut g = self.0.lock().unwrap();
        g.0 = true;
        if let Some(w) = g.1.take() {
            w.wake();
        }
    }

    async fn wait(&self) {
        std::future::poll_fn(|cx| {
            let mut g = self.0.lock().unwrap();
            if g.0 {
                return std::task::Poll::Ready(());
            }
            g.1 = Some(cx.waker().clone());
            std::task::Poll::Pending
        })
        .await
    }
}

/// `call_until` ends with the reply, at the limit, or as soon as its caller's `cancel` ends,
/// whichever is first; a call cancelled before it starts is not written at all.
#[test]
fn a_call_ends_when_its_caller_no_longer_wants_the_answer() {
    let dir = std::env::temp_dir().join(format!("ri-bus-until-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let Some((_daemon, addr)) = daemon(&format!("unix:path={}", dir.join("bus").display())) else {
        return;
    };
    let c = peer(&addr);
    let silent = peer(&addr);
    let silent_name = silent.unique_name().unwrap().to_owned();
    let mut calls = zbus::blocking::MessageIterator::from(&silent).filter_map(|m| {
        let m = m.ok()?;
        (m.message_type() == zbus::message::Type::MethodCall)
            .then(|| m.header().member().unwrap().to_string())
    });
    let held = |cancel: &Flag, limit: Duration, method: &str| {
        bus::call_until(
            &c,
            limit,
            cancel.wait(),
            Some(silent_name.as_str()),
            "/held",
            Some("org.example.Silent"),
            method,
            &(),
        )
    };

    // Cancelled once the peer has the call: with a limit no test outlives, returning proves
    // the cancel ended the wait.
    let cancel = Flag::default();
    let got = std::thread::scope(|s| {
        s.spawn(|| {
            assert_eq!(calls.next().as_deref(), Some("Hold"));
            cancel.set();
        });
        held(&cancel, Duration::from_secs(86_400), "Hold")
    });
    assert!(got.is_none(), "a cancelled call ended with {got:?}");

    // Cancelled before it starts: nothing is written.
    cancel.set();
    assert!(held(&cancel, Duration::from_secs(86_400), "NeverSent").is_none());

    // Not cancelled: the limit ends it.
    match held(&Flag::default(), Duration::from_millis(200), "Limited") {
        Some(Err(zbus::Error::InputOutput(e))) => {
            assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}")
        }
        other => panic!("a held call ended with {other:?}"),
    }
    assert_eq!(
        calls.next().as_deref(),
        Some("Limited"),
        "the cancelled call was written"
    );

    // Not cancelled, answered: the reply ends it.
    let id: String = bus::call_until(
        &c,
        Duration::from_secs(86_400),
        Flag::default().wait(),
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "GetId",
        &(),
    )
    .expect("not cancelled")
    .unwrap()
    .body()
    .deserialize()
    .unwrap();
    assert!(!id.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
