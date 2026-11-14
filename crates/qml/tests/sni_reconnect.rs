//! The tray outlives its session-bus connection: when the bus drops it, the tray connects
//! again, serves the Watcher on the new connection and lists what registers there; when there
//! is no bus to connect to, its items go and it serves again once the bus socket is back. The
//! shell's connection runs through a relay socket so the test can end just that connection
//! while the bus (and the test's own peers, connected to it directly) stay up.

mod sni_watcher;

use qml::runtime::Runtime;
use sni_watcher::{Loop, Named, TRAY_QML, WATCHER, ids};
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use zbus::blocking::Connection;

const TEST: &str = concat!(
    module_path!(),
    "::the_tray_serves_again_after_its_connection_closes"
);
const TEST_AWAY: &str = concat!(
    module_path!(),
    "::the_tray_serves_again_once_the_bus_is_back"
);

/// A private `dbus-daemon` at `path`; closing the returned stdin makes its wrapper kill it.
fn daemon(path: &str) -> Option<ChildStdin> {
    let cfg = std::env::temp_dir().join(format!("qml-sni-reconnect-{}.conf", std::process::id()));
    std::fs::write(
        &cfg,
        "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>\
         <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
    )
    .ok()?;
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

/// Each new owner of the Watcher name ("" once nobody owns it), from a match added before this
/// returns.
fn watcher_owners(c: Connection, ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<String> {
    let it = zbus::blocking::fdo::DBusProxy::new(&c)
        .unwrap()
        .receive_name_owner_changed_with_args(&[(0, WATCHER)])
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

/// `test` in a process of its own, its shell's session bus a relay (`DBUS_SESSION_BUS_ADDRESS`)
/// to the bus socket named by `SNI_RECONNECT_BUS`. False in the process that ran it there.
fn own_process(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = |n: &str| dir.path().join(n).display().to_string();
    let relayed = format!("unix:path={}", path("relay"));
    testenv::run_in_own_process(
        test,
        [
            ("DBUS_SESSION_BUS_ADDRESS", relayed),
            ("SNI_RECONNECT_BUS", path("bus")),
        ],
    );
    false
}

/// A connection straight to the bus at `bus` serving item `id`, registered with the Watcher.
fn item_on(bus: &str, id: &'static str) -> Connection {
    let c = zbus::blocking::connection::Builder::address(format!("unix:path={bus}").as_str())
        .unwrap()
        .serve_at("/StatusNotifierItem", Named(id))
        .unwrap()
        .build()
        .unwrap();
    let me = c.unique_name().unwrap().to_string();
    c.call_method(
        Some(WATCHER),
        "/StatusNotifierWatcher",
        Some(WATCHER),
        "RegisterStatusNotifierItem",
        &(me.as_str(),),
    )
    .unwrap();
    c
}

/// The bus drops the tray while its socket is away, as a session bus that restarts does
/// between its exit and its next listen(2): the tray's items go with the connection, and it
/// serves again on the socket's return, with no timer and no second chance needed.
#[test]
fn the_tray_serves_again_once_the_bus_is_back() {
    if !own_process(TEST_AWAY) {
        return;
    }
    let bus = std::env::var("SNI_RECONNECT_BUS").unwrap();
    let Some(_daemon) = daemon(&bus) else {
        return;
    };
    let relayed = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let relayed = relayed.strip_prefix("unix:path=").unwrap();
    let ends = relay(relayed, &bus);
    let direct = || {
        zbus::blocking::connection::Builder::address(format!("unix:path={bus}").as_str())
            .unwrap()
            .build()
            .unwrap()
    };

    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let owners = watcher_owners(direct(), lp.ping());
    let root = rt.load_str(TRAY_QML).unwrap();
    let (_, first) = lp.recv_until(&mut rt, &owners, "the tray serves the Watcher", |o| {
        !o.is_empty()
    });
    let _a = item_on(&bus, "a");
    lp.until(&mut rt, "item a", |rt| ids(rt, root) == "a");

    // The socket goes before the connection ends, so the tray's next connect finds no bus.
    std::fs::remove_file(relayed).unwrap();
    cut(&ends);
    lp.recv_until(&mut rt, &owners, "the Watcher released", |o| o.is_empty());
    let ends = relay(relayed, &bus);
    let (_, again) = lp.recv_until(&mut rt, &owners, "the Watcher served again", |o| {
        !o.is_empty()
    });
    assert_ne!(again, first, "served again on a new connection");
    let _b = item_on(&bus, "b");
    lp.until(&mut rt, "item b on the new connection", |rt| {
        ids(rt, root) == "b"
    });

    // While the bus is away the tray shows nothing of the connection it lost.
    std::fs::remove_file(relayed).unwrap();
    cut(&ends);
    lp.until(&mut rt, "the lost connection's items to go", |rt| {
        ids(rt, root).is_empty()
    });
    let _ends = relay(relayed, &bus);
    lp.recv_until(&mut rt, &owners, "the Watcher served a third time", |o| {
        !o.is_empty() && *o != again
    });
    let _c = item_on(&bus, "c");
    lp.until(&mut rt, "item c on the third connection", |rt| {
        ids(rt, root) == "c"
    });
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn the_tray_serves_again_after_its_connection_closes() {
    if !own_process(TEST) {
        return;
    }
    let bus = std::env::var("SNI_RECONNECT_BUS").unwrap();
    let Some(_daemon) = daemon(&bus) else {
        return;
    };
    let relayed = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let ends = relay(relayed.strip_prefix("unix:path=").unwrap(), &bus);
    let direct = || {
        zbus::blocking::connection::Builder::address(format!("unix:path={bus}").as_str())
            .unwrap()
            .build()
            .unwrap()
    };
    let item = |id: &'static str| item_on(&bus, id);

    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let owners = watcher_owners(direct(), lp.ping());
    let root = rt.load_str(TRAY_QML).unwrap();
    let (_, first) = lp.recv_until(&mut rt, &owners, "the tray serves the Watcher", |o| {
        !o.is_empty()
    });
    let _a = item("a");
    lp.until(&mut rt, "item a", |rt| ids(rt, root) == "a");

    cut(&ends);
    lp.recv_until(&mut rt, &owners, "the Watcher released", |o| o.is_empty());
    let (_, again) = lp.recv_until(&mut rt, &owners, "the Watcher served again", |o| {
        !o.is_empty()
    });
    assert_ne!(again, first, "served again on a new connection");
    // Items register with the new Watcher (as every item does when a Watcher appears).
    let _b = item("b");
    lp.until(&mut rt, "item b on the new connection", |rt| {
        ids(rt, root) == "b"
    });
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
