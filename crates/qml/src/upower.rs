//! Native `Rustshell.Services.UPower`: the `UPower` singleton (display device, device model,
//! onBattery) and `PowerProfiles` (power-profiles-daemon), both on the system bus.

use crate::eval::{new_obj, set_own};
use crate::mpris::V;
use crate::runtime::{Runtime, StopOnDrop, World};
use crate::sysbus::{self, Delta, Props};
use crate::value::Value;
use std::sync::mpsc::Receiver;
use zbus::blocking::Connection;
use zbus::message::Message;

/// power-profiles-daemon calls, one at a time in the order made: a daemon that never answers
/// holds only this lane, never the shared workers of `workpool::run`.
static PROFILE_CALLS: workpool::Lane = workpool::Lane::new();

const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const DEVICE: &str = "org.freedesktop.UPower.Device";

/// Last UPower state seen by the worker; `display`/`devices` hold fully fetched (ready) devices.
#[derive(Default)]
struct UpState {
    root: Props,
    display: Option<(String, Props)>,
    devices: Vec<(String, Props)>,
}

fn device_props(conn: &Connection, path: &str) -> Option<Props> {
    sysbus::get_all(conn, UPOWER, path, DEVICE)
}

fn up_call(conn: &Connection, method: &str) -> zbus::Result<Message> {
    sysbus::call(conn, UPOWER, UPOWER_PATH, UPOWER, method, &())
}

/// Quickshell `UPower::init`: GetAll on the service, GetDisplayDevice, EnumerateDevices.
/// Calling methods on the well-known name also bus-activates UPower (`tryLaunchService`).
fn up_sync(conn: &Connection) -> UpState {
    let root = sysbus::get_all(conn, UPOWER, UPOWER_PATH, UPOWER).unwrap_or_default();
    let display = sysbus::object_path(up_call(conn, "GetDisplayDevice"))
        .and_then(|p| device_props(conn, &p).map(|pr| (p, pr)));
    let devices = sysbus::object_paths(up_call(conn, "EnumerateDevices"))
        .into_iter()
        .filter_map(|p| device_props(conn, &p).map(|pr| (p, pr)))
        .collect();
    UpState {
        root,
        display,
        devices,
    }
}

fn up_handle(conn: &Connection, m: &Message) -> Option<Delta<UpState>> {
    if let Some((name, owned)) = sysbus::owner_changed(m) {
        if name != UPOWER {
            return None;
        }
        // Vanished: drop the devices (the display device keeps its last values, it is never null).
        return Some(Delta::Reset(if owned {
            up_sync(conn)
        } else {
            UpState::default()
        }));
    }
    if let Some(c) = sysbus::props_changed(m) {
        let ours = (c.iface == UPOWER && c.path == UPOWER_PATH) || c.iface == DEVICE;
        return ours.then(|| Delta::Changed(sysbus::resolve(conn, UPOWER, c)));
    }
    let (iface, member, path) = sysbus::member(m);
    if iface != UPOWER || path != UPOWER_PATH {
        return None;
    }
    let dev = m
        .body()
        .deserialize::<zbus::zvariant::OwnedObjectPath>()
        .ok()?
        .to_string();
    match member.as_str() {
        "DeviceAdded" => {
            let props = device_props(conn, &dev)?;
            Some(Delta::Added(dev, [(DEVICE.to_string(), props)].into()))
        }
        "DeviceRemoved" => Some(Delta::Removed(dev, vec![DEVICE.to_string()])),
        _ => None,
    }
}

fn up_rules() -> Vec<zbus::MatchRule<'static>> {
    [
        sysbus::props_rule(UPOWER, UPOWER_PATH),
        sysbus::iface_rule(UPOWER, UPOWER, UPOWER_PATH),
        sysbus::owner_rule(UPOWER),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn up_session(
    conn: &Connection,
    rx: &Receiver<Message>,
    tx: &crate::svc::Sender<Vec<Delta<UpState>>>,
) -> bool {
    sysbus::relay(
        rx,
        up_sync(conn),
        |m| up_handle(conn, m),
        |ds| tx.send(ds).is_ok(),
    )
}

struct Device {
    item: usize,
    last: Props,
}

pub struct UPower {
    rx: Receiver<Vec<Delta<UpState>>>,
    root: Props,
    display: Option<(String, Props)>,
    devices: Vec<(String, Device)>,
    /// Ends the UPower worker with this instance.
    _stop: StopOnDrop,
}

fn up_start(h: &crate::svc::Handle) -> UPower {
    let (tx, rx) = h.channel();
    let stop = StopOnDrop::default();
    sysbus::spawn(up_rules(), stop.0.clone(), move |c, r| {
        up_session(c, r, &tx)
    });
    UPower {
        rx,
        root: Props::new(),
        display: None,
        devices: vec![],
        _stop: stop,
    }
}

/// Write the D-Bus properties that changed onto a `UPowerDevice` (QS_DBUS_PROPERTY_BINDINGs in
/// quickshell's device.hpp); out-of-range enums are rejected like its DBusDataTransforms.
fn apply_device(w: &mut World, i: usize, ch: &Props) {
    let n = |k: &str| sysbus::num(ch.get(k));
    let b = |k: &str| sysbus::boolean(ch.get(k));
    let s = |k: &str| sysbus::string(ch.get(k));
    if let Some(t) = n("Type").filter(|t| (0.0..=28.0).contains(t)) {
        w.write(i, "type", Value::Num(t));
    }
    if let Some(v) = b("PowerSupply") {
        w.write(i, "powerSupply", Value::Bool(v));
    }
    for (wire, prop) in [
        ("Energy", "energy"),
        ("EnergyFull", "energyCapacity"),
        ("EnergyRate", "changeRate"),
        ("TimeToEmpty", "timeToEmpty"),
        ("TimeToFull", "timeToFull"),
        ("Capacity", "healthPercentage"),
    ] {
        if let Some(v) = n(wire) {
            w.write(i, prop, Value::Num(v));
        }
    }
    if let Some(v) = n("Percentage") {
        w.write(i, "percentage", Value::Num(v * 0.01));
    }
    if let Some(v) = b("IsPresent") {
        w.write(i, "isPresent", Value::Bool(v));
    }
    if let Some(v) = n("State").filter(|v| (0.0..=6.0).contains(v)) {
        w.write(i, "state", Value::Num(v));
    }
    for (wire, prop) in [
        ("IconName", "iconName"),
        ("NativePath", "nativePath"),
        ("Model", "model"),
    ] {
        if let Some(v) = s(wire) {
            w.write(i, prop, Value::str(&v));
        }
    }
    let ty = w.read(i, "type").map_or(0.0, |v| v.to_number());
    let ps = w.read(i, "powerSupply").is_some_and(|v| v.truthy());
    w.write(i, "isLaptopBattery", Value::Bool(ty == 2.0 && ps));
    let health = w.read(i, "healthPercentage").map_or(0.0, |v| v.to_number());
    w.write(i, "healthSupported", Value::Bool(health != 0.0));
}

pub(crate) fn item_ids(w: &mut World, model: usize) -> Vec<usize> {
    match w.read(model, "values") {
        Some(Value::Obj(o)) => o
            .borrow()
            .array
            .iter()
            .flatten()
            .filter_map(|v| match v {
                Value::Item(i) => Some(*i),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

/// Set an ObjectModel's `values` when its membership or order changed.
pub(crate) fn set_model(w: &mut World, owner: usize, prop: &str, ids: &[usize]) {
    let Some(Value::Item(model)) = w.read(owner, prop) else {
        return;
    };
    if item_ids(w, model) != ids {
        let vals = ids.iter().map(|&i| Value::Item(i)).collect();
        w.write(model, "values", Value::array(vals));
    }
}

/// A full snapshot: reconcile the display device and the device model with it.
fn up_reset(w: &mut World, root: usize, u: &mut UPower, st: UpState, gone: &mut Vec<usize>) {
    let ch = sysbus::diff(Some(&u.root), &st.root);
    if let Some(v) = sysbus::boolean(ch.get("OnBattery")) {
        w.write(root, "onBattery", Value::Bool(v));
    }
    if !st.root.is_empty() {
        u.root = st.root;
    }
    if let (Some((path, props)), Some(Value::Item(d))) = (st.display, w.read(root, "displayDevice"))
    {
        let ch = sysbus::diff(u.display.as_ref().map(|(_, p)| p), &props);
        apply_device(w, d, &ch);
        w.write(d, "ready", Value::Bool(true));
        u.display = Some((path, props));
    }
    let mut next = Vec::with_capacity(st.devices.len());
    for (path, props) in st.devices {
        let d = match u.devices.iter().position(|(p, _)| *p == path) {
            Some(k) => {
                let (_, mut d) = u.devices.remove(k);
                let ch = sysbus::diff(Some(&d.last), &props);
                apply_device(w, d.item, &ch);
                d.last = props;
                d
            }
            None => new_device(w, props),
        };
        next.push((path, d));
    }
    gone.extend(
        std::mem::replace(&mut u.devices, next)
            .into_iter()
            .map(|(_, d)| d.item),
    );
}

fn new_device(w: &mut World, props: Props) -> Device {
    let item = crate::types::native_singleton(w, "UPowerDevice");
    apply_device(w, item, &props);
    w.write(item, "ready", Value::Bool(true));
    Device { item, last: props }
}

/// Apply one worker delta to the object it names; true when the device model changed.
fn up_apply(
    w: &mut World,
    root: usize,
    u: &mut UPower,
    d: Delta<UpState>,
    gone: &mut Vec<usize>,
) -> bool {
    match d {
        Delta::Reset(st) => {
            up_reset(w, root, u, st, gone);
            true
        }
        Delta::Changed(c) if c.iface == UPOWER => {
            let ch = sysbus::diff(Some(&u.root), &c.props);
            if let Some(v) = sysbus::boolean(ch.get("OnBattery")) {
                w.write(root, "onBattery", Value::Bool(v));
            }
            sysbus::apply_changed(&mut u.root, &c);
            false
        }
        Delta::Changed(c) => {
            let target = match &mut u.display {
                Some((p, last)) if *p == c.path => match w.read(root, "displayDevice") {
                    Some(Value::Item(i)) => Some((i, last)),
                    _ => None,
                },
                _ => u
                    .devices
                    .iter_mut()
                    .find(|(p, _)| *p == c.path)
                    .map(|(_, d)| (d.item, &mut d.last)),
            };
            if let Some((item, last)) = target {
                let ch = sysbus::diff(Some(last), &c.props);
                apply_device(w, item, &ch);
                sysbus::apply_changed(last, &c);
            }
            false
        }
        Delta::Added(path, mut ifaces) => {
            let Some(props) = ifaces.remove(DEVICE) else {
                return false;
            };
            match u.devices.iter_mut().find(|(p, _)| *p == path) {
                Some((_, d)) => {
                    let ch = sysbus::diff(Some(&d.last), &props);
                    apply_device(w, d.item, &ch);
                    d.last = props;
                    false
                }
                None => {
                    let d = new_device(w, props);
                    u.devices.push((path, d));
                    true
                }
            }
        }
        Delta::Removed(path, _) => match u.devices.iter().position(|(p, _)| *p == path) {
            Some(k) => {
                gone.push(u.devices.remove(k).1.item);
                true
            }
            None => false,
        },
    }
}

fn up_poll(w: &mut World, root: usize) -> bool {
    let mut u = w
        .upower
        .take()
        .unwrap_or_else(|| up_start(&w.waker.handle(crate::svc::Svc::Upower)));
    let mut changed = false;
    let mut models = false;
    let mut gone = vec![];
    while let Ok(ds) = u.rx.try_recv() {
        changed = true;
        for d in ds {
            models |= up_apply(w, root, &mut u, d, &mut gone);
        }
    }
    if models {
        let ids: Vec<usize> = u.devices.iter().map(|(_, d)| d.item).collect();
        set_model(w, root, "devices", &ids);
    }
    for i in gone {
        w.destroy(i);
    }
    w.upower = Some(u);
    changed
}

/// (bus name, object path, interface): the current name first, then the legacy one (ppd < 0.20).
type Target = (&'static str, &'static str, &'static str);
const PPD: [Target; 2] = [
    (
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles",
    ),
    (
        "net.hadess.PowerProfiles",
        "/net/hadess/PowerProfiles",
        "net.hadess.PowerProfiles",
    ),
];

#[derive(Default)]
struct PpState {
    target: Option<Target>,
    props: Props,
}

enum PpEv {
    Conn(Connection),
    Deltas(Vec<Delta<PpState>>),
    /// The daemon refused write number `n` of `profile`.
    SetFailed(u64),
}

fn pp_sync(conn: &Connection) -> PpState {
    for t in PPD {
        if let Some(props) = sysbus::get_all(conn, t.0, t.1, t.2) {
            return PpState {
                target: Some(t),
                props,
            };
        }
    }
    PpState::default()
}

/// `target`: the daemon currently tracked (the worker's only state).
fn pp_handle(
    conn: &Connection,
    target: &mut Option<Target>,
    m: &Message,
) -> Option<Delta<PpState>> {
    if let Some((name, owned)) = sysbus::owner_changed(m) {
        if !PPD.iter().any(|t| t.0 == name) {
            return None;
        }
        let gone = target.is_some_and(|t| t.0 == name) && !owned;
        let st = if gone {
            PpState::default()
        } else if owned && target.is_none_or(|t| t.0 != PPD[0].0) {
            pp_sync(conn)
        } else {
            return None;
        };
        *target = st.target;
        return Some(Delta::Reset(st));
    }
    let c = sysbus::props_changed(m)?;
    match *target {
        Some(t) if t.1 == c.path && t.2 == c.iface => {
            Some(Delta::Changed(sysbus::resolve(conn, t.0, c)))
        }
        _ => None,
    }
}

fn pp_session(conn: &Connection, rx: &Receiver<Message>, tx: &crate::svc::Sender<PpEv>) -> bool {
    if tx.send(PpEv::Conn(conn.clone())).is_err() {
        return false;
    }
    let st = pp_sync(conn);
    let mut target = st.target;
    sysbus::relay(
        rx,
        st,
        |m| pp_handle(conn, &mut target, m),
        |ds| tx.send(PpEv::Deltas(ds)).is_ok(),
    )
}

fn pp_rules() -> Vec<zbus::MatchRule<'static>> {
    PPD.iter()
        .flat_map(|t| [sysbus::props_rule(t.0, t.1), sysbus::owner_rule(t.0)])
        .flatten()
        .collect()
}

pub struct PowerProfiles {
    rx: Receiver<PpEv>,
    tx: crate::svc::Sender<PpEv>,
    conn: Option<Connection>,
    target: Option<Target>,
    last: Props,
    /// `profile` as last set by the daemon or an accepted write (detects QML writes).
    profile: f64,
    /// The last valid `ActiveProfile` the daemon reported: what a refused write goes back to
    /// (`last` may hold a value that is no profile).
    reported: f64,
    /// Writes of `profile` sent so far; only a refusal of the latest one restores the daemon's.
    writes: u64,
    has_perf: bool,
    /// Ends the power-profiles-daemon worker with this instance.
    _stop: StopOnDrop,
}

fn pp_start(h: &crate::svc::Handle) -> PowerProfiles {
    let (tx, rx) = h.channel();
    let t = tx.clone();
    let stop = StopOnDrop::default();
    sysbus::spawn(pp_rules(), stop.0.clone(), move |c, r| pp_session(c, r, &t));
    PowerProfiles {
        rx,
        tx,
        conn: None,
        target: None,
        last: Props::new(),
        profile: 1.0,
        reported: 1.0,
        writes: 0,
        has_perf: false,
        _stop: stop,
    }
}

const PROFILES: [&str; 3] = ["power-saver", "balanced", "performance"];

fn profile_of(v: Option<&V>) -> Option<f64> {
    let s = sysbus::string(v)?;
    PROFILES.iter().position(|p| *p == s).map(|i| i as f64)
}

fn entries(v: &V) -> Option<&Vec<V>> {
    match v {
        V::L(l) => Some(l),
        _ => None,
    }
}

fn field<'a>(e: &'a V, k: &str) -> Option<&'a V> {
    match e {
        V::M(m) => m.iter().find(|(n, _)| n == k).map(|(_, v)| v),
        _ => None,
    }
}

/// Apply changed ppd properties (DBusDataTransforms in quickshell's powerprofiles.cpp: an
/// unknown profile/reason anywhere rejects the whole property update).
fn pp_apply(w: &mut World, root: usize, p: &mut PowerProfiles, ch: &Props) {
    if let Some(v) = profile_of(ch.get("ActiveProfile")) {
        p.profile = v;
        p.reported = v;
        w.write(root, "profile", Value::Num(v));
    }
    if let Some(l) = ch.get("Profiles").and_then(entries) {
        let ps: Option<Vec<f64>> = l.iter().map(|e| profile_of(field(e, "Profile"))).collect();
        if let Some(ps) = ps {
            p.has_perf = ps.contains(&2.0);
            w.write(root, "hasPerformanceProfile", Value::Bool(p.has_perf));
        }
    }
    if let Some(s) = sysbus::string(ch.get("PerformanceDegraded")) {
        let r = match s.as_str() {
            "" => Some(0.0),
            "lap-detected" => Some(1.0),
            "high-operating-temperature" => Some(2.0),
            _ => None,
        };
        if let Some(r) = r {
            w.write(root, "degradationReason", Value::Num(r));
        }
    }
    if let Some(l) = ch.get("ActiveProfileHolds").and_then(entries) {
        let holds: Option<Vec<Value>> = l
            .iter()
            .map(|e| {
                let profile = profile_of(field(e, "Profile"))?;
                let o = new_obj();
                set_own(&o, "profile", Value::Num(profile));
                let s = |k| sysbus::string(field(e, k)).unwrap_or_default();
                set_own(&o, "applicationId", Value::str(&s("ApplicationId")));
                set_own(&o, "reason", Value::str(&s("Reason")));
                Some(Value::Obj(o))
            })
            .collect();
        if let Some(h) = holds {
            w.write(root, "holds", Value::array(h));
        }
    }
}

fn pp_delta(w: &mut World, root: usize, p: &mut PowerProfiles, d: Delta<PpState>) {
    match d {
        Delta::Reset(st) => {
            if st.target.is_some() && st.target != p.target {
                p.last.clear();
            }
            p.target = st.target;
            if st.target.is_some() {
                let ch = sysbus::diff(Some(&p.last), &st.props);
                pp_apply(w, root, p, &ch);
                p.last = st.props;
            }
        }
        Delta::Changed(c) if p.target.is_some() => {
            let ch = sysbus::diff(Some(&p.last), &c.props);
            pp_apply(w, root, p, &ch);
            sysbus::apply_changed(&mut p.last, &c);
        }
        _ => {}
    }
}

fn pp_poll(w: &mut World, root: usize) -> bool {
    let mut p = w
        .power_profiles
        .take()
        .unwrap_or_else(|| pp_start(&w.waker.handle(crate::svc::Svc::Upower)));
    let mut changed = false;
    // `PowerProfiles.profile = x` from QML → PowerProfiles::setProfile.
    let cur = w.read(root, "profile").map_or(p.profile, |v| v.to_number());
    if cur != p.profile {
        changed = true;
        let conn = p.conn.clone().zip(p.target);
        let ok = if conn.is_none() {
            elog::eprintln!(
                "PowerProfiles: Cannot set power profile: power-profiles-daemon not accessible or not running"
            );
            false
        } else if cur == 2.0 && !p.has_perf {
            elog::eprintln!(
                "PowerProfiles: Cannot request performance profile as it is not present for this device."
            );
            false
        } else if !(cur == 0.0 || cur == 1.0 || cur == 2.0) {
            elog::eprintln!("PowerProfiles: Tried to request invalid power profile {cur}");
            false
        } else {
            true
        };
        match conn {
            Some((c, t)) if ok => {
                p.profile = cur;
                p.writes += 1;
                let (tx, n) = (p.tx.clone(), p.writes);
                let wire = PROFILES[cur as usize];
                sysbus::set_prop(
                    &PROFILE_CALLS,
                    &c,
                    t.0,
                    t.1,
                    t.2,
                    "ActiveProfile",
                    wire.into(),
                    move || {
                        let _ = tx.send(PpEv::SetFailed(n));
                    },
                );
            }
            _ => {
                w.write(root, "profile", Value::Num(p.profile));
            }
        }
    }
    while let Ok(ev) = p.rx.try_recv() {
        changed = true;
        match ev {
            PpEv::Conn(c) => p.conn = Some(c),
            PpEv::Deltas(ds) => {
                for d in ds {
                    pp_delta(w, root, &mut p, d);
                }
            }
            PpEv::SetFailed(n) if n == p.writes => {
                p.profile = p.reported;
                w.write(root, "profile", Value::Num(p.reported));
            }
            PpEv::SetFailed(_) => {}
        }
    }
    w.power_profiles = Some(p);
    changed
}

/// `toString()` of the UPower enum singletons (quickshell's `*::toString`).
pub(crate) fn enum_string(ty: &str, n: f64) -> Option<String> {
    let k = n as i64;
    let pick = |names: &[&str], invalid: &str| {
        usize::try_from(k)
            .ok()
            .filter(|_| n.fract() == 0.0)
            .and_then(|i| names.get(i).copied())
            .unwrap_or(invalid)
            .to_string()
    };
    Some(match ty {
        "UPowerDeviceState" => pick(
            &[
                "Unknown",
                "Charging",
                "Discharging",
                "Empty",
                "Fully Charged",
                "Pending Charge",
                "Pending Discharge",
            ],
            "Invalid Status",
        ),
        "UPowerDeviceType" => pick(
            &[
                "Unknown",
                "Line Power",
                "Battery",
                "Ups",
                "Monitor",
                "Mouse",
                "Keyboard",
                "Pda",
                "Phone",
                "Media Player",
                "Tablet",
                "Computer",
                "Gaming Input",
                "Pen",
                "Touchpad",
                "Modem",
                "Network",
                "Headset",
                "Speakers",
                "Headphones",
                "Video",
                "Other Audio",
                "Remote Control",
                "Printer",
                "Scanner",
                "Camera",
                "Wearable",
                "Toy",
                "Bluetooth Generic",
            ],
            "Invalid Type",
        ),
        "PowerProfile" => pick(&["PowerSaver", "Balanced", "Performance"], "Invalid"),
        // quickshell maps None to "Invalid" too (no case for it).
        "PerformanceDegradationReason" => {
            pick(&["Invalid", "LapDetected", "HighTemperature"], "Invalid")
        }
        _ => return None,
    })
}

impl Runtime {
    /// Drive the `UPower` / `PowerProfiles` singletons (if instantiated).
    pub(crate) fn poll_upower(&mut self) -> bool {
        let (up, pp) = {
            let w = self.world.borrow();
            (
                w.singletons.get("UPower").copied(),
                w.singletons.get("PowerProfiles").copied(),
            )
        };
        let mut w = self.world.borrow_mut();
        let mut changed = false;
        if let Some(root) = up {
            changed |= up_poll(&mut w, root);
        }
        if let Some(root) = pp {
            w.mirror(root, crate::svc::Svc::Upower);
            changed |= pp_poll(&mut w, root);
        }
        changed
    }
}
