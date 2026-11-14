//! The `Network` singleton of the `Impulse` module (`qmltypes/Impulse/network.qmltypes`):
//! NetworkManager state, the Wi-Fi list and the Wi-Fi actions over the system bus (zbus).
//! Nothing is spawned. One worker mirrors NetworkManager's objects: a single
//! `GetManagedObjects` from its ObjectManager, then InterfacesAdded/Removed and the
//! PropertiesChanged of the interfaces shown; everything QML sees is derived from that mirror
//! without further calls. Each action is one or two D-Bus calls on a lane of its own (`CALLS`,
//! one call at a time, never the shared workers), and its outcome (activation, scan,
//! deactivation) is read from the mirror as the signals arrive.

use crate::mpris::{V, conv};
use crate::runtime::{Runtime, StopOnDrop, World};
use crate::sysbus::{self, Props};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};
use zbus::blocking::Connection;
use zbus::message::Message;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

/// NetworkManager calls, one at a time in the order made: a NetworkManager that never answers
/// holds only this lane, never the shared workers of `workpool::run`.
static CALLS: workpool::Lane = workpool::Lane::new();

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
/// NetworkManager's ObjectManager (NetworkManager 1.22+), covering every object it exports.
const OM_PATH: &str = "/org/freedesktop";
const OM: &str = "org.freedesktop.DBus.ObjectManager";
const DEV: &str = "org.freedesktop.NetworkManager.Device";
const WL: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const IP4: &str = "org.freedesktop.NetworkManager.IP4Config";
const IP6: &str = "org.freedesktop.NetworkManager.IP6Config";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const SCONN: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const SETTINGS: &str = "org.freedesktop.NetworkManager.Settings";
/// A device's refusal of a request, e.g. `RequestScan` while it is unavailable.
const NOT_ALLOWED: &str = "org.freedesktop.NetworkManager.Device.NotAllowed";
const SEC: &str = "802-11-wireless-security";

type Settings = HashMap<String, HashMap<String, OwnedValue>>;
type Ifaces = HashMap<String, HashMap<String, OwnedValue>>;

/// NetworkManager's objects (path, interface, properties) and its saved profiles' settings.
#[derive(Default)]
pub(crate) struct Cache {
    running: bool,
    objs: HashMap<String, HashMap<String, Props>>,
    /// `GetSettings` of every profile: read once, again on its `Updated`.
    settings: HashMap<String, Settings>,
    /// NetworkManager's unique name and the serial of the `GetManagedObjects` reply: its
    /// signals with a lower serial were sent before the reply and are already in it.
    origin: Option<(String, u32)>,
    /// The last `State` of active connections removed while an activation call was unanswered
    /// (`State::starting`), so one that ended before its waiter existed still decides it.
    /// NetworkManager numbers active connections without reusing paths.
    ended: HashMap<String, i64>,
}

impl Cache {
    fn has(&self, path: &str, iface: &str) -> bool {
        self.objs.get(path).is_some_and(|o| o.contains_key(iface))
    }

    fn stale(&self, m: &Message) -> bool {
        let Some((owner, serial)) = &self.origin else {
            return false;
        };
        m.header().sender().is_some_and(|s| s.as_str() == owner)
            && m.primary_header().serial_num().get() < *serial
    }
}

fn get(c: &Cache, path: &str, iface: &str, name: &str) -> Option<V> {
    c.objs.get(path)?.get(iface)?.get(name).cloned()
}

fn num(v: Option<V>) -> i64 {
    match v {
        Some(V::I(i)) => i,
        Some(V::B(b)) => b as i64,
        _ => 0,
    }
}

fn s(v: Option<V>) -> String {
    match v {
        Some(V::S(s)) => s,
        _ => String::new(),
    }
}

fn is_path(p: &str) -> bool {
    !p.is_empty() && p != "/"
}

fn paths(v: Option<V>) -> Vec<String> {
    match v {
        Some(V::L(l)) => l
            .into_iter()
            .filter_map(|p| match p {
                V::S(s) if is_path(&s) => Some(s),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

fn bytes(v: Option<V>) -> Vec<u8> {
    match v {
        Some(V::L(b)) => b.into_iter().map(|x| num(Some(x)) as u8).collect(),
        _ => vec![],
    }
}

/// A call on NetworkManager that ends with an error when no reply came
/// ([`sysbus::REPLY_TIMEOUT`]), so the action that made it ends and the next one runs.
macro_rules! call {
    ($c:expr, $path:expr, $iface:expr, $method:expr, $body:expr) => {
        sysbus::call($c, NM, AsRef::<str>::as_ref($path), $iface, $method, $body)
    };
}

fn owned(c: &Connection) -> bool {
    zbus::blocking::fdo::DBusProxy::new(c)
        .ok()
        .and_then(|p| p.name_has_owner(NM.try_into().unwrap()).ok())
        .unwrap_or(false)
}

fn fetch_settings(c: &Connection, conn: &str) -> Option<Settings> {
    call!(c, conn, SCONN, "GetSettings", &())
        .ok()?
        .body()
        .deserialize::<Settings>()
        .ok()
}

fn conv_ifaces(m: Ifaces) -> HashMap<String, Props> {
    m.into_iter()
        .map(|(k, v)| (k, sysbus::conv_map(v)))
        .collect()
}

/// A fresh mirror: one `GetManagedObjects`, then the settings of every saved profile. An error
/// when NetworkManager is on the bus and its objects could not be read: nothing is known of it
/// then, which is not a NetworkManager with no devices.
fn sync(conn: &Connection) -> zbus::Result<Cache> {
    type Managed = HashMap<OwnedObjectPath, Ifaces>;
    let mut cache = Cache {
        running: owned(conn),
        ..Default::default()
    };
    if !cache.running {
        return Ok(cache);
    }
    let r = call!(conn, OM_PATH, OM, "GetManagedObjects", &())?;
    let m = r.body().deserialize::<Managed>()?;
    cache.origin = r
        .header()
        .sender()
        .map(|s| (s.to_string(), r.primary_header().serial_num().get()));
    cache.objs = m
        .into_iter()
        .map(|(p, i)| (p.to_string(), conv_ifaces(i)))
        .collect();
    let profiles: Vec<String> = cache
        .objs
        .iter()
        .filter(|(_, i)| i.contains_key(SCONN))
        .map(|(p, _)| p.clone())
        .collect();
    for p in profiles {
        if let Some(st) = fetch_settings(conn, &p) {
            cache.settings.insert(p, st);
        }
    }
    Ok(cache)
}

/// What one NetworkManager signal did to the mirror.
#[derive(Debug, PartialEq)]
enum Applied {
    Nothing,
    Changed,
    /// A profile appeared or changed: its settings are read again.
    Profile(String),
    /// NetworkManager (re)started: the mirror is built again.
    Resync,
    Gone,
}

fn apply(c: &mut Cache, m: &Message) -> Applied {
    if let Some((name, owned)) = sysbus::owner_changed(m) {
        return match (name == NM, owned) {
            (false, _) => Applied::Nothing,
            (true, true) => Applied::Resync,
            (true, false) => Applied::Gone,
        };
    }
    if c.stale(m) {
        return Applied::Nothing;
    }
    let (iface, member, path) = sysbus::member(m);
    match (iface.as_str(), member.as_str()) {
        (sysbus::PROPS, "PropertiesChanged") => {
            let Some(ch) = sysbus::props_changed(m) else {
                return Applied::Nothing;
            };
            let Some(obj) = c.objs.get_mut(&ch.path) else {
                return Applied::Nothing;
            };
            let props = obj.entry(ch.iface).or_default();
            props.extend(ch.props);
            // NetworkManager always sends values; an invalidated one is unknown from here on.
            for k in ch.invalidated {
                props.remove(&k);
            }
            Applied::Changed
        }
        (OM, "InterfacesAdded") => {
            let Ok((p, ifaces)) = m.body().deserialize::<(OwnedObjectPath, Ifaces)>() else {
                return Applied::Nothing;
            };
            let p = p.to_string();
            let profile = ifaces.contains_key(SCONN);
            let obj = c.objs.entry(p.clone()).or_default();
            for (i, props) in conv_ifaces(ifaces) {
                obj.entry(i).or_default().extend(props);
            }
            if profile {
                Applied::Profile(p)
            } else {
                Applied::Changed
            }
        }
        (OM, "InterfacesRemoved") => {
            let Ok((p, ifaces)) = m.body().deserialize::<(OwnedObjectPath, Vec<String>)>() else {
                return Applied::Nothing;
            };
            let p = p.to_string();
            if ifaces.iter().any(|i| i == ACTIVE) {
                let last = num(get(c, &p, ACTIVE, "State"));
                c.ended.insert(p.clone(), last);
            }
            let Some(obj) = c.objs.get_mut(&p) else {
                return Applied::Nothing;
            };
            for i in &ifaces {
                obj.remove(i);
            }
            if obj.is_empty() {
                c.objs.remove(&p);
            }
            if ifaces.iter().any(|i| i == SCONN) {
                c.settings.remove(&p);
            }
            Applied::Changed
        }
        // libnm follows a device's state from this signal (new, old, reason).
        (DEV, "StateChanged") => {
            let Ok((new, _, _)) = m.body().deserialize::<(u32, u32, u32)>() else {
                return Applied::Nothing;
            };
            match c.objs.get_mut(&path).and_then(|o| o.get_mut(DEV)) {
                Some(d) => {
                    d.insert("State".into(), V::I(new as i64));
                    Applied::Changed
                }
                None => Applied::Nothing,
            }
        }
        (SCONN, "Updated") => Applied::Profile(path),
        _ => Applied::Nothing,
    }
}

/// The signals the mirror needs and no others: PropertiesChanged of the interfaces shown (not
/// device statistics, DHCP, settings or the legacy per-interface PropertiesChanged), objects
/// coming and going, device state and profile updates, NetworkManager starting or stopping.
fn rules() -> Vec<zbus::MatchRule<'static>> {
    type B = zbus::match_rule::Builder<'static>;
    fn rule(f: impl FnOnce(B) -> zbus::Result<B>) -> Option<zbus::MatchRule<'static>> {
        let b = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(NM)
            .ok()?;
        f(b).ok().map(|b| b.build())
    }
    let mut out: Vec<_> = [NM, DEV, WL, AP, ACTIVE, IP4, IP6]
        .into_iter()
        .map(|i| {
            rule(|b| {
                b.interface(sysbus::PROPS)?
                    .member("PropertiesChanged")?
                    .path_namespace(NM_PATH)?
                    .arg(0, i)
            })
        })
        .collect();
    out.push(rule(|b| b.interface(OM)?.path(OM_PATH)));
    out.push(rule(|b| {
        b.interface(DEV)?
            .member("StateChanged")?
            .path_namespace(NM_PATH)
    }));
    out.push(rule(|b| {
        b.interface(SCONN)?
            .member("Updated")?
            .path_namespace(SETTINGS_PATH)
    }));
    out.push(sysbus::owner_rule(NM));
    out.into_iter().flatten().collect()
}

/// Sort key of one active connection, as nmcli's `nmc_active_connection_cmp` sees it.
struct ActiveRow {
    rank: (i64, bool, bool, bool, bool, bool, usize),
    id: String,
    conn: String,
}

fn ip_method_shared(settings: Option<&Settings>, group: &str) -> bool {
    settings
        .and_then(|s| s.get(group))
        .and_then(|g| g.get("method"))
        .is_some_and(|v| matches!(conv(v), V::S(s) if s == "shared"))
}

fn addr_count(c: &Cache, cfg: Option<V>, iface: &str) -> usize {
    match s(cfg) {
        p if !is_path(&p) => 0,
        p => match get(c, &p, iface, "AddressData") {
            Some(V::L(l)) => l.len(),
            _ => 0,
        },
    }
}

fn active_row(c: &Cache, ac: &str) -> ActiveRow {
    let state = num(get(c, ac, ACTIVE, "State"));
    let external = num(get(c, ac, ACTIVE, "StateFlags")) & 0x80 != 0;
    // active_connection_get_state_ord: unknown, deactivated, deactivating, activating, activated.
    let ord = match [0, 4, 3, 1, 2].iter().position(|&x| x == state) {
        Some(i) => i as i64 + if external { 0 } else { 5 },
        None if external => -2,
        None => -1,
    };
    let conn = s(get(c, ac, ACTIVE, "Connection"));
    let st = is_path(&conn).then(|| c.settings.get(&conn)).flatten();
    let flag = |n| matches!(get(c, ac, ACTIVE, n), Some(V::B(true)));
    let addrs = addr_count(c, get(c, ac, ACTIVE, "Ip4Config"), IP4)
        + addr_count(c, get(c, ac, ACTIVE, "Ip6Config"), IP6);
    ActiveRow {
        rank: (
            ord,
            ip_method_shared(st, "ipv6"),
            ip_method_shared(st, "ipv4"),
            flag("Vpn"),
            flag("Default"),
            flag("Default6"),
            addrs,
        ),
        id: s(get(c, ac, ACTIVE, "Id")),
        conn,
    }
}

/// `g_utf8_collate` in the inherited LC_COLLATE, like nmcli's `nm_utf8_collate0`.
fn collate(a: &str, b: &str) -> std::cmp::Ordering {
    use std::ffi::CString;
    thread_local! {
        // SAFETY: `c""` is NUL-terminated and newlocale(3) allows a null base (a new object).
        static LOC: libc::locale_t = unsafe {
            libc::newlocale(libc::LC_COLLATE_MASK, c"".as_ptr(), std::ptr::null_mut())
        };
    }
    let (Ok(ca), Ok(cb)) = (CString::new(a), CString::new(b)) else {
        return a.cmp(b);
    };
    LOC.with(|&l| {
        if l.is_null() {
            return a.cmp(b);
        }
        unsafe extern "C" {
            fn strcoll_l(
                a: *const libc::c_char,
                b: *const libc::c_char,
                l: libc::locale_t,
            ) -> libc::c_int;
        }
        // SAFETY: `ca` and `cb` are live NUL-terminated strings; `l` is this thread's non-null
        // locale from newlocale(3), never freed.
        unsafe { strcoll_l(ca.as_ptr(), cb.as_ptr(), l) }.cmp(&0)
    })
}

/// nm_utils_dbus_path_cmp: numeric suffixes of the same object prefix compare as numbers.
fn dbus_path_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |p: &str| {
        let i = p.rfind('/').map_or(0, |i| i + 1);
        (p[..i].to_string(), p[i..].parse::<u64>().ok())
    };
    match (split(a), split(b)) {
        ((pa, Some(na)), (pb, Some(nb))) if pa == pb => na.cmp(&nb),
        _ => a.cmp(b),
    }
}

/// Active connection names in `nmcli c show --active` order (con_show_get_items_cmp).
fn active_names(c: &Cache) -> Vec<String> {
    let mut rows: Vec<ActiveRow> = paths(get(c, NM_PATH, NM, "ActiveConnections"))
        .iter()
        .map(|a| active_row(c, a))
        .collect();
    rows.sort_by(|a, b| {
        let visible = |r: &ActiveRow| is_path(&r.conn);
        visible(a)
            .cmp(&visible(b))
            .then_with(|| b.rank.cmp(&a.rank))
            .then_with(|| collate(&a.id, &b.id))
            .then_with(|| dbus_path_cmp(&a.conn, &b.conn))
    });
    rows.into_iter().map(|r| r.id).collect()
}

fn wifi_devices(c: &Cache) -> Vec<String> {
    paths(get(c, NM_PATH, NM, "Devices"))
        .into_iter()
        .filter(|d| num(get(c, d, DEV, "DeviceType")) == 2)
        .collect()
}

/// One device as the original's connection-type check sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DevRow {
    pub ty: i64,
    pub state: i64,
    pub external: bool,
}

/// What one status update yields; `None` leaves the property as it was (no output in the original).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Status {
    pub wifi: bool,
    pub ethernet: bool,
    pub wifi_status: &'static str,
    pub wifi_enabled: bool,
    pub name: Option<String>,
}

/// The original's `updateConnectionType` without its `nmcli` line-matching slips: only a connected
/// ethernet device counts (the original's `includes("connected")` also matched "disconnected", showing the
/// ethernet icon with the cable unplugged), and the best Wi-Fi device in nmcli's state order
/// decides both `wifi` and the status (the original let the last one win the status while `hasWifi` stuck
/// from an earlier one, so a connected card plus an idle one read "disconnected").
pub(crate) fn connection_type(devs: &[DevRow], connectivity: i64) -> (bool, bool, &'static str) {
    let ethernet = devs.iter().any(|d| d.ty == 1 && d.state == 100);
    let best = devs
        .iter()
        .filter(|d| d.ty == 2)
        .min_by_key(|d| match d.state {
            100 if d.external => 1,
            100 => 0,
            40..=90 => 2,
            30 => 3,
            20 => 4,
            10 => 5,
            _ => 6,
        });
    let (wifi, status) = match best.map(|d| d.state) {
        Some(100) if connectivity == 3 => (false, "limited"),
        Some(100) => (true, "connected"),
        Some(40..=90) => (false, "connecting"),
        Some(20) => (false, "disabled"),
        _ => (false, "disconnected"),
    };
    (wifi, ethernet, status)
}

fn status(c: &Cache) -> Status {
    if !c.running {
        return Status {
            wifi: false,
            ethernet: false,
            wifi_status: "disconnected",
            wifi_enabled: false,
            name: None,
        };
    }
    let devs: Vec<DevRow> = paths(get(c, NM_PATH, NM, "Devices"))
        .iter()
        .map(|d| {
            let state = num(get(c, d, DEV, "State"));
            let ac = s(get(c, d, DEV, "ActiveConnection"));
            DevRow {
                ty: num(get(c, d, DEV, "DeviceType")),
                state,
                // NM_ACTIVATION_STATE_FLAG_EXTERNAL
                external: state == 100
                    && is_path(&ac)
                    && num(get(c, &ac, ACTIVE, "StateFlags")) & 0x80 != 0,
            }
        })
        .collect();
    let (wifi, ethernet, wifi_status) =
        connection_type(&devs, num(get(c, NM_PATH, NM, "Connectivity")));
    Status {
        wifi,
        ethernet,
        wifi_status,
        wifi_enabled: num(get(c, NM_PATH, NM, "WirelessEnabled")) != 0,
        // The profile name as is: the original read `nmcli -t`, which escaped ':' and '\\' in it.
        name: active_names(c).into_iter().next(),
    }
}

/// The in-use access points' (BSSID, signal) as NetworkManager updates them, the last one being
/// `networkStrength`'s (the original's `nmcli -f IN-USE,SIGNAL device wifi`, which also rescanned devices
/// not scanned for 30 s on every report; the signal needs no scan).
fn live(c: &Cache) -> Vec<(String, i64)> {
    wifi_devices(c)
        .iter()
        .map(|d| s(get(c, d, WL, "ActiveAccessPoint")))
        .filter(|ap| is_path(ap) && c.has(ap, AP))
        .map(|ap| {
            (
                s(get(c, &ap, AP, "HwAddress")),
                num(get(c, &ap, AP, "Strength")),
            )
        })
        .collect()
}

/// `nmcli -g ACTIVE,SIGNAL,FREQ,SSID,BSSID,SECURITY d w` as the original's `getNetworks` parses it, from
/// the access points NetworkManager knows (it scans on its own; the original's `--rescan auto` scanned
/// again whenever the last scan was 30 s old).
fn list(c: &Cache) -> Vec<ApInfo> {
    networks(&raw_aps(c))
}

/// One access point as NetworkManager reports it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawAp {
    pub active: bool,
    pub strength: i64,
    pub frequency: i64,
    /// The SSID's bytes: IEEE 802.11 gives no encoding, so they need not be UTF-8.
    pub ssid: Vec<u8>,
    pub bssid: String,
    pub security: String,
}

/// A `WifiAccessPoint.lastIpcObject`, and the SSID bytes it shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ApInfo {
    pub active: bool,
    pub strength: f64,
    pub frequency: f64,
    /// The SSID as shown: its bytes as UTF-8, invalid sequences replaced.
    pub ssid: String,
    pub bssid: String,
    pub security: String,
    /// The SSID's bytes, which the actions look the network up by; not shown to QML.
    pub ssid_bytes: Vec<u8>,
}

bits::bit_flags! {
const PRIVACY: i64 = 0x1;
}
bits::bit_flags! {
const KEY_MGMT_PSK: i64 = 0x100;
const KEY_MGMT_802_1X: i64 = 0x200;
const KEY_MGMT_SAE: i64 = 0x400;
const KEY_MGMT_OWE: i64 = 0x800;
const KEY_MGMT_OWE_TM: i64 = 0x1000;
const KEY_MGMT_EAP_SUITE_B_192: i64 = 0x2000;
}

/// nmcli's SECURITY column as nmcli 1.44+ prints it (devices.c fill_output_access_point).
pub(crate) fn security(flags: i64, wpa: i64, rsn: i64) -> String {
    let mut out: Vec<&str> = Vec::new();
    if flags & PRIVACY != 0 && wpa == 0 && rsn == 0 {
        return "WEP".into();
    }
    if wpa != 0 {
        out.push("WPA1");
    }
    if rsn & (KEY_MGMT_PSK | KEY_MGMT_802_1X) != 0 {
        out.push("WPA2");
    }
    if rsn & KEY_MGMT_SAE != 0 {
        out.push("WPA3");
    }
    if rsn & KEY_MGMT_OWE != 0 {
        out.push("OWE");
    } else if rsn & KEY_MGMT_OWE_TM != 0 {
        out.push("OWE-TM");
    }
    if (wpa | rsn) & KEY_MGMT_802_1X != 0 {
        out.push("802.1X");
    }
    if (wpa | rsn) & KEY_MGMT_EAP_SUITE_B_192 != 0 {
        out.push("WPA-EAP-SUITE-B-192");
    }
    out.join(" ")
}

/// What joining an access point without a saved profile takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Key {
    /// Open or OWE: NetworkManager completes the profile itself.
    None,
    /// A passphrase for this `802-11-wireless-security.key-mgmt`.
    Psk(&'static str),
    Wep,
    /// 802.1X needs an EAP method and identity the original UI does not ask for: left to the
    /// NetworkManager secret agent of the session, as `nmcli dev wifi connect` does.
    Enterprise,
}

/// The key an access point's flags call for (NetworkManager's AccessPoint `Flags`, `WpaFlags`,
/// `RsnFlags`): WPA3-only networks take SAE, WPA2/WPA3 transition networks WPA-PSK, which
/// every driver supports.
pub(crate) fn key_for(flags: i64, wpa: i64, rsn: i64) -> Key {
    if (wpa | rsn) & KEY_MGMT_802_1X != 0 {
        Key::Enterprise
    } else if (wpa | rsn) & KEY_MGMT_PSK != 0 {
        Key::Psk("wpa-psk")
    } else if rsn & KEY_MGMT_SAE != 0 {
        Key::Psk("sae")
    } else if flags & PRIVACY != 0 && wpa == 0 && rsn == 0 {
        Key::Wep
    } else {
        Key::None
    }
}

/// The settings `AddAndActivateConnection` gets for a new profile; NetworkManager completes
/// the rest (SSID, mode, id) from the access point.
pub(crate) fn new_profile(k: Key, secret: Option<&str>) -> Settings {
    let v = |x: zbus::zvariant::Value<'_>| OwnedValue::try_from(x).ok();
    let mut st = Settings::new();
    let sec: Vec<(&str, Option<OwnedValue>)> = match (k, secret) {
        (Key::Psk(km), Some(p)) => vec![("key-mgmt", v(km.into())), ("psk", v(p.into()))],
        (Key::Wep, Some(p)) => {
            // NM_WEP_KEY_TYPE_KEY for a 40/104-bit key (ASCII or hex), else a passphrase.
            let hex = p.chars().all(|c| c.is_ascii_hexdigit());
            let key = matches!(p.len(), 5 | 13) || (hex && matches!(p.len(), 10 | 26));
            vec![
                ("key-mgmt", v("none".into())),
                ("wep-key0", v(p.into())),
                ("wep-key-type", v((if key { 1u32 } else { 2u32 }).into())),
            ]
        }
        _ => vec![],
    };
    if !sec.is_empty() {
        st.insert(
            SEC.into(),
            sec.into_iter()
                .filter_map(|(k, v)| Some((k.to_string(), v?)))
                .collect(),
        );
    }
    st
}

fn raw_aps(c: &Cache) -> Vec<RawAp> {
    let mut out = vec![];
    for d in wifi_devices(c) {
        let active = s(get(c, &d, WL, "ActiveAccessPoint"));
        let mut dev = vec![];
        for ap in paths(get(c, &d, WL, "AccessPoints")) {
            if !c.has(&ap, AP) {
                continue;
            }
            let g = |n| get(c, &ap, AP, n);
            let rate = num(g("MaxBitrate"));
            dev.push((
                rate,
                RawAp {
                    active: ap == active,
                    strength: num(g("Strength")),
                    frequency: num(g("Frequency")),
                    ssid: bytes(g("Ssid")),
                    bssid: s(g("HwAddress")),
                    security: security(num(g("Flags")), num(g("WpaFlags")), num(g("RsnFlags"))),
                },
            ));
        }
        // nmcli's compare_aps: strongest first, then lower frequency, then faster.
        dev.sort_by(|(ra, a), (rb, b)| {
            (b.strength.cmp(&a.strength))
                .then(a.frequency.cmp(&b.frequency))
                .then(rb.cmp(ra))
        });
        out.extend(dev.into_iter().map(|(_, ap)| ap));
    }
    out
}

/// The original's `getNetworks` result for these access points: its field split (which keeps an escaped
/// ':' in an SSID as a placeholder and a '\' doubled, as it reads `nmcli -g`), then one entry
/// per SSID (by its bytes: two SSIDs that are not UTF-8 can show alike), the active one or else
/// the strongest, in first-seen order.
pub(crate) fn networks(aps: &[RawAp]) -> Vec<ApInfo> {
    let mut map: Vec<ApInfo> = vec![];
    for a in aps {
        // Fields straight from NetworkManager: the original split `nmcli -t` lines on ':' and only undid
        // the escaping in the BSSID, so an SSID with ':' showed a placeholder (and could not be
        // connected to) and one ending in '\\' swallowed the BSSID.
        let n = ApInfo {
            active: a.active,
            strength: a.strength as f64,
            frequency: a.frequency as f64,
            ssid: String::from_utf8_lossy(&a.ssid).into_owned(),
            bssid: a.bssid.clone(),
            security: a.security.clone(),
            ssid_bytes: a.ssid.clone(),
        };
        if n.ssid_bytes.is_empty() {
            continue;
        }
        match map.iter_mut().find(|e| e.ssid_bytes == n.ssid_bytes) {
            None => map.push(n),
            Some(e) => {
                if (n.active && !e.active) || (!n.active && !e.active && n.strength > e.strength) {
                    *e = n;
                }
            }
        }
    }
    map
}

enum Ev {
    Conn(Connection),
    Status(Status),
    List(Vec<ApInfo>),
    Connected {
        generation: u64,
        ok: bool,
    },
    /// In-use access points' (BSSID, signal), the last one being `networkStrength`'s.
    Live(Vec<(String, i64)>),
    /// A requested scan ended, or none is coming.
    ScanDone,
    /// `changePassword` finished: `key` is set when no saved profile took the password, so the
    /// next attempt creates the profile with it.
    PasswordSet {
        ssid: Vec<u8>,
        key: Option<String>,
    },
}

type Check = Box<dyn FnMut(&Cache) -> Option<Vec<Ev>> + Send>;
type Lost = Box<dyn FnOnce(&Cache) -> Vec<Ev> + Send>;

/// An action waiting for NetworkManager to report its outcome: `check` runs after every signal
/// the mirror applies, `lost` when the mirror is rebuilt first (NetworkManager stopped or
/// restarted, the bus reconnected), since the objects it waits on are gone. Nothing else ends
/// it: NetworkManager bounds activations, deactivations and scans itself and reports each end.
struct Waiter {
    check: Check,
    lost: Lost,
}

#[derive(Default)]
struct State {
    cache: Cache,
    waiters: Vec<Waiter>,
    /// Bumped whenever the mirror is rebuilt: an action that read an older one acted on objects
    /// that may not exist any more.
    epoch: u64,
    /// Activation calls not answered yet; while there are none, `Cache::ended` is emptied.
    starting: usize,
}

/// What one batch of signals did besides the waiters' events.
#[derive(Default)]
struct Absorbed {
    evs: Vec<Ev>,
    resync: bool,
    profiles: Vec<String>,
}

impl State {
    /// Wait for `check` on the mirror of `epoch` (decided now, or by a later signal).
    fn when(&mut self, epoch: u64, mut check: Check, lost: Lost) -> Vec<Ev> {
        if epoch != self.epoch {
            return lost(&self.cache);
        }
        if let Some(evs) = check(&self.cache) {
            return evs;
        }
        self.waiters.push(Waiter { check, lost });
        vec![]
    }

    /// Run the waiters the mirror now satisfies.
    fn settle(&mut self) -> Vec<Ev> {
        let State { cache, waiters, .. } = self;
        let mut out = vec![];
        waiters.retain_mut(|w| match (w.check)(cache) {
            Some(evs) => {
                out.extend(evs);
                false
            }
            None => true,
        });
        out
    }

    /// Replace the mirror; every waiter is lost.
    fn reset(&mut self, cache: Cache) -> Vec<Ev> {
        self.epoch += 1;
        self.cache = cache;
        let lost: Vec<Waiter> = std::mem::take(&mut self.waiters);
        lost.into_iter()
            .flat_map(|w| (w.lost)(&self.cache))
            .collect()
    }

    /// Apply a batch of signals, running the waiters after each one: a state that lasts only
    /// within the batch (an activation failing and removed at once) still decides them.
    fn absorb(&mut self, ms: &[Message]) -> Absorbed {
        let mut out = Absorbed::default();
        for m in ms {
            match apply(&mut self.cache, m) {
                // The new daemon's objects are not the old ones': drop them now, the snapshot
                // follows.
                Applied::Resync => {
                    out.evs.extend(self.reset(Cache::default()));
                    out.resync = true;
                }
                Applied::Gone => {
                    out.evs.extend(self.reset(Cache::default()));
                    out.resync = false;
                }
                Applied::Profile(p) => {
                    out.profiles.push(p);
                    out.evs.extend(self.settle());
                }
                Applied::Changed => out.evs.extend(self.settle()),
                Applied::Nothing => {}
            }
        }
        if self.starting == 0 {
            self.cache.ended.clear();
        }
        out
    }
}

/// The mirror and its waiters, shared by the worker and the actions on the pool.
#[derive(Clone)]
struct Shared {
    st: Arc<Mutex<State>>,
    /// Each send is one batch of events, applied in one poll.
    tx: crate::svc::Sender<Vec<Ev>>,
    /// `connection.uuid` of profiles made by `connect` whose activation was lost with
    /// NetworkManager: deleted once a NetworkManager is back.
    orphans: Orphans,
    /// The shell switched the radio on and NetworkManager has not shown it yet.
    radio: Arc<AtomicBool>,
}

type Orphans = Arc<Mutex<Vec<String>>>;

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn orphans(&self) -> MutexGuard<'_, Vec<String>> {
        self.orphans.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Send `evs` as one batch, so QML never sees a part of what one change did.
    fn send(&self, evs: Vec<Ev>) -> bool {
        evs.is_empty() || self.tx.send(evs).is_ok()
    }

    fn when(&self, epoch: u64, check: Check, lost: Lost) {
        let evs = self.lock().when(epoch, check, lost);
        self.send(evs);
    }
}

/// What the worker last reported, so that it reports changes only.
#[derive(Default)]
struct Reported {
    status: Option<Status>,
    live: Option<Vec<(String, i64)>>,
    list: Option<Vec<ApInfo>>,
}

fn report(c: &Cache, seen: &mut Reported) -> Vec<Ev> {
    let mut out = vec![];
    let st = status(c);
    if seen.status.as_ref() != Some(&st) {
        seen.status = Some(st.clone());
        out.push(Ev::Status(st));
    }
    let live = live(c);
    if seen.live.as_ref() != Some(&live) {
        seen.live = Some(live.clone());
        out.push(Ev::Live(live));
    }
    let l = list(c);
    if seen.list.as_ref() != Some(&l) {
        seen.list = Some(l.clone());
        out.push(Ev::List(l));
    }
    out
}

fn session(conn: &Connection, rx: &Receiver<Message>, sh: &Shared) -> bool {
    if !sh.send(vec![Ev::Conn(conn.clone())]) {
        return false;
    }
    let mut seen = Reported::default();
    let mut fresh = Some(sync(conn));
    // NetworkManager is on the bus and its objects could not be read: shown as no
    // NetworkManager, and read again on its next signal.
    let mut unread = false;
    let mut fetched: Vec<(String, Settings)> = vec![];
    loop {
        let (evs, orphaned) = {
            let mut st = sh.lock();
            let mut evs = vec![];
            let mut orphaned = false;
            if let Some(c) = fresh.take() {
                let c = match c {
                    Ok(c) => {
                        unread = false;
                        c
                    }
                    Err(e) => {
                        if !unread {
                            elog::eprintln!(
                                "NetworkManager: cannot read its objects, reading them again on its next signal: {e}"
                            );
                        }
                        unread = true;
                        Cache::default()
                    }
                };
                evs.extend(st.reset(c));
                orphaned = st.cache.running && !sh.orphans().is_empty();
            }
            for (p, s) in fetched.drain(..) {
                if st.cache.has(&p, SCONN) {
                    st.cache.settings.insert(p, s);
                }
            }
            evs.extend(report(&st.cache, &mut seen));
            if radio_on(&st.cache) {
                sh.radio.store(false, Ordering::SeqCst);
            }
            (evs, orphaned)
        };
        if !sh.send(evs) {
            return false;
        }
        // Lost when this session's snapshot replaced the last one: the mirror follows the
        // removals.
        if orphaned {
            delete_orphans(conn, sh);
        }
        let Some(ms) = sysbus::batch(rx) else {
            return true;
        };
        let Absorbed {
            evs,
            resync,
            mut profiles,
        } = sh.lock().absorb(&ms);
        if !sh.send(evs) {
            return false;
        }
        // The calls run without the lock: actions keep reading the mirror meanwhile.
        fresh = (resync || unread).then(|| sync(conn));
        profiles.dedup();
        fetched = profiles
            .into_iter()
            .filter_map(|p| fetch_settings(conn, &p).map(|s| (p, s)))
            .collect();
    }
}

fn boottime_ms() -> i64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a live timespec for the kernel to fill.
    unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000
}

fn scanned(c: &Cache) -> Vec<Ev> {
    vec![Ev::ScanDone, Ev::List(list(c))]
}

/// Whether a scan result follows a `RequestScan` reply: only when the request was taken. Since
/// NetworkManager 1.26 a request while scanning is taken too and refused (`NotAllowed`) only
/// while the device is unavailable. NetworkManager 1.22 to 1.24 also refuse while scanning or
/// right after a scan, telling those apart only in the error's text, which is not API: a scan
/// already running there is not waited for, and the list shows what NetworkManager has so far.
fn scan_coming(r: &zbus::Result<Message>) -> bool {
    r.is_ok()
}

/// Whether the Wi-Fi radio is on, in software and in hardware (rfkill).
fn radio_on(c: &Cache) -> bool {
    num(get(c, NM_PATH, NM, "WirelessEnabled")) != 0
        && get(c, NM_PATH, NM, "WirelessHardwareEnabled").is_none_or(|v| num(Some(v)) != 0)
}

/// NM_DEVICE_STATE_UNAVAILABLE: managed, but not ready (radio off, supplicant not up yet).
const UNAVAILABLE: i64 = 20;

/// Devices that refused a scan asked by `scan_check`.
type Refused = Arc<Mutex<Vec<String>>>;
/// Asks a device for a scan again, adding it to `Refused` (and settling) if it refuses.
type Ask = Box<dyn Fn(String, Refused) + Send>;

fn request_scan(conn: &Connection, dev: &str) -> zbus::Result<Message> {
    let opts: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
    call!(conn, dev, WL, "RequestScan", &(&opts,))
}

/// Done once every device in `pending` finished a scan after its cutoff (`LastScan`,
/// CLOCK_BOOTTIME ms, which NetworkManager documents as the way to learn a requested scan
/// ended) or became unavailable, which ends any scan. A device in `waiting` refused the request
/// (`NotAllowed`) while UNAVAILABLE, with the radio on or being switched on (`Shared::radio`):
/// it is asked again once the mirror shows it available (`State` DISCONNECTED or later) with
/// the radio on, and dropped when it is gone, the radio is off and not being switched on, it
/// leaves UNAVAILABLE for UNMANAGED or UNKNOWN (no scan of it is coming), or it refuses again
/// (NetworkManager 1.22 to 1.24 also refuse while scanning).
fn scan_check(
    mut pending: Vec<(String, i64)>,
    mut waiting: Vec<String>,
    radio: Arc<AtomicBool>,
    ask: Ask,
) -> Check {
    let refused = Refused::default();
    Box::new(move |c| {
        let on = radio_on(c);
        waiting.retain(|d| {
            if !c.has(d, WL) || !(on || radio.load(Ordering::SeqCst)) {
                return false;
            }
            match num(get(c, d, DEV, "State")) {
                UNAVAILABLE => true,
                s if s < UNAVAILABLE => false,
                _ if !on => true,
                _ => {
                    pending.push((d.clone(), boottime_ms()));
                    ask(d.clone(), refused.clone());
                    false
                }
            }
        });
        let refused = refused.lock().unwrap_or_else(|p| p.into_inner());
        (waiting.is_empty()
            && pending.iter().all(|(d, cutoff)| {
                refused.contains(d)
                    || num(get(c, d, WL, "LastScan")) >= *cutoff
                    || num(get(c, d, DEV, "State")) < 30
            }))
        .then(|| scanned(c))
    })
}

/// `nmcli dev wifi list --rescan yes`: `RequestScan` on every Wi-Fi device, then the list once
/// each device that is scanning finished. A device that refuses while UNAVAILABLE (just after
/// the radio was switched on: `enableWifi(); rescanWifi()`, as the original's Wi-Fi dialog does) is
/// asked again once it can scan (nmcli asked again every second for 15 s); one that refuses
/// otherwise (UNMANAGED, say) is not waited for, as no scan of it is coming. ScanDone without
/// NetworkManager.
fn rescan(sh: &Shared, conn: &Connection) {
    let cutoff = boottime_ms();
    let (devs, epoch) = {
        let st = sh.lock();
        let c = &st.cache;
        let devs = c.running.then(|| {
            wifi_devices(c)
                .into_iter()
                .map(|d| {
                    let unavailable = num(get(c, &d, DEV, "State")) == UNAVAILABLE;
                    (d, unavailable)
                })
                .collect::<Vec<_>>()
        });
        (devs, st.epoch)
    };
    let Some(devs) = devs else {
        sh.send(vec![Ev::ScanDone]);
        return;
    };
    let (mut pending, mut waiting) = (vec![], vec![]);
    for (d, unavailable) in devs {
        let r = request_scan(conn, &d);
        if scan_coming(&r) {
            pending.push((d, cutoff));
        } else if unavailable
            && matches!(&r, Err(zbus::Error::MethodError(n, ..)) if n.as_str() == NOT_ALLOWED)
        {
            waiting.push(d);
        } else if let Err(e) = r {
            elog::eprintln!("NetworkManager: {d} did not take a scan request: {e}");
        }
    }
    let (sh2, conn) = (sh.clone(), conn.clone());
    let ask: Ask = Box::new(move |d, refused| {
        let (sh, conn) = (sh2.clone(), conn.clone());
        CALLS.run(move || {
            if let Err(e) = request_scan(&conn, &d) {
                elog::eprintln!("NetworkManager: {d} refused a scan: {e}");
                refused.lock().unwrap_or_else(|p| p.into_inner()).push(d);
                let evs = sh.lock().settle();
                sh.send(evs);
            }
        });
    });
    let check = scan_check(pending, waiting, sh.radio.clone(), ask);
    sh.when(epoch, check, Box::new(scanned));
}

fn obj(p: &str) -> Option<OwnedObjectPath> {
    OwnedObjectPath::try_from(p.to_string()).ok()
}

/// A saved profile's `802-11-wireless.ssid` bytes.
fn profile_ssid(st: &Settings) -> Vec<u8> {
    st.get("802-11-wireless")
        .and_then(|w| w.get("ssid"))
        .map(|v| bytes(Some(conv(v))))
        .unwrap_or_default()
}

/// What `nmcli dev wifi connect SSID` acts on: the first Wi-Fi device seeing the SSID, its
/// access point, a saved profile for the network that device can activate, and the key a new
/// profile needs.
struct Target {
    dev: String,
    ap: String,
    profile: Option<String>,
    key: Key,
}

fn target(c: &Cache, ssid: &[u8]) -> Option<Target> {
    let (dev, ap) = wifi_devices(c).into_iter().find_map(|d| {
        paths(get(c, &d, WL, "AccessPoints"))
            .into_iter()
            .find(|ap| bytes(get(c, ap, AP, "Ssid")) == ssid)
            .map(|ap| (d, ap))
    })?;
    let profile = paths(get(c, &dev, DEV, "AvailableConnections"))
        .into_iter()
        .find(|p| c.settings.get(p).is_some_and(|st| profile_ssid(st) == ssid));
    let f = |n| num(get(c, &ap, AP, n));
    let key = key_for(f("Flags"), f("WpaFlags"), f("RsnFlags"));
    Some(Target {
        dev,
        ap,
        profile,
        key,
    })
}

/// The outcome of the activation `ac` once NetworkManager decided it: activated (2), or
/// deactivating, deactivated or removed. `seen` remembers that the mirror had it: the reply can
/// come before its InterfacesAdded, so absent before that is not an outcome yet.
fn activation(c: &Cache, ac: &str, seen: &mut bool) -> Option<bool> {
    match get(c, ac, ACTIVE, "State").map(|v| num(Some(v))) {
        Some(2) => Some(true),
        Some(3 | 4) => Some(false),
        Some(_) => {
            *seen = true;
            None
        }
        None => match c.ended.get(ac) {
            Some(last) => Some(*last == 2),
            None => seen.then_some(false),
        },
    }
}

fn activation_check(
    ac: String,
    mut finish: impl FnMut(&Cache, bool) -> Vec<Ev> + Send + 'static,
) -> Check {
    let mut seen = false;
    Box::new(move |c| activation(c, &ac, &mut seen).map(|ok| finish(c, ok)))
}

/// Join the Wi-Fi network `ssid` and report `Connected` once NetworkManager decided the
/// activation, however long it takes (802.1X, a secret agent's prompt, a slow DHCP server). A
/// saved profile is activated; without one, an open network gets a new profile from
/// NetworkManager, and a secured one first asks for its password (no call, no half-made
/// profile), then is added with it in `802-11-wireless-security`. A profile created here that
/// fails to activate is deleted again, so a wrong or cancelled password leaves nothing behind.
fn connect(sh: &Shared, conn: &Connection, ssid: &[u8], secret: Option<&str>, generation: u64) {
    let started_err = |e: &zbus::Error| {
        elog::eprintln!("NetworkManager: the connection was not started: {e}");
    };
    let fail = || {
        sh.send(vec![Ev::Connected {
            generation,
            ok: false,
        }]);
    };
    let ready = {
        let mut st = sh.lock();
        let t = st.cache.running.then(|| target(&st.cache, ssid)).flatten();
        let ready = t.filter(|t| {
            t.profile.is_some() || secret.is_some() || !matches!(t.key, Key::Psk(_) | Key::Wep)
        });
        if ready.is_some() {
            st.starting += 1;
        }
        ready.map(|t| (t, st.epoch))
    };
    let Some((t, epoch)) = ready else {
        return fail();
    };
    let started = match (obj(&t.dev), obj(&t.ap)) {
        (Some(devp), Some(app)) => match &t.profile {
            Some(p) => obj(p)
                .and_then(|p| {
                    let r = call!(conn, NM_PATH, NM, "ActivateConnection", &(p, devp, app));
                    sysbus::object_path(r.inspect_err(started_err))
                })
                .map(|a| (a, None)),
            None => {
                // Named by a UUID of our own: after a restart NetworkManager numbers its
                // profiles anew, the UUID still finds this one.
                let uuid = new_uuid();
                let mut st = new_profile(t.key, secret);
                st.entry("connection".into()).or_default().insert(
                    "uuid".into(),
                    OwnedValue::try_from(zbus::zvariant::Value::from(uuid.as_str())).unwrap(),
                );
                call!(
                    conn,
                    NM_PATH,
                    NM,
                    "AddAndActivateConnection",
                    &(st, devp, app)
                )
                .inspect_err(started_err)
                .ok()
                .and_then(|m| {
                    m.body()
                        .deserialize::<(OwnedObjectPath, OwnedObjectPath)>()
                        .ok()
                })
                .map(|(p, a)| (a.to_string(), Some((p.to_string(), uuid))))
            }
        },
        _ => None,
    };
    let mut st = sh.lock();
    st.starting -= 1;
    let Some((active, created)) = started else {
        drop(st);
        return fail();
    };
    let (conn, sh2) = (conn.clone(), sh.clone());
    let lost = activation_lost(
        &sh.orphans,
        created.as_ref().map(|c| c.1.clone()),
        generation,
    );
    let finish = move |c: &Cache, ok: bool| {
        if !ok && let Some((p, _)) = created.clone() {
            let (conn, sh) = (conn.clone(), sh2.clone());
            CALLS.run(move || delete_then_fail(&sh, &conn, &p, epoch, generation));
            return vec![];
        }
        let mut evs = vec![Ev::Connected { generation, ok }];
        if ok {
            evs.push(Ev::List(list(c)));
        }
        evs
    };
    // Registered under the lock that ended `starting`, so no removal is pruned in between.
    let evs = st.when(epoch, activation_check(active, finish), lost);
    drop(st);
    sh.send(evs);
}

/// An activation lost with NetworkManager failed; the profile `created` (its `connection.uuid`)
/// for it is left to `delete_orphans`.
fn activation_lost(orphans: &Orphans, created: Option<String>, generation: u64) -> Lost {
    let orphans = orphans.clone();
    Box::new(move |_| {
        if let Some(u) = created {
            orphans.lock().unwrap_or_else(|p| p.into_inner()).push(u);
        }
        vec![Ev::Connected {
            generation,
            ok: false,
        }]
    })
}

/// Delete the profiles of activations lost with the last NetworkManager, found by UUID in the
/// running one's (`Settings.GetConnectionByUuid`) once a snapshot shows it running; until then
/// they stay queued.
fn delete_orphans(conn: &Connection, sh: &Shared) {
    let uuids = std::mem::take(&mut *sh.orphans());
    for u in uuids {
        let Some(p) = sysbus::object_path(call!(
            conn,
            SETTINGS_PATH,
            SETTINGS,
            "GetConnectionByUuid",
            &(u.as_str(),)
        )) else {
            continue;
        };
        if let Err(e) = call!(conn, p.as_str(), SCONN, "Delete", &()) {
            elog::eprintln!("NetworkManager: failed to delete {p}: {e}");
        }
    }
}

/// A random (version 4) UUID, RFC 9562 section 5.4.
fn new_uuid() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("platform RNG failed");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// Delete the profile `p` a failed attempt created, and report the failure once the mirror no
/// longer has it: the password prompt that follows must not find the half-made profile (a
/// password typed at once would go to `set_psk` on a profile being deleted).
fn delete_then_fail(sh: &Shared, c: &Connection, p: &str, epoch: u64, generation: u64) {
    let failed = move || {
        vec![Ev::Connected {
            generation,
            ok: false,
        }]
    };
    if let Err(e) = call!(c, p, SCONN, "Delete", &()) {
        elog::eprintln!("NetworkManager: failed to delete {p}: {e}");
        sh.send(failed());
        return;
    }
    let p = p.to_string();
    sh.when(
        epoch,
        Box::new(move |c| (!c.has(&p, SCONN)).then(failed)),
        Box::new(move |_| failed()),
    );
}

/// Whether an active connection is on the Wi-Fi network `ssid`: its access point, or its
/// profile when NetworkManager reports no access point.
fn active_on_ssid(c: &Cache, ac: &str, ssid: &[u8]) -> bool {
    let ap = s(get(c, ac, ACTIVE, "SpecificObject"));
    if is_path(&ap) && bytes(get(c, &ap, AP, "Ssid")) == ssid {
        return true;
    }
    let conn = s(get(c, ac, ACTIVE, "Connection"));
    is_path(&conn)
        && c.settings
            .get(&conn)
            .is_some_and(|st| profile_ssid(st) == ssid)
}

fn active_on(c: &Cache, ssid: &[u8]) -> Vec<String> {
    let active: Vec<String> = wifi_devices(c)
        .into_iter()
        .map(|d| s(get(c, &d, DEV, "ActiveConnection")))
        .filter(|a| is_path(a) && active_on_ssid(c, a, ssid))
        .collect();
    if !active.is_empty() {
        return active;
    }
    paths(get(c, NM_PATH, NM, "ActiveConnections"))
        .into_iter()
        .filter(|a| active_on_ssid(c, a, ssid))
        .collect()
}

/// Done once each of the active connections is deactivated or gone.
fn deactivation_check(active: Vec<String>) -> Check {
    Box::new(move |c| {
        active
            .iter()
            .all(|ac| matches!(get(c, ac, ACTIVE, "State"), None | Some(V::I(4))))
            .then(|| vec![Ev::List(list(c))])
    })
}

/// Take down the Wi-Fi network `ssid`: every active connection on it, whatever its profile is
/// called (the original ran `nmcli connection down SSID`, which only found a profile named like the
/// network, so "MyNet 1" or a renamed profile was silently left up). Reported once they are
/// all down, or NetworkManager is gone with them.
fn disconnect(sh: &Shared, conn: &Connection, ssid: &[u8]) {
    let (active, epoch) = {
        let st = sh.lock();
        if !st.cache.running {
            return;
        }
        (active_on(&st.cache, ssid), st.epoch)
    };
    if active.is_empty() {
        return;
    }
    let mut ok = true;
    for ac in &active {
        let Some(acp) = obj(ac) else { return };
        ok &= call!(conn, NM_PATH, NM, "DeactivateConnection", &(acp,)).is_ok();
    }
    if !ok {
        return;
    }
    sh.when(
        epoch,
        deactivation_check(active),
        Box::new(|c| vec![Ev::List(list(c))]),
    );
}

/// Set the password of the saved profile for the Wi-Fi network `ssid`, found by its SSID bytes
/// (the original ran `nmcli connection modify SSID wifi-sec.psk PASSWORD`, which missed profiles not
/// named like the network). Among several, the one in use or a device can activate wins.
/// `None` when no saved secured profile exists.
fn set_psk(sh: &Shared, c: &Connection, ssid: &[u8], psk: &str) -> Option<bool> {
    let path = {
        let st = sh.lock();
        let cache = &st.cache;
        let devices = wifi_devices(cache);
        let in_use: Vec<String> = devices
            .iter()
            .map(|d| s(get(cache, d, DEV, "ActiveConnection")))
            .filter(|a| is_path(a))
            .map(|a| s(get(cache, &a, ACTIVE, "Connection")))
            .collect();
        let available: Vec<String> = devices
            .iter()
            .flat_map(|d| paths(get(cache, d, DEV, "AvailableConnections")))
            .collect();
        let mut found: Vec<&String> = cache
            .settings
            .iter()
            .filter(|(p, st)| {
                cache.has(p, SCONN)
                    && profile_ssid(st) == ssid
                    && st.get(SEC).is_some_and(|g| g.contains_key("key-mgmt"))
            })
            .map(|(p, _)| p)
            .collect();
        found.sort_by(|a, b| {
            (!in_use.contains(a), !available.contains(a))
                .cmp(&(!in_use.contains(b), !available.contains(b)))
                .then_with(|| dbus_path_cmp(a, b))
        });
        found.first().map(|p| p.to_string())
    }?;
    let Some(mut st) = fetch_settings(c, &path) else {
        return Some(false);
    };
    if let Ok(m) = call!(c, &path, SCONN, "GetSecrets", &(SEC,))
        && let Ok(secrets) = m.body().deserialize::<Settings>()
    {
        for (g, kv) in secrets {
            st.entry(g).or_default().extend(kv);
        }
    }
    let Ok(v) = OwnedValue::try_from(zbus::zvariant::Value::from(psk.to_string())) else {
        return Some(false);
    };
    st.entry(SEC.to_string())
        .or_default()
        .insert("psk".into(), v);
    Some(call!(c, &path, SCONN, "Update", &(st,)).is_ok())
}

pub struct Network {
    rx: Receiver<Vec<Ev>>,
    shared: Shared,
    conn: Option<Connection>,
    root: usize,
    list: List,
    generation: u64,
    scanning: bool,
    /// Ends the NetworkManager worker with this instance.
    _stop: StopOnDrop,
}

/// The `WifiAccessPoint` objects and the order `friendlyWifiNetworks` last got.
#[derive(Default)]
struct List {
    aps: Vec<(ApInfo, usize)>,
    friendly: Vec<usize>,
}

fn effect(it: &mut crate::eval::Interp, m: &str, a: &[Value]) {
    if let Some(w) = crate::runtime::world_of(it) {
        w.borrow_mut()
            .effects
            .push(crate::svc::Effect::Network(crate::svc::Call::new(
                m,
                a.to_vec(),
            )));
    }
}

/// The `Network` singleton was created: what its properties start as. Its worker starts with
/// the first poll.
fn init(w: &mut World, id: usize) {
    let init = [
        ("wifi", Value::Bool(true)),
        ("ethernet", Value::Bool(false)),
        ("wifiEnabled", Value::Bool(false)),
        ("wifiScanning", Value::Bool(false)),
        ("wifiConnecting", Value::Bool(false)),
        ("wifiConnectTarget", Value::Null),
        ("wifiNetworks", Value::array(vec![])),
        ("active", Value::Null),
        ("friendlyWifiNetworks", Value::array(vec![])),
        ("wifiStatus", Value::str("disconnected")),
        ("networkName", Value::str("")),
        ("networkStrength", Value::Num(0.0)),
        ("materialSymbol", Value::str("wifi_find")),
    ];
    for (p, v) in init {
        w.items[id].props.get_mut(p).unwrap().value = v;
    }
    w.stamp(id);
    w.network_root = Some(id);
}

fn queued(it: &mut crate::eval::Interp, m: &str, a: &[Value]) -> crate::eval::R<Value> {
    effect(it, m, a);
    Ok(Value::Undefined)
}

/// The `Network` singleton of the `Impulse` module (`qmltypes/Impulse/network.qmltypes`).
pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "Network",
    methods: &[
        ("enableWifi", |it, _, a| queued(it, "enableWifi", a)),
        ("toggleWifi", |it, _, a| queued(it, "toggleWifi", a)),
        ("rescanWifi", |it, _, a| queued(it, "rescanWifi", a)),
        ("connectToWifiNetwork", |it, _, a| {
            queued(it, "connectToWifiNetwork", a)
        }),
        ("disconnectWifiNetwork", |it, _, a| {
            queued(it, "disconnectWifiNetwork", a)
        }),
        ("openPublicWifiPortal", |it, _, _| {
            if let Some(w) = crate::runtime::world_of(it) {
                let argv = ["xdg-open", "https://nmcheck.gnome.org/"].map(Value::str);
                w.borrow_mut()
                    .effects
                    .push(crate::svc::Effect::ExecDetached(vec![Value::array(
                        argv.to_vec(),
                    )]));
            }
            Ok(Value::Undefined)
        }),
        ("changePassword", |it, _, a| queued(it, "changePassword", a)),
    ],
    init: Some(init),
};

fn start(root: usize, h: &crate::svc::Handle) -> Network {
    let (tx, rx) = h.channel();
    let shared = Shared {
        st: Arc::default(),
        tx,
        orphans: Arc::default(),
        radio: Arc::default(),
    };
    let stop = StopOnDrop::default();
    if !crate::native::sandboxed() {
        let sh = shared.clone();
        sysbus::spawn(rules(), stop.0.clone(), move |c, r| session(c, r, &sh));
    }
    Network {
        rx,
        shared,
        conn: None,
        root,
        list: List::default(),
        generation: 0,
        scanning: false,
        _stop: stop,
    }
}

/// Run one NetworkManager action on its lane ([`CALLS`]); without NetworkManager on the bus
/// `none` reports what nmcli's failure would.
fn action(
    n: &Network,
    f: impl FnOnce(&Shared, &Connection) + Send + 'static,
    none: impl FnOnce() -> Vec<Ev> + Send + 'static,
) {
    let (conn, sh) = (n.conn.clone(), n.shared.clone());
    CALLS.run(move || match conn {
        Some(c) => f(&sh, &c),
        None => {
            sh.send(none());
        }
    });
}

fn ap_str(w: &mut World, i: usize, p: &str) -> String {
    w.read(i, p).map(|v| v.to_js_string()).unwrap_or_default()
}

/// The SSID bytes of the `WifiAccessPoint` `i`: those of its list entry, else its `ssid` text.
fn ssid_of(w: &mut World, n: &Network, i: usize) -> Vec<u8> {
    match n.list.aps.iter().find(|(_, j)| *j == i) {
        Some((a, _)) => a.ssid_bytes.clone(),
        None => ap_str(w, i, "ssid").into_bytes(),
    }
}

fn start_connect(w: &mut World, n: &mut Network, ssid: Vec<u8>, secret: Option<String>) {
    n.generation += 1;
    w.write(n.root, "wifiConnecting", Value::Bool(true));
    let generation = n.generation;
    action(
        n,
        move |sh, c| connect(sh, c, &ssid, secret.as_deref(), generation),
        move || {
            vec![Ev::Connected {
                generation,
                ok: false,
            }]
        },
    );
}

fn method(w: &mut World, n: &mut Network, m: &str, a: &[Value]) {
    let root = n.root;
    match m {
        "enableWifi" | "toggleWifi" => {
            let on = if m == "toggleWifi" {
                !w.read(root, "wifiEnabled").is_some_and(|v| v.truthy())
            } else {
                a.first()
                    .is_none_or(|v| matches!(v, Value::Undefined) || v.truthy())
            };
            if let Some(c) = &n.conn {
                let v = zbus::zvariant::Value::from(on);
                let sh = n.shared.clone();
                sh.radio.store(on, Ordering::SeqCst);
                sysbus::set_prop(
                    &CALLS,
                    c,
                    NM,
                    NM_PATH,
                    NM,
                    "WirelessEnabled",
                    v,
                    move || {
                        // Refused: a rescan waiting for the radio waits no longer.
                        sh.radio.store(false, Ordering::SeqCst);
                        let evs = sh.lock().settle();
                        sh.send(evs);
                    },
                );
            }
        }
        "rescanWifi" => {
            w.write(root, "wifiScanning", Value::Bool(true));
            if !n.scanning {
                n.scanning = true;
                action(n, rescan, || vec![Ev::ScanDone]);
            }
        }
        "connectToWifiNetwork" => {
            let Some(Value::Item(ap)) = a.first() else {
                return;
            };
            w.write(*ap, "askingPassword", Value::Bool(false));
            w.write(root, "wifiConnectTarget", Value::Item(*ap));
            let ssid = ssid_of(w, n, *ap);
            start_connect(w, n, ssid, None);
        }
        "disconnectWifiNetwork" => {
            let Some(Value::Item(ap)) = w.read(root, "active") else {
                return;
            };
            let ssid = ssid_of(w, n, ap);
            action(n, move |sh, c| disconnect(sh, c, &ssid), Vec::new);
        }
        "changePassword" => {
            let Some(Value::Item(ap)) = a.first() else {
                return;
            };
            w.write(*ap, "askingPassword", Value::Bool(false));
            let ssid = ssid_of(w, n, *ap);
            let psk = a.get(1).map(|v| v.to_js_string()).unwrap_or_default();
            let none = {
                let ssid = ssid.clone();
                move || vec![Ev::PasswordSet { ssid, key: None }]
            };
            action(
                n,
                move |sh, c| {
                    let key = set_psk(sh, c, &ssid, &psk).is_none().then_some(psk);
                    sh.send(vec![Ev::PasswordSet { ssid, key }]);
                },
                none,
            );
        }
        _ => {}
    }
}

fn ipc_object(a: &ApInfo) -> Value {
    let o = crate::eval::new_obj();
    crate::eval::set_own(&o, "active", Value::Bool(a.active));
    crate::eval::set_own(&o, "strength", Value::Num(a.strength));
    crate::eval::set_own(&o, "frequency", Value::Num(a.frequency));
    crate::eval::set_own(&o, "ssid", Value::str(&a.ssid));
    crate::eval::set_own(&o, "bssid", Value::str(&a.bssid));
    crate::eval::set_own(&o, "security", Value::str(&a.security));
    Value::Obj(o)
}

fn write_ap(w: &mut World, i: usize, a: &ApInfo) {
    w.write(i, "lastIpcObject", ipc_object(a));
    w.write(i, "ssid", Value::str(&a.ssid));
    w.write(i, "bssid", Value::str(&a.bssid));
    w.write(i, "strength", Value::Num(a.strength.trunc()));
    w.write(i, "frequency", Value::Num(a.frequency.trunc()));
    w.write(i, "active", Value::Bool(a.active));
    w.write(i, "security", Value::str(&a.security));
    w.write(i, "isSecure", Value::Bool(!a.security.is_empty()));
}

/// The original's list update: drop entries no longer listed, refresh matches, append new ones. Only
/// entries whose fields changed are written, and `wifiNetworks` only when its members or their
/// order changed (a new array always notifies, equal or not). An entry is its network (SSID
/// bytes): the original matched on frequency and BSSID as well, so when a dual-band or mesh network's
/// entry moved to another access point its `WifiAccessPoint` was replaced, dropping
/// `wifiConnectTarget` mid-connect (no password prompt after a refused password).
fn apply_list(w: &mut World, root: usize, list: &mut List, new: Vec<ApInfo>) {
    let key = |a: &ApInfo, b: &ApInfo| a.ssid_bytes == b.ssid_bytes;
    let before: Vec<usize> = list.aps.iter().map(|(_, i)| *i).collect();
    let (keep, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut list.aps)
        .into_iter()
        .partition(|(a, _)| new.iter().any(|l| key(l, a)));
    for (_, i) in gone {
        w.destroy(i);
    }
    list.aps = keep;
    for l in new {
        if let Some((a, i)) = list.aps.iter_mut().find(|(a, _)| key(&l, a)) {
            if *a != l {
                write_ap(w, *i, &l);
                *a = l;
            }
        } else {
            let i = w.new_inst("WifiAccessPoint", Some(root));
            for (p, ty) in [
                ("lastIpcObject", "var"),
                ("ssid", "string"),
                ("bssid", "string"),
                ("strength", "int"),
                ("frequency", "int"),
                ("active", "bool"),
                ("security", "string"),
                ("isSecure", "bool"),
                ("askingPassword", "bool"),
            ] {
                w.declare(i, p, ty);
            }
            w.write(i, "askingPassword", Value::Bool(false));
            write_ap(w, i, &l);
            list.aps.push((l, i));
        }
    }
    let ids: Vec<usize> = list.aps.iter().map(|(_, i)| *i).collect();
    if ids != before {
        let ids = ids.into_iter().map(Value::Item).collect();
        w.write(root, "wifiNetworks", Value::array(ids));
    }
}

fn num_of(w: &mut World, i: usize, p: &str) -> f64 {
    w.read(i, p).map_or(0.0, |v| v.to_number())
}

/// Derived bindings of Network.qml: `active`, `friendlyWifiNetworks` (written only when its
/// order changed), `materialSymbol`.
fn derive(w: &mut World, root: usize, list: &mut List) {
    let ids: Vec<usize> = list.aps.iter().map(|(_, i)| *i).collect();
    let active = ids
        .iter()
        .copied()
        .find(|&i| w.read(i, "active").is_some_and(|v| v.truthy()));
    w.write(root, "active", active.map_or(Value::Null, Value::Item));
    let mut friendly: Vec<(bool, f64, usize)> = ids
        .iter()
        .map(|&i| {
            (
                w.read(i, "active").is_some_and(|v| v.truthy()),
                num_of(w, i, "strength"),
                i,
            )
        })
        .collect();
    // `[...wifiNetworks].sort(...)`: V4's unstable sort decides the order of equal strengths.
    crate::v4sort::sort(&mut friendly, |a, b| match (a.0, b.0) {
        (true, false) => true,
        (false, true) => false,
        _ => b.1 - a.1 < 0.0,
    });
    let friendly: Vec<usize> = friendly.into_iter().map(|f| f.2).collect();
    if friendly != list.friendly {
        let arr = friendly.iter().map(|&i| Value::Item(i)).collect();
        w.write(root, "friendlyWifiNetworks", Value::array(arr));
        list.friendly = friendly;
    }
    let flag = |w: &mut World, p| w.read(root, p).is_some_and(|v| v.truthy());
    let status = ap_str(w, root, "wifiStatus");
    let strength = active.map_or(0.0, |i| num_of(w, i, "strength"));
    let sym = if flag(w, "ethernet") {
        "lan"
    } else if flag(w, "wifiEnabled") && status == "connected" {
        match strength {
            s if s > 83.0 => "signal_wifi_4_bar",
            s if s > 67.0 => "network_wifi",
            s if s > 50.0 => "network_wifi_3_bar",
            s if s > 33.0 => "network_wifi_2_bar",
            s if s > 17.0 => "network_wifi_1_bar",
            _ => "signal_wifi_0_bar",
        }
    } else {
        match status.as_str() {
            "connecting" => "signal_wifi_statusbar_not_connected",
            "disconnected" => "wifi_find",
            "disabled" => "signal_wifi_off",
            _ => "signal_wifi_bad",
        }
    };
    w.write(root, "materialSymbol", Value::str(sym));
}

fn poll(w: &mut World, root: usize, calls: Vec<crate::svc::Call>) -> bool {
    let mut n = w
        .network
        .take()
        .unwrap_or_else(|| start(root, &w.waker.handle(crate::svc::Svc::Network)));
    let mut changed = false;
    for crate::svc::Call { method: k, args: a } in calls {
        method(w, &mut n, &k, &a);
        changed = true;
    }
    let evs: Vec<Ev> = std::iter::from_fn(|| n.rx.try_recv().ok())
        .flatten()
        .collect();
    for ev in evs {
        changed = true;
        match ev {
            Ev::Conn(c) => n.conn = Some(c),
            Ev::Status(st) => {
                w.write(root, "wifiStatus", Value::str(st.wifi_status));
                w.write(root, "ethernet", Value::Bool(st.ethernet));
                w.write(root, "wifi", Value::Bool(st.wifi));
                w.write(root, "wifiEnabled", Value::Bool(st.wifi_enabled));
                w.write(
                    root,
                    "networkName",
                    Value::str(st.name.as_deref().unwrap_or("")),
                );
            }
            Ev::List(l) => apply_list(w, root, &mut n.list, l),
            Ev::Connected { generation, ok } => {
                if generation != n.generation {
                    continue;
                }
                w.write(root, "wifiConnecting", Value::Bool(false));
                // The original's onExited throws on a null target (a re-attempt after changePassword).
                if let Some(Value::Item(t)) = w.read(root, "wifiConnectTarget") {
                    w.write(t, "askingPassword", Value::Bool(!ok));
                    w.write(root, "wifiConnectTarget", Value::Null);
                }
            }
            Ev::Live(aps) => {
                let strength = aps.last().map_or(0.0, |(_, v)| *v as f64);
                w.write(root, "networkStrength", Value::Num(strength));
                for (bssid, v) in aps {
                    for (a, i) in n.list.aps.iter_mut() {
                        if a.active && a.bssid == bssid && a.strength != v as f64 {
                            a.strength = v as f64;
                            write_ap(w, *i, a);
                        }
                    }
                }
            }
            // No scan result is coming either way: the original's spinner stayed on when nmcli printed
            // nothing, which left the rescan button disabled.
            Ev::ScanDone => {
                n.scanning = false;
                w.write(root, "wifiScanning", Value::Bool(false));
            }
            // Join the network the password was typed for, with the password unless its saved
            // profile took it, and ask again if it is refused.
            Ev::PasswordSet { ssid, key } => {
                let ap = n
                    .list
                    .aps
                    .iter()
                    .find(|(a, _)| a.ssid_bytes == ssid)
                    .map(|(_, i)| *i);
                if let Some(ap) = ap {
                    w.write(root, "wifiConnectTarget", Value::Item(ap));
                }
                start_connect(w, &mut n, ssid, key);
            }
        }
    }
    if changed {
        derive(w, root, &mut n.list);
    }
    w.network = Some(n);
    changed
}

impl Runtime {
    /// Drive the native `Network` singleton (if instantiated).
    pub(crate) fn poll_network(&mut self, calls: Vec<crate::svc::Call>) -> bool {
        let Some(root) = self.world.borrow().network_root else {
            return false;
        };
        poll(&mut self.world.borrow_mut(), root, calls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(ty: i64, state: i64) -> DevRow {
        DevRow {
            ty,
            state,
            external: false,
        }
    }

    #[test]
    fn connection_type_reports_device_states() {
        // An unplugged cable is not ethernet (the original matched "connected" in "disconnected").
        assert_eq!(
            connection_type(&[dev(1, 30)], 4),
            (false, false, "disconnected")
        );
        assert_eq!(
            connection_type(&[dev(1, 100)], 4),
            (false, true, "disconnected")
        );
        assert_eq!(
            connection_type(&[dev(1, 20)], 4),
            (false, false, "disconnected")
        );
        assert_eq!(
            connection_type(&[dev(2, 100)], 4),
            (true, false, "connected")
        );
        assert_eq!(
            connection_type(&[dev(2, 100)], 3),
            (false, false, "limited")
        );
        assert_eq!(
            connection_type(&[dev(2, 70)], 4),
            (false, false, "connecting")
        );
        assert_eq!(
            connection_type(&[dev(2, 20)], 4),
            (false, false, "disabled")
        );
        assert_eq!(
            connection_type(&[dev(2, 110)], 4),
            (false, false, "disconnected")
        );
        // A connected card wins over an idle second one, in either order.
        assert_eq!(
            connection_type(&[dev(2, 30), dev(2, 100)], 4),
            (true, false, "connected")
        );
        assert_eq!(
            connection_type(&[dev(2, 100), dev(2, 30)], 4),
            (true, false, "connected")
        );
    }

    fn ap(active: bool, strength: i64, ssid: &str, bssid: &str) -> RawAp {
        RawAp {
            active,
            strength,
            frequency: 2412,
            ssid: ssid.as_bytes().to_vec(),
            bssid: bssid.into(),
            security: String::new(),
        }
    }

    #[test]
    fn networks_keep_ssid_bytes() {
        let raw = |ssid: &[u8], bssid: &str| RawAp {
            ssid: ssid.to_vec(),
            ..ap(false, 50, "", bssid)
        };
        // Latin-1 "café" and "cafë": distinct networks that both show as "caf\u{fffd}".
        let l = networks(&[raw(b"caf\xe9", "01"), raw(b"caf\xeb", "02")]);
        assert_eq!(l.len(), 2, "{l:?}");
        assert_eq!(
            (l[0].ssid.as_str(), &l[0].ssid_bytes[..]),
            ("caf\u{fffd}", &b"caf\xe9"[..])
        );
        assert_eq!(&l[1].ssid_bytes[..], b"caf\xeb");
    }

    #[test]
    fn networks_dedupes_like_the_original() {
        let l = networks(&[
            ap(false, 50, "a", "01"),
            ap(false, 80, "b", "02"),
            ap(false, 70, "a", "03"),
            ap(true, 20, "b", "04"),
            ap(false, 90, "b", "05"),
            ap(false, 99, "", "06"),
        ]);
        let got: Vec<_> = l
            .iter()
            .map(|a| (a.ssid.as_str(), a.bssid.as_str()))
            .collect();
        assert_eq!(got, [("a", "03"), ("b", "04")]);
    }

    #[test]
    fn networks_keep_ssids_with_separators() {
        let l = networks(&[ap(false, 1, "x:y", "AA:BB")]);
        assert_eq!(l[0].ssid, "x:y");
        assert_eq!(l[0].bssid, "AA:BB");
        assert_eq!(l[0].frequency, 2412.0);
        let l = networks(&[ap(false, 1, "z\\", "AA")]);
        assert_eq!((l[0].ssid.as_str(), l[0].bssid.as_str()), ("z\\", "AA"));
    }

    #[test]
    fn security_matches_nmcli() {
        assert_eq!(security(0, 0, 0), "");
        assert_eq!(security(1, 0, 0), "WEP");
        assert_eq!(security(1, 0x100, 0x100), "WPA1 WPA2");
        assert_eq!(security(1, 0, 0x400), "WPA3");
        assert_eq!(security(0, 0, 0x800), "OWE");
        assert_eq!(security(0, 0, 0x1000), "OWE-TM");
        assert_eq!(security(0, 0, 0x2000), "WPA-EAP-SUITE-B-192");
        assert_eq!(security(0, 0, 0x900), "WPA2 OWE");
        assert_eq!(security(1, 0, 0x500), "WPA2 WPA3");
        assert_eq!(security(1, 0x200, 0x200), "WPA1 WPA2 802.1X");
    }

    type ZProps = HashMap<&'static str, zbus::zvariant::Value<'static>>;

    fn op(p: &str) -> zbus::zvariant::Value<'static> {
        zbus::zvariant::ObjectPath::try_from(p.to_string())
            .unwrap()
            .into()
    }

    fn ops(ps: &[&str]) -> zbus::zvariant::Value<'static> {
        let v: Vec<zbus::zvariant::ObjectPath<'static>> = ps
            .iter()
            .map(|p| zbus::zvariant::ObjectPath::try_from(p.to_string()).unwrap())
            .collect();
        v.into()
    }

    fn added(path: &str, ifaces: Vec<(&'static str, ZProps)>) -> Message {
        let ifaces: HashMap<&str, ZProps> = ifaces.into_iter().collect();
        Message::signal(OM_PATH, OM, "InterfacesAdded")
            .unwrap()
            .build(&(zbus::zvariant::ObjectPath::try_from(path).unwrap(), ifaces))
            .unwrap()
    }

    fn changed(path: &str, iface: &str, props: ZProps) -> Message {
        Message::signal(path, sysbus::PROPS, "PropertiesChanged")
            .unwrap()
            .build(&(iface, props, Vec::<String>::new()))
            .unwrap()
    }

    const D: &str = "/org/freedesktop/NetworkManager/Devices/3";
    const A1: &str = "/org/freedesktop/NetworkManager/AccessPoint/1";
    const A2: &str = "/org/freedesktop/NetworkManager/AccessPoint/2";
    const AC: &str = "/org/freedesktop/NetworkManager/ActiveConnection/7";

    fn ap_props(ssid: &str, bssid: &str, strength: u8, rsn: u32) -> ZProps {
        HashMap::from([
            ("Ssid", ssid.as_bytes().to_vec().into()),
            ("HwAddress", bssid.to_string().into()),
            ("Strength", strength.into()),
            ("Frequency", 2412u32.into()),
            ("Flags", 1u32.into()),
            ("WpaFlags", 0u32.into()),
            ("RsnFlags", rsn.into()),
        ])
    }

    /// A Wi-Fi device connected to "Home" (A1) with "Cafe" (A2) in range, built only from
    /// ObjectManager signals.
    fn home() -> Cache {
        let mut c = Cache {
            running: true,
            ..Default::default()
        };
        let ms = [
            added(
                NM_PATH,
                vec![(
                    NM,
                    HashMap::from([
                        ("Devices", ops(&[D])),
                        ("ActiveConnections", ops(&[AC])),
                        ("WirelessEnabled", true.into()),
                        ("Connectivity", 4u32.into()),
                    ]),
                )],
            ),
            added(
                D,
                vec![
                    (
                        DEV,
                        HashMap::from([
                            ("DeviceType", 2u32.into()),
                            ("State", 100u32.into()),
                            ("ActiveConnection", op(AC)),
                        ]),
                    ),
                    (
                        WL,
                        HashMap::from([
                            ("ActiveAccessPoint", op(A1)),
                            ("AccessPoints", ops(&[A1, A2])),
                        ]),
                    ),
                ],
            ),
            added(A1, vec![(AP, ap_props("Home", "AA:01", 70, 0x100))]),
            added(A2, vec![(AP, ap_props("Cafe", "AA:02", 40, 0))]),
            added(
                AC,
                vec![(
                    ACTIVE,
                    HashMap::from([("Id", "Home 1".to_string().into()), ("State", 2u32.into())]),
                )],
            ),
        ];
        for m in &ms {
            assert_ne!(apply(&mut c, m), Applied::Nothing);
        }
        c
    }

    #[test]
    fn mirror_follows_object_manager_signals() {
        let mut c = home();
        let st = status(&c);
        assert_eq!(
            (st.wifi, st.wifi_status, st.wifi_enabled, st.name.as_deref()),
            (true, "connected", true, Some("Home 1"))
        );
        assert_eq!(live(&c), [("AA:01".to_string(), 70)]);
        let l = list(&c);
        assert_eq!(
            l.iter()
                .map(|a| (a.ssid.as_str(), a.strength, a.active, a.security.as_str()))
                .collect::<Vec<_>>(),
            [("Home", 70.0, true, "WPA2"), ("Cafe", 40.0, false, "WEP")]
        );
        // The in-use access point's signal comes from its PropertiesChanged, no scan or call.
        let m = changed(A1, AP, HashMap::from([("Strength", 30u8.into())]));
        assert_eq!(apply(&mut c, &m), Applied::Changed);
        assert_eq!(live(&c), [("AA:01".to_string(), 30)]);
        // A device state from StateChanged(new, old, reason).
        let m = Message::signal(D, DEV, "StateChanged")
            .unwrap()
            .build(&(30u32, 100u32, 0u32))
            .unwrap();
        assert_eq!(apply(&mut c, &m), Applied::Changed);
        assert_eq!(status(&c).wifi_status, "disconnected");
        let m = Message::signal(OM_PATH, OM, "InterfacesRemoved")
            .unwrap()
            .build(&(
                zbus::zvariant::ObjectPath::try_from(A2).unwrap(),
                vec![AP.to_string()],
            ))
            .unwrap();
        assert_eq!(apply(&mut c, &m), Applied::Changed);
        assert!(!c.objs.contains_key(A2));
        assert_eq!(list(&c).len(), 1);
        // A profile appearing or changing has its settings read again.
        let p = "/org/freedesktop/NetworkManager/Settings/4";
        let m = added(p, vec![(SCONN, HashMap::new())]);
        assert_eq!(apply(&mut c, &m), Applied::Profile(p.into()));
        let m = Message::signal(p, SCONN, "Updated")
            .unwrap()
            .build(&())
            .unwrap();
        assert_eq!(apply(&mut c, &m), Applied::Profile(p.into()));
    }

    #[test]
    fn signals_sent_before_the_snapshot_are_dropped() {
        let pc = |v: u8| {
            Message::signal(A1, sysbus::PROPS, "PropertiesChanged")
                .unwrap()
                .sender(":1.5")
                .unwrap()
                .build(&(
                    AP,
                    HashMap::from([("Strength", zbus::zvariant::Value::U8(v))]),
                    Vec::<String>::new(),
                ))
                .unwrap()
        };
        let early = pc(10);
        let mut c = home();
        c.origin = Some((":1.5".into(), early.primary_header().serial_num().get() + 1));
        assert_eq!(apply(&mut c, &early), Applied::Nothing);
        assert_eq!(live(&c), [("AA:01".to_string(), 70)]);
        let late = pc(20);
        assert_eq!(apply(&mut c, &late), Applied::Changed);
        assert_eq!(live(&c), [("AA:01".to_string(), 20)]);
    }

    fn removed(path: &str, iface: &str) -> Message {
        Message::signal(OM_PATH, OM, "InterfacesRemoved")
            .unwrap()
            .build(&(
                zbus::zvariant::ObjectPath::try_from(path).unwrap(),
                vec![iface.to_string()],
            ))
            .unwrap()
    }

    fn ac_state(path: &str, state: u32) -> Message {
        changed(path, ACTIVE, HashMap::from([("State", state.into())]))
    }

    const NEW: &str = "/org/freedesktop/NetworkManager/ActiveConnection/8";

    fn new_ac(state: u32) -> Message {
        added(
            NEW,
            vec![(ACTIVE, HashMap::from([("State", state.into())]))],
        )
    }

    fn home_state() -> State {
        State {
            cache: home(),
            epoch: 1,
            ..Default::default()
        }
    }

    fn connected(ac: &str) -> Check {
        activation_check(ac.into(), |_, ok| vec![Ev::Connected { generation: 1, ok }])
    }

    /// What a connect reports when the mirror it waited on is rebuilt first.
    fn lost_connect() -> Lost {
        Box::new(|_| {
            vec![Ev::Connected {
                generation: 1,
                ok: false,
            }]
        })
    }

    /// What the events say: `Connected` outcomes, `ScanDone`.
    fn said(evs: &[Ev]) -> Vec<String> {
        evs.iter()
            .filter_map(|e| match e {
                Ev::Connected { ok, .. } => Some(format!("connected {ok}")),
                Ev::ScanDone => Some("scanned".into()),
                _ => None,
            })
            .collect()
    }

    /// An activation that fails and is removed before its waiter exists (the router applied
    /// the signals before the action got the reply) is decided when the waiter registers.
    #[test]
    fn activation_ended_before_the_reply_fails_at_once() {
        let mut st = home_state();
        st.starting = 1;
        let a = st.absorb(&[new_ac(1), ac_state(NEW, 4), removed(NEW, ACTIVE)]);
        assert!(said(&a.evs).is_empty());
        st.starting -= 1;
        let evs = st.when(1, connected(NEW), lost_connect());
        assert_eq!(said(&evs), ["connected false"]);
        assert!(st.waiters.is_empty());
        // The last state is kept only while an activation call is unanswered.
        st.absorb(&[changed(A1, AP, HashMap::from([("Strength", 1u8.into())]))]);
        assert!(st.cache.ended.is_empty());
    }

    /// Added, failed and removed in one batch: the waiter sees the failure within the batch.
    #[test]
    fn activation_failing_within_one_batch_is_reported_by_it() {
        let mut st = home_state();
        assert!(st.when(1, connected(NEW), lost_connect()).is_empty());
        let a = st.absorb(&[new_ac(1), ac_state(NEW, 4), removed(NEW, ACTIVE)]);
        assert_eq!(said(&a.evs), ["connected false"]);
        assert!(st.waiters.is_empty());
        // Removed while still activating, in a later batch, is a failure too.
        const NEXT: &str = "/org/freedesktop/NetworkManager/ActiveConnection/9";
        assert!(st.when(1, connected(NEXT), lost_connect()).is_empty());
        let a = st.absorb(&[added(
            NEXT,
            vec![(ACTIVE, HashMap::from([("State", 1u32.into())]))],
        )]);
        assert!(said(&a.evs).is_empty());
        let a = st.absorb(&[removed(NEXT, ACTIVE)]);
        assert_eq!(said(&a.evs), ["connected false"]);
    }

    /// A slow activation (802.1X, a secret agent's prompt, long DHCP) only ends when
    /// NetworkManager says so; whatever else happens meanwhile does not fail it.
    #[test]
    fn slow_activation_waits_for_networkmanager() {
        let mut st = home_state();
        assert!(st.when(1, connected(NEW), lost_connect()).is_empty());
        for s in [1, 1, 1] {
            let a = st.absorb(&[
                new_ac(s),
                changed(A1, AP, HashMap::from([("Strength", 50u8.into())])),
            ]);
            assert!(said(&a.evs).is_empty());
        }
        assert_eq!(st.waiters.len(), 1);
        let a = st.absorb(&[ac_state(NEW, 2)]);
        assert_eq!(said(&a.evs), ["connected true"]);
        assert!(st.waiters.is_empty());
    }

    /// Losing NetworkManager ends every waiter through `lost`, and a waiter for the mirror
    /// before a restart is lost at once.
    #[test]
    fn waiters_end_with_the_daemon() {
        let mut st = home_state();
        assert!(st.when(1, connected(NEW), lost_connect()).is_empty());
        let gone = Message::signal(
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameOwnerChanged",
        )
        .unwrap()
        .build(&(NM, ":1.5", ""))
        .unwrap();
        let a = st.absorb(&[gone]);
        assert_eq!(said(&a.evs), ["connected false"]);
        assert!(!a.resync && st.waiters.is_empty() && !st.cache.running);
        let evs = st.when(1, connected(NEW), lost_connect());
        assert_eq!(said(&evs), ["connected false"]);
        assert!(st.waiters.is_empty());
    }

    fn owner(old: &str, new: &str) -> Message {
        Message::signal(
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameOwnerChanged",
        )
        .unwrap()
        .build(&(NM, old, new))
        .unwrap()
    }

    /// The Wi-Fi list follows the mirror like the status: access points NetworkManager's own
    /// scans add or drop, and NetworkManager stopping, reach the shell without a scan.
    #[test]
    fn list_follows_the_mirror() {
        let mut st = home_state();
        let mut seen = Reported::default();
        let lists = |evs: &[Ev]| -> Vec<Vec<String>> {
            evs.iter()
                .filter_map(|e| match e {
                    Ev::List(l) => Some(l.iter().map(|a| a.ssid.clone()).collect()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(lists(&report(&st.cache, &mut seen)), [["Home", "Cafe"]]);
        assert!(lists(&report(&st.cache, &mut seen)).is_empty());
        st.absorb(&[changed(
            D,
            WL,
            HashMap::from([("AccessPoints", ops(&[A1]))]),
        )]);
        assert_eq!(lists(&report(&st.cache, &mut seen)), [["Home"]]);
        st.absorb(&[owner(":1.5", "")]);
        assert_eq!(lists(&report(&st.cache, &mut seen)), [Vec::<String>::new()]);
    }

    /// An activation lost with NetworkManager fails at once, and the profile made for it is left
    /// to delete by its UUID once a NetworkManager is back (its path names nothing by then).
    #[test]
    fn profile_of_an_activation_lost_with_the_daemon_is_deleted_by_uuid() {
        let mut st = home_state();
        let orphans = Orphans::default();
        let uuid = new_uuid();
        let lost = activation_lost(&orphans, Some(uuid.clone()), 1);
        assert!(st.when(1, connected(NEW), lost).is_empty());
        let saved = activation_lost(&orphans, None, 1);
        let next = "/org/freedesktop/NetworkManager/ActiveConnection/9";
        assert!(st.when(1, connected(next), saved).is_empty());
        let a = st.absorb(&[owner(":1.5", "")]);
        assert_eq!(said(&a.evs), ["connected false", "connected false"]);
        assert_eq!(*orphans.lock().unwrap(), std::slice::from_ref(&uuid));
        // RFC 9562 version 4, variant 10.
        let b = uuid.as_bytes();
        assert_eq!(
            (uuid.len(), b[8], b[13], b[14], b[18]),
            (36, b'-', b'-', b'4', b'-')
        );
        assert!(matches!(b[19], b'8' | b'9' | b'a' | b'b'), "{uuid}");
        assert_ne!(new_uuid(), uuid);
    }

    /// A deactivation is reported whenever it completes, not dropped after a while.
    #[test]
    fn late_deactivation_is_reported() {
        let mut st = home_state();
        assert!(
            st.when(1, deactivation_check(vec![AC.into()]), lost_connect())
                .is_empty()
        );
        // A deactivation reports the Wi-Fi list as it is once the connection is down.
        let lists = |evs: &[Ev]| evs.iter().filter(|e| matches!(e, Ev::List(_))).count();
        for _ in 0..3 {
            let a = st.absorb(&[ac_state(AC, 3)]);
            assert_eq!((lists(&a.evs), st.waiters.len()), (0, 1));
        }
        let a = st.absorb(&[ac_state(AC, 4), removed(AC, ACTIVE)]);
        assert_eq!((lists(&a.evs), st.waiters.len()), (1, 0));
    }

    fn not_allowed(msg: &str) -> zbus::Result<Message> {
        let m = Message::method_call(D, "RequestScan")
            .unwrap()
            .build(&())
            .unwrap();
        Err(zbus::Error::MethodError(
            NOT_ALLOWED.try_into().unwrap(),
            Some(msg.into()),
            m,
        ))
    }

    /// A refused RequestScan is not waited for, whatever its text says (NetworkManager 1.22 to
    /// 1.24 refuse in several cases with one error name); a taken request is, until `LastScan`
    /// passes the cutoff.
    #[test]
    fn scan_waits_only_for_scans_that_come() {
        for msg in [
            "Scanning not allowed while unavailable",
            "Scanning not allowed while unavailable or activating",
            "Scanning not allowed while already scanning",
            "Scanning not allowed immediately following previous scan",
        ] {
            assert!(!scan_coming(&not_allowed(msg)), "{msg}");
        }
        let ok = Message::method_call(D, "RequestScan")
            .unwrap()
            .build(&())
            .unwrap();
        assert!(scan_coming(&Ok(ok)));

        let mut st = home_state();
        let last = |t: i64| changed(D, WL, HashMap::from([("LastScan", t.into())]));
        st.absorb(&[last(900)]);
        let evs = st.when(
            1,
            scan_check(vec![], vec![], Arc::default(), no_ask()),
            Box::new(scanned),
        );
        assert_eq!(said(&evs), ["scanned"]);
        assert!(
            st.when(
                1,
                scan_check(vec![(D.into(), 1000)], vec![], Arc::default(), no_ask()),
                Box::new(scanned)
            )
            .is_empty()
        );
        assert!(said(&st.absorb(&[last(950)]).evs).is_empty());
        assert_eq!(said(&st.absorb(&[last(1200)]).evs), ["scanned"]);
        // A device becoming unavailable ends its scan.
        assert!(
            st.when(
                1,
                scan_check(vec![(D.into(), 5000)], vec![], Arc::default(), no_ask()),
                Box::new(scanned)
            )
            .is_empty()
        );
        let unavailable = Message::signal(D, DEV, "StateChanged")
            .unwrap()
            .build(&(20u32, 100u32, 0u32))
            .unwrap();
        assert_eq!(said(&st.absorb(&[unavailable]).evs), ["scanned"]);
    }

    fn no_ask() -> Ask {
        Box::new(|_, _| panic!("no device waits"))
    }

    /// A device that refused the scan while unavailable is asked again once the mirror shows it
    /// available with the radio on, and the scan ends with its LastScan or its refusal; with the
    /// radio off it is not waited for.
    #[test]
    fn refused_scan_is_asked_again_when_the_device_can_scan() {
        let mut st = home_state();
        let state = |s: u32| {
            Message::signal(D, DEV, "StateChanged")
                .unwrap()
                .build(&(s, 0u32, 0u32))
                .unwrap()
        };
        let asked: Arc<Mutex<Vec<(String, Refused)>>> = Arc::default();
        let a = asked.clone();
        let ask: Ask = Box::new(move |d, r| a.lock().unwrap().push((d, r)));
        st.absorb(&[state(20)]);
        let check = scan_check(vec![], vec![D.into()], Arc::default(), ask);
        assert!(st.when(1, check, Box::new(scanned)).is_empty());
        assert!(asked.lock().unwrap().is_empty());
        assert!(said(&st.absorb(&[state(30)]).evs).is_empty());
        let (d, refused) = asked.lock().unwrap().pop().unwrap();
        assert_eq!(d, D);
        // Refused again: not waited for.
        refused.lock().unwrap().push(d);
        assert_eq!(said(&st.settle()), ["scanned"]);

        // The radio goes off while the device is unavailable.
        st.absorb(&[state(20)]);
        let radio = Arc::new(AtomicBool::new(false));
        let check = scan_check(vec![], vec![D.into()], radio, no_ask());
        assert!(st.when(1, check, Box::new(scanned)).is_empty());
        let off = changed(
            NM_PATH,
            NM,
            HashMap::from([("WirelessEnabled", false.into())]),
        );
        assert_eq!(said(&st.absorb(&[off]).evs), ["scanned"]);
    }

    #[test]
    fn match_rules_take_only_what_is_shown() {
        let rules = rules();
        let wanted = |m: &Message| rules.iter().any(|r| r.matches(m).unwrap());
        let props = |path: &str, iface: &str| changed(path, iface, HashMap::new());
        for (path, iface) in [
            (NM_PATH, NM),
            (D, DEV),
            (D, WL),
            (A1, AP),
            (AC, ACTIVE),
            ("/org/freedesktop/NetworkManager/IP4Config/2", IP4),
        ] {
            assert!(wanted(&props(path, iface)), "{iface}");
        }
        for (path, iface) in [
            (D, "org.freedesktop.NetworkManager.Device.Statistics"),
            (
                "/org/freedesktop/NetworkManager/DHCP4Config/2",
                "org.freedesktop.NetworkManager.DHCP4Config",
            ),
            (SETTINGS_PATH, SETTINGS),
            (
                "/org/freedesktop/NetworkManager/AgentManager",
                "org.freedesktop.NetworkManager.AgentManager",
            ),
        ] {
            assert!(!wanted(&props(path, iface)), "{iface}");
        }
        // The legacy per-interface PropertiesChanged NetworkManager sends besides the standard one.
        let legacy = Message::signal(A1, AP, "PropertiesChanged")
            .unwrap()
            .build(&(HashMap::<&str, zbus::zvariant::Value>::new(),))
            .unwrap();
        assert!(!wanted(&legacy));
        let other = Message::signal(D, WL, "AccessPointAdded")
            .unwrap()
            .build(&(zbus::zvariant::ObjectPath::try_from(A1).unwrap(),))
            .unwrap();
        assert!(!wanted(&other));
        assert!(wanted(&added(A1, vec![])));
    }

    #[test]
    fn new_profiles_carry_the_key() {
        assert_eq!(key_for(0, 0, 0), Key::None);
        assert_eq!(key_for(0, 0, 0x800), Key::None);
        assert_eq!(key_for(1, 0, 0), Key::Wep);
        assert_eq!(key_for(1, 0x100, 0x100), Key::Psk("wpa-psk"));
        assert_eq!(key_for(1, 0, 0x500), Key::Psk("wpa-psk"));
        assert_eq!(key_for(1, 0, 0x400), Key::Psk("sae"));
        assert_eq!(key_for(1, 0x200, 0x200), Key::Enterprise);
        let str_of = |st: &Settings, k: &str| st.get(SEC).and_then(|g| g.get(k)).map(|v| conv(v));
        let st = new_profile(Key::Psk("sae"), Some("hunter22"));
        assert_eq!(str_of(&st, "key-mgmt"), Some(V::S("sae".into())));
        assert_eq!(str_of(&st, "psk"), Some(V::S("hunter22".into())));
        let st = new_profile(Key::Wep, Some("abcde"));
        assert_eq!(str_of(&st, "key-mgmt"), Some(V::S("none".into())));
        assert_eq!(str_of(&st, "wep-key-type"), Some(V::I(1)));
        let st = new_profile(Key::Wep, Some("a long passphrase"));
        assert_eq!(str_of(&st, "wep-key-type"), Some(V::I(2)));
        assert!(new_profile(Key::None, None).is_empty());
        assert!(new_profile(Key::Enterprise, Some("x")).is_empty());
    }

    #[test]
    fn list_arrays_are_written_only_when_members_or_order_change() {
        let rt = Runtime::new(None);
        let mut w = rt.world.borrow_mut();
        let root = crate::types::native_singleton(&mut w, "Network");
        let mut l = List::default();
        let info = |ssid: &str, strength: f64, active: bool| ApInfo {
            active,
            strength,
            frequency: 2412.0,
            ssid: ssid.into(),
            bssid: format!("AA:{ssid}"),
            security: String::new(),
            ssid_bytes: ssid.as_bytes().to_vec(),
        };
        let arrays = |w: &mut World| {
            (
                w.read(root, "wifiNetworks").unwrap(),
                w.read(root, "friendlyWifiNetworks").unwrap(),
            )
        };
        apply_list(
            &mut w,
            root,
            &mut l,
            vec![info("a", 70.0, true), info("b", 40.0, false)],
        );
        derive(&mut w, root, &mut l);
        let (nets, friendly) = arrays(&mut w);
        let ipc = w.read(l.aps[1].1, "lastIpcObject").unwrap();
        // The same list again, then only the in-use network's signal: no new arrays, and the
        // unchanged entry keeps its object.
        apply_list(
            &mut w,
            root,
            &mut l,
            vec![info("a", 70.0, true), info("b", 40.0, false)],
        );
        derive(&mut w, root, &mut l);
        apply_list(
            &mut w,
            root,
            &mut l,
            vec![info("a", 60.0, true), info("b", 40.0, false)],
        );
        derive(&mut w, root, &mut l);
        let (n2, f2) = arrays(&mut w);
        assert!(n2.strict_eq(&nets) && f2.strict_eq(&friendly));
        assert!(w.read(l.aps[1].1, "lastIpcObject").unwrap().strict_eq(&ipc));
        assert_eq!(num_of(&mut w, l.aps[0].1, "strength"), 60.0);
        // A new member, then an order change, write new arrays.
        apply_list(
            &mut w,
            root,
            &mut l,
            vec![
                info("a", 60.0, false),
                info("b", 40.0, false),
                info("c", 90.0, false),
            ],
        );
        derive(&mut w, root, &mut l);
        let (n3, f3) = arrays(&mut w);
        assert!(!n3.strict_eq(&n2) && !f3.strict_eq(&f2));
        let order = |w: &mut World, v: &Value| {
            let Value::Obj(o) = v else { panic!() };
            let ids: Vec<String> = o
                .borrow()
                .array
                .clone()
                .unwrap()
                .iter()
                .map(|v| match v {
                    Value::Item(i) => ap_str(w, *i, "ssid"),
                    _ => String::new(),
                })
                .collect();
            ids
        };
        assert_eq!(order(&mut w, &f3), ["c", "a", "b"]);
        // A dual-band network's entry moving to its other access point keeps its object (it may
        // be `wifiConnectTarget`).
        let a = l.aps[0].1;
        let moved = ApInfo {
            frequency: 5180.0,
            bssid: "BB:a".into(),
            ..info("a", 60.0, false)
        };
        let rest = vec![info("b", 40.0, false), info("c", 90.0, false)];
        apply_list(&mut w, root, &mut l, [vec![moved], rest].concat());
        derive(&mut w, root, &mut l);
        assert_eq!(l.aps[0].1, a);
        assert_eq!(ap_str(&mut w, a, "bssid"), "BB:a");
        assert_eq!(num_of(&mut w, a, "frequency"), 5180.0);
        drop(w);
        drop(rt);
    }
}
