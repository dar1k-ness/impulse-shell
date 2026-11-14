//! Quickshell keeps one `QDBusConnection` per bus (`sessionBus()` / `systemBus()`) for every
//! service. With MPRIS, the tray, notifications, UPower, power profiles, BlueZ and
//! NetworkManager all running against private buses, the native shell must likewise own exactly
//! one connection on each. Fails when dbus-daemon is unavailable (`testenv`).

use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use zbus::blocking::fdo::DBusProxy;

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
        .map_err(|e| testenv::unavailable(format!("no dbus-daemon ({e})")))
        .ok()?;
    let mut addr = String::new();
    BufReader::new(c.stdout.take()?).read_line(&mut addr).ok()?;
    Some((Daemon(c), addr.trim().to_string()))
}

/// Connections on the bus at `addr` owned by this process, not counting the one asking.
fn ours(addr: &str) -> usize {
    let me = zbus::blocking::connection::Builder::address(addr)
        .unwrap()
        .build()
        .unwrap();
    ours_besides(&me)
}

/// Connections on `me`'s bus owned by this process, not counting `me`.
fn ours_besides(me: &zbus::blocking::Connection) -> usize {
    let dbus = DBusProxy::new(me).unwrap();
    let pid = std::process::id();
    let mine = me.unique_name().unwrap().to_string();
    dbus.list_names()
        .unwrap()
        .into_iter()
        .filter(|n| n.starts_with(':') && n.as_str() != mine)
        .filter(|n| {
            dbus.get_connection_unix_process_id(n.clone().into())
                .is_ok_and(|p| p == pid)
        })
        .count()
}

const QML: &str = r#"import QtQuick
import Rustshell
import Rustshell.Bluetooth
import Rustshell.Networking
import Rustshell.Services.Mpris
import Rustshell.Services.Notifications
import Rustshell.Services.SystemTray
import Rustshell.Services.UPower
Item {
    property int players: Mpris.players.values.length
    property int tray: SystemTray.items.values.length
    property bool battery: UPower.displayDevice.ready
    property int profile: PowerProfiles.profile
    property var adapter: Bluetooth.defaultAdapter
    property bool wifi: Networking.wifiEnabled
    NotificationServer {}
}
"#;

#[test]
fn one_connection_per_bus() {
    const TEST: &str = concat!(module_path!(), "::one_connection_per_bus");
    // The services find the buses through the environment: run in a process of its own with
    // each address naming a socket in a fresh directory.
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let at = |n: &str| format!("unix:path={}", dir.path().join(n).display());
        testenv::run_in_own_process(
            TEST,
            [
                ("DBUS_SESSION_BUS_ADDRESS", at("session")),
                ("DBUS_SYSTEM_BUS_ADDRESS", at("system")),
            ],
        );
        return;
    }
    let Some((_session, sa)) = daemon(&std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap()) else {
        return;
    };
    let Some((_system, ya)) = daemon(&std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap()) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    // Woken by every name owner change on either bus (a connection appearing is one), from
    // match rules added before the services start: on the session bus through the shell's own
    // connection, on the system bus through one of the test's, which the counts leave out.
    let probe = qml::dbus::session().unwrap();
    let watcher = zbus::blocking::connection::Builder::address(ya.as_str())
        .unwrap()
        .build()
        .unwrap();
    for c in [&probe, &watcher] {
        let it = DBusProxy::new(c)
            .unwrap()
            .receive_name_owner_changed()
            .unwrap();
        let ping = wake.ping();
        std::thread::spawn(move || {
            for _ in it {
                ping();
            }
        });
    }
    rt.load_str(QML).unwrap();
    rt.settle();
    // Every service is up: notifications and the tray own their names on the session bus, and
    // the system-bus services have connected.
    let dbus = DBusProxy::new(&probe).unwrap();
    let names = [
        "org.freedesktop.Notifications",
        "org.kde.StatusNotifierWatcher",
    ];
    eprintln!("waiting for the services");
    loop {
        rt.poll_native();
        rt.settle();
        if names
            .iter()
            .all(|n| dbus.name_has_owner((*n).try_into().unwrap()).unwrap())
            && ours_besides(&watcher) > 0
        {
            break;
        }
        wake.wait(None, None);
    }
    assert_eq!(ours(&sa), 1, "session-bus connections");
    assert_eq!(ours_besides(&watcher), 1, "system-bus connections");
    assert_eq!(qml::dbus::opened(), 2);
}
