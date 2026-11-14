//! MPRIS outlives its session-bus connection: when the bus drops it, its players leave the
//! list (they were that connection's) and the service lists the bus again on the next
//! connection; with no usable bus at its start it subscribes once the bus socket appears. Each
//! test runs in a process of its own whose session bus is a private `dbus-daemon`; the shell
//! reaches it through a relay socket, so the test can end just the shell's connection while the
//! bus and the test's players, connected to it directly, stay up.

use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use testenv::Wakeups;
use zbus::blocking::Connection;

const QML: &str = "import QtQuick\nimport Rustshell.Services.Mpris\nItem {\n\
    property string names: Mpris.players.values.map(p => p.identity).sort().join(',') }";

/// A private `dbus-daemon` at `path`; closing the returned stdin makes its wrapper kill it.
fn daemon(path: &str) -> Option<ChildStdin> {
    let cfg = std::env::temp_dir().join(format!("qml-mpris-reconnect-{}.conf", std::process::id()));
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

/// A socket at `listen` that hangs up on whoever connects, as a bus that is not up yet: each
/// connection it dropped is reported on the receiver (and pings the test's loop).
fn refusing(listen: &str, ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<()> {
    let l = UnixListener::bind(listen).unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for client in l.incoming() {
            drop(client);
            if tx.send(()).is_err() {
                return;
            }
            ping();
        }
    });
    rx
}

struct Root(&'static str);

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        self.0.into()
    }
}

struct Player;

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        "Paused".into()
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// A player named `name` on the bus at `bus` itself, not through the relay. Exported before
/// the name is taken, as real players do.
fn player(bus: &str, name: &'static str) -> Connection {
    let path = "/org/mpris/MediaPlayer2";
    zbus::blocking::connection::Builder::address(format!("unix:path={bus}").as_str())
        .unwrap()
        .serve_at(path, Root(name))
        .unwrap()
        .serve_at(path, Player)
        .unwrap()
        .name(format!("org.mpris.MediaPlayer2.{name}"))
        .unwrap()
        .build()
        .unwrap()
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

fn names(rt: &Runtime, root: usize) -> String {
    rt.get(root, "names").to_js_string()
}

/// `test` in a process of its own, its shell's session bus a relay (`DBUS_SESSION_BUS_ADDRESS`)
/// to the bus socket named by `MPRIS_RECONNECT_BUS`. False in the process that ran it there.
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
            ("MPRIS_RECONNECT_BUS", path("bus")),
        ],
    );
    false
}

#[test]
fn mpris_players_follow_the_bus_connection() {
    if !own_process(concat!(
        module_path!(),
        "::mpris_players_follow_the_bus_connection"
    )) {
        return;
    }
    let bus = std::env::var("MPRIS_RECONNECT_BUS").unwrap();
    let Some(_daemon) = daemon(&bus) else {
        return;
    };
    let relayed = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let relayed = relayed.strip_prefix("unix:path=").unwrap();
    let ends = relay(relayed, &bus);
    let wake = Wakeups::new();

    let mut rt = Runtime::new(None);
    rt.world.borrow().waker.set_ping(wake.ping());
    let root = rt.load_str(QML).unwrap();
    let one = player(&bus, "one");
    until(&mut rt, &wake, "the first player", |rt| {
        names(rt, root) == "one"
    });

    // The bus drops the shell and is not there to connect to again: the socket is gone
    // before the connection ends, so the shell's next connect finds none.
    std::fs::remove_file(relayed).unwrap();
    cut(&ends);
    until(
        &mut rt,
        &wake,
        "the lost connection's players to go",
        |rt| names(rt, root).is_empty(),
    );

    // Who is on the bus changes meanwhile; then its socket is back.
    drop(one);
    let _two = player(&bus, "two");
    let _three = player(&bus, "three");
    let _ends = relay(relayed, &bus);
    until(&mut rt, &wake, "the players of the new connection", |rt| {
        names(rt, root) == "three,two"
    });
    // The service follows the new connection's signals too.
    let _four = player(&bus, "four");
    until(
        &mut rt,
        &wake,
        "a player started after the reconnect",
        |rt| names(rt, root) == "four,three,two",
    );
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

#[test]
fn mpris_lists_players_once_the_bus_appears() {
    if !own_process(concat!(
        module_path!(),
        "::mpris_lists_players_once_the_bus_appears"
    )) {
        return;
    }
    let bus = std::env::var("MPRIS_RECONNECT_BUS").unwrap();
    let relayed = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let relayed = relayed.strip_prefix("unix:path=").unwrap();
    let wake = Wakeups::new();
    let ping: Arc<dyn Fn() + Send + Sync> = Arc::new(wake.ping());
    // No bus behind the session address yet: whoever connects is hung up on.
    let refused = refusing(relayed, ping);

    let mut rt = Runtime::new(None);
    rt.world.borrow().waker.set_ping(wake.ping());
    let root = rt.load_str(QML).unwrap();
    until(
        &mut rt,
        &wake,
        "the service's first connect to fail",
        |_| refused.try_recv().is_ok(),
    );
    assert_eq!(qml::dbus::opened(), 0, "connected with no bus");

    // The bus comes up at the address the shell was given (here through the relay).
    let Some(_daemon) = daemon(&bus) else {
        return;
    };
    let _one = player(&bus, "one");
    let _ends = relay(relayed, &bus);
    until(&mut rt, &wake, "the player of the late bus", |rt| {
        names(rt, root) == "one"
    });
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

const INSTANTIATOR: &str = "import QtQuick\nimport QtQml\nimport Rustshell.Services.Mpris\nItem {\n\
    id: root\n\
    property string log: ''\n\
    Instantiator {\n\
        model: Mpris.players\n\
        QtObject {\n\
            required property MprisPlayer modelData\n\
            Component.onCompleted: root.log += '+' + modelData.identity + ' '\n\
            Component.onDestruction: root.log += '-' + modelData.identity + ' '\n\
        }\n\
        onObjectAdded: (i, o) => root.log += 'added(' + i + ',' + o.modelData.identity + ') '\n\
        onObjectRemoved: (i, o) => root.log += 'removed(' + i + ',' + o.modelData.identity + ') '\n\
    } }";

/// An Instantiator over `Mpris.players` (MprisController.qml) follows the model's rows, as
/// Quickshell's under Qt 6.12: a player that comes adds one object, one that leaves takes its
/// own away, and that object still reads the player while it goes (MprisWatcher deletes it
/// later).
#[test]
fn instantiator_follows_the_players_rows() {
    if !own_process(concat!(
        module_path!(),
        "::instantiator_follows_the_players_rows"
    )) {
        return;
    }
    let bus = std::env::var("MPRIS_RECONNECT_BUS").unwrap();
    let Some(_daemon) = daemon(&bus) else {
        return;
    };
    let relayed = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let relayed = relayed.strip_prefix("unix:path=").unwrap();
    let _ends = relay(relayed, &bus);
    let wake = Wakeups::new();

    let mut rt = Runtime::new(None);
    rt.world.borrow().waker.set_ping(wake.ping());
    let root = rt.load_str(INSTANTIATOR).unwrap();
    let log = |rt: &Runtime| rt.get(root, "log").to_js_string();
    let one = player(&bus, "one");
    until(&mut rt, &wake, "the first player's object", |rt| {
        log(rt).contains("added(0,one)")
    });
    let _two = player(&bus, "two");
    until(&mut rt, &wake, "the second player's object", |rt| {
        log(rt).contains("added(1,two)")
    });
    drop(one);
    until(&mut rt, &wake, "the first player's object to go", |rt| {
        log(rt).contains("-")
    });
    assert_eq!(
        log(&rt),
        "+one added(0,one) +two added(1,two) removed(0,one) -one "
    );
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
