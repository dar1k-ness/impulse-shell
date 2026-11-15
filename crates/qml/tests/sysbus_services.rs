//! UPower / PowerProfiles / Bluetooth native services against fake daemons (tests/fake_sysbus.py)
//! on a private `dbus-daemon` used as the system (and session) bus. Fails when dbus-daemon or python3 with
//! dbus-python + gi is unavailable (`testenv`).

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// The test's host loop wakeups (one loop per test process): pinged by the runtime and by every
/// line a fake prints.
fn wake() -> &'static testenv::Wakeups {
    static WAKE: OnceLock<testenv::Wakeups> = OnceLock::new();
    WAKE.get_or_init(testenv::Wakeups::new)
}

/// Runs what is queued for the loop now, polling again while a poll's own writes changed the
/// world (they count for the next poll), as the shell's loop does.
fn turn(rt: &mut Runtime) {
    loop {
        let rev = rt.revision();
        rt.poll_native();
        rt.settle();
        if rt.revision() == rev {
            return;
        }
    }
}

/// Runs loop turns, sleeping on the wakeups between them, until `done`.
fn until(rt: &mut Runtime, what: &str, mut done: impl FnMut(&mut Runtime) -> bool) {
    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for {what}");
    loop {
        turn(rt);
        let rev = rt.revision();
        if done(rt) {
            return;
        }
        // A call in `done` can create objects (a service singleton) that only the next poll
        // starts: turn again before sleeping.
        if rt.revision() != rev {
            continue;
        }
        wake().wait(None, None);
    }
}

/// The runtime's connection to the bus a fake of `mode` is on.
fn runtime_bus(mode: &str) -> qml::dbus::Bus {
    if mode == "notifd" {
        qml::dbus::Bus::Session
    } else {
        qml::dbus::Bus::System
    }
}

/// The well-known name a fake of `mode` serves its objects under.
fn fake_name(mode: &str) -> &'static str {
    match mode {
        "upower" => "org.freedesktop.UPower",
        "ppd" => "org.freedesktop.UPower.PowerProfiles",
        "ppd-legacy" => "net.hadess.PowerProfiles",
        "bluez" => "org.bluez",
        "nm" => "org.freedesktop.NetworkManager",
        "notifd" => "org.freedesktop.Notifications",
        m => panic!("no name for fake {m}"),
    }
}

fn python() -> Option<&'static str> {
    ["/usr/bin/python3", "python3"].into_iter().find(|p| {
        Command::new(p)
            .args([
                "-c",
                "import dbus, dbus.mainloop.glib; from gi.repository import GLib",
            ])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

struct Bus {
    /// Closing it (test process exit) makes the wrapper shell kill the daemon.
    _keepalive: ChildStdin,
    python: &'static str,
}

/// In `test`'s own process (see `testenv::run_in_own_process`): a private bus at the address
/// the environment names as both the system and the session bus (a socket in a directory the
/// parent made), as the services find their buses through the environment. `None` in the
/// process that ran the test there, or without dbus-daemon or python3 with dbus-python + gi.
fn bus(test: &str) -> Option<Bus> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(
            test,
            [
                ("DBUS_SYSTEM_BUS_ADDRESS", &addr),
                ("DBUS_SESSION_BUS_ADDRESS", &addr),
            ],
        );
        return None;
    }
    let Some(python) = python() else {
        testenv::unavailable("python3 with dbus-python and gi not found");
        return None;
    };
    let daemon = Command::new("sh")
        .args([
            "-c",
            "command -v dbus-daemon >/dev/null || exit 1; \
             dbus-daemon --session --address=\"$0\" --nofork --print-address=1 & read _; kill $!",
        ])
        .arg(std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut daemon) = daemon else {
        testenv::unavailable("dbus-daemon not found");
        return None;
    };
    let mut line = String::new();
    let _ = BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut line);
    if line.trim().is_empty() {
        testenv::unavailable("dbus-daemon not available");
        return None;
    }
    Some(Bus {
        _keepalive: daemon.stdin.take().unwrap(),
        python,
    })
}

struct Fake {
    child: Child,
    stdin: ChildStdin,
    out: Receiver<String>,
    mode: &'static str,
}

impl Fake {
    fn start(bus: &Bus, mode: &'static str, init: &str) -> Fake {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake_sysbus.py");
        let mut child = Command::new(bus.python)
            .args([script, mode, init])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn fake");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, out) = channel();
        let ping = wake().ping();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    return;
                }
                ping();
            }
        });
        let f = Fake {
            child,
            stdin,
            out,
            mode,
        };
        f.expect("READY");
        f
    }

    /// Wait for an output line containing `s`; returns it.
    fn expect(&self, s: &str) -> String {
        eprintln!("waiting for the fake to print {s:?}");
        loop {
            match self.out.recv() {
                Ok(l) if l.contains(s) => return l,
                Ok(_) => {}
                Err(_) => panic!("fake exited without printing {s:?}"),
            }
        }
    }

    fn cmd(&mut self, json: &str) {
        writeln!(self.stdin, "{json}").unwrap();
        self.stdin.flush().unwrap();
        self.expect("DONE");
    }

    /// The daemon disappearing (bluetoothd/upowerd crash or restart).
    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// No line containing `s` for anything the runtime has done so far: once every signal the
    /// buses sent the runtime is handed to its services and the loop has turned, a `Mark` call
    /// from the runtime's connection to the fake, run as a task on that connection's executor
    /// behind the calls and replies the runtime queued before it, arrives after all of them.
    fn quiet(&self, rt: &mut Runtime, s: &str) {
        for b in [qml::dbus::Bus::Session, qml::dbus::Bus::System] {
            assert!(qml::dbus::sync(b), "{b:?} bus gone");
        }
        turn(rt);
        let c = qml::dbus::connection(runtime_bus(self.mode))
            .unwrap()
            .inner()
            .clone();
        let (on, dest) = (c.clone(), fake_name(self.mode));
        let task = c.executor().spawn(
            async move {
                on.call_method(
                    Some(dest),
                    "/org/test/Mark",
                    Some("org.test.Mark"),
                    "Mark",
                    &(),
                )
                .await
                .map(|_| ())
            },
            "test mark",
        );
        zbus::block_on(task).expect("mark task").expect("Mark");
        loop {
            match self.out.recv() {
                Ok(l) if l == "MARK" => return,
                Ok(l) => assert!(!l.contains(s), "unexpected {l:?}"),
                Err(_) => panic!("fake exited before MARK"),
            }
        }
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn load(src: &str) -> (Runtime, usize) {
    let mut rt = Runtime::new(None);
    rt.world.borrow().waker.set_ping(wake().ping());
    let r = rt.load_str(src).unwrap();
    rt.settle();
    (rt, r)
}

fn val(rt: &Runtime, r: usize, p: &str) -> String {
    rt.get(r, p).to_js_string()
}

/// Run the loop until `prop` reads `want`.
fn wait(rt: &mut Runtime, r: usize, prop: &str, want: &str) {
    until(rt, &format!("{prop} = {want:?}"), |rt| {
        val(rt, r, prop) == want
    });
}

/// Each new owner of `name` on the system bus ("" once nobody owns it), from a match rule added
/// before this returns, on a connection of the test's own.
fn owners(name: &'static str) -> Receiver<String> {
    let c = zbus::blocking::Connection::system().unwrap();
    let it = zbus::blocking::fdo::DBusProxy::new(&c)
        .unwrap()
        .receive_name_owner_changed_with_args(&[(0, name)])
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
        }
    });
    rx
}

/// Kills the fake and returns once the runtime has handled its name losing its owner: the bus
/// announced it (`owners`, watched from before), the runtime's router has handed every signal
/// up to then to its services (`dbus::sync`), and the loop has turned.
fn kill_and_settle(rt: &mut Runtime, f: Fake, owners: &Receiver<String>) {
    f.kill();
    eprintln!("waiting for the fake's name to lose its owner");
    while !owners.recv().expect("name watch ended").is_empty() {}
    assert!(qml::dbus::sync(qml::dbus::Bus::System));
    turn(rt);
}

fn set(rt: &mut Runtime, r: usize, prop: &str, v: f64) {
    rt.world.borrow_mut().write(r, prop, Value::Num(v));
    rt.settle();
    // The poll after a QML write is what forwards it to the daemon (as Qt's setter would).
    rt.poll_native();
}

const BAT: &str = "/org/freedesktop/UPower/devices/battery_BAT0";
const AC: &str = "/org/freedesktop/UPower/devices/line_power_AC";
const DISPLAY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";

fn upower_init(pct: f64) -> String {
    format!(
        r#"{{"display": "{DISPLAY}", "root": {{"OnBattery": true, "DaemonVersion": "1.90"}},
        "objects": {{
          "{DISPLAY}": {{"org.freedesktop.UPower.Device": {{"Type": 2, "PowerSupply": true,
             "Percentage": {pct}, "State": 2, "Energy": 30.5, "EnergyFull": 61.0, "EnergyRate": 7.25,
             "TimeToEmpty": 15000, "TimeToFull": 0, "IsPresent": true, "IconName": "battery-good-symbolic"}}}},
          "{BAT}": {{"org.freedesktop.UPower.Device": {{"Type": 2, "PowerSupply": true, "Percentage": {pct},
             "State": 2, "Capacity": 87.5, "NativePath": "BAT0", "Model": "5B10W51867"}}}},
          "{AC}": {{"org.freedesktop.UPower.Device": {{"Type": 1, "PowerSupply": true, "Online": false,
             "NativePath": "AC", "Model": ""}}}}
        }}}}"#
    )
}

#[test]
fn upower_devices_changes_hotplug_restart() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::upower_devices_changes_hotplug_restart"
    )) else {
        return;
    };
    let bus = &g;
    // The singleton exists (and displayDevice is non-null, not ready) before UPower appears.
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Services.UPower\nItem {\n\
         property bool hasDd: UPower.displayDevice !== null\n\
         property bool ready: UPower.displayDevice.ready\n\
         property real pct: UPower.displayDevice.percentage\n\
         property int st: UPower.displayDevice.state\n\
         property bool charging: UPower.displayDevice.state == UPowerDeviceState.Charging\n\
         property string stName: UPowerDeviceState.toString(UPower.displayDevice.state)\n\
         property string tyName: UPowerDeviceType.toString(UPower.displayDevice.type)\n\
         property bool lap: UPower.displayDevice.isLaptopBattery\n\
         property real energy: UPower.displayDevice.energy\n\
         property real cap: UPower.displayDevice.energyCapacity\n\
         property real rate: UPower.displayDevice.changeRate\n\
         property real tte: UPower.displayDevice.timeToEmpty\n\
         property string icon: UPower.displayDevice.iconName\n\
         property bool onBat: UPower.onBattery\n\
         property int n: UPower.devices.values.length\n\
         property string devs: UPower.devices.values.map(d => [d.nativePath, d.type, d.ready, d.isLaptopBattery, d.healthPercentage, d.healthSupported, d.model, d.percentage].join(':')).join(',')\n}",
    );
    turn(&mut rt);
    assert_eq!(val(&rt, r, "hasDd"), "true");
    assert_eq!(val(&rt, r, "ready"), "false");
    assert_eq!(val(&rt, r, "n"), "0");

    // Initial state.
    let mut f = Fake::start(bus, "upower", &upower_init(50.0));
    wait(&mut rt, r, "ready", "true");
    wait(&mut rt, r, "n", "2");
    assert_eq!(val(&rt, r, "pct"), "0.5");
    assert_eq!(val(&rt, r, "st"), "2");
    assert_eq!(val(&rt, r, "stName"), "Discharging");
    assert_eq!(val(&rt, r, "tyName"), "Battery");
    assert_eq!(val(&rt, r, "lap"), "true");
    assert_eq!(val(&rt, r, "energy"), "30.5");
    assert_eq!(val(&rt, r, "cap"), "61");
    assert_eq!(val(&rt, r, "rate"), "7.25");
    assert_eq!(val(&rt, r, "tte"), "15000");
    assert_eq!(val(&rt, r, "icon"), "battery-good-symbolic");
    assert_eq!(val(&rt, r, "onBat"), "true");
    assert_eq!(
        val(&rt, r, "devs"),
        "BAT0:2:true:true:87.5:true:5B10W51867:0.5,AC:1:true:false:0:false::0"
    );

    // Property changes (PropertiesChanged, invalidation, out-of-range enum ignored).
    f.cmd(&format!(
        r#"{{"op":"set","path":"{DISPLAY}","iface":"org.freedesktop.UPower.Device","props":{{"Percentage":42.0,"State":1}}}}"#
    ));
    wait(&mut rt, r, "pct", "0.42");
    wait(&mut rt, r, "charging", "true");
    assert_eq!(val(&rt, r, "stName"), "Charging");
    f.cmd(&format!(
        r#"{{"op":"set","path":"{DISPLAY}","iface":"org.freedesktop.UPower.Device","props":{{"State":99,"Energy":25.0}}}}"#
    ));
    wait(&mut rt, r, "energy", "25");
    assert_eq!(val(&rt, r, "st"), "1");
    f.cmd(&format!(
        r#"{{"op":"invalidate","path":"{DISPLAY}","iface":"org.freedesktop.UPower.Device","props":{{"TimeToEmpty":1234,"Energy":30.0}}}}"#
    ));
    wait(&mut rt, r, "tte", "1234");
    wait(&mut rt, r, "energy", "30");
    // Both invalidated properties come from one GetAll, not one Get each.
    f.expect(&format!("GETALL {DISPLAY} org.freedesktop.UPower.Device"));
    f.quiet(&mut rt, "GET");
    f.cmd(r#"{"op":"set","path":"/org/freedesktop/UPower","iface":"org.freedesktop.UPower","props":{"OnBattery":false}}"#);
    wait(&mut rt, r, "onBat", "false");

    // Hotplug.
    let mouse = "/org/freedesktop/UPower/devices/mouse_hidpp_battery_0";
    f.cmd(&format!(
        r#"{{"op":"add","path":"{mouse}","ifaces":{{"org.freedesktop.UPower.Device":{{"Type":5,"PowerSupply":false,"Percentage":25.0,"NativePath":"hidpp_battery_0","Model":"MX Master 3"}}}}}}"#
    ));
    wait(&mut rt, r, "n", "3");
    assert!(
        val(&rt, r, "devs").ends_with(",hidpp_battery_0:5:true:false:0:false:MX Master 3:0.25")
    );
    f.cmd(&format!(r#"{{"op":"remove","path":"{AC}"}}"#));
    wait(&mut rt, r, "n", "2");
    assert!(!val(&rt, r, "devs").contains("AC:"));

    // UPower disappears: devices go, the display device stays (never null), nothing crashes.
    f.kill();
    wait(&mut rt, r, "n", "0");
    assert_eq!(val(&rt, r, "hasDd"), "true");
    // ... and comes back with new state.
    let _f = Fake::start(bus, "upower", &upower_init(80.0));
    wait(&mut rt, r, "pct", "0.8");
    wait(&mut rt, r, "n", "2");
    wait(&mut rt, r, "onBat", "true");
    assert_eq!(val(&rt, r, "st"), "2");
}

const PPD_INIT: &str = r#"{"objects": {"/org/freedesktop/UPower/PowerProfiles": {"org.freedesktop.UPower.PowerProfiles": {
  "ActiveProfile": "balanced", "PerformanceDegraded": "lap-detected",
  "Profiles": [{"Profile": "power-saver", "Driver": "amd_pstate"}, {"Profile": "balanced", "Driver": "amd_pstate"}, {"Profile": "performance", "Driver": "amd_pstate"}],
  "ActiveProfileHolds": [{"Profile": "performance", "ApplicationId": "org.game", "Reason": "playing"}]}}}}"#;
const PPD_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const PPD_IFACE: &str = "org.freedesktop.UPower.PowerProfiles";

fn ppd_qml() -> (Runtime, usize) {
    load(
        "import QtQuick\nimport Rustshell.Services.UPower\nItem { id: r\n\
         property int prof: PowerProfiles.profile\n\
         property bool perf: PowerProfiles.hasPerformanceProfile\n\
         property int deg: PowerProfiles.degradationReason\n\
         property string degName: PerformanceDegradationReason.toString(PowerProfiles.degradationReason)\n\
         property string holds: PowerProfiles.holds.map(h => h.profile + ':' + h.applicationId + ':' + h.reason).join(',')\n\
         property string pname: PowerProfile.toString(PowerProfiles.profile)\n\
         property int req: -1\n\
         onReqChanged: PowerProfiles.profile = req }",
    )
}

#[test]
fn power_profiles_state_writes_restart() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::power_profiles_state_writes_restart"
    )) else {
        return;
    };
    let bus = &g;
    let (mut rt, r) = ppd_qml();
    // Defaults before the daemon exists; a write is refused (reverts).
    turn(&mut rt);
    assert_eq!(val(&rt, r, "prof"), "1");
    set(&mut rt, r, "req", 0.0);
    wait(&mut rt, r, "prof", "1");

    let mut f = Fake::start(bus, "ppd", PPD_INIT);
    wait(&mut rt, r, "perf", "true");
    assert_eq!(val(&rt, r, "prof"), "1");
    assert_eq!(val(&rt, r, "pname"), "Balanced");
    assert_eq!(val(&rt, r, "deg"), "1");
    assert_eq!(val(&rt, r, "degName"), "LapDetected");
    assert_eq!(val(&rt, r, "holds"), "2:org.game:playing");

    // Write from QML → ActiveProfile set on the daemon (which drops the holds).
    set(&mut rt, r, "req", 2.0);
    assert!(f.expect("SET").contains("ActiveProfile \"performance\""));
    wait(&mut rt, r, "holds", "");
    assert_eq!(val(&rt, r, "prof"), "2");
    set(&mut rt, r, "req", 0.0);
    assert!(f.expect("SET").contains("\"power-saver\""));
    wait(&mut rt, r, "prof", "0");

    // Daemon-side changes.
    f.cmd(&format!(
        r#"{{"op":"set","path":"{PPD_PATH}","iface":"{PPD_IFACE}","props":{{"ActiveProfile":"balanced","PerformanceDegraded":"high-operating-temperature"}}}}"#
    ));
    wait(&mut rt, r, "prof", "1");
    wait(&mut rt, r, "deg", "2");
    // Invalid wire values are rejected.
    f.cmd(&format!(
        r#"{{"op":"set","path":"{PPD_PATH}","iface":"{PPD_IFACE}","props":{{"ActiveProfile":"turbo","PerformanceDegraded":""}}}}"#
    ));
    wait(&mut rt, r, "deg", "0");
    assert_eq!(val(&rt, r, "prof"), "1");
    // Without a performance profile, requesting it is refused locally.
    f.cmd(&format!(
        r#"{{"op":"set","path":"{PPD_PATH}","iface":"{PPD_IFACE}","props":{{"Profiles":[{{"Profile":"power-saver"}},{{"Profile":"balanced"}}]}}}}"#
    ));
    wait(&mut rt, r, "perf", "false");
    set(&mut rt, r, "req", 2.0);
    wait(&mut rt, r, "prof", "1");
    f.quiet(&mut rt, "SET");

    // Daemon gone: writes refused, last values kept; back: state resynced.
    let gone = owners(PPD_IFACE);
    kill_and_settle(&mut rt, f, &gone);
    set(&mut rt, r, "req", 0.0);
    wait(&mut rt, r, "prof", "1");
    let f = Fake::start(bus, "ppd", PPD_INIT);
    wait(&mut rt, r, "perf", "true");
    wait(&mut rt, r, "holds", "2:org.game:playing");
    set(&mut rt, r, "req", 2.0);
    assert!(f.expect("SET").contains("\"performance\""));
    wait(&mut rt, r, "prof", "2");
}

#[test]
fn power_profiles_legacy_bus_name() {
    let Some(g) = bus(concat!(module_path!(), "::power_profiles_legacy_bus_name")) else {
        return;
    };
    let bus = &g;
    let init = PPD_INIT
        .replace(
            "/org/freedesktop/UPower/PowerProfiles",
            "/net/hadess/PowerProfiles",
        )
        .replace(
            "org.freedesktop.UPower.PowerProfiles",
            "net.hadess.PowerProfiles",
        )
        .replace(
            "\"balanced\", \"PerformanceDegraded\"",
            "\"power-saver\", \"PerformanceDegraded\"",
        );
    let f = Fake::start(bus, "ppd-legacy", &init);
    let (mut rt, r) = ppd_qml();
    wait(&mut rt, r, "prof", "0");
    assert_eq!(val(&rt, r, "perf"), "true");
    set(&mut rt, r, "req", 1.0);
    assert!(
        f.expect("SET")
            .contains("/net/hadess/PowerProfiles ActiveProfile \"balanced\"")
    );
}

const HCI0: &str = "/org/bluez/hci0";
const DEV_A: &str = "/org/bluez/hci0/dev_11_22_33_44_55_66";
const DEV_B: &str = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF";

fn bluez_init() -> String {
    format!(
        r#"{{"objects": {{
      "{HCI0}": {{"org.bluez.Adapter1": {{"Address": "00:1A:7D:DA:71:13", "Alias": "arch", "Name": "arch",
          "Powered": true, "PowerState": "on", "Discoverable": false, "DiscoverableTimeout": 180,
          "Discovering": false, "Pairable": true, "PairableTimeout": 0}}}},
      "{DEV_A}": {{"org.bluez.Device1": {{"Address": "11:22:33:44:55:66", "Name": "WH-1000XM4", "Alias": "Headphones",
          "Icon": "audio-headset", "Paired": false, "Bonded": false, "Trusted": false, "Blocked": false,
          "Connected": false, "WakeAllowed": false, "Adapter": "{HCI0}"}}}},
      "{DEV_B}": {{"org.bluez.Device1": {{"Address": "AA:BB:CC:DD:EE:FF", "Name": "MX Keys", "Alias": "MX Keys",
          "Icon": "input-keyboard", "Paired": true, "Bonded": true, "Trusted": true, "Blocked": false,
          "Connected": true, "Adapter": "{HCI0}"}},
          "org.bluez.Battery1": {{"Percentage": 80}}}}
    }}}}"#
    )
}

#[test]
fn bluetooth_adapters_devices_writes_methods_restart() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::bluetooth_adapters_devices_writes_methods_restart"
    )) else {
        return;
    };
    let bus = &g;
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Bluetooth\nItem { id: r\n\
         property int na: Bluetooth.adapters.values.length\n\
         property int nd: Bluetooth.devices.values.length\n\
         property bool hasDefault: Bluetooth.defaultAdapter !== null\n\
         property string aid: Bluetooth.defaultAdapter?.adapterId ?? ''\n\
         property string adapter: Bluetooth.defaultAdapter ? [Bluetooth.defaultAdapter.name, Bluetooth.defaultAdapter.enabled, Bluetooth.defaultAdapter.state, Bluetooth.defaultAdapter.discovering, Bluetooth.defaultAdapter.discoverable, Bluetooth.defaultAdapter.discoverableTimeout, Bluetooth.defaultAdapter.pairable, Bluetooth.defaultAdapter.dbusPath, Bluetooth.defaultAdapter.devices.values.length].join(':') : ''\n\
         property var dev: Bluetooth.devices.values.find(d => d.address == '11:22:33:44:55:66') ?? null\n\
         property var kb: Bluetooth.devices.values.find(d => d.address == 'AA:BB:CC:DD:EE:FF') ?? null\n\
         property string a: dev ? [dev.name, dev.deviceName, dev.icon, dev.connected, dev.state, dev.paired, dev.bonded, dev.trusted, dev.pairing, dev.batteryAvailable, dev.adapter === Bluetooth.defaultAdapter].join(':') : ''\n\
         property string b: kb ? [kb.connected, BluetoothDeviceState.toString(kb.state), kb.batteryAvailable, kb.battery].join(':') : ''\n\
         property int dstate: dev?.state ?? -1\n\
         property bool dconn: dev?.connected ?? false\n\
         property string stName: BluetoothAdapterState.toString(Bluetooth.defaultAdapter?.state ?? 0)\n\
         property int step: 0\n\
         onStepChanged: {\n\
           if (step == 1) Bluetooth.defaultAdapter.enabled = false\n\
           else if (step == 2) Bluetooth.defaultAdapter.enabled = true\n\
           else if (step == 3) Bluetooth.defaultAdapter.discovering = true\n\
           else if (step == 4) dev.connected = true\n\
           else if (step == 5) dev.disconnect()\n\
           else if (step == 6) dev.connect()\n\
           else if (step == 7) dev.trusted = true\n\
           else if (step == 8) dev.pair()\n\
           else if (step == 9) dev.name = 'My cans'\n\
           else if (step == 10) dev.forget()\n\
         } }",
    );
    turn(&mut rt);
    assert_eq!(val(&rt, r, "na"), "0");
    assert_eq!(val(&rt, r, "hasDefault"), "false");

    // Initial state.
    let mut f = Fake::start(bus, "bluez", &bluez_init());
    wait(&mut rt, r, "nd", "2");
    assert_eq!(val(&rt, r, "na"), "1");
    assert_eq!(val(&rt, r, "aid"), "hci0");
    assert_eq!(
        val(&rt, r, "adapter"),
        "arch:true:1:false:false:180:true:/org/bluez/hci0:2"
    );
    assert_eq!(val(&rt, r, "stName"), "Enabled");
    assert_eq!(
        val(&rt, r, "a"),
        "Headphones:WH-1000XM4:audio-headset:false:0:false:false:false:false:false:true"
    );
    assert_eq!(val(&rt, r, "b"), "true:Connected:true:0.8");

    // Adapter writes: enabled is optimistic + Powered set; discovering calls StartDiscovery.
    set(&mut rt, r, "step", 1.0);
    assert!(f.expect("SET").contains("Powered false"));
    wait(&mut rt, r, "stName", "Disabled");
    set(&mut rt, r, "step", 2.0);
    assert!(f.expect("SET").contains("Powered true"));
    wait(&mut rt, r, "stName", "Enabled");
    set(&mut rt, r, "step", 3.0);
    f.expect("CALL /org/bluez/hci0 StartDiscovery");
    wait(
        &mut rt,
        r,
        "adapter",
        "arch:true:1:true:false:180:true:/org/bluez/hci0:2",
    );

    // connected = true → Connect (state Connecting until bluez reports Connected).
    set(&mut rt, r, "step", 4.0);
    // Like Qt's setConnected(): `connected` itself only follows bluez; state goes Connecting.
    assert!(
        val(&rt, r, "dconn") == "false" && val(&rt, r, "dstate") == "3"
            || val(&rt, r, "dconn") == "true"
    );
    f.expect(&format!("CALL {DEV_A} Connect"));
    wait(&mut rt, r, "dconn", "true");
    wait(&mut rt, r, "dstate", "1");
    // disconnect(): Disconnecting while bluez has not answered; the failed call resets the
    // state from connected. The fake holds the call open until the state is checked.
    f.cmd(r#"{"op":"flag","name":"hold_disconnect","value":true}"#);
    set(&mut rt, r, "step", 5.0);
    f.expect(&format!("HELD Disconnect {DEV_A}"));
    turn(&mut rt);
    assert_eq!(val(&rt, r, "dstate"), "2");
    f.cmd(r#"{"op":"fail-held"}"#);
    wait(&mut rt, r, "dstate", "1");
    assert_eq!(val(&rt, r, "dconn"), "true");
    f.cmd(r#"{"op":"flag","name":"hold_disconnect","value":false}"#);
    // connect() while connected is refused.
    set(&mut rt, r, "step", 6.0);
    f.quiet(&mut rt, "CALL");
    // trusted (optimistic + Set), pair(), name (Alias), forget().
    set(&mut rt, r, "step", 7.0);
    assert!(f.expect("SET").contains("Trusted true"));
    set(&mut rt, r, "step", 8.0);
    turn(&mut rt);
    f.expect(&format!("CALL {DEV_A} Pair"));
    wait(
        &mut rt,
        r,
        "a",
        "Headphones:WH-1000XM4:audio-headset:true:1:true:false:true:false:false:true",
    );
    set(&mut rt, r, "step", 9.0);
    assert!(f.expect("SET").contains("Alias \"My cans\""));
    wait(
        &mut rt,
        r,
        "a",
        "My cans:WH-1000XM4:audio-headset:true:1:true:false:true:false:false:true",
    );
    set(&mut rt, r, "step", 10.0);
    turn(&mut rt);
    f.expect(&format!("CALL {HCI0} RemoveDevice {DEV_A}"));
    wait(&mut rt, r, "nd", "1");
    assert_eq!(val(&rt, r, "a"), "");

    // Battery interface hotplug.
    f.cmd(&format!(
        r#"{{"op":"remove","path":"{DEV_B}","ifaces":["org.bluez.Battery1"]}}"#
    ));
    wait(&mut rt, r, "b", "true:Connected:false:0");
    f.cmd(&format!(
        r#"{{"op":"add","path":"{DEV_B}","ifaces":{{"org.bluez.Battery1":{{"Percentage":55}}}}}}"#
    ));
    wait(&mut rt, r, "b", "true:Connected:true:0.55");
    f.cmd(&format!(
        r#"{{"op":"set","path":"{DEV_B}","iface":"org.bluez.Device1","props":{{"Connected":false}}}}"#
    ));
    wait(&mut rt, r, "b", "false:Disconnected:true:0.55");

    // Device and adapter hotplug; defaultAdapter follows the first adapter.
    f.cmd(r#"{"op":"add","path":"/org/bluez/hci1","ifaces":{"org.bluez.Adapter1":{"Alias":"usb","Powered":false,"PowerState":"off-blocked"}}}"#);
    wait(&mut rt, r, "na", "2");
    assert_eq!(val(&rt, r, "aid"), "hci0");
    f.cmd(r#"{"op":"add","path":"/org/bluez/hci1/dev_01_02_03_04_05_06","ifaces":{"org.bluez.Device1":{"Address":"01:02:03:04:05:06","Alias":"Phone","Adapter":"/org/bluez/hci1"}}}"#);
    wait(&mut rt, r, "nd", "2");
    assert!(val(&rt, r, "adapter").ends_with(":1"));
    f.cmd(&format!(r#"{{"op":"remove","path":"{HCI0}"}}"#));
    wait(&mut rt, r, "aid", "hci1");
    assert_eq!(val(&rt, r, "stName"), "Blocked");
    assert!(val(&rt, r, "adapter").starts_with("usb:false:4:"));
    assert!(val(&rt, r, "adapter").ends_with(":1"));
    // Enabling a blocked adapter is refused (no Set).
    set(&mut rt, r, "step", 0.0);
    set(&mut rt, r, "step", 2.0);
    f.quiet(&mut rt, "SET");
    assert!(val(&rt, r, "adapter").starts_with("usb:false:4:"));

    // bluetoothd exits: everything goes away; restart: recovered from GetManagedObjects.
    f.kill();
    wait(&mut rt, r, "na", "0");
    wait(&mut rt, r, "nd", "0");
    assert_eq!(val(&rt, r, "hasDefault"), "false");
    let _f = Fake::start(bus, "bluez", &bluez_init());
    wait(&mut rt, r, "nd", "2");
    assert_eq!(val(&rt, r, "aid"), "hci0");
    assert_eq!(val(&rt, r, "b"), "true:Connected:true:0.8");
}

/// A USB adapter dropping off and coming back (bluetoothd removes the devices, then the adapter,
/// and re-adds them from storage): BluetoothStatus-style state and actions recover by themselves.
#[test]
fn bluetooth_adapter_drop_and_return() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::bluetooth_adapter_drop_and_return"
    )) else {
        return;
    };
    let bus = &g;
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Bluetooth\nItem { id: r\n\
         property bool available: Bluetooth.adapters.values.length > 0\n\
         property bool enabled: Bluetooth.defaultAdapter?.enabled ?? false\n\
         property string first: Bluetooth.defaultAdapter?.devices.values.find(device => device.connected)?.name ?? ''\n\
         property int active: Bluetooth.defaultAdapter?.devices.values.filter(device => device.connected).length ?? 0\n\
         property bool connected: Bluetooth.devices.values.some(d => d.connected)\n\
         property string paired: Bluetooth.devices.values.filter(d => d.paired && !d.connected).map(d => d.name).join(',')\n\
         property string st: BluetoothAdapterState.toString(Bluetooth.defaultAdapter?.state ?? 0)\n\
         property string aid: Bluetooth.defaultAdapter?.adapterId ?? ''\n\
         property int nd: Bluetooth.defaultAdapter?.devices.values.length ?? -1\n\
         property string s: [available, enabled, st, aid, nd, first, active, connected, paired].join(':')\n\
         property int step: 0\n\
         onStepChanged: if (step == 1) Bluetooth.devices.values.find(d => d.address == '11:22:33:44:55:66').connect()\n\
         }",
    );
    let mut f = Fake::start(bus, "bluez", &bluez_init());
    wait(&mut rt, r, "s", "true:true:Enabled:hci0:2:MX Keys:1:true:");

    for p in [DEV_A, DEV_B, HCI0] {
        f.cmd(&format!(r#"{{"op":"remove","path":"{p}"}}"#));
    }
    wait(&mut rt, r, "s", "false:false:Disabled::-1::0:false:");

    f.cmd(&format!(
        r#"{{"op":"add","path":"{HCI0}","ifaces":{{"org.bluez.Adapter1":{{"Address":"00:1A:7D:DA:71:13","Alias":"arch","Name":"arch","Powered":true,"PowerState":"on","Discoverable":false,"DiscoverableTimeout":180,"Discovering":false,"Pairable":true,"PairableTimeout":0}}}}}}"#
    ));
    wait(&mut rt, r, "s", "true:true:Enabled:hci0:0::0:false:");
    f.cmd(&format!(
        r#"{{"op":"add","path":"{DEV_B}","ifaces":{{"org.bluez.Device1":{{"Address":"AA:BB:CC:DD:EE:FF","Name":"MX Keys","Alias":"MX Keys","Icon":"input-keyboard","Paired":true,"Bonded":true,"Trusted":true,"Blocked":false,"Connected":false,"Adapter":"{HCI0}"}}}}}}"#
    ));
    f.cmd(&format!(
        r#"{{"op":"add","path":"{DEV_A}","ifaces":{{"org.bluez.Device1":{{"Address":"11:22:33:44:55:66","Name":"WH-1000XM4","Alias":"Headphones","Icon":"audio-headset","Paired":false,"Bonded":false,"Trusted":false,"Blocked":false,"Connected":false,"Adapter":"{HCI0}"}}}}}}"#
    ));
    wait(&mut rt, r, "s", "true:true:Enabled:hci0:2::0:false:MX Keys");
    // The paired keyboard reconnects on its own.
    f.cmd(&format!(
        r#"{{"op":"set","path":"{DEV_B}","iface":"org.bluez.Device1","props":{{"Connected":true}}}}"#
    ));
    wait(&mut rt, r, "s", "true:true:Enabled:hci0:2:MX Keys:1:true:");
    // Actions reach the re-added objects.
    set(&mut rt, r, "step", 1.0);
    f.expect(&format!("CALL {DEV_A} Connect"));
    wait(&mut rt, r, "s", "true:true:Enabled:hci0:2:MX Keys:2:true:");
}

const AGENT: &str = "/org/rustshell/BluetoothAgent";

fn agent(f: &mut Fake, method: &str, args: &str) {
    f.cmd(&format!(
        r#"{{"op":"agent","method":"{method}","args":{args}}}"#
    ));
}

/// The NOTIFY line's id and JSON payload.
fn notified(n: &Fake) -> (u32, String) {
    let l = n.expect("NOTIFY ");
    let mut it = l.splitn(3, ' ');
    it.next();
    let id = it.next().unwrap().parse().unwrap();
    (id, it.next().unwrap().to_string())
}

/// Waits until the fake has printed `CLOSE <id>` for every one of `ids`, in any order.
fn closed(n: &Fake, ids: &[u32]) {
    let mut left: Vec<String> = ids.iter().map(|i| format!("CLOSE {i}")).collect();
    while !left.is_empty() {
        let l = n.expect("CLOSE ");
        left.retain(|x| *x != l);
    }
}

/// Whether a line the fake printed by now contains `s` (the lines before it are dropped).
fn printed(f: &Fake, s: &str) -> bool {
    loop {
        match f.out.try_recv() {
            Ok(l) if l.contains(s) => return true,
            Ok(_) => {}
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => panic!("fake exited without printing {s:?}"),
        }
    }
}

/// `method` called with `args`, with bluetoothd's Agent1.Cancel coming while the agent still
/// waits for the device's alias (zbus runs every call as a task of its own).
fn cancel_while_alias_held(f: &mut Fake, method: &str, args: &str) {
    f.cmd(r#"{"op":"hold-get","prop":"Alias"}"#);
    agent(f, method, args);
    f.expect("HELD GET Alias");
    agent(f, "Cancel", "[]");
    f.expect("AGENT-REPLY Cancel");
    f.cmd(r#"{"op":"release-get"}"#);
}

/// A request bluetoothd cancels before the agent asked or showed anything ends Canceled, and
/// nothing is asked or shown for it afterwards.
#[test]
fn bluetooth_pairing_cancelled_while_looking_up_the_device() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::bluetooth_pairing_cancelled_while_looking_up_the_device"
    )) else {
        return;
    };
    let bus = &g;
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Bluetooth\nItem { property int nd: Bluetooth.devices.values.length }",
    );
    let n = Fake::start(bus, "notifd", "{}");
    let mut f = Fake::start(bus, "bluez", &bluez_init());
    wait(&mut rt, r, "nd", "2");
    f.expect(&format!("AGENT RequestDefaultAgent {AGENT}"));
    for (m, args) in [
        ("RequestConfirmation", format!(r#"["{DEV_A}", 7]"#)),
        ("RequestAuthorization", format!(r#"["{DEV_A}"]"#)),
        ("DisplayPasskey", format!(r#"["{DEV_B}", 1234, 0]"#)),
        ("DisplayPinCode", format!(r#"["{DEV_B}", "0000"]"#)),
    ] {
        cancel_while_alias_held(&mut f, m, &args);
        // Whichever comes first: the request's end, or a notification for it.
        let (mut ended, mut shown) = (false, false);
        until(&mut rt, &format!("{m} to end or show something"), |_| {
            ended |= printed(&f, &format!("AGENT-ERROR {m} org.bluez.Error.Canceled"));
            shown |= printed(&n, "NOTIFY ");
            ended || shown
        });
        assert!(!shown, "{m}: shown after bluetoothd cancelled it");
        n.quiet(&mut rt, "NOTIFY ");
    }
    // A request nobody cancels still asks.
    agent(&mut f, "RequestConfirmation", &format!(r#"["{DEV_A}", 7]"#));
    let (id, _) = notified(&n);
    let mut n = n;
    n.cmd(&format!(r#"{{"op":"invoke","id":{id},"key":"accept"}}"#));
    f.expect("AGENT-REPLY RequestConfirmation");
}

/// bluetoothd leaving the bus sends no Agent1.Cancel: its open request ends with it, and the
/// question it asked closes instead of waiting for an answer nobody takes.
#[test]
fn bluetooth_pairing_ends_when_bluetoothd_leaves() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::bluetooth_pairing_ends_when_bluetoothd_leaves"
    )) else {
        return;
    };
    let bus = &g;
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Bluetooth\nItem { property int nd: Bluetooth.devices.values.length }",
    );
    let n = Fake::start(bus, "notifd", "{}");
    let mut f = Fake::start(bus, "bluez", &bluez_init());
    wait(&mut rt, r, "nd", "2");
    f.expect(&format!("AGENT RequestDefaultAgent {AGENT}"));
    agent(&mut f, "RequestConfirmation", &format!(r#"["{DEV_A}", 7]"#));
    let (id, _) = notified(&n);
    // A passkey shown to type stops showing too.
    agent(
        &mut f,
        "DisplayPasskey",
        &format!(r#"["{DEV_B}", 1234, 0]"#),
    );
    f.expect("AGENT-REPLY DisplayPasskey");
    let (shown, _) = notified(&n);
    f.kill();
    wait(&mut rt, r, "nd", "0");
    closed(&n, &[id, shown]);
}

/// impulse-shell's own BlueZ pairing agent (a deliberate addition: quickshell registers none, so
/// pairing failed with "No agent available" unless a separate agent ran).
#[test]
fn bluetooth_pairing_agent() {
    let Some(g) = bus(concat!(module_path!(), "::bluetooth_pairing_agent")) else {
        return;
    };
    let bus = &g;
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Bluetooth\nItem { property int nd: Bluetooth.devices.values.length }",
    );
    let mut n = Fake::start(bus, "notifd", "{}");
    let mut f = Fake::start(bus, "bluez", &bluez_init());
    wait(&mut rt, r, "nd", "2");
    assert!(
        f.expect("AGENT RegisterAgent")
            .ends_with(&format!("{AGENT} DisplayYesNo"))
    );
    f.expect(&format!("AGENT RequestDefaultAgent {AGENT}"));

    // Numeric comparison: accepted from the notification's action.
    agent(
        &mut f,
        "RequestConfirmation",
        &format!(r#"["{DEV_A}", 123456]"#),
    );
    let (id, js) = notified(&n);
    assert_eq!(
        js,
        r#"{"app": "Bluetooth", "icon": "bluetooth", "summary": "Bluetooth pairing", "body": "Pair with Headphones? Confirm that it shows 123456", "actions": ["accept", "Pair", "reject", "Reject"], "urgency": 2, "expire": 0}"#
    );
    n.cmd(&format!(r#"{{"op":"invoke","id":{id},"key":"accept"}}"#));
    f.expect("AGENT-REPLY RequestConfirmation");
    n.expect(&format!("CLOSE {id}"));

    // Rejected from the action, or by dismissing the notification.
    agent(
        &mut f,
        "RequestConfirmation",
        &format!(r#"["{DEV_A}", 42]"#),
    );
    let (id, js) = notified(&n);
    assert!(js.contains("Confirm that it shows 000042"));
    n.cmd(&format!(r#"{{"op":"invoke","id":{id},"key":"reject"}}"#));
    f.expect("AGENT-ERROR RequestConfirmation org.bluez.Error.Rejected");
    agent(&mut f, "RequestAuthorization", &format!(r#"["{DEV_A}"]"#));
    let (id, js) = notified(&n);
    assert!(js.contains(r#""body": "Pair with Headphones?""#));
    n.cmd(&format!(r#"{{"op":"dismiss","id":{id}}}"#));
    f.expect("AGENT-ERROR RequestAuthorization org.bluez.Error.Rejected");
    // An action for another notification does not answer.
    agent(&mut f, "RequestAuthorization", &format!(r#"["{DEV_A}"]"#));
    let (id, _) = notified(&n);
    n.cmd(&format!(
        r#"{{"op":"invoke","id":{},"key":"accept"}}"#,
        id + 100
    ));
    f.quiet(&mut rt, "AGENT-");
    n.cmd(&format!(r#"{{"op":"invoke","id":{id},"key":"accept"}}"#));
    f.expect("AGENT-REPLY RequestAuthorization");

    // bluetoothd cancelling (timeout, device gone) closes the question.
    agent(&mut f, "RequestConfirmation", &format!(r#"["{DEV_A}", 7]"#));
    let (id, _) = notified(&n);
    agent(&mut f, "Cancel", "[]");
    n.expect(&format!("CLOSE {id}"));
    f.expect("AGENT-ERROR RequestConfirmation org.bluez.Error.Canceled");

    // Passkey/PIN to type on a keyboard: shown in one notification, replaced as typing goes on.
    agent(
        &mut f,
        "DisplayPasskey",
        &format!(r#"["{DEV_B}", 1234, 0]"#),
    );
    f.expect("AGENT-REPLY DisplayPasskey");
    let (id, js) = notified(&n);
    assert!(js.contains(r#""body": "Type 001234 on MX Keys, then press Enter", "actions": [], "urgency": 1, "expire": -1"#));
    // org.bluez.Agent1: called again with the number of keys typed on the device so far.
    agent(
        &mut f,
        "DisplayPasskey",
        &format!(r#"["{DEV_B}", 1234, 3]"#),
    );
    let (again, js) = notified(&n);
    assert_eq!(again, id);
    assert!(
        js.contains(r#""body": "Type 001234 on MX Keys, then press Enter (3 of 6 digits typed)""#),
        "{js}"
    );
    agent(&mut f, "DisplayPinCode", &format!(r#"["{DEV_B}", "0000"]"#));
    f.expect("AGENT-REPLY DisplayPinCode");
    assert!(
        notified(&n)
            .1
            .contains("Type 0000 on MX Keys, then press Enter")
    );
    // Shown until bluetoothd's Cancel: it closes, and the next code to type shows anew.
    agent(&mut f, "Cancel", "[]");
    f.expect("AGENT-REPLY Cancel");
    agent(&mut f, "DisplayPasskey", &format!(r#"["{DEV_B}", 1, 0]"#));
    let mut seen = vec![];
    while !seen
        .last()
        .is_some_and(|l: &String| l.starts_with("NOTIFY "))
    {
        seen.push(n.expect(""));
    }
    assert!(seen.contains(&format!("CLOSE {id}")), "{seen:?}");
    let shown: u32 = seen
        .last()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(shown, id, "{seen:?}");

    // No PolkitAgent to type into: typed PIN/passkey requests are refused.
    agent(&mut f, "RequestPinCode", &format!(r#"["{DEV_A}"]"#));
    f.expect("AGENT-ERROR RequestPinCode org.bluez.Error.Rejected");
    agent(&mut f, "RequestPasskey", &format!(r#"["{DEV_A}"]"#));
    f.expect("AGENT-ERROR RequestPasskey org.bluez.Error.Rejected");

    // Services: a Trusted device needs no question; a paired one that is not trusted only
    // with the user's consent (bluetoothd asks exactly because Trusted is false); an unpaired
    // one is refused without a question.
    let a2dp = "0000110d-0000-1000-8000-00805f9b34fb";
    agent(
        &mut f,
        "AuthorizeService",
        &format!(r#"["{DEV_B}", "{a2dp}"]"#),
    );
    f.expect("AGENT-REPLY AuthorizeService");
    agent(
        &mut f,
        "AuthorizeService",
        &format!(r#"["{DEV_A}", "{a2dp}"]"#),
    );
    f.expect("AGENT-ERROR AuthorizeService org.bluez.Error.Rejected");
    n.quiet(&mut rt, "NOTIFY ");
    f.cmd(&format!(
        r#"{{"op":"set","path":"{DEV_A}","iface":"org.bluez.Device1","props":{{"Paired":true}}}}"#
    ));
    agent(
        &mut f,
        "AuthorizeService",
        &format!(r#"["{DEV_A}", "{a2dp}"]"#),
    );
    let (id, js) = notified(&n);
    assert!(
        js.contains(r#""body": "Allow Headphones to use Advanced Audio?", "actions": ["accept", "Allow", "reject", "Reject"]"#),
        "{js}"
    );
    f.quiet(&mut rt, "AGENT-");
    n.cmd(&format!(r#"{{"op":"invoke","id":{id},"key":"reject"}}"#));
    f.expect("AGENT-ERROR AuthorizeService org.bluez.Error.Rejected");
    let hid = "00001124-0000-1000-8000-00805f9b34fb";
    agent(
        &mut f,
        "AuthorizeService",
        &format!(r#"["{DEV_A}", "{hid}"]"#),
    );
    let (id, js) = notified(&n);
    assert!(js.contains("Allow Headphones to use Input Device?"), "{js}");
    n.cmd(&format!(r#"{{"op":"invoke","id":{id},"key":"accept"}}"#));
    f.expect("AGENT-REPLY AuthorizeService");

    // Only bluetoothd (the owner of org.bluez) may call the agent: a forged request from
    // another connection shows nothing and is rejected.
    for (m, args) in [
        ("RequestConfirmation", format!(r#"["{DEV_A}", 1]"#)),
        ("AuthorizeService", format!(r#"["{DEV_A}", "{hid}"]"#)),
        ("DisplayPasskey", format!(r#"["{DEV_B}", 1, 0]"#)),
        ("Cancel", "[]".to_string()),
    ] {
        f.cmd(&format!(
            r#"{{"op":"agent-forged","method":"{m}","args":{args}}}"#
        ));
        f.expect(&format!("AGENT-ERROR {m} org.bluez.Error.Rejected"));
    }
    n.quiet(&mut rt, "NOTIFY ");
    // Released (bluetoothd unregistered the agent): the code to type stops showing.
    agent(&mut f, "Release", "[]");
    f.expect("AGENT-REPLY Release");
    closed(&n, &[shown]);

    // bluetoothd restarts: the agent is registered again.
    f.kill();
    wait(&mut rt, r, "nd", "0");
    let f = Fake::start(bus, "bluez", &bluez_init());
    wait(&mut rt, r, "nd", "2");
    f.expect(&format!("AGENT RegisterAgent {AGENT} DisplayYesNo"));
    f.expect(&format!("AGENT RequestDefaultAgent {AGENT}"));
    drop(n);
}

const DIALOG_QML: &str = r#"
import QtQuick
import Rustshell.Bluetooth
import Rustshell.Services.Polkit
Item { id: r
    property int nd: Bluetooth.devices.values.length
    property bool on: true
    property bool active: false
    property string log: ""
    property string input
    property int seq
    property var fl: null
    Loader { active: r.on; sourceComponent: PolkitAgent { id: agent
        onIsActiveChanged: r.active = isActive
        onAuthenticationRequestStarted: {
            const f = agent.flow;
            r.fl = f;
            r.log += "started " + [f.actionId, f.message, f.iconName, f.inputPrompt, f.responseVisible,
                f.isResponseRequired, f.selectedIdentity !== null].join("|") + ";";
        }
    } }
    Connections { target: r.fl
        function onAuthenticationFailed() {
            r.log += "failed " + r.fl.supplementaryMessage + "|" + r.fl.supplementaryIsError + "|" + r.fl.isResponseRequired + ";"
        }
        function onAuthenticationSucceeded() { r.log += "succeeded;" }
        function onAuthenticationRequestCancelled() { r.log += "bluez-cancelled;" }
        function onIsCompletedChanged() {
            r.log += "completed " + r.fl.isSuccessful + "|" + r.fl.isCancelled + ";"
        }
    }
    onSeqChanged: input === "cancel" ? fl.cancelAuthenticationRequest() : fl.submit(input)
}
"#;

/// Run the loop until `log` has `s`; clears it.
fn logged(rt: &mut Runtime, r: usize, s: &str) {
    until(rt, &format!("log {s:?}"), |rt| {
        val(rt, r, "log").contains(s)
    });
    rt.world.borrow_mut().write(r, "log", Value::str(""));
}

/// [`Fake::expect`] while running the loop (the line answers something the World handles).
fn expect_pumped(rt: &mut Runtime, f: &Fake, s: &str) {
    until(rt, &format!("the fake to print {s:?}"), |_| {
        loop {
            match f.out.try_recv() {
                Ok(l) if l.contains(s) => return true,
                Ok(_) => {}
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => panic!("fake exited without printing {s:?}"),
            }
        }
    });
}

fn type_in(rt: &mut Runtime, r: usize, s: &str) {
    rt.world.borrow_mut().write(r, "input", Value::str(s));
    let n = rt.get(r, "seq").to_number();
    set(rt, r, "seq", n + 1.0);
}

/// PIN codes / passkeys bluetoothd needs typed go to the PolkitAgent's dialog as flows.
#[test]
fn bluetooth_pairing_dialog() {
    let Some(g) = bus(concat!(module_path!(), "::bluetooth_pairing_dialog")) else {
        return;
    };
    let bus = &g;
    let (mut rt, r) = load(DIALOG_QML);
    let mut f = Fake::start(bus, "bluez", &bluez_init());
    wait(&mut rt, r, "nd", "2");
    // A dialog exists: the agent can take typed keys.
    f.expect(&format!("AGENT RegisterAgent {AGENT} KeyboardDisplay"));
    f.expect(&format!("AGENT RequestDefaultAgent {AGENT}"));

    // PIN: an empty one fails the attempt (the field is re-enabled), a real one answers.
    agent(&mut f, "RequestPinCode", &format!(r#"["{DEV_A}"]"#));
    logged(
        &mut rt,
        r,
        "started org.bluez.Agent1.RequestPinCode|Enter the PIN shown on or provided with Headphones|bluetooth|PIN|true|true|true;",
    );
    wait(&mut rt, r, "active", "true");
    type_in(&mut rt, r, "  ");
    logged(
        &mut rt,
        r,
        "failed The PIN is 1 to 16 characters long|true|true;",
    );
    type_in(&mut rt, r, "0123456789abcdefg");
    logged(
        &mut rt,
        r,
        "failed The PIN is 1 to 16 characters long|true|true;",
    );
    f.quiet(&mut rt, "AGENT-");
    type_in(&mut rt, r, "1234");
    f.expect("AGENT-REPLY RequestPinCode 1234");
    logged(&mut rt, r, "completed true|false;");
    wait(&mut rt, r, "active", "false");

    // Passkey: decimal 0..=999999 only.
    agent(&mut f, "RequestPasskey", &format!(r#"["{DEV_B}"]"#));
    logged(
        &mut rt,
        r,
        "started org.bluez.Agent1.RequestPasskey|Enter the passkey shown on MX Keys|bluetooth|Passkey|true|true|true;",
    );
    for bad in ["1000000", "12a4", "-1", ""] {
        type_in(&mut rt, r, bad);
        logged(
            &mut rt,
            r,
            "failed The passkey is a number from 0 to 999999|true|true;",
        );
    }
    f.quiet(&mut rt, "AGENT-");
    type_in(&mut rt, r, "004321");
    f.expect("AGENT-REPLY RequestPasskey 4321");
    logged(&mut rt, r, "succeeded;");

    // Requests queue: the second starts once the first is answered.
    agent(&mut f, "RequestPinCode", &format!(r#"["{DEV_A}"]"#));
    agent(&mut f, "RequestPasskey", &format!(r#"["{DEV_B}"]"#));
    logged(&mut rt, r, "started org.bluez.Agent1.RequestPinCode|");
    type_in(&mut rt, r, "0000");
    f.expect("AGENT-REPLY RequestPinCode 0000");
    logged(&mut rt, r, "started org.bluez.Agent1.RequestPasskey|");
    type_in(&mut rt, r, "999999");
    f.expect("AGENT-REPLY RequestPasskey 999999");

    // Cancel in the dialog refuses the pairing.
    agent(&mut f, "RequestPinCode", &format!(r#"["{DEV_A}"]"#));
    logged(&mut rt, r, "started org.bluez.Agent1.RequestPinCode|");
    type_in(&mut rt, r, "cancel");
    f.expect("AGENT-ERROR RequestPinCode org.bluez.Error.Rejected");
    logged(&mut rt, r, "completed false|true;");
    wait(&mut rt, r, "active", "false");

    // bluetoothd cancelling (timeout, device gone) closes the dialog.
    agent(&mut f, "RequestPasskey", &format!(r#"["{DEV_B}"]"#));
    logged(&mut rt, r, "started org.bluez.Agent1.RequestPasskey|");
    agent(&mut f, "Cancel", "[]");
    expect_pumped(
        &mut rt,
        &f,
        "AGENT-ERROR RequestPasskey org.bluez.Error.Canceled",
    );
    logged(&mut rt, r, "completed false|true;bluez-cancelled;");
    wait(&mut rt, r, "active", "false");
    // Cancelled while the agent still looks the device up: no flow starts for it.
    cancel_while_alias_held(&mut f, "RequestPasskey", &format!(r#"["{DEV_B}"]"#));
    let mut ended = false;
    until(&mut rt, "RequestPasskey to end or start a flow", |rt| {
        ended |= printed(&f, "AGENT-ERROR RequestPasskey org.bluez.Error.Canceled");
        ended || val(rt, r, "log").contains("started")
    });
    assert_eq!(val(&rt, r, "log"), "");

    // The dialog goes away: back to DisplayYesNo, typed requests are refused.
    set(&mut rt, r, "on", 0.0);
    expect_pumped(&mut rt, &f, &format!("AGENT UnregisterAgent {AGENT}"));
    expect_pumped(
        &mut rt,
        &f,
        &format!("AGENT RegisterAgent {AGENT} DisplayYesNo"),
    );
    expect_pumped(&mut rt, &f, &format!("AGENT RequestDefaultAgent {AGENT}"));
    agent(&mut f, "RequestPinCode", &format!(r#"["{DEV_A}"]"#));
    f.expect("AGENT-ERROR RequestPinCode org.bluez.Error.Rejected");
    // And comes back.
    set(&mut rt, r, "on", 1.0);
    expect_pumped(
        &mut rt,
        &f,
        &format!("AGENT RegisterAgent {AGENT} KeyboardDisplay"),
    );
    expect_pumped(&mut rt, &f, &format!("AGENT RequestDefaultAgent {AGENT}"));
    agent(&mut f, "RequestPasskey", &format!(r#"["{DEV_B}"]"#));
    logged(&mut rt, r, "started org.bluez.Agent1.RequestPasskey|");
    type_in(&mut rt, r, "7");
    f.expect("AGENT-REPLY RequestPasskey 7");
    set(&mut rt, r, "on", 0.0);
    expect_pumped(
        &mut rt,
        &f,
        &format!("AGENT RegisterAgent {AGENT} DisplayYesNo"),
    );
}

/// Another local client (any UID may emit signals on the system bus) forging `member` of `iface`
/// at `path`. The round trip afterwards guarantees the bus has routed the signal before returning.
fn forge<B: serde::Serialize + zbus::zvariant::DynamicType>(
    path: &str,
    iface: &str,
    member: &str,
    body: &B,
) {
    let c = zbus::blocking::Connection::system().expect("forger connects");
    c.emit_signal(None::<&str>, path, iface, member, body)
        .expect("forged signal sent");
    c.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "GetId",
        &(),
    )
    .expect("bus round trip");
}

fn forge_props(path: &str, iface: &str, props: &[(&str, zbus::zvariant::Value<'_>)]) {
    let changed: std::collections::HashMap<&str, &zbus::zvariant::Value<'_>> =
        props.iter().map(|(k, v)| (*k, v)).collect();
    forge(
        path,
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
        &(iface, changed, Vec::<&str>::new()),
    );
}

/// UPower, power-profiles-daemon and BlueZ only take signals from the daemon owning their bus
/// name: a forged `Percentage = 1` on the display device would make the original's Battery service run
/// `systemctl suspend` for any local user.
#[test]
fn system_services_ignore_forged_signals() {
    use zbus::zvariant::Value as Z;
    let Some(g) = bus(concat!(
        module_path!(),
        "::system_services_ignore_forged_signals"
    )) else {
        return;
    };
    let bus = &g;
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Services.UPower\nimport Rustshell.Bluetooth\nItem {\n\
         property real pct: UPower.displayDevice.percentage\n\
         property real energy: UPower.displayDevice.energy\n\
         property int n: UPower.devices.values.length\n\
         property int prof: PowerProfiles.profile\n\
         property int deg: PowerProfiles.degradationReason\n\
         property string devs: Bluetooth.devices.values.map(d => d.address + ':' + d.name + ':' + d.connected).join(',')\n}",
    );

    let mut up = Fake::start(bus, "upower", &upower_init(50.0));
    wait(&mut rt, r, "pct", "0.5");
    wait(&mut rt, r, "n", "2");
    forge_props(
        DISPLAY,
        "org.freedesktop.UPower.Device",
        &[("Percentage", Z::F64(1.0)), ("State", Z::U32(2))],
    );
    forge(
        "/org/freedesktop/UPower",
        "org.freedesktop.UPower",
        "DeviceRemoved",
        &(zbus::zvariant::ObjectPath::try_from(BAT).unwrap(),),
    );
    // The daemon's own change, sent after the forgeries, arrives; they did not.
    up.cmd(&format!(
        r#"{{"op":"set","path":"{DISPLAY}","iface":"org.freedesktop.UPower.Device","props":{{"Energy":25.0}}}}"#
    ));
    wait(&mut rt, r, "energy", "25");
    assert_eq!(val(&rt, r, "pct"), "0.5");
    assert_eq!(val(&rt, r, "n"), "2");

    let mut ppd = Fake::start(bus, "ppd", PPD_INIT);
    wait(&mut rt, r, "deg", "1");
    assert_eq!(val(&rt, r, "prof"), "1");
    forge_props(
        PPD_PATH,
        PPD_IFACE,
        &[("ActiveProfile", Z::from("power-saver"))],
    );
    ppd.cmd(&format!(
        r#"{{"op":"set","path":"{PPD_PATH}","iface":"{PPD_IFACE}","props":{{"PerformanceDegraded":"high-operating-temperature"}}}}"#
    ));
    wait(&mut rt, r, "deg", "2");
    assert_eq!(val(&rt, r, "prof"), "1");

    let mut bt = Fake::start(bus, "bluez", &bluez_init());
    let devs = "11:22:33:44:55:66:Headphones:false,AA:BB:CC:DD:EE:FF:MX Keys:true";
    wait(&mut rt, r, "devs", devs);
    forge_props(
        DEV_A,
        "org.bluez.Device1",
        &[("Connected", Z::Bool(true)), ("Name", Z::from("forged"))],
    );
    let ghost = "/org/bluez/hci0/dev_DE_AD_BE_EF_00_01";
    let mut dev: std::collections::HashMap<&str, Z<'_>> = std::collections::HashMap::new();
    dev.insert("Address", Z::from("DE:AD:BE:EF:00:01"));
    dev.insert("Name", Z::from("ghost"));
    dev.insert(
        "Adapter",
        Z::from(zbus::zvariant::ObjectPath::try_from(HCI0).unwrap()),
    );
    let ifaces = std::collections::HashMap::from([("org.bluez.Device1", dev)]);
    forge(
        "/",
        "org.freedesktop.DBus.ObjectManager",
        "InterfacesAdded",
        &(zbus::zvariant::ObjectPath::try_from(ghost).unwrap(), ifaces),
    );
    bt.cmd(&format!(
        r#"{{"op":"set","path":"{DEV_B}","iface":"org.bluez.Device1","props":{{"Connected":false}}}}"#
    ));
    wait(
        &mut rt,
        r,
        "devs",
        "11:22:33:44:55:66:Headphones:false,AA:BB:CC:DD:EE:FF:MX Keys:false",
    );
}

/// A write the daemon refuses (polkit, rfkill, `org.bluez.Error.*`) comes back as an error
/// reply and no PropertiesChanged: the property shows the daemon's value again.
#[test]
fn refused_writes_show_the_daemon_value() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::refused_writes_show_the_daemon_value"
    )) else {
        return;
    };
    let bus = &g;
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Services.UPower\nimport Rustshell.Bluetooth\nItem { id: r\n\
         property int prof: PowerProfiles.profile\n\
         property bool perf: PowerProfiles.hasPerformanceProfile\n\
         property int holds: PowerProfiles.holds.length\n\
         property int req: -1\n\
         onReqChanged: PowerProfiles.profile = req\n\
         property var ad: Bluetooth.defaultAdapter\n\
         property var dev: Bluetooth.devices.values.find(d => d.address == '11:22:33:44:55:66') ?? null\n\
         property string a: ad ? [ad.discoverable, ad.pairableTimeout].join(':') : ''\n\
         property string d: dev ? [dev.trusted, dev.name].join(':') : ''\n\
         property int step: 0\n\
         onStepChanged: {\n\
           if (step == 1) { ad.discoverable = true; ad.pairableTimeout = 60 }\n\
           else if (step == 2) { dev.trusted = true; dev.name = 'My cans' }\n\
         } }",
    );
    let mut p = Fake::start(bus, "ppd", PPD_INIT);
    wait(&mut rt, r, "perf", "true");
    assert_eq!(val(&rt, r, "prof"), "1");
    p.cmd(r#"{"op":"flag","name":"reject_set","value":true}"#);
    set(&mut rt, r, "req", 2.0);
    assert!(p.expect("SET").contains("\"performance\""));
    wait(&mut rt, r, "prof", "1");
    // An accepted write after a refused one sticks.
    p.cmd(r#"{"op":"flag","name":"reject_set","value":false}"#);
    set(&mut rt, r, "req", 0.0);
    assert!(p.expect("SET").contains("\"power-saver\""));
    wait(&mut rt, r, "prof", "0");
    // The daemon's value is the last profile it reported (with the accepted write it dropped
    // its holds), also after it reported something that is none.
    wait(&mut rt, r, "holds", "0");
    p.cmd(&format!(
        r#"{{"op":"set","path":"{PPD_PATH}","iface":"{PPD_IFACE}","props":{{"ActiveProfile":"turbo"}}}}"#
    ));
    p.cmd(r#"{"op":"flag","name":"reject_set","value":true}"#);
    set(&mut rt, r, "req", 2.0);
    assert!(p.expect("SET").contains("\"performance\""));
    wait(&mut rt, r, "prof", "0");

    let mut b = Fake::start(bus, "bluez", &bluez_init());
    wait(&mut rt, r, "d", "false:Headphones");
    assert_eq!(val(&rt, r, "a"), "false:0");
    b.cmd(r#"{"op":"flag","name":"reject_set","value":true}"#);
    set(&mut rt, r, "step", 1.0);
    let sets = [b.expect("SET"), b.expect("SET")].join("\n");
    assert!(sets.contains("Discoverable true"), "{sets}");
    assert!(sets.contains("PairableTimeout 60"), "{sets}");
    wait(&mut rt, r, "a", "false:0");
    set(&mut rt, r, "step", 2.0);
    let sets = [b.expect("SET"), b.expect("SET")].join("\n");
    assert!(sets.contains("Trusted true"), "{sets}");
    assert!(sets.contains("Alias \"My cans\""), "{sets}");
    wait(&mut rt, r, "d", "false:Headphones");
}

/// Returns once every worker of `workpool::run` runs a job of this test at the same time: no
/// worker is held by a call a daemon never answers. Settings saves, notification images and
/// file jobs run on those workers; one held there waits as long as the call. A held worker
/// leaves this waiting, and nextest's slow-timeout ends the test.
fn pool_free() {
    eprintln!("waiting for every pool worker");
    let all = std::sync::Arc::new(std::sync::Barrier::new(workpool::WORKERS + 1));
    for _ in 0..workpool::WORKERS {
        let all = all.clone();
        workpool::run(move || {
            all.wait();
        });
    }
    all.wait();
}

/// A power-profiles-daemon that never answers the profile write holds only its own calls.
#[test]
fn held_power_profile_write_leaves_the_pool_free() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::held_power_profile_write_leaves_the_pool_free"
    )) else {
        return;
    };
    let (mut rt, r) = ppd_qml();
    let mut p = Fake::start(&g, "ppd", PPD_INIT);
    wait(&mut rt, r, "perf", "true");
    p.cmd(r#"{"op":"flag","name":"hold_set","value":true}"#);
    set(&mut rt, r, "req", 2.0);
    p.expect("HELD SET ActiveProfile");
    pool_free();
}

/// A bluetoothd that never answers a property write holds only its own calls.
#[test]
fn held_bluetooth_write_leaves_the_pool_free() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::held_bluetooth_write_leaves_the_pool_free"
    )) else {
        return;
    };
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Bluetooth\nItem { id: r\n\
         property var ad: Bluetooth.defaultAdapter\n\
         property bool ready: ad != null\n\
         property int step: 0\n\
         onStepChanged: ad.discoverable = true }",
    );
    let mut b = Fake::start(&g, "bluez", &bluez_init());
    wait(&mut rt, r, "ready", "true");
    b.cmd(r#"{"op":"flag","name":"hold_set","value":true}"#);
    set(&mut rt, r, "step", 1.0);
    b.expect("HELD SET Discoverable");
    pool_free();
}

/// The shell's exit waits until what bluetoothd was asked is on the bus (`bluez::flush`), not
/// for the answer, as Quickshell does not: a write bluetoothd holds without answering does not
/// hold the exit. A second write waits its turn behind the first (they go one at a time) and is
/// not on the bus: for that one the exit waits, as long as its limit, and the process then
/// ends. A flush without the limit would wait as long as the first write, 25 s.
#[test]
fn bluetooth_flush_waits_for_the_write_to_be_sent_not_answered() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::bluetooth_flush_waits_for_the_write_to_be_sent_not_answered"
    )) else {
        return;
    };
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Bluetooth\nItem { id: r\n\
         property var ad: Bluetooth.defaultAdapter\n\
         property bool ready: ad != null\n\
         property int step: 0\n\
         onStepChanged: { if (step == 1) ad.discoverable = true; else ad.pairable = false } }",
    );
    let mut b = Fake::start(&g, "bluez", &bluez_init());
    wait(&mut rt, r, "ready", "true");
    assert!(qml::bluez::flush(), "nothing was asked yet");
    b.cmd(r#"{"op":"flag","name":"hold_set","value":true}"#);
    set(&mut rt, r, "step", 1.0);
    assert!(qml::bluez::flush(), "the write is on the bus");
    b.expect("HELD SET Discoverable");
    set(&mut rt, r, "step", 2.0);
    assert!(
        !qml::bluez::flush(),
        "the second write is behind the first: only the limit ends the wait"
    );
}

/// A bluetoothd that never answers the pairing agent's withdrawal (the last Bluetooth service
/// gone) holds only its own calls.
#[test]
fn held_bluetooth_agent_withdrawal_leaves_the_pool_free() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::held_bluetooth_agent_withdrawal_leaves_the_pool_free"
    )) else {
        return;
    };
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Bluetooth\nItem { property int nd: Bluetooth.devices.values.length }",
    );
    let mut b = Fake::start(&g, "bluez", &bluez_init());
    wait(&mut rt, r, "nd", "2");
    b.expect(&format!("AGENT RequestDefaultAgent {AGENT}"));
    b.cmd(r#"{"op":"flag","name":"hold_unregister_agent","value":true}"#);
    drop(rt);
    b.expect(&format!("HELD UnregisterAgent {AGENT}"));
    pool_free();
}

const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const WIFI_DEV: &str = "/org/freedesktop/NetworkManager/Devices/1";

/// NetworkManager with the radio on and one Wi-Fi device, for the original's `Network` singleton (native).
fn nm_init() -> String {
    format!(
        r#"{{"objects": {{
      "{NM_PATH}": {{"org.freedesktop.NetworkManager": {{"WirelessEnabled": true, "WirelessHardwareEnabled": true,
          "State": 20, "Connectivity": 1, "Devices": ["{WIFI_DEV}"], "ActiveConnections": []}}}},
      "{WIFI_DEV}": {{"org.freedesktop.NetworkManager.Device": {{"DeviceType": 2, "State": 30, "Interface": "wlan0"}},
          "org.freedesktop.NetworkManager.Device.Wireless": {{"AccessPoints": [], "LastScan": 0}}}}
    }}}}"#
    )
}

/// The original's `Network` singleton (qml/services/network), loaded as the shell does; `step` 1 turns
/// the radio on, 2 asks for a scan.
fn network_qml() -> (Runtime, usize) {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"));
    let root = root.canonicalize().unwrap_or(root);
    let mut rt = Runtime::new(Some(root.clone()));
    rt.world.borrow().waker.set_ping(wake().ping());
    rt.load_singletons_where(
        &[root.join("services"), root.join("services/network")],
        &|n| n == "Network",
    );
    let r = rt
        .load_str(
            "import QtQuick\nimport ri.services\nItem {\n\
             property bool on: Network.wifiEnabled\n\
             property bool scanning: Network.wifiScanning\n\
             property int step: 0\n\
             onStepChanged: { if (step == 1) Network.enableWifi(true); else if (step == 2) Network.rescanWifi() } }",
        )
        .unwrap();
    rt.settle();
    (rt, r)
}

/// A NetworkManager that never answers the radio switch holds only its own calls.
#[test]
fn held_network_manager_write_leaves_the_pool_free() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::held_network_manager_write_leaves_the_pool_free"
    )) else {
        return;
    };
    let (mut rt, r) = network_qml();
    let mut n = Fake::start(&g, "nm", &nm_init());
    wait(&mut rt, r, "on", "true");
    n.cmd(r#"{"op":"flag","name":"hold_set","value":true}"#);
    set(&mut rt, r, "step", 1.0);
    n.expect("HELD SET WirelessEnabled");
    pool_free();
}

/// A NetworkManager that never answers a scan request holds only its own calls.
#[test]
fn held_network_manager_scan_leaves_the_pool_free() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::held_network_manager_scan_leaves_the_pool_free"
    )) else {
        return;
    };
    let (mut rt, r) = network_qml();
    let mut n = Fake::start(&g, "nm", &nm_init());
    wait(&mut rt, r, "on", "true");
    n.cmd(r#"{"op":"flag","name":"hold_scan","value":true}"#);
    set(&mut rt, r, "step", 2.0);
    n.expect(&format!("HELD RequestScan {WIFI_DEV}"));
    pool_free();
}

/// How long the tests of a daemon that never answers let a reply take, instead of the 25 s the
/// shell gives it. The tests wait for what the end of that wait does, not for the time.
fn short_replies() {
    qml::sysbus::set_reply_timeout(std::time::Duration::from_millis(200));
}

/// A NetworkManager that never answers a scan request: the scan ends (`wifiScanning` goes
/// back), and the next action reaches NetworkManager instead of waiting behind it for ever.
#[test]
fn silent_network_manager_scan_ends_and_the_next_action_runs() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::silent_network_manager_scan_ends_and_the_next_action_runs"
    )) else {
        return;
    };
    short_replies();
    let (mut rt, r) = network_qml();
    let mut n = Fake::start(&g, "nm", &nm_init());
    wait(&mut rt, r, "on", "true");
    n.cmd(r#"{"op":"flag","name":"hold_scan","value":true}"#);
    set(&mut rt, r, "step", 2.0);
    assert_eq!(val(&rt, r, "scanning"), "true");
    n.expect(&format!("HELD RequestScan {WIFI_DEV}"));
    wait(&mut rt, r, "scanning", "false");
    set(&mut rt, r, "step", 1.0);
    n.expect(&format!("SET {NM_PATH} WirelessEnabled true"));
}

/// A NetworkManager that never answers the radio switch: the write ends as a refused one, and
/// the scan asked for after it reaches NetworkManager.
#[test]
fn silent_network_manager_write_ends_and_the_next_action_runs() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::silent_network_manager_write_ends_and_the_next_action_runs"
    )) else {
        return;
    };
    short_replies();
    let (mut rt, r) = network_qml();
    let mut n = Fake::start(&g, "nm", &nm_init());
    wait(&mut rt, r, "on", "true");
    n.cmd(r#"{"op":"flag","name":"hold_set","value":true}"#);
    set(&mut rt, r, "step", 1.0);
    n.expect("HELD SET WirelessEnabled");
    set(&mut rt, r, "step", 2.0);
    n.expect(&format!("SCAN {WIFI_DEV}"));
}

/// A NetworkManager that has its name and cannot give its objects yet is not shown as one with
/// no devices for good: its objects are read again on its next signal.
#[test]
fn network_manager_objects_are_read_again_after_a_failed_read() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::network_manager_objects_are_read_again_after_a_failed_read"
    )) else {
        return;
    };
    let mut n = Fake::start(&g, "nm", &nm_init());
    n.cmd(r#"{"op":"flag","name":"fail_managed","value":true}"#);
    let (mut rt, r) = network_qml();
    until(&mut rt, "the failed read", |_| {
        printed(&n, "FAILED GetManagedObjects")
    });
    assert_eq!(val(&rt, r, "on"), "false");
    n.cmd(r#"{"op":"flag","name":"fail_managed","value":false}"#);
    // Any signal of NetworkManager's: this one changes nothing that is shown.
    n.cmd(&format!(
        r#"{{"op":"set","path":"{WIFI_DEV}","iface":"org.freedesktop.NetworkManager.Device.Wireless","props":{{"LastScan":1}}}}"#
    ));
    wait(&mut rt, r, "on", "true");
}

/// A bluetoothd that never answers a property write: the property shows bluetoothd's value
/// again, and the next write reaches bluetoothd.
#[test]
fn silent_bluetooth_write_ends_and_the_next_write_runs() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::silent_bluetooth_write_ends_and_the_next_write_runs"
    )) else {
        return;
    };
    short_replies();
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Bluetooth\nItem { id: r\n\
         property var ad: Bluetooth.defaultAdapter\n\
         property bool ready: ad != null\n\
         property bool disc: ad ? ad.discoverable : false\n\
         property int step: 0\n\
         onStepChanged: { if (step == 1) ad.discoverable = true; else ad.pairable = false } }",
    );
    let mut b = Fake::start(&g, "bluez", &bluez_init());
    wait(&mut rt, r, "ready", "true");
    assert_eq!(val(&rt, r, "disc"), "false");
    b.cmd(r#"{"op":"flag","name":"hold_set","value":true}"#);
    set(&mut rt, r, "step", 1.0);
    b.expect("HELD SET Discoverable");
    wait(&mut rt, r, "disc", "false");
    set(&mut rt, r, "step", 2.0);
    b.expect("HELD SET Pairable");
}

/// A power-profiles-daemon that never answers the profile write: the profile shows the
/// daemon's again, and the next write reaches the daemon.
#[test]
fn silent_power_profile_write_ends_and_the_next_write_runs() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::silent_power_profile_write_ends_and_the_next_write_runs"
    )) else {
        return;
    };
    short_replies();
    let (mut rt, r) = ppd_qml();
    let mut p = Fake::start(&g, "ppd", PPD_INIT);
    wait(&mut rt, r, "perf", "true");
    assert_eq!(val(&rt, r, "prof"), "1");
    p.cmd(r#"{"op":"flag","name":"hold_set","value":true}"#);
    set(&mut rt, r, "req", 2.0);
    p.expect("HELD SET ActiveProfile");
    wait(&mut rt, r, "prof", "1");
    set(&mut rt, r, "req", 0.0);
    p.expect("HELD SET ActiveProfile");
}

/// A notification daemon that never answers while the pairing agent shows a code to type: the
/// next code is shown all the same.
#[test]
fn silent_notification_daemon_does_not_hold_the_pairing_codes() {
    let Some(g) = bus(concat!(
        module_path!(),
        "::silent_notification_daemon_does_not_hold_the_pairing_codes"
    )) else {
        return;
    };
    short_replies();
    let (mut rt, r) = load(
        "import QtQuick\nimport Rustshell.Bluetooth\nItem { property int nd: Bluetooth.devices.values.length }",
    );
    let mut n = Fake::start(&g, "notifd", "{}");
    let mut f = Fake::start(&g, "bluez", &bluez_init());
    wait(&mut rt, r, "nd", "2");
    f.expect(&format!("AGENT RequestDefaultAgent {AGENT}"));
    n.cmd(r#"{"op":"flag","name":"hold_notify","value":true}"#);
    agent(
        &mut f,
        "DisplayPasskey",
        &format!(r#"["{DEV_B}", 1234, 0]"#),
    );
    n.expect("HELD Notify Type 001234 on MX Keys");
    n.cmd(r#"{"op":"flag","name":"hold_notify","value":false}"#);
    agent(&mut f, "DisplayPinCode", &format!(r#"["{DEV_B}", "0000"]"#));
    assert!(
        notified(&n)
            .1
            .contains("Type 0000 on MX Keys, then press Enter")
    );
}

const WAFFLE_PAGES: &str = r#"import QtQuick
import Rustshell.Bluetooth
import ri.modules.waffle.actionCenter.nightLight
import ri.modules.waffle.actionCenter.bluetooth
Item {
    id: r
    width: 400
    height: 600
    property bool night: false
    property bool bluetooth: false
    property bool hasDefault: Bluetooth.defaultAdapter !== null
    property bool discovering: Bluetooth.defaultAdapter?.discovering ?? false
    property bool nightOpen: nightPage.item !== null
    property bool bluetoothOpen: bluetoothPage.item !== null
    Loader { id: nightPage; anchors.fill: parent; active: r.night; sourceComponent: NightLightControl {} }
    Loader { id: bluetoothPage; anchors.fill: parent; active: r.bluetooth; sourceComponent: BluetoothControl {} }
}
"#;

/// The waffle action center's night light page leaves Bluetooth discovery alone; its Bluetooth
/// page scans while it is open.
#[test]
fn waffle_only_the_bluetooth_page_scans() {
    const TEST: &str = concat!(module_path!(), "::waffle_only_the_bluetooth_page_scans");
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::Builder::new()
            .prefix("waffle-bluetooth-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        let mut vars = testenv::home_vars(dir.path());
        vars.push(("DBUS_SYSTEM_BUS_ADDRESS", addr.clone().into()));
        vars.push(("DBUS_SESSION_BUS_ADDRESS", addr.into()));
        testenv::run_in_own_process(TEST, vars);
        return;
    }
    let Some(g) = bus(TEST) else {
        return;
    };
    // The pages beside the tree's shell.qml, as the files that use them are.
    let qml = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join("tree");
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(qml).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("zzmain.qml"), WAFFLE_PAGES).unwrap();
    // Midday for the engine: the original's night light schedule (Hyprsunset, loaded with the services)
    // decides nothing here from the hour the test runs at.
    qml::builtins::set_clock(Some(testenv::local_time_ms(12, 0)));
    let mut rt = Runtime::new(Some(d.clone()));
    rt.world.borrow().waker.set_ping(wake().ping());
    rt.load_singletons(&[
        d.clone(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
        d.join("modules/waffle/looks"),
    ]);
    rt.settle();
    let r = rt.load_file(&d.join("zzmain.qml")).unwrap();
    turn(&mut rt);
    let f = Fake::start(&g, "bluez", &bluez_init());
    // The tree's services run processes: the loop waits on their descriptor too.
    let pump = |rt: &mut Runtime, what: &str, done: &mut dyn FnMut(&mut Runtime) -> bool| {
        eprintln!("waiting for {what}");
        let fd = rt.process_fd();
        loop {
            workpool::wait_idle();
            let rev = rt.revision();
            rt.poll_native();
            rt.settle();
            if rt.revision() != rev {
                continue;
            }
            if done(rt) {
                return;
            }
            if wake().wait(fd, None).fd {
                rt.process_fd_ready();
            }
        }
    };
    let printed = |rt: &mut Runtime, s: &str| {
        pump(rt, &format!("the fake to print {s:?}"), &mut |_| loop {
            match f.out.try_recv() {
                Ok(l) if l.contains(s) => return true,
                Ok(_) => {}
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => panic!("fake exited without printing {s:?}"),
            }
        });
    };
    let reads = |rt: &mut Runtime, prop: &str, want: &str| {
        pump(rt, &format!("{prop} = {want:?}"), &mut |rt| {
            val(rt, r, prop) == want
        });
    };
    let show = |rt: &mut Runtime, page: &str, on: bool| {
        rt.world.borrow_mut().write(r, page, Value::Bool(on));
        rt.settle();
        rt.poll_native();
    };
    reads(&mut rt, "hasDefault", "true");

    show(&mut rt, "night", true);
    assert_eq!(val(&rt, r, "nightOpen"), "true");
    f.quiet(&mut rt, "Discovery");
    show(&mut rt, "night", false);
    f.quiet(&mut rt, "Discovery");
    assert_eq!(val(&rt, r, "discovering"), "false");

    show(&mut rt, "bluetooth", true);
    assert_eq!(val(&rt, r, "bluetoothOpen"), "true");
    printed(&mut rt, "CALL /org/bluez/hci0 StartDiscovery");
    reads(&mut rt, "discovering", "true");
    show(&mut rt, "bluetooth", false);
    printed(&mut rt, "CALL /org/bluez/hci0 StopDiscovery");
    reads(&mut rt, "discovering", "false");
    assert_eq!(rt.errors(), Vec::<String>::new());
}
