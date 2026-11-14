//! Native `Rustshell.Bluetooth`: BlueZ adapters and devices from org.bluez's ObjectManager,
//! with hotplug, property writes/methods from QML and bluetoothd restart recovery.

use crate::mpris::V;
use crate::runtime::{Runtime, StopOnDrop, World};
use crate::sysbus::{self, Delta, Ifaces, Props};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use zbus::blocking::Connection;
use zbus::message::Message;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const BLUEZ: &str = "org.bluez";

/// bluetoothd calls made off the main thread, one at a time in the order made: a bluetoothd that
/// never answers holds only this lane, never the shared workers of `workpool::run`. Pairing and
/// connecting, which take long by design, run on `workpool::long`.
pub(crate) static CALLS: workpool::Lane = workpool::Lane::new();

/// Calls and property writes handed to a thread and not written to the bus yet, and the
/// wakeup of [`flush_until`] when none are left.
static UNSENT: (std::sync::Mutex<usize>, std::sync::Condvar) =
    (std::sync::Mutex::new(0), std::sync::Condvar::new());

/// One call or write from when it is handed to a thread until that thread wrote it to the bus
/// (or it was dropped unsent).
struct Unsent;

impl Unsent {
    fn new() -> Unsent {
        *UNSENT.0.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        Unsent
    }
}

impl Drop for Unsent {
    fn drop(&mut self) {
        *UNSENT.0.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        UNSENT.1.notify_all();
    }
}

/// Waits until the calls and writes made so far are on the bus, for a process about to end:
/// `discovering = false` in `Component.onDestruction` at `Qt.quit()` stops the discovery as it
/// does in Quickshell, which writes the call before it exits. Nothing waits for bluetoothd's
/// answer, as Quickshell does not: with nothing left to write this returns at once. A write
/// still queued behind one bluetoothd has not answered (they go one at a time, [`CALLS`]) is
/// waited for until `until`, then the process ends without it: `false`.
pub fn flush_until(until: std::time::Instant) -> bool {
    let n = UNSENT.0.lock().unwrap_or_else(|e| e.into_inner());
    let left = until.saturating_duration_since(std::time::Instant::now());
    let (_n, limit) = UNSENT
        .1
        .wait_timeout_while(n, left, |n| *n > 0)
        .unwrap_or_else(|e| e.into_inner());
    !limit.timed_out()
}

/// [`flush_until`] with the limit the shell's exit gives every service together
/// ([`crate::notifier::FLUSH`]).
pub fn flush() -> bool {
    flush_until(std::time::Instant::now() + crate::notifier::FLUSH)
}

const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const BATTERY: &str = "org.bluez.Battery1";
const OM: &str = "org.freedesktop.DBus.ObjectManager";

/// Objects in tracking order (quickshell's ObjectModels append on InterfacesAdded).
type Objects = Vec<(String, Ifaces)>;

enum Ev {
    Conn(Connection),
    Deltas(Vec<Delta<Objects>>),
    /// A Connect/Disconnect call failed: quickshell resets `state` from `connected`.
    ConnectFailed(String),
    /// Pair/CancelPairing finished: `pairing` goes false.
    PairDone(String),
    /// bluez refused write `n` of `prop` on the object at `path`.
    SetFailed {
        path: String,
        prop: &'static str,
        n: u64,
    },
}

/// The interfaces the Bluetooth types are made of; bluetoothd's other objects and interfaces
/// (media endpoints, players, GATT) never reach the UI thread.
fn used(iface: &str) -> bool {
    matches!(iface, ADAPTER | DEVICE | BATTERY)
}

fn conv_ifaces(m: HashMap<String, HashMap<String, OwnedValue>>) -> Ifaces {
    m.into_iter()
        .filter(|(k, _)| used(k))
        .map(|(k, v)| (k, sysbus::conv_map(v)))
        .collect()
}

fn sync(conn: &Connection) -> Objects {
    type Managed = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;
    let r = sysbus::call(conn, BLUEZ, "/", OM, "GetManagedObjects", &());
    let Ok(m) = r.and_then(|r| r.body().deserialize::<Managed>()) else {
        return vec![];
    };
    let mut v: Objects = m
        .into_iter()
        .map(|(p, i)| (p.to_string(), conv_ifaces(i)))
        .filter(|(_, i)| !i.is_empty())
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn handle(conn: &Connection, m: &Message) -> Option<Delta<Objects>> {
    if let Some((name, owned)) = sysbus::owner_changed(m) {
        if name != BLUEZ {
            return None;
        }
        if !owned {
            crate::bluez_agent::bluez_gone(conn);
            return Some(Delta::Reset(vec![]));
        }
        let st = sync(conn);
        crate::bluez_agent::register(conn);
        return Some(Delta::Reset(st));
    }
    if let Some(c) = sysbus::props_changed(m) {
        return used(&c.iface).then(|| Delta::Changed(sysbus::resolve(conn, BLUEZ, c)));
    }
    let (iface, member, _) = sysbus::member(m);
    if iface != OM {
        return None;
    }
    match member.as_str() {
        "InterfacesAdded" => {
            type Added = (
                OwnedObjectPath,
                HashMap<String, HashMap<String, OwnedValue>>,
            );
            let (path, ifaces) = m.body().deserialize::<Added>().ok()?;
            let ifaces = conv_ifaces(ifaces);
            (!ifaces.is_empty()).then(|| Delta::Added(path.to_string(), ifaces))
        }
        "InterfacesRemoved" => {
            let (path, mut ifaces) = m
                .body()
                .deserialize::<(OwnedObjectPath, Vec<String>)>()
                .ok()?;
            ifaces.retain(|i| used(i));
            (!ifaces.is_empty()).then(|| Delta::Removed(path.to_string(), ifaces))
        }
        _ => None,
    }
}

fn session(conn: &Connection, rx: &Receiver<Message>, tx: &crate::svc::Sender<Ev>) -> bool {
    if tx.send(Ev::Conn(conn.clone())).is_err() {
        return false;
    }
    let st = sync(conn);
    crate::bluez_agent::register(conn);
    sysbus::relay(
        rx,
        st,
        |m| handle(conn, m),
        |ds| tx.send(Ev::Deltas(ds)).is_ok(),
    )
}

fn rules() -> Vec<zbus::MatchRule<'static>> {
    [
        sysbus::iface_rule(BLUEZ, OM, "/"),
        sysbus::props_rule(BLUEZ, "/org/bluez"),
        sysbus::owner_rule(BLUEZ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

struct Obj {
    item: usize,
    last: Ifaces,
    /// Writable properties as last applied by us (a different value means QML wrote it).
    shown: HashMap<&'static str, Value>,
    /// The latest write sent per property: only its refusal restores bluez's value.
    sent: HashMap<&'static str, u64>,
}

pub struct Bluez {
    rx: Receiver<Ev>,
    tx: crate::svc::Sender<Ev>,
    conn: Option<Connection>,
    adapters: Vec<(String, Obj)>,
    devices: Vec<(String, Obj)>,
    /// Objects with only a Battery1 so far: they become devices once Device1 is added.
    others: HashMap<String, Ifaces>,
    /// Property writes sent so far.
    writes: u64,
    /// Ends the BlueZ worker with this instance (dropped before the agent hold, so no session
    /// registers the agent again after its withdrawal).
    _stop: StopOnDrop,
    /// Keeps the pairing agent served and registered while a Bluetooth service lives.
    _agent: crate::bluez_agent::Hold,
}

fn start(h: &crate::svc::Handle) -> Bluez {
    let (tx, rx) = h.channel();
    let t = tx.clone();
    let agent = crate::bluez_agent::hold();
    let stop = StopOnDrop::default();
    sysbus::spawn(rules(), stop.0.clone(), move |c, r| session(c, r, &t));
    Bluez {
        rx,
        tx,
        conn: None,
        adapters: vec![],
        devices: vec![],
        others: HashMap::new(),
        writes: 0,
        _stop: stop,
        _agent: agent,
    }
}

const ADAPTER_W: [(&str, &str); 6] = [
    ("enabled", "Powered"),
    ("discoverable", "Discoverable"),
    ("discoverableTimeout", "DiscoverableTimeout"),
    ("discovering", "Discovering"),
    ("pairable", "Pairable"),
    ("pairableTimeout", "PairableTimeout"),
];
const DEVICE_W: [(&str, &str); 5] = [
    ("name", "Alias"),
    ("connected", "Connected"),
    ("trusted", "Trusted"),
    ("blocked", "Blocked"),
    ("wakeAllowed", "WakeAllowed"),
];

fn to_value(v: &V) -> Option<Value> {
    Some(match v {
        V::S(s) => Value::str(s),
        V::B(b) => Value::Bool(*b),
        V::I(i) => Value::Num(*i as f64),
        V::F(f) => Value::Num(*f),
        _ => return None,
    })
}

fn put(w: &mut World, o: &mut Obj, prop: &'static str, v: Value) {
    w.write(o.item, prop, v.clone());
    o.shown.insert(prop, v);
}

fn adapter_state(s: &str) -> Option<f64> {
    Some(match s {
        "off" => 0.0,
        "on" => 1.0,
        "off-enabling" => 2.0,
        "on-disabling" => 3.0,
        "off-blocked" => 4.0,
        _ => return None,
    })
}

fn apply_adapter(w: &mut World, o: &mut Obj, ch: &Props) {
    if let Some(v) = sysbus::string(ch.get("Alias")) {
        w.write(o.item, "name", Value::str(&v));
    }
    if let Some(v) = sysbus::string(ch.get("PowerState")).and_then(|s| adapter_state(&s)) {
        w.write(o.item, "state", Value::Num(v));
    }
    for (prop, wire) in ADAPTER_W {
        if let Some(v) = ch.get(wire).and_then(to_value) {
            put(w, o, prop, v);
        }
    }
}

fn apply_device(w: &mut World, o: &mut Obj, ch: &Props) {
    for (prop, wire) in [
        ("address", "Address"),
        ("deviceName", "Name"),
        ("icon", "Icon"),
    ] {
        if let Some(v) = sysbus::string(ch.get(wire)) {
            w.write(o.item, prop, Value::str(&v));
        }
    }
    for (prop, wire) in [("paired", "Paired"), ("bonded", "Bonded")] {
        if let Some(v) = sysbus::boolean(ch.get(wire)) {
            w.write(o.item, prop, Value::Bool(v));
        }
    }
    for (prop, wire) in DEVICE_W {
        if let Some(v) = ch.get(wire).and_then(to_value) {
            put(w, o, prop, v);
        }
    }
    // BluetoothDevice::onConnectedChanged.
    if let Some(c) = sysbus::boolean(ch.get("Connected")) {
        w.write(o.item, "state", Value::Num(if c { 1.0 } else { 0.0 }));
    }
}

fn apply_battery(w: &mut World, item: usize, old: Option<&Props>, new: Option<&Props>) {
    match new {
        Some(p) => {
            w.write(item, "batteryAvailable", Value::Bool(true));
            let ch = sysbus::diff(old, p);
            if let Some(v) = sysbus::num(ch.get("Percentage")) {
                w.write(item, "battery", Value::Num(v * 0.01));
            }
        }
        None if old.is_some() => {
            w.write(item, "batteryAvailable", Value::Bool(false));
            w.write(item, "battery", Value::Num(0.0));
        }
        None => {}
    }
}

fn adapter_path(o: &Obj) -> String {
    o.last
        .get(DEVICE)
        .and_then(|p| sysbus::string(p.get("Adapter")))
        .unwrap_or_default()
}

fn new_obj(w: &mut World, ty: &str, path: &str) -> Obj {
    let item = crate::types::native_singleton(w, ty);
    w.write_watch.insert(item, crate::svc::Svc::Bluez);
    w.mirror(item, crate::svc::Svc::Bluez);
    w.write(item, "dbusPath", Value::str(path));
    if ty == "BluetoothAdapter" {
        let id = path.rsplit('/').next().unwrap_or(path);
        w.write(item, "adapterId", Value::str(id));
    }
    Obj {
        item,
        last: Ifaces::new(),
        shown: HashMap::new(),
        sent: HashMap::new(),
    }
}

/// Write what changed between `o.last` and `ifaces` onto the item, then keep `ifaces`.
fn refresh(w: &mut World, o: &mut Obj, ifaces: Ifaces) {
    if let Some(p) = ifaces.get(ADAPTER) {
        let ch = sysbus::diff(o.last.get(ADAPTER), p);
        apply_adapter(w, o, &ch);
    } else if let Some(p) = ifaces.get(DEVICE) {
        let ch = sysbus::diff(o.last.get(DEVICE), p);
        apply_device(w, o, &ch);
        apply_battery(w, o.item, o.last.get(BATTERY), ifaces.get(BATTERY));
    }
    o.last = ifaces;
}

/// Reconcile tracked objects with a snapshot: an object is an adapter/device while it carries
/// org.bluez.Adapter1/Device1 (quickshell `Bluez::onInterfacesAdded/Removed`).
fn apply(w: &mut World, b: &mut Bluez, objs: Objects) {
    let mut adapters = vec![];
    let mut devices = vec![];
    b.others.clear();
    for (path, ifaces) in objs {
        let is_adapter = ifaces.contains_key(ADAPTER);
        if !is_adapter && !ifaces.contains_key(DEVICE) {
            b.others.insert(path, ifaces);
            continue;
        }
        let list = if is_adapter {
            &mut b.adapters
        } else {
            &mut b.devices
        };
        let mut o = match list.iter().position(|(p, _)| *p == path) {
            Some(k) => list.remove(k).1,
            None if is_adapter => new_obj(w, "BluetoothAdapter", &path),
            None => new_obj(w, "BluetoothDevice", &path),
        };
        refresh(w, &mut o, ifaces);
        if is_adapter {
            adapters.push((path, o));
        } else {
            devices.push((path, o));
        }
    }
    let gone: Vec<usize> = std::mem::replace(&mut b.adapters, adapters)
        .into_iter()
        .chain(std::mem::replace(&mut b.devices, devices))
        .map(|(_, o)| o.item)
        .collect();
    relink(w, b);
    for i in gone {
        w.destroy(i);
    }
}

/// Device → adapter links and the models, after objects came, went or moved.
fn relink(w: &mut World, b: &Bluez) {
    let Some(&root) = w.singletons.get("Bluetooth") else {
        return;
    };
    for (_, d) in &b.devices {
        let ap = adapter_path(d);
        let a = b.adapters.iter().find(|(p, _)| *p == ap);
        w.write(
            d.item,
            "adapter",
            a.map_or(Value::Null, |(_, a)| Value::Item(a.item)),
        );
    }
    let ids: Vec<usize> = b.adapters.iter().map(|(_, o)| o.item).collect();
    crate::upower::set_model(w, root, "adapters", &ids);
    let ids: Vec<usize> = b.devices.iter().map(|(_, o)| o.item).collect();
    crate::upower::set_model(w, root, "devices", &ids);
    for (ap, a) in &b.adapters {
        let ids: Vec<usize> = b
            .devices
            .iter()
            .filter(|(_, d)| adapter_path(d) == *ap)
            .map(|(_, d)| d.item)
            .collect();
        crate::upower::set_model(w, a.item, "devices", &ids);
    }
    let def = b
        .adapters
        .first()
        .map_or(Value::Null, |(_, a)| Value::Item(a.item));
    w.write(root, "defaultAdapter", def);
}

/// Give the object at `path` its new interfaces, touching nothing else: it is created,
/// updated in place, or dropped (into `gone`) as it gains or loses Adapter1/Device1. True
/// when the models or device → adapter links need [`relink`].
fn upsert(w: &mut World, b: &mut Bluez, path: &str, ifaces: Ifaces, gone: &mut Vec<usize>) -> bool {
    let is_adapter = ifaces.contains_key(ADAPTER);
    let qualifies = is_adapter || ifaces.contains_key(DEVICE);
    b.others.remove(path);
    let (list, other) = if is_adapter {
        (&mut b.adapters, &mut b.devices)
    } else {
        (&mut b.devices, &mut b.adapters)
    };
    let untrack = |l: &mut Vec<(String, Obj)>, gone: &mut Vec<usize>| {
        let k = l.iter().position(|(p, _)| p == path);
        k.map(|k| gone.push(l.remove(k).1.item)).is_some()
    };
    // An object of the other kind under this path (or of no kind any more) is dropped.
    let mut moved = untrack(other, gone);
    if !qualifies {
        moved |= untrack(list, gone);
        if !ifaces.is_empty() {
            b.others.insert(path.to_string(), ifaces);
        }
        return moved;
    }
    match list.iter_mut().find(|(p, _)| p == path) {
        Some((_, o)) => {
            let before = adapter_path(o);
            refresh(w, o, ifaces);
            moved || adapter_path(o) != before
        }
        None => {
            let ty = if is_adapter {
                "BluetoothAdapter"
            } else {
                "BluetoothDevice"
            };
            let mut o = new_obj(w, ty, path);
            refresh(w, &mut o, ifaces);
            list.push((path.to_string(), o));
            true
        }
    }
}

/// The object's interfaces as last seen.
fn current(b: &Bluez, path: &str) -> Option<Ifaces> {
    b.adapters
        .iter()
        .chain(&b.devices)
        .find(|(p, _)| p == path)
        .map(|(_, o)| o.last.clone())
        .or_else(|| b.others.get(path).cloned())
}

/// Apply one worker delta; true when [`relink`] is due.
fn apply_delta(w: &mut World, b: &mut Bluez, d: Delta<Objects>, gone: &mut Vec<usize>) -> bool {
    let (path, ifaces) = match d {
        Delta::Reset(objs) => {
            apply(w, b, objs);
            return false;
        }
        Delta::Changed(c) => {
            // A change of an interface the object does not carry (yet) is ignored, as before.
            let Some(mut ifaces) = current(b, &c.path).filter(|i| i.contains_key(&c.iface)) else {
                return false;
            };
            if let Some(p) = ifaces.get_mut(&c.iface) {
                sysbus::apply_changed(p, &c);
            }
            (c.path, ifaces)
        }
        Delta::Added(path, added) => {
            let mut ifaces = current(b, &path).unwrap_or_default();
            for (i, props) in added {
                ifaces.entry(i).or_default().extend(props);
            }
            (path, ifaces)
        }
        Delta::Removed(path, removed) => {
            let Some(mut ifaces) = current(b, &path) else {
                return false;
            };
            for i in &removed {
                ifaces.remove(i);
            }
            (path, ifaces)
        }
    };
    upsert(w, b, &path, ifaces, gone)
}

/// Apply a batch of deltas: only the objects named in it are touched, and the models are
/// rebuilt only when membership or links changed.
fn apply_deltas(w: &mut World, b: &mut Bluez, ds: Vec<Delta<Objects>>) {
    let mut gone = vec![];
    let mut relink_due = false;
    for d in ds {
        relink_due |= apply_delta(w, b, d, &mut gone);
    }
    if relink_due {
        relink(w, b);
    }
    for i in gone {
        w.destroy(i);
    }
}

fn num(w: &mut World, i: usize, p: &str) -> f64 {
    w.read(i, p).map_or(0.0, |v| v.to_number())
}

fn flag(w: &mut World, i: usize, p: &str) -> bool {
    w.read(i, p).is_some_and(|v| v.truthy())
}

/// Run a BlueZ method off the main thread; `done(ok)` reports back to the poller.
fn call(
    b: &Bluez,
    path: &str,
    iface: &'static str,
    method: &'static str,
    arg: Option<String>,
    done: Option<fn(String) -> Ev>,
) {
    let Some(conn) = b.conn.clone() else {
        elog::eprintln!("Bluetooth: bluez is not connected, cannot {method} {path}");
        return;
    };
    let (path, tx) = (path.to_string(), b.tx.clone());
    let unsent = Unsent::new();
    workpool::long(move || {
        let sent = move || drop(unsent);
        let to = (BLUEZ, path.as_str(), iface, method);
        let r = match &arg {
            Some(a) => match OwnedObjectPath::try_from(a.as_str()) {
                Ok(p) => sysbus::call_sent(&conn, to, &(p,), sent),
                Err(e) => Err(e.into()),
            },
            None => sysbus::call_sent(&conn, to, &(), sent),
        };
        let failed = r.is_err();
        if let Err(e) = r {
            elog::eprintln!("Bluetooth: {method} on {path} failed: {e}");
        }
        match done {
            Some(f) if failed || method.contains("Pair") => {
                let _ = tx.send(f(path));
            }
            _ => {}
        }
    });
}

/// BluetoothDevice::connect()/disconnect().
fn set_connected(w: &mut World, b: &Bluez, path: &str, item: usize, on: bool) {
    let (connected, st) = (flag(w, item, "connected"), num(w, item, "state"));
    if on == connected {
        let what = if on { "connected" } else { "disconnected" };
        elog::eprintln!("Bluetooth: device {path} is already {what}");
        return;
    }
    let (busy, method) = if on {
        (3.0, "Connect")
    } else {
        (2.0, "Disconnect")
    };
    if st == busy {
        elog::eprintln!(
            "Bluetooth: device {path} is already {}",
            method.to_lowercase() + "ing"
        );
        return;
    }
    w.write(item, "state", Value::Num(busy));
    call(b, path, DEVICE, method, None, Some(Ev::ConnectFailed));
}

fn method(w: &mut World, b: &Bluez, name: &str, item: usize) {
    let Some((path, d)) = b.devices.iter().find(|(_, d)| d.item == item) else {
        return;
    };
    match name {
        "connect" => set_connected(w, b, path, item, true),
        "disconnect" => set_connected(w, b, path, item, false),
        "pair" => {
            if flag(w, item, "paired") {
                elog::eprintln!("Bluetooth: device {path} is already paired");
            } else if flag(w, item, "pairing") {
                elog::eprintln!("Bluetooth: device {path} is already pairing");
            } else {
                w.write(item, "pairing", Value::Bool(true));
                call(b, path, DEVICE, "Pair", None, Some(Ev::PairDone));
            }
        }
        "cancelPair" => {
            if !flag(w, item, "pairing") {
                elog::eprintln!("Bluetooth: device {path} is not currently pairing");
            } else {
                call(b, path, DEVICE, "CancelPairing", None, Some(Ev::PairDone));
            }
        }
        "forget" => {
            let ap = adapter_path(d);
            if b.adapters.iter().any(|(p, _)| *p == ap) {
                call(b, &ap, ADAPTER, "RemoveDevice", Some(path.clone()), None);
            } else {
                elog::eprintln!("Bluetooth: could not find adapter {ap} to forget {path} from");
            }
        }
        _ => {}
    }
}

/// `Properties.Set` of a QML write; a refusal comes back as `Ev::SetFailed`.
fn send_write(
    b: &mut Bluez,
    o: &mut Obj,
    path: &str,
    iface: &str,
    prop: &'static str,
    wire: &str,
    v: zbus::zvariant::Value<'static>,
) {
    let Some(c) = &b.conn else {
        return;
    };
    b.writes += 1;
    o.sent.insert(prop, b.writes);
    let (tx, path2, n) = (b.tx.clone(), path.to_string(), b.writes);
    let unsent = Unsent::new();
    let sent = move || drop(unsent);
    sysbus::set_prop_sent(&CALLS, c, BLUEZ, path, iface, wire, v, sent, move || {
        let _ = tx.send(Ev::SetFailed {
            path: path2,
            prop,
            n,
        });
    });
}

/// Shows bluez's value again after it refused the latest write of `prop`, as the
/// PropertiesChanged it never sends would have.
fn restore(w: &mut World, b: &mut Bluez, path: &str, prop: &'static str, n: u64) {
    let (objs, iface, table) = if b.adapters.iter().any(|(p, _)| p == path) {
        (&mut b.adapters, ADAPTER, &ADAPTER_W[..])
    } else {
        (&mut b.devices, DEVICE, &DEVICE_W[..])
    };
    let Some((_, o)) = objs.iter_mut().find(|(p, _)| p == path) else {
        return;
    };
    if o.sent.get(prop) != Some(&n) {
        return;
    }
    let Some(wire) = table.iter().find(|(q, _)| *q == prop).map(|(_, w)| *w) else {
        return;
    };
    if let Some(v) = o
        .last
        .get(iface)
        .and_then(|m| m.get(wire))
        .and_then(to_value)
    {
        put(w, o, prop, v);
    }
}

/// Property writes from QML, detected against the last applied value (quickshell setters).
/// Forwards QML writes on the `dirty` objects to bluez.
fn writes(w: &mut World, b: &mut Bluez, dirty: &[usize]) -> bool {
    let mut changed = false;
    let mut adapters = std::mem::take(&mut b.adapters);
    for (path, o) in adapters.iter_mut().filter(|(_, o)| dirty.contains(&o.item)) {
        for (prop, wire) in ADAPTER_W {
            let Some(cur) = w.read(o.item, prop) else {
                continue;
            };
            let Some(old) = o.shown.get(prop).cloned() else {
                continue;
            };
            if cur.strict_eq(&old)
                || matches!((&cur, &old), (Value::Num(a), Value::Num(b)) if a == b)
            {
                continue;
            }
            changed = true;
            match prop {
                "discovering" => {
                    // setDiscovering only calls Start/StopDiscovery; the value follows bluez.
                    w.write(o.item, prop, old);
                    let m = if cur.truthy() {
                        "StartDiscovery"
                    } else {
                        "StopDiscovery"
                    };
                    call(b, path, ADAPTER, m, None, None);
                }
                "enabled" if cur.truthy() && num(w, o.item, "state") == 4.0 => {
                    elog::eprintln!(
                        "Bluetooth: Cannot enable adapter because it is blocked by rfkill."
                    );
                    w.write(o.item, prop, old);
                }
                _ => {
                    let v: zbus::zvariant::Value<'static> = match prop {
                        "enabled" | "discoverable" | "pairable" => cur.truthy().into(),
                        _ => (cur.to_number().max(0.0) as u32).into(),
                    };
                    o.shown.insert(prop, cur);
                    send_write(b, o, path, ADAPTER, prop, wire, v);
                }
            }
        }
    }
    b.adapters = adapters;
    let mut devices = std::mem::take(&mut b.devices);
    let mut connects = vec![];
    for (path, o) in devices.iter_mut().filter(|(_, o)| dirty.contains(&o.item)) {
        for (prop, wire) in DEVICE_W {
            let Some(cur) = w.read(o.item, prop) else {
                continue;
            };
            let Some(old) = o.shown.get(prop).cloned() else {
                continue;
            };
            if cur.strict_eq(&old) {
                continue;
            }
            changed = true;
            if prop == "connected" {
                // setConnected → connect()/disconnect(); `connected` follows bluez.
                w.write(o.item, prop, old);
                connects.push((path.clone(), o.item, cur.truthy()));
                continue;
            }
            let v: zbus::zvariant::Value<'static> = match prop {
                "name" => cur.to_js_string().into(),
                _ => cur.truthy().into(),
            };
            o.shown.insert(prop, cur);
            send_write(b, o, path, DEVICE, prop, wire, v);
        }
    }
    b.devices = devices;
    for (path, item, on) in connects {
        set_connected(w, b, &path, item, on);
    }
    changed
}

fn poll(w: &mut World, calls: Vec<crate::svc::Call>) -> bool {
    let mut b = w
        .bluez
        .take()
        .unwrap_or_else(|| start(&w.waker.handle(crate::svc::Svc::Bluez)));
    let dirty = w.take_watched(crate::svc::Svc::Bluez);
    w.native_writer = Some(crate::svc::Svc::Bluez);
    let mut changed = !dirty.is_empty() && writes(w, &mut b, &dirty);
    for crate::svc::Call { method: k, args: a } in calls {
        if let Some(Value::Item(i)) = a.first() {
            method(w, &b, &k, *i);
            changed = true;
        }
    }
    while let Ok(ev) = b.rx.try_recv() {
        changed = true;
        match ev {
            Ev::Conn(c) => b.conn = Some(c),
            Ev::Deltas(ds) => apply_deltas(w, &mut b, ds),
            Ev::ConnectFailed(p) => {
                if let Some((_, d)) = b.devices.iter().find(|(q, _)| *q == p) {
                    let c = flag(w, d.item, "connected");
                    w.write(d.item, "state", Value::Num(if c { 1.0 } else { 0.0 }));
                }
            }
            Ev::PairDone(p) => {
                if let Some((_, d)) = b.devices.iter().find(|(q, _)| *q == p) {
                    w.write(d.item, "pairing", Value::Bool(false));
                }
            }
            Ev::SetFailed { path, prop, n } => restore(w, &mut b, &path, prop, n),
        }
    }
    w.native_writer = None;
    w.bluez = Some(b);
    changed
}

/// `toString()` of the Bluetooth enum singletons.
pub(crate) fn enum_string(ty: &str, n: f64) -> Option<String> {
    let names: &[&str] = match ty {
        "BluetoothAdapterState" => &["Disabled", "Enabled", "Enabling", "Disabling", "Blocked"],
        "BluetoothDeviceState" => &["Disconnected", "Connected", "Disconnecting", "Connecting"],
        _ => return None,
    };
    let s = usize::try_from(n as i64)
        .ok()
        .filter(|_| n.fract() == 0.0)
        .and_then(|i| names.get(i).copied())
        .unwrap_or("Unknown");
    Some(s.to_string())
}

impl Runtime {
    /// Drive the `Bluetooth` singleton (if instantiated).
    pub(crate) fn poll_bluez(&mut self, calls: Vec<crate::svc::Call>) -> bool {
        if !self.world.borrow().singletons.contains_key("Bluetooth") {
            return false;
        }
        poll(&mut self.world.borrow_mut(), calls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysbus::Changed;

    const HCI0: &str = "/org/bluez/hci0";
    const A: &str = "/org/bluez/hci0/dev_A";
    const B: &str = "/org/bluez/hci0/dev_B";

    fn props(kv: &[(&str, V)]) -> Props {
        kv.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    fn device(alias: &str) -> Ifaces {
        let p = props(&[
            ("Alias", V::S(alias.into())),
            ("Adapter", V::S(HCI0.into())),
            ("Connected", V::B(false)),
        ]);
        [(DEVICE.to_string(), p)].into()
    }

    fn changed(path: &str, iface: &str, kv: &[(&str, V)]) -> Delta<Objects> {
        Delta::Changed(Changed {
            path: path.into(),
            iface: iface.into(),
            props: props(kv),
            invalidated: vec![],
        })
    }

    fn ids(w: &mut World, owner: usize, prop: &str) -> Vec<usize> {
        let Some(Value::Item(m)) = w.read(owner, prop) else {
            panic!("no {prop} model");
        };
        crate::upower::item_ids(w, m)
    }

    #[test]
    fn deltas_touch_only_the_objects_they_name() {
        let mut rt = Runtime::new(None);
        rt.load_str("import QtQuick\nimport Rustshell.Bluetooth\nItem { property int n: Bluetooth.devices.values.length }")
            .unwrap();
        rt.settle();
        let mut w = rt.world.borrow_mut();
        let root = w.singletons["Bluetooth"];
        let (tx, rx) = w.waker.handle(crate::svc::Svc::Bluez).channel();
        let mut b = Bluez {
            rx,
            tx,
            conn: None,
            adapters: vec![],
            devices: vec![],
            others: HashMap::new(),
            writes: 0,
            _stop: StopOnDrop::default(),
            _agent: crate::bluez_agent::hold(),
        };
        let adapter: Ifaces = [(
            ADAPTER.to_string(),
            props(&[("Alias", V::S("arch".into()))]),
        )]
        .into();
        apply_deltas(
            &mut w,
            &mut b,
            vec![Delta::Reset(vec![
                (HCI0.into(), adapter),
                (A.into(), device("Headphones")),
                (B.into(), device("Keyboard")),
            ])],
        );
        let (ia, ib) = (b.devices[0].1.item, b.devices[1].1.item);
        assert_eq!(ids(&mut w, root, "devices"), [ia, ib]);
        let hci = b.adapters[0].1.item;
        assert_eq!(ids(&mut w, hci, "devices"), [ia, ib]);

        // Mark the models and B's link: property deltas of A must leave them alone (no model
        // rebuild, no relinking, nothing written to B).
        let Some(Value::Item(model)) = w.read(root, "devices") else {
            panic!()
        };
        w.write(model, "values", Value::array(vec![]));
        w.write(ib, "adapter", Value::Null);
        apply_deltas(
            &mut w,
            &mut b,
            vec![
                changed(A, DEVICE, &[("RSSI", V::I(-40))]),
                changed(
                    A,
                    DEVICE,
                    &[("Alias", V::S("XM4".into())), ("RSSI", V::I(-41))],
                ),
                changed(A, "org.bluez.MediaControl1", &[("Connected", V::B(true))]),
            ],
        );
        assert_eq!(w.read(ia, "name").unwrap().to_js_string(), "XM4");
        assert_eq!(w.read(ib, "name").unwrap().to_js_string(), "Keyboard");
        assert!(ids(&mut w, root, "devices").is_empty());
        assert!(matches!(w.read(ib, "adapter"), Some(Value::Null)));
        assert_eq!(
            sysbus::num(b.devices[0].1.last[DEVICE].get("RSSI")),
            Some(-41.0)
        );

        // A battery appearing on B only touches B.
        apply_deltas(
            &mut w,
            &mut b,
            vec![Delta::Added(
                B.into(),
                [(BATTERY.to_string(), props(&[("Percentage", V::I(55))]))].into(),
            )],
        );
        assert_eq!(w.read(ib, "battery").unwrap().to_number(), 0.55);
        assert!(w.read(ib, "batteryAvailable").unwrap().truthy());
        assert!(ids(&mut w, root, "devices").is_empty());
        apply_deltas(
            &mut w,
            &mut b,
            vec![Delta::Removed(B.into(), vec![BATTERY.into()])],
        );
        assert!(!w.read(ib, "batteryAvailable").unwrap().truthy());

        // Membership changes rebuild the models and links once per batch.
        let c = "/org/bluez/hci0/dev_C";
        apply_deltas(
            &mut w,
            &mut b,
            vec![
                // A Battery1-only object is no device until its Device1 comes.
                Delta::Added(
                    c.into(),
                    [(BATTERY.to_string(), props(&[("Percentage", V::I(10))]))].into(),
                ),
                Delta::Removed(A.into(), vec![DEVICE.into()]),
            ],
        );
        assert_eq!(ids(&mut w, root, "devices"), [ib]);
        assert_eq!(ids(&mut w, hci, "devices"), [ib]);
        assert!(matches!(w.read(ib, "adapter"), Some(Value::Item(i)) if i == hci));
        assert!(b.others.contains_key(c));
        apply_deltas(
            &mut w,
            &mut b,
            vec![Delta::Added(c.into(), device("Mouse"))],
        );
        let ic = b.devices[1].1.item;
        assert_eq!(ids(&mut w, root, "devices"), [ib, ic]);
        assert_eq!(w.read(ic, "name").unwrap().to_js_string(), "Mouse");
        assert_eq!(w.read(ic, "battery").unwrap().to_number(), 0.1);
        assert!(b.others.is_empty());
    }
}
