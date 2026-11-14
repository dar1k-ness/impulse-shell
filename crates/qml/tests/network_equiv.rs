//! The `Network` singleton of the `Impulse` module (`qml::network`) on a scenario against
//! python-dbusmock's NetworkManager template on a private bus used as the system bus: every
//! step must show its effect in the QML-visible state. Fails when dbus-daemon or
//! python3-dbusmock is unavailable (`testenv`).

use qml::runtime::Runtime;
use qml::value::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Instant;
use zbus::blocking::Connection;

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const MOCK: &str = "org.freedesktop.DBus.Mock";
const WL: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const DEV: &str = "org.freedesktop.NetworkManager.Device";
const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
const PYTHON: &str = "/usr/bin/python3";

fn available() -> bool {
    let ok = |c: &str, a: &[&str]| {
        Command::new(c)
            .args(a)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    ok(PYTHON, &["-c", "import dbusmock.templates.networkmanager"])
        && ok("sh", &["-c", "command -v dbus-daemon"])
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// In `test`'s own process, as root of its own user namespace (see
/// `testenv::run_in_own_process_as_root`): a private bus used as the system bus at the address
/// the environment names (a socket in a directory the parent made), as the service and the
/// mock both find it through the environment. The mock NetworkManager is then owned by root,
/// whichever user runs the test. `None` in the process that ran the test there.
fn private_bus(test: &str) -> Option<Daemon> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process_as_root(test, [("DBUS_SYSTEM_BUS_ADDRESS", addr)]);
        return None;
    }
    let mut d = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .arg(format!(
            "--address={}",
            std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap()
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut addr = String::new();
    BufReader::new(d.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    Some(Daemon(d))
}

macro_rules! mock {
    ($c:expr, $path:expr, $m:expr, $body:expr) => {
        $c.call_method(Some(NM), $path, Some(MOCK), $m, $body)
            .map(|m| m.body().deserialize::<String>().unwrap_or_default())
            .unwrap_or_else(|e| panic!("{}: {e}", $m))
    };
}

fn boottime_ms() -> i64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid, writable timespec for clock_gettime(2) to fill.
    unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000
}

struct Nm {
    _daemon: Daemon,
    c: Connection,
    wifi: String,
    eth: String,
    aps: Vec<String>,
}

/// A fresh mock NetworkManager: a Wi-Fi device connected to "Home:Net" (two BSSIDs, one
/// in use), an open "Cafe", a WPA "Ofis\\x", a hidden network and a disconnected ethernet port.
fn start_nm() -> Nm {
    start_nm_with(&["--template", "networkmanager"])
}

/// [`start_nm`] with these dbusmock template arguments.
fn start_nm_with(template: &[&str]) -> Nm {
    let c = Connection::system().unwrap();
    let bus = zbus::blocking::fdo::DBusProxy::new(&c).unwrap();
    // Subscribed before the mock starts, so its NameOwnerChanged is not missed.
    let mut owners = bus
        .receive_name_owner_changed_with_args(&[(0, NM)])
        .unwrap();
    let daemon = Daemon(
        Command::new(PYTHON)
            .args(["-m", "dbusmock", "--system"])
            .args(template)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    if !bus.name_has_owner(NM.try_into().unwrap()).unwrap() {
        owners
            .find(|s| s.args().is_ok_and(|a| a.new_owner().is_some()))
            .expect("dbusmock never appeared");
    }
    drop(owners);
    let wifi = mock!(
        c,
        NM_PATH,
        "AddWiFiDevice",
        &("mock_Wifi1", "wlan0", 100i32)
    );
    let eth = mock!(
        c,
        NM_PATH,
        "AddEthernetDevice",
        &("mock_Eth1", "eth0", 30i32)
    );
    // nmcli waits for a LastScan newer than its cutoff after RequestScan.
    let scan = format!(
        "import time; self.UpdateProperties('{WL}', {{'LastScan': dbus.Int64(int(time.clock_gettime(time.CLOCK_BOOTTIME) * 1000))}})"
    );
    let _ = mock!(
        c,
        wifi.as_str(),
        "AddProperty",
        &(WL, "LastScan", zbus::zvariant::Value::from(boottime_ms()))
    );
    let _ = mock!(
        c,
        wifi.as_str(),
        "AddMethod",
        &(WL, "RequestScan", "a{sv}", "", scan.as_str())
    );
    let ap = |name: &str, ssid: &str, hw: &str, freq: u32, strength: u8, sec: u32| {
        mock!(
            c,
            NM_PATH,
            "AddAccessPoint",
            &(
                wifi.as_str(),
                name,
                ssid,
                hw,
                2u32,
                freq,
                54000u32,
                strength,
                sec
            )
        )
    };
    let aps = vec![
        ap("AP1", "Home:Net", "11:22:33:44:55:01", 2437, 70, 0x100),
        ap("AP2", "Home:Net", "11:22:33:44:55:02", 5180, 90, 0x100),
        ap("AP3", "Cafe", "11:22:33:44:55:03", 2412, 40, 0),
        ap("AP4", "Ofis\\x", "11:22:33:44:55:04", 2462, 60, 0x200),
        ap("AP5", "", "11:22:33:44:55:05", 2422, 80, 0x100),
        ap("AP7", "Zeta", "11:22:33:44:55:07", 5200, 40, 0),
        ap("AP8", "Sae", "11:22:33:44:55:08", 2427, 30, 0x100),
        ap("AP9", "Owe", "11:22:33:44:55:09", 2432, 20, 0x100),
        ap("AP10", "Tail\\", "11:22:33:44:55:0A", 2447, 10, 0),
        ap("AP11", "Mix", "11:22:33:44:55:0B", 2452, 9, 0x100),
        ap("AP12", "OweTm", "11:22:33:44:55:0C", 2457, 8, 0x100),
        ap("AP13", "OwePsk", "11:22:33:44:55:0D", 2467, 7, 0x100),
        ap("AP14", "SuiteB", "11:22:33:44:55:0E", 2472, 6, 0x100),
    ];
    // Flags AddAccessPoint cannot express: SAE, OWE, transition modes.
    for (i, rsn) in [
        (6, 0x400u32),
        (7, 0x800),
        (9, 0x500),
        (10, 0x1000),
        (11, 0x900),
        (12, 0x2000),
    ] {
        set(&c, &aps[i], AP, "WpaFlags", 0u32.into());
        set(&c, &aps[i], AP, "RsnFlags", rsn.into());
    }
    let conn = mock!(
        c,
        NM_PATH,
        "AddWiFiConnection",
        &(wifi.as_str(), "HomeConn", "Home:Net", "wpa-psk")
    );
    let _ = mock!(
        c,
        NM_PATH,
        "AddActiveConnection",
        &(
            vec![wifi.as_str()],
            conn.as_str(),
            aps[0].as_str(),
            "Home",
            2u32
        )
    );
    let _ = mock!(
        c,
        wifi.as_str(),
        "AddProperty",
        &(
            WL,
            "ActiveAccessPoint",
            zbus::zvariant::Value::from(
                zbus::zvariant::ObjectPath::try_from(aps[0].as_str()).unwrap()
            )
        )
    );
    set(&c, NM_PATH, NM, "Connectivity", 4u32.into());
    Nm {
        _daemon: daemon,
        c,
        wifi,
        eth,
        aps,
    }
}

fn set(c: &Connection, path: &str, iface: &str, name: &str, v: zbus::zvariant::Value) {
    let _ = mock!(c, NM_PATH, "SetProperty", &(path, iface, name, v));
}

const PROBE: &str = r#"
import QtQuick
import Impulse
Item {
    property string snap: JSON.stringify({
        wifi: Network.wifi, ethernet: Network.ethernet, enabled: Network.wifiEnabled,
        scanning: Network.wifiScanning, connecting: Network.wifiConnecting,
        target: Network.wifiConnectTarget ? Network.wifiConnectTarget.ssid : null,
        status: Network.wifiStatus, name: Network.networkName, strength: Network.networkStrength,
        symbol: Network.materialSymbol, active: Network.active ? Network.active.bssid : null,
        nets: Network.wifiNetworks.map(a => [a.ssid, a.bssid, a.strength, a.frequency, a.active,
            a.security, a.isSecure, a.askingPassword, a.lastIpcObject.ssid]),
        friendly: Network.friendlyWifiNetworks.map(a => a.bssid)
    })
    function act(what, ssid, arg) {
        const ap = Network.wifiNetworks.find(a => a.ssid === ssid || a.bssid === ssid);
        if (what === "toggle") Network.toggleWifi();
        else if (what === "enable") Network.enableWifi(arg === "on");
        else if (what === "rescan") Network.rescanWifi();
        else if (what === "connect") Network.connectToWifiNetwork(ap);
        else if (what === "disconnect") Network.disconnectWifiNetwork();
        else if (what === "password") Network.changePassword(ap, arg);
    }
}
"#;

/// The mock's profile of "Home:Net".
const HOME_CONN: &str = "/org/freedesktop/NetworkManager/Settings/HomeConn";

fn ri_root() -> PathBuf {
    let r = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"));
    r.canonicalize().unwrap_or(r)
}

/// Runs `rt`'s loop until `done` holds for its snapshot: between checks it blocks on what
/// can change it, the runtime's wakeups (the native service's D-Bus replies and signals, the
/// worker pool) and its process epoll set, never on wall time;
/// nextest's slow-timeout ends a hang. Each check follows an idle pool and a pass that changed
/// nothing.
fn wait_snap(
    rt: &mut Runtime,
    probe: usize,
    wake: &testenv::Wakeups,
    ep: Option<i32>,
    mut done: impl FnMut(&Runtime, &str) -> bool,
) -> String {
    loop {
        workpool::wait_idle();
        let rev = rt.revision();
        rt.poll_native();
        if rt.revision() != rev {
            continue;
        }
        let s = rt.get(probe, "snap").to_js_string();
        if done(rt, &s) {
            return s;
        }
        if wake.wait(ep, None).fd {
            rt.process_fd_ready();
        }
    }
}

/// No `Process` is running (the service starts none).
fn idle(rt: &Runtime) -> bool {
    rt.world.borrow().procs.is_empty()
}

/// A step: its name, what it changes in NetworkManager, the action it takes in the service,
/// and what the native service's snapshot (and the mock, for what the snapshot cannot show)
/// shows once the step took effect.
type Step = (
    &'static str,
    fn(&Nm),
    Option<(&'static str, &'static str, &'static str)>,
    fn(&Nm, &serde_json::Value, &str) -> bool,
);

/// The entry for `ssid` in a snapshot's network list.
fn net<'a>(v: &'a serde_json::Value, ssid: &str) -> Option<&'a serde_json::Value> {
    v["nets"].as_array().unwrap().iter().find(|n| n[0] == ssid)
}

/// The next AddAndActivateConnection fails like a WPA network nmcli has no password for:
/// the new profile stays, the activation goes DEACTIVATED (NO_SECRETS) and the device FAILED
/// (reason 7, "Secrets were required, but not provided") and then DISCONNECTED.
/// A request that carries the password (the native service asks for it first) is joined, and
/// keeps the password in the new profile.
fn fail_next_activation(n: &Nm) {
    fail_activations(n, false);
}

/// Every AddAndActivateConnection fails as above, password or not.
fn fail_activations(n: &Nm, always: bool) {
    let code = r#"
import dbus, dbusmock
import dbusmock.templates.networkmanager as t
from gi.repository import GLib
sec = args[0].get('802-11-wireless-security', {})
if not ALWAYS and (getattr(self, 'failed_once', False) or 'psk' in sec):
    ret = t.add_and_activate_connection(self, args[0], args[1], args[2])
    if 'psk' in sec:
        o = dbusmock.get_object(ret[0])
        o.settings = {k: dict(v) for k, v in o.settings.items()}
        o.settings.setdefault('802-11-wireless-security', {})['psk'] = sec['psk']
else:
    self.failed_once = True
    dev, ap = args[1], args[2]
    ssid = bytes(dbusmock.get_object(ap).Get(t.ACCESS_POINT_IFACE, 'Ssid')).decode()
    conn = dbus.ObjectPath(t.AddWiFiConnection(self, dev, ap.rsplit('/', 1)[1], ssid, ''))
    ac = dbus.ObjectPath(t.AddActiveConnection(self, [dev], conn, ap, 'Fail', dbus.UInt32(1)))
    # AddActiveConnection marks the device ACTIVATED; a real one waits for secrets instead.
    d = dbusmock.get_object(dev)
    d.Set(t.DEVICE_IFACE, 'State', dbus.UInt32(60))
    d.Set(t.DEVICE_IFACE, 'StateReason', (dbus.UInt32(60), dbus.UInt32(0)))
    d.EmitSignal(t.DEVICE_IFACE, 'StateChanged', 'uuu', [dbus.UInt32(60), dbus.UInt32(100), dbus.UInt32(0)])
    def fail(self=self, dev=dev, ac=ac, t=t, dbus=dbus, dbusmock=dbusmock, GLib=GLib):
        a = dbusmock.get_object(ac)
        a.Set(t.ACTIVE_CONNECTION_IFACE, 'State', dbus.UInt32(4))
        a.EmitSignal(t.ACTIVE_CONNECTION_IFACE, 'StateChanged', 'uu', [dbus.UInt32(4), dbus.UInt32(9)])
        d = dbusmock.get_object(dev)
        old = d.Get(t.DEVICE_IFACE, 'State')
        d.Set(t.DEVICE_IFACE, 'State', dbus.UInt32(120))
        d.Set(t.DEVICE_IFACE, 'StateReason', (dbus.UInt32(120), dbus.UInt32(7)))
        d.EmitSignal(t.DEVICE_IFACE, 'StateChanged', 'uuu', [dbus.UInt32(120), old, dbus.UInt32(7)])
        t.RemoveActiveConnection(self, dev, ac)
        GLib.idle_add(lambda self=self, dev=dev, t=t: t.SetDeviceDisconnected(self, dev) and False)
        return False
    # Fails after this call's reply has gone out, as NetworkManager does.
    GLib.idle_add(fail)
    ret = (conn, ac)
"#
    .replace("ALWAYS", if always { "True" } else { "False" });
    let _ = mock!(
        n.c,
        NM_PATH,
        "AddMethod",
        &(
            NM,
            "AddAndActivateConnection",
            "a{sa{sv}}oo",
            "oo",
            code.as_str()
        )
    );
}

/// The template's profiles only have `Update`, while nmcli's `connection modify` commits with
/// `Update2`; and its PSK profiles share one security dict, so copy it before writing.
fn add_update2(c: &Connection) {
    let list = c
        .call_method(
            Some(NM),
            "/org/freedesktop/NetworkManager/Settings",
            Some("org.freedesktop.NetworkManager.Settings"),
            "ListConnections",
            &(),
        )
        .ok()
        .and_then(|m| {
            m.body()
                .deserialize::<Vec<zbus::zvariant::OwnedObjectPath>>()
                .ok()
        })
        .unwrap_or_default();
    const COPY: &str = "self.settings = {k: dict(v) for k, v in self.settings.items()}; \
                        self.ConnectionUpdate(self, args[0])";
    for p in list {
        for (name, sig, out, ret) in [
            ("Update", "a{sa{sv}}", "", ""),
            (
                "Update2",
                "a{sa{sv}}ua{sv}",
                "a{sv}",
                "; ret = dbus.Dictionary({}, signature='sv')",
            ),
        ] {
            let code = format!("{COPY}{ret}");
            let _ = mock!(
                c,
                p.as_str(),
                "AddMethod",
                &(
                    "org.freedesktop.NetworkManager.Settings.Connection",
                    name,
                    sig,
                    out,
                    code.as_str()
                )
            );
        }
    }
}

/// Saved profiles as `id[:psk]`, to compare what connect/changePassword left behind.
fn profiles(c: &Connection) -> String {
    let list = c
        .call_method(
            Some(NM),
            "/org/freedesktop/NetworkManager/Settings",
            Some("org.freedesktop.NetworkManager.Settings"),
            "ListConnections",
            &(),
        )
        .ok()
        .and_then(|m| {
            m.body()
                .deserialize::<Vec<zbus::zvariant::OwnedObjectPath>>()
                .ok()
        })
        .unwrap_or_default();
    type S = HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>;
    let mut ids: Vec<String> = list
        .iter()
        .filter_map(|p| {
            let get = |m: &str, body: &dyn Fn(&str) -> zbus::Result<zbus::message::Message>| {
                body(m).ok().and_then(|r| r.body().deserialize::<S>().ok())
            };
            let call = |m: &str| {
                if m == "GetSecrets" {
                    c.call_method(
                        Some(NM),
                        p.as_str(),
                        Some("org.freedesktop.NetworkManager.Settings.Connection"),
                        m,
                        &("802-11-wireless-security",),
                    )
                } else {
                    c.call_method(
                        Some(NM),
                        p.as_str(),
                        Some("org.freedesktop.NetworkManager.Settings.Connection"),
                        m,
                        &(),
                    )
                }
            };
            let st = get("GetSettings", &call)?;
            let field = |s: &S, g: &str, k: &str| {
                s.get(g)
                    .and_then(|g| g.get(k))
                    .and_then(|v| <&str>::try_from(&**v).ok().map(str::to_string))
            };
            let id = field(&st, "connection", "id")?;
            let psk = field(&st, "802-11-wireless-security", "psk").or_else(|| {
                get("GetSecrets", &call).and_then(|s| field(&s, "802-11-wireless-security", "psk"))
            });
            Some(match psk {
                Some(k) => format!("{id}:{k}"),
                None => id,
            })
        })
        .collect();
    ids.sort();
    ids.join(",")
}

fn steps() -> Vec<Step> {
    vec![
        (
            "initial",
            |_| {},
            None,
            |_, v, _| v["status"] == "connected" && v["active"] == "11:22:33:44:55:01",
        ),
        (
            "limited",
            |n| set(&n.c, NM_PATH, NM, "Connectivity", 3u32.into()),
            None,
            |_, v, _| v["status"] == "limited",
        ),
        (
            "full again",
            |n| set(&n.c, NM_PATH, NM, "Connectivity", 4u32.into()),
            None,
            |_, v, _| v["status"] == "connected",
        ),
        // The service follows the active access point's signal as NetworkManager reports it.
        (
            "ap strength",
            |n| {
                set(
                    &n.c,
                    &n.aps[0],
                    AP,
                    "Strength",
                    zbus::zvariant::Value::from(55u8),
                )
            },
            None,
            |_, v, _| v["strength"] == 55,
        ),
        (
            "radio off",
            |_| {},
            Some(("enable", "", "off")),
            |_, v, _| v["enabled"] == false,
        ),
        (
            "device unavailable",
            |n| set(&n.c, &n.wifi, DEV, "State", 20u32.into()),
            None,
            |_, v, _| v["status"] == "disabled",
        ),
        (
            "radio on",
            |_| {},
            Some(("enable", "", "on")),
            |_, v, _| v["enabled"] == true,
        ),
        (
            "device disconnected",
            |n| {
                let _ = mock!(n.c, NM_PATH, "SetDeviceDisconnected", &(n.wifi.as_str(),));
                set(
                    &n.c,
                    &n.wifi,
                    WL,
                    "ActiveAccessPoint",
                    zbus::zvariant::ObjectPath::try_from("/").unwrap().into(),
                );
            },
            None,
            // The device's state and its dropped access point arrive apart.
            |_, v, _| {
                v["status"] == "disconnected"
                    && v["active"].is_null()
                    && v["nets"]
                        .as_array()
                        .is_some_and(|n| n.iter().all(|a| a[4] != true))
            },
        ),
        (
            "rescan",
            |n| {
                let _ = mock!(
                    n.c,
                    NM_PATH,
                    "AddAccessPoint",
                    &(
                        n.wifi.as_str(),
                        "AP6",
                        "Kafe 2",
                        "11:22:33:44:55:06",
                        2u32,
                        2417u32,
                        54000u32,
                        75u8,
                        0x100u32
                    )
                );
                let _ = mock!(
                    n.c,
                    NM_PATH,
                    "RemoveAccessPoint",
                    &(n.wifi.as_str(), n.aps[3].as_str())
                );
            },
            Some(("rescan", "", "")),
            |_, v, _| v["scanning"] == false && net(v, "Kafe 2").is_some(),
        ),
        (
            "connect open",
            |_| {},
            Some(("connect", "Cafe", "")),
            |_, v, _| v["status"] == "connected" && v["name"] == "Cafe",
        ),
        // Nothing is active to disconnect (the open network's access point is not the device's
        // active one): the native service does nothing.
        (
            "disconnect",
            |_| {},
            Some(("disconnect", "", "")),
            |_, _, _| true,
        ),
        (
            "ethernet up",
            |n| {
                set(&n.c, &n.eth, DEV, "State", 100u32.into());
                // NetworkManager signals the transition as well; libnm and the native
                // service follow that signal.
                let state = |v: u32| zbus::zvariant::Value::from(v);
                let _ = n.c.call_method(
                    Some(NM),
                    n.eth.as_str(),
                    Some(MOCK),
                    "EmitSignal",
                    &(
                        DEV,
                        "StateChanged",
                        "uuu",
                        vec![state(100), state(30), state(0)],
                    ),
                );
            },
            None,
            |_, v, _| v["ethernet"] == true,
        ),
        (
            "change password",
            |_| {},
            Some(("password", "11:22:33:44:55:02", "hunter22")),
            // The native service then joins the network with the profile that took the
            // password: done once that activation was asked for (and, as every native step, has
            // ended), not when the profile changed before it.
            |n, _, s| {
                s.contains(":hunter22")
                    && method_calls(&n.c, NM_PATH, "ActivateConnection")
                        .iter()
                        .any(|a| {
                            zbus::zvariant::OwnedObjectPath::try_from(a[0].try_clone().unwrap())
                                .is_ok_and(|p| p.as_str() == HOME_CONN)
                        })
            },
        ),
        (
            "connect wrong password",
            fail_next_activation,
            Some(("connect", "Kafe 2", "")),
            |_, v, _| net(v, "Kafe 2").is_some_and(|n| n[7] == true),
        ),
        (
            "retry with password",
            |_| {},
            Some(("password", "Kafe 2", "hunter22")),
            |_, v, s| {
                s.contains("Kafe 2:hunter22") && net(v, "Kafe 2").is_some_and(|n| n[7] == false)
            },
        ),
        (
            "radio toggle",
            |_| {},
            Some(("toggle", "", "")),
            |_, v, _| v["enabled"] == false,
        ),
    ]
}

/// Runs every step, each until `done` holds for its snapshot (`i`: the step, then the snapshot
/// parsed and as compared, with the profiles NetworkManager keeps). Returns each step's
/// snapshot.
fn run(done: &dyn Fn(usize, &Nm, &serde_json::Value, &str) -> bool) -> Vec<String> {
    let nm = start_nm();
    let (mut rt, probe, wake) = native_probe();
    let ep = rt.process_fd();
    let mut out = vec![];
    for (i, (name, mutate, act, _)) in steps().into_iter().enumerate() {
        let t0 = Instant::now();
        add_update2(&nm.c);
        mutate(&nm);
        if let Some((what, ssid, arg)) = act {
            let args = [what, ssid, arg].map(Value::str).to_vec();
            rt.call_method(probe, "act", args).unwrap().unwrap();
        }
        let mut snap = String::new();
        wait_snap(&mut rt, probe, &wake, ep, |rt, s| {
            let seen = format!("{s} profiles={}", profiles(&nm.c));
            if seen != snap {
                // What a step that never gets done is stuck on.
                eprintln!("{name} so far: {seen}");
                snap = seen;
            }
            idle(rt) && done(i, &nm, &serde_json::from_str(s).unwrap(), &snap)
        });
        eprintln!("{name}: {:?} {snap}", t0.elapsed());
        out.push(snap);
    }
    out
}

/// The service runs every step of the scenario until it shows the step's effect, with no
/// process started. A step it never shows hangs until nextest's slow-timeout, the log showing
/// what it got to.
#[test]
fn network_follows_the_scenario() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(module_path!(), "::network_follows_the_scenario")) else {
        return;
    };
    let steps = steps();
    // Every step ends with no connection attempt under way.
    let shown = run(&|i, nm, v, s| v["connecting"] == false && (steps[i].3)(nm, v, s));
    for ((name, ..), n) in steps.iter().zip(&shown) {
        if *name == "device disconnected" {
            let (json, _) = n.split_once(" profiles=").unwrap();
            let v: serde_json::Value = serde_json::from_str(json).unwrap();
            let in_use = v["nets"].as_array().unwrap().iter().any(|a| a[4] == true);
            assert_eq!(
                (&v["active"], &v["strength"], in_use),
                (&serde_json::Value::Null, &0.into(), false),
                "{n}"
            );
        }
    }
    // The scenario exercised what it is meant to (not empty states).
    assert!(shown[0].contains(r#""status":"connected""#), "{}", shown[0]);
    assert!(shown[1].contains(r#""status":"limited""#), "{}", shown[1]);
    assert!(shown[8].contains("Kafe 2"), "{}", shown[8]);
    // The failed activation left "Kafe 2" asking for its password.
    assert!(
        shown[13].contains(r#"["Kafe 2","11:22:33:44:55:06",75,2417,false,"WPA1 WPA2",true,true,"#),
        "{}",
        shown[13]
    );
}

/// The Wi-Fi icon follows the in-use access point's signal as NetworkManager updates it, with
/// no rescan and no `nmcli monitor` report (the original showed the signal of its last scan).
#[test]
fn native_wifi_icon_follows_active_ap_strength() {
    let ok = Command::new(PYTHON)
        .args(["-c", "import dbusmock.templates.networkmanager"])
        .status()
        .is_ok_and(|s| s.success())
        && Command::new("sh")
            .args(["-c", "command -v dbus-daemon"])
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    if !ok {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(
        module_path!(),
        "::native_wifi_icon_follows_active_ap_strength"
    )) else {
        return;
    };
    let nm = start_nm();
    let (mut rt, probe, wake) = native_probe();
    let home = |v: &serde_json::Value| {
        v["nets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n[1] == "11:22:33:44:55:01")
            .map(|n| n[2].clone())
    };
    // The snapshot once the service has the active connection and shows its access point at
    // `signal`, both in the list and as the active one's strength: each is its own D-Bus read,
    // and either can land first.
    let mut snap = |signal: u8| {
        act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
            v["status"] == "connected" && home(v) == Some(signal.into()) && v["strength"] == signal
        })
    };
    let v = snap(70);
    assert_eq!(
        (&v["symbol"], &v["strength"], home(&v)),
        (&"network_wifi".into(), &70.into(), Some(70.into())),
        "{v}"
    );
    for (signal, symbol) in [(30u8, "network_wifi_1_bar"), (90, "signal_wifi_4_bar")] {
        set(&nm.c, &nm.aps[0], AP, "Strength", signal.into());
        let v = snap(signal);
        assert_eq!(
            (&v["symbol"], &v["strength"], home(&v)),
            (&symbol.into(), &signal.into(), Some(signal.into())),
            "{v}"
        );
    }
}

/// Disconnect and change password find the network's profile by its SSID, not by its name: the
/// in-use "Home:Net" profile is called "Home:Net 1" and an older, unused profile is called
/// "Home:Net". The original's `nmcli connection down Home:Net` / `connection modify Home:Net` left the
/// network up and set the password on the unused profile.
#[test]
fn native_disconnect_and_password_find_profile_by_ssid() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(
        module_path!(),
        "::native_disconnect_and_password_find_profile_by_ssid"
    )) else {
        return;
    };
    let nm = start_nm();
    let _ = mock!(
        nm.c,
        NM_PATH,
        "AddWiFiConnection",
        &(nm.wifi.as_str(), "HomeOld", "Home:Net", "wpa-psk")
    );
    add_update2(&nm.c);
    const SCONN: &str = "org.freedesktop.NetworkManager.Settings.Connection";
    const HOME: &str = "/org/freedesktop/NetworkManager/Settings/HomeConn";
    type S = HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>;
    let mut st: S =
        nm.c.call_method(Some(NM), HOME, Some(SCONN), "GetSettings", &())
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
    st.get_mut("connection").unwrap().insert(
        "id".into(),
        zbus::zvariant::Value::from("Home:Net 1")
            .try_into()
            .unwrap(),
    );
    nm.c.call_method(Some(NM), HOME, Some(SCONN), "Update", &(st,))
        .unwrap();
    set(
        &nm.c,
        "/org/freedesktop/NetworkManager/ActiveConnection/Home",
        "org.freedesktop.NetworkManager.Connection.Active",
        "Id",
        "Home:Net 1".into(),
    );
    assert_eq!(profiles(&nm.c), "Home:Net,Home:Net 1", "setup");

    let (mut rt, probe, wake) = native_probe();
    let mut act = |what: &str, ssid: &str, arg: &str, done: &dyn Fn(&serde_json::Value) -> bool| {
        act_until(&mut rt, probe, &wake, (what, ssid, arg), done)
    };

    act("", "", "", &|v| v["active"] == "11:22:33:44:55:01");
    act("password", "11:22:33:44:55:01", "hunter22", &|_| {
        profiles(&nm.c) != "Home:Net,Home:Net 1"
    });
    assert_eq!(profiles(&nm.c), "Home:Net,Home:Net 1:hunter22");
    let active = || {
        let v =
            nm.c.call_method(
                Some(NM),
                NM_PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(NM, "ActiveConnections"),
            )
            .unwrap()
            .body()
            .deserialize::<zbus::zvariant::OwnedValue>()
            .unwrap();
        Vec::<zbus::zvariant::OwnedObjectPath>::try_from(v).unwrap()
    };
    // The saved profile took it: the network it was typed for is joined with it.
    act("", "", "", &|v| {
        v["connecting"] == false && !method_calls(&nm.c, NM_PATH, "ActivateConnection").is_empty()
    });

    // NetworkManager replaces the device's active connection; the mock only adds one.
    const OLD: &str = "/org/freedesktop/NetworkManager/ActiveConnection/Home";
    let left: Vec<zbus::zvariant::ObjectPath> = active()
        .into_iter()
        .filter(|a| a.as_str() != OLD)
        .map(|a| a.into_inner())
        .collect();
    assert_eq!(left.len(), 1, "{left:?}");
    set(&nm.c, NM_PATH, NM, "ActiveConnections", left.into());

    let v = act("disconnect", "", "", &|v| {
        v["status"] == "disconnected" && active().is_empty()
    });
    assert!(active().is_empty(), "{:?}", active());
    assert_eq!(v["status"], "disconnected", "{v}");
}

fn method_calls(c: &Connection, path: &str, method: &str) -> Vec<Vec<zbus::zvariant::OwnedValue>> {
    c.call_method(Some(NM), path, Some(MOCK), "GetMethodCalls", &(method,))
        .unwrap()
        .body()
        .deserialize::<Vec<(u64, Vec<zbus::zvariant::OwnedValue>)>>()
        .unwrap()
        .into_iter()
        .map(|(_, a)| a)
        .collect()
}

/// The probe over `Network`; the runtime pings the returned wakeups.
fn native_probe() -> (Runtime, usize, testenv::Wakeups) {
    let mut rt = Runtime::new(Some(ri_root()));
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let probe = rt.load_str(PROBE).unwrap();
    rt.settle();
    (rt, probe, wake)
}

/// Run the loop until `done` holds: actions run on the worker pool and wake the runtime with
/// what they report, so each wait blocks until then (nextest's slow-timeout ends a hang). Each
/// check follows an idle pool, so the D-Bus calls an action made have been answered.
fn act_until(
    rt: &mut Runtime,
    probe: usize,
    wake: &testenv::Wakeups,
    act: (&str, &str, &str),
    done: &dyn Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let (what, ssid, arg) = act;
    if !what.is_empty() {
        let args = [what, ssid, arg].map(Value::str).to_vec();
        rt.call_method(probe, "act", args).unwrap().unwrap();
    }
    eprintln!("waiting after {act:?}");
    let fd = rt.process_fd();
    loop {
        workpool::wait_idle();
        let rev = rt.revision();
        rt.poll_native();
        if rt.revision() != rev {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(&rt.get(probe, "snap").to_js_string()).unwrap();
        if done(&v) {
            return v;
        }
        if wake.wait(fd, None).fd {
            rt.process_fd_ready();
        }
    }
}

/// State changes never scan: the original's status update rescanned every device whose last scan was
/// 30 s old on each `nmcli monitor` report (`--rescan auto`), the native service reads the
/// signal NetworkManager reports and scans only for `rescanWifi` (the Wi-Fi panel opening).
#[test]
fn native_scans_only_when_asked() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(module_path!(), "::native_scans_only_when_asked")) else {
        return;
    };
    let nm = start_nm();
    set(
        &nm.c,
        &nm.wifi,
        WL,
        "LastScan",
        (boottime_ms() - 60_000).into(),
    );
    let (mut rt, probe, wake) = native_probe();
    act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["status"] == "connected" && v["nets"].as_array().is_some_and(|n| !n.is_empty())
    });
    // After each step the in-use access point's signal is set as a marker: the mock's signals
    // reach the service in the order sent, so once the service shows the marker it has handled
    // the step's signals (and started any scan they caused) as well. The signal step goes last,
    // its own signal the marker.
    for step in [0u8, 1, 3, 2] {
        match step {
            0 => set(&nm.c, NM_PATH, NM, "Connectivity", 3u32.into()),
            1 => set(&nm.c, NM_PATH, NM, "Connectivity", 4u32.into()),
            2 => {}
            _ => {
                set(&nm.c, &nm.wifi, DEV, "State", 30u32.into());
                set(&nm.c, &nm.wifi, DEV, "State", 100u32.into());
            }
        }
        let marker = if step == 2 { 33 } else { 50 + step };
        set(&nm.c, &nm.aps[0], AP, "Strength", marker.into());
        act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
            v["strength"] == marker
        });
    }
    let v: serde_json::Value = serde_json::from_str(&rt.get(probe, "snap").to_js_string()).unwrap();
    assert_eq!(
        (&v["strength"], &v["status"]),
        (&33.into(), &"connected".into()),
        "{v}"
    );
    assert_eq!(method_calls(&nm.c, &nm.wifi, "RequestScan").len(), 0);
    let v = act_until(&mut rt, probe, &wake, ("rescan", "", ""), &|v| {
        v["scanning"] == false
    });
    assert_eq!(v["scanning"], false, "{v}");
    assert_eq!(method_calls(&nm.c, &nm.wifi, "RequestScan").len(), 1);
}

/// Joining a new WPA3 network: no profile is added before the password is known; the password
/// goes into AddAndActivateConnection (key-mgmt from the access point's flags); a refused
/// password deletes the profile it created and asks again; the right one connects.
#[test]
fn native_new_secured_network_asks_first_and_leaves_nothing_on_failure() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(
        module_path!(),
        "::native_new_secured_network_asks_first_and_leaves_nothing_on_failure"
    )) else {
        return;
    };
    let nm = start_nm();
    let before = profiles(&nm.c);
    let (mut rt, probe, wake) = native_probe();
    let asking = |v: &serde_json::Value| {
        v["nets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n[0] == "Sae" && n[7] == true)
    };
    act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["nets"].as_array().is_some_and(|n| !n.is_empty())
    });
    let v = act_until(&mut rt, probe, &wake, ("connect", "Sae", ""), &asking);
    assert!(asking(&v), "{v}");
    assert_eq!(v["connecting"], false, "{v}");
    assert!(method_calls(&nm.c, NM_PATH, "AddAndActivateConnection").is_empty());
    assert_eq!(profiles(&nm.c), before);

    fail_activations(&nm, true);
    let v = act_until(
        &mut rt,
        probe,
        &wake,
        ("password", "Sae", "wrong pass"),
        &|v| asking(v) && v["connecting"] == false,
    );
    assert!(asking(&v), "{v}");
    let calls = method_calls(&nm.c, NM_PATH, "AddAndActivateConnection");
    assert_eq!(calls.len(), 1);
    type S = HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>;
    let st = S::try_from(calls[0][0].try_clone().unwrap()).unwrap();
    let field = |k: &str| {
        st.get("802-11-wireless-security")
            .and_then(|g| g.get(k))
            .and_then(|v| <&str>::try_from(&**v).ok().map(str::to_string))
    };
    assert_eq!(field("key-mgmt").as_deref(), Some("sae"));
    assert_eq!(field("psk").as_deref(), Some("wrong pass"));
    assert_eq!(
        profiles(&nm.c),
        before,
        "asked again before the failed attempt's profile was deleted"
    );

    // Joined once the password is right (the mock keeps it in the new profile).
    fail_next_activation(&nm);
    let sae = nm.aps[6].clone();
    let on_sae = || active_on_ap(&nm.c, &sae);
    let v = act_until(
        &mut rt,
        probe,
        &wake,
        ("password", "Sae", "hunter22"),
        &|v| on_sae() && v["connecting"] == false,
    );
    assert!(on_sae(), "{v}");
    assert!(!asking(&v), "{v}");
    assert!(
        profiles(&nm.c).split(',').any(|p| p == "Sae:hunter22"),
        "{}",
        profiles(&nm.c)
    );
}

/// Whether an activated connection is on the access point `ap`.
fn active_on_ap(c: &Connection, ap: &str) -> bool {
    const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
    let get = |path: &str, iface: &str, name: &str| {
        c.call_method(
            Some(NM),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(iface, name),
        )
        .ok()
        .and_then(|m| m.body().deserialize::<zbus::zvariant::OwnedValue>().ok())
    };
    let Some(list) = get(NM_PATH, NM, "ActiveConnections") else {
        return false;
    };
    let list = Vec::<zbus::zvariant::OwnedObjectPath>::try_from(list).unwrap_or_default();
    list.iter().any(|a| {
        let specific = get(a.as_str(), ACTIVE, "SpecificObject")
            .and_then(|v| zbus::zvariant::OwnedObjectPath::try_from(v).ok());
        let state = get(a.as_str(), ACTIVE, "State").and_then(|v| u32::try_from(v).ok());
        specific.is_some_and(|p| p.as_str() == ap) && state == Some(2)
    })
}

fn nets_active(v: &serde_json::Value) -> Vec<(String, bool)> {
    v["nets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| (n[1].as_str().unwrap_or("").to_string(), n[4] == true))
        .collect()
}

fn path_value(p: &str) -> zbus::zvariant::Value<'static> {
    zbus::zvariant::ObjectPath::try_from(p.to_string())
        .unwrap()
        .into()
}

/// NetworkManager roams to the other BSSID of the same network: the in-use entry, `active`
/// and the bar icon must follow ActiveAccessPoint.
#[test]
fn native_list_follows_roam() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(module_path!(), "::native_list_follows_roam")) else {
        return;
    };
    let nm = start_nm();
    let (mut rt, probe, wake) = native_probe();
    let v = act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["active"] == "11:22:33:44:55:01" && v["strength"] == 70
    });
    eprintln!("before: {v}");
    set(
        &nm.c,
        &nm.wifi,
        WL,
        "ActiveAccessPoint",
        path_value(&nm.aps[1]),
    );
    set(
        &nm.c,
        "/org/freedesktop/NetworkManager/ActiveConnection/Home",
        "org.freedesktop.NetworkManager.Connection.Active",
        "SpecificObject",
        path_value(&nm.aps[1]),
    );
    // The whole roam: the in-use entry, networkStrength from the new in-use AP (90) and the
    // icon that follows it.
    let v = act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["active"] == "11:22:33:44:55:02"
            && v["strength"] == 90
            && v["symbol"] == "signal_wifi_4_bar"
    });
    eprintln!("after roam: {v}");
    let home = net(&v, "Home:Net").unwrap();
    assert_eq!(
        (&home[1], &home[4]),
        (&"11:22:33:44:55:02".into(), &true.into()),
        "{v}"
    );
}

/// NetworkManager connects on its own after the shell started (autoconnect at login): the
/// network must show as in use and the bar icon must show its signal.
#[test]
fn native_list_follows_autoconnect() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(module_path!(), "::native_list_follows_autoconnect"))
    else {
        return;
    };
    let nm = start_nm();
    let _ = mock!(nm.c, NM_PATH, "SetDeviceDisconnected", &(nm.wifi.as_str(),));
    set(&nm.c, &nm.wifi, WL, "ActiveAccessPoint", path_value("/"));
    let (mut rt, probe, wake) = native_probe();
    let v = act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["status"] == "disconnected" && v["nets"].as_array().is_some_and(|n| !n.is_empty())
    });
    eprintln!("disconnected: {v}");
    let _ = mock!(
        nm.c,
        NM_PATH,
        "AddActiveConnection",
        &(
            vec![nm.wifi.as_str()],
            "/org/freedesktop/NetworkManager/Settings/HomeConn",
            nm.aps[0].as_str(),
            "Home2",
            2u32
        )
    );
    set(
        &nm.c,
        &nm.wifi,
        WL,
        "ActiveAccessPoint",
        path_value(&nm.aps[0]),
    );
    // NetworkManager's signals arrive one by one: wait for the list that follows them all.
    let v = act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["status"] == "connected" && v["strength"] == 70 && !v["active"].is_null()
    });
    eprintln!("autoconnected: {v}");
    assert_eq!(
        (&v["active"], &v["symbol"]),
        (&"11:22:33:44:55:01".into(), &"network_wifi".into()),
        "{v}; nets {:?}",
        nets_active(&v)
    );
}

/// The connection drops (out of range, another client took it down): the network must no
/// longer show as in use, and networkName must not name it.
#[test]
fn native_list_and_name_follow_external_disconnect() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(
        module_path!(),
        "::native_list_and_name_follow_external_disconnect"
    )) else {
        return;
    };
    let nm = start_nm();
    let (mut rt, probe, wake) = native_probe();
    act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["active"] == "11:22:33:44:55:01"
    });
    // The access point went away or another client disconnected: NetworkManager drops the
    // active connection.
    let ac =
        nm.c.call_method(
            Some(NM),
            nm.wifi.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(DEV, "ActiveConnection"),
        )
        .unwrap();
    let ac: zbus::zvariant::OwnedValue = ac.body().deserialize().unwrap();
    let ac = zbus::zvariant::OwnedObjectPath::try_from(ac).unwrap();
    let _ = mock!(
        nm.c,
        NM_PATH,
        "RemoveActiveConnection",
        &(nm.wifi.as_str(), ac.as_str())
    );
    // NetworkManager clears the access point with the activation; the mock leaves it.
    set(&nm.c, &nm.wifi, WL, "ActiveAccessPoint", path_value("/"));
    let v = act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["status"] == "disconnected" && v["active"].is_null()
    });
    assert_eq!(
        (&v["active"], &v["name"], &v["strength"]),
        (&serde_json::Value::Null, &"".into(), &0.into()),
        "{v}"
    );
}

/// The password typed into network A's prompt must retry A, not whatever was clicked last.
#[test]
fn native_password_retries_its_own_network() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(
        module_path!(),
        "::native_password_retries_its_own_network"
    )) else {
        return;
    };
    let nm = start_nm();
    add_update2(&nm.c);
    let (mut rt, probe, wake) = native_probe();
    act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["nets"].as_array().is_some_and(|n| !n.is_empty())
    });
    // Join the open "Cafe" (a new profile): the last network clicked.
    let v = act_until(&mut rt, probe, &wake, ("connect", "Cafe", ""), &|v| {
        v["connecting"] == false
    });
    eprintln!("after cafe: {v}");
    let calls = || {
        method_calls(&nm.c, NM_PATH, "ActivateConnection").len()
            + method_calls(&nm.c, NM_PATH, "AddAndActivateConnection").len()
    };
    let before_all = calls();
    let before = method_calls(&nm.c, NM_PATH, "ActivateConnection").len();
    // Now the password for the saved "Home:Net" profile (its prompt was open).
    let v = act_until(
        &mut rt,
        probe,
        &wake,
        ("password", "Home:Net", "hunter22"),
        &|v| v["connecting"] == false && calls() > before_all,
    );
    eprintln!("after password: {v}");
    eprintln!(
        "AddAndActivate calls: {:?}",
        method_calls(&nm.c, NM_PATH, "AddAndActivateConnection").len()
    );
    let calls = method_calls(&nm.c, NM_PATH, "ActivateConnection");
    assert!(
        calls.len() > before,
        "no ActivateConnection after the password: a new profile was added instead"
    );
    let conn =
        zbus::zvariant::OwnedObjectPath::try_from(calls[before][0].try_clone().unwrap()).unwrap();
    assert_eq!(
        conn.as_str(),
        "/org/freedesktop/NetworkManager/Settings/HomeConn",
        "the retry after Home:Net's password activated another profile; profiles {}",
        profiles(&nm.c)
    );
}

/// Opening the original's Wi-Fi dialog with the radio off runs `enableWifi(); rescanWifi()`: the
/// RequestScan reaches a device that is still unavailable and is refused. The scan still
/// happens once the device can scan (the original's `nmcli --rescan yes` asked again), and the list is not
/// reported as scanned before that.
#[test]
fn native_rescan_right_after_radio_on_waits_for_the_device() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(
        module_path!(),
        "::native_rescan_right_after_radio_on_waits_for_the_device"
    )) else {
        return;
    };
    let nm = start_nm();
    let code = format!(
        "import time\n\
         if self.Get('{DEV}', 'State') < 30:\n    \
             raise dbus.exceptions.DBusException('Scanning not allowed while unavailable', name='org.freedesktop.NetworkManager.Device.NotAllowed')\n\
         self.UpdateProperties('{WL}', {{'LastScan': dbus.Int64(int(time.clock_gettime(time.CLOCK_BOOTTIME) * 1000))}})"
    );
    let _ = mock!(
        nm.c,
        nm.wifi.as_str(),
        "AddMethod",
        &(WL, "RequestScan", "a{sv}", "", code.as_str())
    );
    let _ = mock!(nm.c, NM_PATH, "SetDeviceDisconnected", &(nm.wifi.as_str(),));
    set(&nm.c, &nm.wifi, WL, "ActiveAccessPoint", path_value("/"));
    set(&nm.c, NM_PATH, NM, "WirelessEnabled", false.into());
    set(&nm.c, &nm.wifi, DEV, "State", 20u32.into());
    // dbusmock signals every call it takes (MethodCalled) before running it.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(MOCK)
        .unwrap()
        .member("MethodCalled")
        .unwrap()
        .path(nm.wifi.as_str())
        .unwrap()
        .build();
    let mut called = zbus::blocking::MessageIterator::for_match_rule(rule, &nm.c, None).unwrap();
    let (mut rt, probe, wake) = native_probe();
    act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["status"] == "disabled"
    });
    // SidebarRightContent's showWifiDialog handler.
    let args = ["enable", "", "on"].map(Value::str).to_vec();
    rt.call_method(probe, "act", args).unwrap().unwrap();
    act_until(&mut rt, probe, &wake, ("rescan", "", ""), &|v| {
        v["scanning"] == true
    });
    loop {
        let m = called.next().unwrap().unwrap();
        let (name, _) = m
            .body()
            .deserialize::<(String, Vec<zbus::zvariant::OwnedValue>)>()
            .unwrap();
        if name == "RequestScan" {
            break;
        }
    }
    // Refused while unavailable; the shell keeps scanning until the device can scan.
    rt.poll_native();
    let v: serde_json::Value = serde_json::from_str(&rt.get(probe, "snap").to_js_string()).unwrap();
    assert_eq!(v["scanning"], true, "{v}");
    // NetworkManager brings the device up after the radio switch.
    set(&nm.c, &nm.wifi, DEV, "State", 30u32.into());
    let v = act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["scanning"] == false
    });
    assert_eq!(v["status"], "disconnected", "{v}");
    assert_eq!(method_calls(&nm.c, &nm.wifi, "RequestScan").len(), 2, "{v}");
}

/// A Wi-Fi adapter NetworkManager does not manage refuses every scan (`NotAllowed` while
/// UNMANAGED): no scan of it is coming, so the rescan ends, and the next one asks again.
#[test]
fn native_rescan_of_an_unmanaged_device_ends() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(
        module_path!(),
        "::native_rescan_of_an_unmanaged_device_ends"
    )) else {
        return;
    };
    let nm = start_nm();
    let code = "raise dbus.exceptions.DBusException('Scanning not allowed while unmanaged', name='org.freedesktop.NetworkManager.Device.NotAllowed')";
    let _ = mock!(
        nm.c,
        nm.wifi.as_str(),
        "AddMethod",
        &(WL, "RequestScan", "a{sv}", "", code)
    );
    set(&nm.c, &nm.wifi, DEV, "State", 10u32.into());
    let (mut rt, probe, wake) = native_probe();
    act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["nets"].as_array().is_some_and(|n| !n.is_empty())
    });
    let scans = || method_calls(&nm.c, &nm.wifi, "RequestScan").len();
    for n in 1..=2 {
        let v = act_until(&mut rt, probe, &wake, ("rescan", "", ""), &|v| {
            v["scanning"] == false && scans() == n
        });
        eprintln!("after rescan {n}: {v}");
    }
}

/// An SSID is bytes (IEEE 802.11 gives them no encoding): one that is not UTF-8 shows with
/// replacement characters, and joining it activates that access point, found by its bytes.
#[test]
fn native_joins_a_network_whose_ssid_is_not_utf8() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(
        module_path!(),
        "::native_joins_a_network_whose_ssid_is_not_utf8"
    )) else {
        return;
    };
    let nm = start_nm();
    let bssid = "11:22:33:44:55:20";
    let ap = mock!(
        nm.c,
        NM_PATH,
        "AddAccessPoint",
        &(
            nm.wifi.as_str(),
            "AP20",
            "cafe",
            bssid,
            2u32,
            2412u32,
            54000u32,
            50u8,
            0u32
        )
    );
    // Latin-1 "café".
    let latin1 = zbus::zvariant::Value::from(b"caf\xe9".to_vec());
    set(&nm.c, &ap, AP, "Ssid", latin1);
    let (mut rt, probe, wake) = native_probe();
    let v = act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        net(v, "caf\u{fffd}").is_some_and(|a| a[1] == bssid)
    });
    eprintln!("before: {v}");
    let v = act_until(&mut rt, probe, &wake, ("connect", bssid, ""), &|v| {
        v["connecting"] == false
    });
    eprintln!("after connect: {v}");
    let calls = method_calls(&nm.c, NM_PATH, "AddAndActivateConnection");
    assert_eq!(calls.len(), 1, "{v}");
    let specific =
        zbus::zvariant::OwnedObjectPath::try_from(calls[0][2].try_clone().unwrap()).unwrap();
    assert_eq!(specific.as_str(), ap, "{v}");
}

/// A profile the shell made for a network it is joining is deleted when NetworkManager restarts
/// before the activation ends: found again by its UUID (its object path changes with the daemon),
/// and only that one.
#[test]
fn native_profile_of_an_activation_lost_with_networkmanager_is_deleted() {
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    let Some(_bus) = private_bus(concat!(
        module_path!(),
        "::native_profile_of_an_activation_lost_with_networkmanager_is_deleted"
    )) else {
        return;
    };
    let nm = start_nm();
    // The activation stays pending (ACTIVATING, the device preparing) until the daemon goes.
    let code = r#"
import dbus, dbusmock
import dbusmock.templates.networkmanager as t
ssid = bytes(dbusmock.get_object(args[2]).Get(t.ACCESS_POINT_IFACE, 'Ssid')).decode()
# NetworkManager completes the partial profile it is given.
args[0]['connection'].setdefault('id', ssid)
args[0]['connection'].setdefault('type', '802-11-wireless')
settings = dbusmock.get_object(t.SETTINGS_OBJ)
settings.SettingsAddConnection(args[0])
conn = dbus.ObjectPath(settings.ListConnections()[-1])
ac = dbus.ObjectPath(t.AddActiveConnection(self, [args[1]], conn, args[2], 'Pending', dbus.UInt32(1)))
d = dbusmock.get_object(args[1])
d.Set(t.DEVICE_IFACE, 'State', dbus.UInt32(40))
d.EmitSignal(t.DEVICE_IFACE, 'StateChanged', 'uuu', [dbus.UInt32(40), dbus.UInt32(100), dbus.UInt32(0)])
ret = (conn, ac)
"#;
    let _ = mock!(
        nm.c,
        NM_PATH,
        "AddMethod",
        &(NM, "AddAndActivateConnection", "a{sa{sv}}oo", "oo", code)
    );
    let (mut rt, probe, wake) = native_probe();
    act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["nets"].as_array().is_some_and(|n| !n.is_empty())
    });
    act_until(&mut rt, probe, &wake, ("connect", "Zeta", ""), &|v| {
        v["connecting"] == true
            && !method_calls(&nm.c, NM_PATH, "AddAndActivateConnection").is_empty()
    });
    let calls = method_calls(&nm.c, NM_PATH, "AddAndActivateConnection");
    type S = HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>;
    let st = S::try_from(calls[0][0].try_clone().unwrap()).unwrap();
    let uuid = st["connection"]
        .get("uuid")
        .and_then(|v| <&str>::try_from(&**v).ok())
        .map(str::to_string)
        .expect("the new profile carries a UUID");
    let pending =
        nm.c.call_method(
            Some(NM),
            NM_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(NM, "ActiveConnections"),
        )
        .unwrap();
    let pending: zbus::zvariant::OwnedValue = pending.body().deserialize().unwrap();
    assert_eq!(
        <Vec<zbus::zvariant::OwnedObjectPath>>::try_from(pending)
            .unwrap()
            .len(),
        2,
        "the activation never started"
    );

    // dbusmock signals every call it takes (MethodCalled) before running it.
    let watch = Connection::system().unwrap();
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(MOCK)
        .unwrap()
        .member("MethodCalled")
        .unwrap()
        .build();
    let mut called = zbus::blocking::MessageIterator::for_match_rule(rule, &watch, None).unwrap();
    drop(nm);
    act_until(&mut rt, probe, &wake, ("", "", ""), &|v| {
        v["connecting"] == false
    });

    let template = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/nm_saved_profile.py");
    let params = serde_json::json!({ "uuid": uuid, "ssid": "Zeta" }).to_string();
    let nm = start_nm_with(&["--template", template, "--parameters", &params]);
    let deleted = loop {
        let m = called.next().unwrap().unwrap();
        let (name, _) = m
            .body()
            .deserialize::<(String, Vec<zbus::zvariant::OwnedValue>)>()
            .unwrap();
        if name == "Delete" {
            break m.header().path().unwrap().to_string();
        }
    };
    assert!(
        deleted.starts_with("/org/freedesktop/NetworkManager/Settings/"),
        "{deleted}"
    );
    let found = nm.c.call_method(
        Some(NM),
        "/org/freedesktop/NetworkManager/Settings",
        Some("org.freedesktop.NetworkManager.Settings"),
        "GetConnectionByUuid",
        &(uuid.as_str(),),
    );
    assert!(found.is_err(), "the profile is still there");
    assert!(profiles(&nm.c).contains("Home:Net"), "{}", profiles(&nm.c));
}

/// The waffle Wi-Fi page's row of the WPA3 network "Sae", over the tree's own files in a home
/// of its own: `snap` as [`PROBE`]'s, for [`act_until`].
const WAFFLE_ROW: &str = r#"
import QtQuick
import Impulse
import ri.modules.waffle.actionCenter.wifi
Item {
    width: 400
    height: 400
    property string snap: JSON.stringify({
        connecting: Network.wifiConnecting,
        nets: Network.wifiNetworks.map(a => [a.ssid, a.askingPassword])
    })
    Repeater {
        model: Network.wifiNetworks.filter(a => a.ssid === "Sae")
        WWifiNetworkItem {
            required property WifiAccessPoint modelData
            wifiNetwork: modelData
            width: 400
        }
    }
}
"#;

/// A secured network is joined from the waffle Wi-Fi page: its Connect button asks for the
/// password in the row itself (a masked field), and Connect there sends what was typed to
/// NetworkManager.
#[test]
fn waffle_wifi_row_asks_for_the_password_and_joins() {
    const TEST: &str = concat!(
        module_path!(),
        "::waffle_wifi_row_asks_for_the_password_and_joins"
    );
    if !available() {
        testenv::unavailable("needs dbus-daemon and python3-dbusmock");
        return;
    }
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::Builder::new()
            .prefix("waffle-wifi-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .unwrap();
        let bus = |name: &str| format!("unix:path={}", dir.path().join(name).display()).into();
        let mut vars = testenv::home_vars(dir.path());
        vars.push(("DBUS_SYSTEM_BUS_ADDRESS", bus("bus")));
        vars.push(("DBUS_SESSION_BUS_ADDRESS", bus("no-bus")));
        testenv::run_in_own_process_as_root(TEST, vars);
        return;
    }
    let _bus = private_bus(TEST).unwrap();
    let nm = start_nm();

    // The row beside the tree's shell.qml, as the files that use it are.
    let d = PathBuf::from(std::env::var_os("HOME").unwrap()).join("tree");
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(ri_root()).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("zzmain.qml"), WAFFLE_ROW).unwrap();
    // Midday for the engine: the original's night light schedule (Hyprsunset, loaded with the services)
    // decides nothing here from the hour the test runs at.
    qml::builtins::set_clock(Some(testenv::local_time_ms(12, 0)));
    let mut rt = Runtime::new(Some(d.clone()));
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    rt.load_singletons(&[
        d.clone(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
        d.join("modules/waffle/looks"),
    ]);
    rt.settle();
    let root = rt.load_file(&d.join("zzmain.qml")).unwrap();
    rt.settle();

    let asking = |v: &serde_json::Value| {
        v["nets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n[0] == "Sae" && n[1] == true)
    };
    let shown = |rt: &Runtime, ty: &str| -> Vec<usize> {
        let all = rt.world.borrow_mut().of_type(ty);
        // Shown: the item and every item it is in.
        let visible = |mut i: usize| loop {
            if !rt.get(i, "visible").truthy() {
                return false;
            }
            match rt.get(i, "parent") {
                Value::Item(p) => i = p,
                _ => return true,
            }
        };
        all.into_iter().filter(|&i| visible(i)).collect()
    };
    let connect = |rt: &Runtime| -> Vec<usize> {
        shown(rt, "WButton")
            .into_iter()
            .filter(|&b| rt.get(b, "text").to_js_string() == "Connect")
            .collect()
    };
    let click = |rt: &mut Runtime, item: usize| {
        rt.emit(item, "clicked", &[], vec![]);
        rt.settle();
    };

    act_until(&mut rt, root, &wake, ("", "", ""), &|v| {
        v["nets"].as_array().is_some_and(|n| !n.is_empty())
    });
    let rows = shown(&rt, "WWifiNetworkItem");
    assert_eq!(rows.len(), 1, "the row of Sae");
    click(&mut rt, rows[0]);
    let buttons = connect(&rt);
    assert_eq!(buttons.len(), 1, "the expanded row's Connect");
    assert_eq!(shown(&rt, "WTextField").len(), 0, "asked before Connect");
    click(&mut rt, buttons[0]);
    let v = act_until(&mut rt, root, &wake, ("", "", ""), &asking);
    assert_eq!(v["connecting"], false, "{v}");
    assert!(method_calls(&nm.c, NM_PATH, "AddAndActivateConnection").is_empty());

    let fields = shown(&rt, "WTextField");
    assert_eq!(fields.len(), 1, "the password field");
    // TextInput.Password
    assert_eq!(rt.get(fields[0], "echoMode").to_number(), 2.0);
    rt.set(fields[0], "text", Value::str("hunter22"));
    rt.settle();
    let buttons = connect(&rt);
    assert_eq!(buttons.len(), 1, "the prompt's Connect");
    click(&mut rt, buttons[0]);
    let sae = nm.aps[6].clone();
    let v = act_until(&mut rt, root, &wake, ("", "", ""), &|v| {
        active_on_ap(&nm.c, &sae) && v["connecting"] == false
    });
    assert!(!asking(&v), "{v}");
    let calls = method_calls(&nm.c, NM_PATH, "AddAndActivateConnection");
    assert_eq!(calls.len(), 1);
    type S = HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>;
    let st = S::try_from(calls[0][0].try_clone().unwrap()).unwrap();
    let psk = st
        .get("802-11-wireless-security")
        .and_then(|g| g.get("psk"))
        .and_then(|v| <&str>::try_from(&**v).ok());
    assert_eq!(psk, Some("hunter22"));
    assert_eq!(
        shown(&rt, "WTextField").len(),
        0,
        "still asking once joined"
    );
    assert_eq!(rt.errors(), Vec::<String>::new());
}
