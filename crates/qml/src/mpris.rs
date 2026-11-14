//! Native `Rustshell.Services.Mpris`: session-bus players kept like MprisWatcher/MprisPlayer
//! (GetAll once, then PropertiesChanged/Seeked/NameOwnerChanged in order; live `position`).
//! The players are those of one bus connection: when it closes they go, and the service lists
//! the bus again on the next one (at once, or when the bus socket appears).

use crate::eval::{Callable, fn_value, new_obj, set_own};
use crate::runtime::{Runtime, World};
use crate::value::Value;
use std::collections::{BTreeSet, HashMap};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::MatchRule;
use zbus::blocking::{Connection, fdo::DBusProxy};
use zbus::message::Type as MsgType;
use zbus::zvariant::{OwnedValue, Value as ZV};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";

/// Thread-safe copy of a D-Bus value.
#[derive(Clone, Debug, PartialEq)]
pub enum V {
    S(String),
    I(i64),
    F(f64),
    B(bool),
    L(Vec<V>),
    M(Vec<(String, V)>),
    Nil,
}

pub(crate) fn conv(v: &ZV) -> V {
    match v {
        ZV::Str(s) => V::S(s.to_string()),
        ZV::ObjectPath(p) => V::S(p.to_string()),
        ZV::Bool(b) => V::B(*b),
        ZV::U8(i) => V::I(*i as i64),
        ZV::I16(i) => V::I(*i as i64),
        ZV::U16(i) => V::I(*i as i64),
        ZV::I32(i) => V::I(*i as i64),
        ZV::U32(i) => V::I(*i as i64),
        ZV::I64(i) => V::I(*i),
        ZV::U64(i) => V::I(i64::try_from(*i).unwrap_or(i64::MAX)),
        ZV::F64(f) => V::F(*f),
        ZV::Value(b) => conv(b),
        ZV::Array(a) => V::L(a.iter().map(conv).collect()),
        ZV::Dict(d) => V::M(
            d.iter()
                .map(|(k, v)| {
                    let k = match conv(k) {
                        V::S(s) => s,
                        o => format!("{o:?}"),
                    };
                    (k, conv(v))
                })
                .collect(),
        ),
        _ => V::Nil,
    }
}
fn to_value(v: &V) -> Value {
    match v {
        V::S(s) => Value::str(s),
        V::I(i) => Value::Num(*i as f64),
        V::F(f) => Value::Num(*f),
        V::B(b) => Value::Bool(*b),
        V::L(l) => Value::array(l.iter().map(to_value).collect()),
        V::M(m) => {
            let o = new_obj();
            for (k, v) in m {
                set_own(&o, k, to_value(v));
            }
            Value::Obj(o)
        }
        V::Nil => Value::Undefined,
    }
}

fn text(v: Option<&V>) -> String {
    match v {
        Some(V::S(s)) => s.clone(),
        Some(V::L(l)) => l
            .iter()
            .filter_map(|x| match x {
                V::S(s) => Some(s.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

fn number(v: Option<&V>) -> Option<f64> {
    match v? {
        V::I(i) => Some(*i as f64),
        V::F(f) => Some(*f),
        _ => None,
    }
}

fn flag(v: Option<&V>) -> bool {
    matches!(v, Some(V::B(true)))
}

const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
const APP: &str = "org.mpris.MediaPlayer2";

/// Properties QML may assign; routed through MprisPlayer's setter semantics.
pub const SETTABLE: &[&str] = &[
    "position",
    "volume",
    "loopState",
    "shuffle",
    "rate",
    "fullscreen",
    "playbackState",
    "isPlaying",
];

/// MprisPlayer position bookkeeping (bpPosition, lastPositionTimestamp, pausedTime). Its times
/// are the runtime's wall clock in ms ([`crate::builtins::now_ms`], what `Date.now()` reads):
/// a player plays in real time, whatever the loop's animation clock does.
pub struct PosState {
    us: i64,
    at: f64,
    paused_at: f64,
    rate: f64,
    state: i32,
    supported: bool,
}

impl PosState {
    fn ms(&self) -> i64 {
        if !self.supported || self.state == 0 {
            return 0;
        }
        let t = if self.state == 2 {
            self.paused_at
        } else {
            crate::builtins::now_ms()
        };
        // A wall clock set back does not move the position back.
        let off = (t - self.at).max(0.0) as i64;
        self.us / 1000 + off * (self.rate * 1000.0) as i64 / 1000
    }

    /// `MprisPlayer::position()`.
    pub fn live(&self) -> f64 {
        self.ms() as f64 / 1000.0
    }

    /// A new PlaybackStatus or Rate: the position reached so far becomes the new base, so the
    /// extrapolation neither jumps back to the last sync on pause nor over the paused time on
    /// play. From Stopped the base stays the last known position until the player reports one.
    fn retime(&mut self, state: i32, rate: f64) {
        if state == self.state && rate == self.rate {
            return;
        }
        if self.supported && self.state != 0 {
            let now = crate::builtins::now_ms();
            self.us = self.ms() * 1000;
            self.at = now;
            self.paused_at = now;
        }
        self.state = state;
        self.rate = rate;
    }
}

enum Ev {
    /// The service is subscribed on this connection; its players follow.
    Connected(Connection),
    /// That connection closed: its players went with it.
    Closed,
    Ready(String, HashMap<String, V>),
    Props(String, HashMap<String, V>),
    Seeked(String, i64),
    Pos(String, i64),
    Gone(String),
}

enum Req {
    Call(&'static str, &'static str, Arg),
    Set(&'static str, ZV<'static>),
}

enum Arg {
    None,
    I64(i64),
    Path(String, i64),
}

struct Player {
    item: usize,
    props: HashMap<String, V>,
    trackid: String,
    url: String,
    title: String,
    unique: f64,
}

/// Where a connection's events go, each with the number of that connection's session: what a
/// closed connection's calls still report is told from the events of the next one.
#[derive(Clone)]
struct Out {
    tx: crate::svc::Sender<(u64, Ev)>,
    session: u64,
}

impl Out {
    /// Queue a bus event and wake the shell loop so it is handled now, not at the next timer.
    fn post(&self, ev: Ev) -> bool {
        self.tx.send((self.session, ev)).is_ok()
    }
}

pub struct Mpris {
    rx: Receiver<(u64, Ev)>,
    tx: crate::svc::Sender<(u64, Ev)>,
    /// Set by the service thread once subscribed (`Ev::Connected`, before any player), unset
    /// when that connection closes.
    conn: Option<Connection>,
    /// The session `conn` belongs to.
    session: u64,
    /// Unique connection name -> the MPRIS names it owns (one connection may own several).
    owners: Arc<Mutex<HashMap<String, BTreeSet<String>>>>,
    players: HashMap<String, Player>,
    /// MprisWatcher::readyPlayers order.
    order: Vec<String>,
    gens: Arc<Mutex<Gens>>,
    /// Ends the service thread's subscription, and so the thread, with this instance.
    _stop: crate::runtime::StopOnDrop,
}

/// Per player and property, a stamp renewed by every signal carrying or invalidating it: a Get
/// reply is applied only if no newer signal for that property came while it was in flight.
/// Stamps come from one counter, so a name that left and came back never matches a stamp taken
/// for its previous owner.
#[derive(Default)]
struct Gens {
    next: u64,
    map: HashMap<String, HashMap<&'static str, u64>>,
}

impl Gens {
    fn bump(&mut self, bus: &str, k: &'static str) {
        self.next += 1;
        let n = self.next;
        self.map.entry(bus.to_string()).or_default().insert(k, n);
    }

    fn current(&mut self, bus: &str, k: &'static str) -> u64 {
        let next = &mut self.next;
        *self
            .map
            .entry(bus.to_string())
            .or_default()
            .entry(k)
            .or_insert_with(|| {
                *next += 1;
                *next
            })
    }

    fn gone(&mut self, bus: &str) {
        self.map.remove(bus);
    }
}

/// The properties shown, by interface: the ones `request` reads when a player lists them as
/// invalidated instead of sending their values, as Quickshell reads every bound property a
/// PropertiesChanged invalidates (`DBusPropertyGroup::onPropertiesChanged`).
const APP_PROPS: [&str; 6] = [
    "Identity",
    "DesktopEntry",
    "CanQuit",
    "CanRaise",
    "CanSetFullscreen",
    "Fullscreen",
];
const PLAYER_PROPS: [&str; 15] = [
    "CanControl",
    "CanPlay",
    "CanPause",
    "CanSeek",
    "CanGoNext",
    "CanGoPrevious",
    "Position",
    "Volume",
    "Metadata",
    "PlaybackStatus",
    "LoopStatus",
    "Rate",
    "MinimumRate",
    "MaximumRate",
    "Shuffle",
];

/// The interface of a property shown.
fn interface_of(k: &str) -> &'static str {
    if APP_PROPS.contains(&k) { APP } else { PLAYER }
}

async fn get_all(conn: &zbus::Connection, bus: &str, itf: &str) -> Option<HashMap<String, V>> {
    let r = conn
        .call_method(
            Some(bus),
            PATH,
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(itf,),
        )
        .await;
    let m = r
        .and_then(|r| r.body().deserialize::<HashMap<String, OwnedValue>>())
        .ok()?;
    Some(m.into_iter().map(|(k, v)| (k, conv(&v))).collect())
}

async fn get_prop(conn: &zbus::Connection, bus: &str, k: &str) -> Option<V> {
    let r = conn
        .call_method(
            Some(bus),
            PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(interface_of(k), k),
        )
        .await;
    let v = r.and_then(|r| r.body().deserialize::<OwnedValue>()).ok()?;
    Some(conv(&v))
}

/// Bus calls run as tasks on the connection's own executor thread (async-io timers for the
/// delays), not on a thread spawned and put to sleep per call.
fn task(conn: &Connection, name: &str, f: impl std::future::Future<Output = ()> + Send + 'static) {
    conn.inner().executor().spawn(f, name).detach();
}

/// Where a name's initial GetAll stands; no entry once its player is ready.
#[derive(Clone, Copy)]
enum Fetch {
    Running,
    /// A PropertiesChanged arrived meanwhile: the reply may predate it, fetch again.
    Stale,
    /// No player object yet: the next PropertiesChanged from the name fetches again.
    Failed,
}

/// MprisPlayer readiness: one GetAll of both interfaces when a name appears, then `ready`.
/// A name taken before its objects are exported has nothing to get yet; it is fetched again
/// when it signals PropertiesChanged on an MPRIS interface, the first sign its objects exist.
#[derive(Clone)]
struct Ready {
    conn: Connection,
    out: Out,
    fetches: Arc<Mutex<HashMap<String, Fetch>>>,
}

impl Ready {
    fn appeared(&self, bus: String) {
        self.fetches
            .lock()
            .unwrap()
            .insert(bus.clone(), Fetch::Running);
        self.fetch(bus);
    }

    fn gone(&self, bus: &str) {
        self.fetches.lock().unwrap().remove(bus);
    }

    /// A PropertiesChanged from `bus`: whether it applies to a ready player (otherwise its
    /// values come with the GetAll it triggers).
    fn signalled(&self, bus: &str) -> bool {
        let mut f = self.fetches.lock().unwrap();
        match f.get(bus).copied() {
            None => true,
            Some(Fetch::Failed) => {
                f.insert(bus.to_string(), Fetch::Running);
                drop(f);
                self.fetch(bus.to_string());
                false
            }
            Some(_) => {
                f.insert(bus.to_string(), Fetch::Stale);
                false
            }
        }
    }

    fn fetch(&self, bus: String) {
        let (this, c) = (self.clone(), self.conn.inner().clone());
        task(&self.conn, "mpris ready", async move {
            let props = match get_all(&c, &bus, PLAYER).await {
                Some(mut p) => {
                    p.extend(get_all(&c, &bus, APP).await.unwrap_or_default());
                    Some(p)
                }
                None => None,
            };
            let again = {
                let mut f = this.fetches.lock().unwrap();
                match (f.get(&bus).copied(), &props) {
                    (None, _) => return,
                    (Some(Fetch::Stale), _) => {
                        f.insert(bus.clone(), Fetch::Running);
                        true
                    }
                    (_, None) => {
                        f.insert(bus.clone(), Fetch::Failed);
                        false
                    }
                    (_, Some(_)) => {
                        f.remove(&bus);
                        false
                    }
                }
            };
            match props {
                _ if again => this.fetch(bus),
                Some(p) => {
                    this.out.post(Ev::Ready(bus, p));
                }
                None => elog::eprintln!(
                    "mpris: no player object at {bus} yet, waiting for it to signal"
                ),
            }
        });
    }
}

/// One property Get, sent `after` a delay if any. A reply overtaken by a newer signal for that
/// property is stale and the property is read again, as `Fetch::Stale` does for GetAll; nothing
/// is read for a name gone.
fn request(
    conn: &Connection,
    out: &Out,
    gens: &Arc<Mutex<Gens>>,
    bus: &str,
    k: &'static str,
    after: Option<Duration>,
) {
    let (c, out, gens, bus) = (
        conn.inner().clone(),
        out.clone(),
        gens.clone(),
        bus.to_string(),
    );
    task(conn, "mpris get", async move {
        if let Some(d) = after {
            async_io::Timer::after(d).await;
            if !gens.lock().unwrap().map.contains_key(&bus) {
                return;
            }
        }
        let mut stamp = gens.lock().unwrap().current(&bus, k);
        loop {
            let Some(v) = get_prop(&c, &bus, k).await else {
                return;
            };
            let mut g = gens.lock().unwrap();
            match g.map.get(&bus).and_then(|m| m.get(k)) {
                None if !g.map.contains_key(&bus) => return,
                Some(&now) if now == stamp => {}
                _ => {
                    stamp = g.current(&bus, k);
                    continue;
                }
            }
            // Posted under the lock: a signal stamped after this check is queued after it.
            out.post(match (k, &v) {
                ("Position", V::I(us)) => Ev::Pos(bus, *us),
                _ => Ev::Props(bus, HashMap::from([(k.to_string(), v)])),
            });
            return;
        }
    });
}

fn rules() -> zbus::Result<Vec<MatchRule<'static>>> {
    let sig = || MatchRule::builder().msg_type(MsgType::Signal);
    Ok(vec![
        sig()
            .interface("org.freedesktop.DBus.Properties")?
            .member("PropertiesChanged")?
            .path(PATH)?
            .build(),
        sig()
            .interface(PLAYER)?
            .member("Seeked")?
            .path(PATH)?
            .build(),
        // arg0namespace: only names in the MPRIS namespace, not every client of the bus.
        sig()
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg0ns(APP)?
            .build(),
    ])
}

/// The subscription, the initial ListNames/GetNameOwner and the signal loop all wait on the bus
/// daemon: they run on the service thread, the main loop only drains `Ev`s. The thread serves
/// one connection after another ([`crate::sysbus::spawn_on`]): with no bus it waits for the bus
/// socket, and after a connection closed it subscribes on the next one.
fn start(h: &crate::svc::Handle) -> Option<Mpris> {
    let rules = rules().ok()?;
    let (tx, rx) = h.channel();
    let owners: Arc<Mutex<HashMap<String, BTreeSet<String>>>> = Default::default();
    let gens: Arc<Mutex<Gens>> = Default::default();
    let stop = crate::runtime::StopOnDrop::default();
    let spawned = {
        let (tx, owners, gens) = (tx.clone(), owners.clone(), gens.clone());
        let mut number = 0;
        crate::sysbus::spawn_on(
            crate::dbus::Bus::Session,
            std::thread::Builder::new().name("mpris".into()),
            rules,
            stop.0.clone(),
            move |conn, rx| {
                number += 1;
                let out = Out {
                    tx: tx.clone(),
                    session: number,
                };
                session(conn, rx, &out, &owners, &gens)
            },
        )
    };
    if let Err(e) = spawned {
        elog::eprintln!("mpris: cannot start the service thread: {e}");
        return None;
    }
    Some(Mpris {
        rx,
        tx,
        conn: None,
        session: 0,
        owners,
        players: HashMap::new(),
        order: vec![],
        gens,
        _stop: stop,
    })
}

/// The service on one connection: lists the players there, then follows `signals` until the
/// connection closes (true: its players are reported gone) or the instance is gone (false).
fn session(
    conn: &Connection,
    signals: &Receiver<zbus::Message>,
    out: &Out,
    owners: &Arc<Mutex<HashMap<String, BTreeSet<String>>>>,
    gens: &Arc<Mutex<Gens>>,
) -> bool {
    // What the previous connection left: its names are not this bus's.
    owners.lock().unwrap().clear();
    gens.lock().unwrap().map.clear();
    if !out.post(Ev::Connected(conn.clone())) {
        return false;
    }
    let ready = Ready {
        conn: conn.clone(),
        out: out.clone(),
        fetches: Default::default(),
    };
    list_players(&ready, owners);
    // Signals queued since AddMatch are read now; a name listed above and also announced
    // meanwhile is fetched once (`owner_changed`).
    for m in signals.iter() {
        let h = m.header();
        let member = h.member().map(|n| n.to_string()).unwrap_or_default();
        let sender = h.sender().map(|s| s.to_string()).unwrap_or_default();
        let bus_of = |s: &str| owners.lock().unwrap().get(s).cloned().unwrap_or_default();
        let evs: Vec<Ev> = match member.as_str() {
            "NameOwnerChanged" => {
                let Ok((name, old, new)) = m.body().deserialize::<(String, String, String)>()
                else {
                    continue;
                };
                if !name.starts_with(PREFIX) {
                    continue;
                }
                let fetch = owner_changed(&mut owners.lock().unwrap(), &name, &old, &new);
                if !old.is_empty() {
                    ready.gone(&name);
                    gens.lock().unwrap().gone(&name);
                    if !out.post(Ev::Gone(name.clone())) {
                        return false;
                    }
                }
                if fetch {
                    ready.appeared(name);
                }
                continue;
            }
            "PropertiesChanged" => {
                let buses = bus_of(&sender);
                let Ok((itf, ch, inv)) = m
                    .body()
                    .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                else {
                    continue;
                };
                let shown: &[&'static str] = match itf.as_str() {
                    PLAYER => &PLAYER_PROPS,
                    APP => &APP_PROPS,
                    _ => continue,
                };
                let inv: Vec<&'static str> = shown
                    .iter()
                    .copied()
                    .filter(|x| inv.iter().any(|k| k == x))
                    .collect();
                let stamped: Vec<&'static str> = shown
                    .iter()
                    .copied()
                    .filter(|x| inv.contains(x) || ch.contains_key(*x))
                    .collect();
                {
                    let mut g = gens.lock().unwrap();
                    for bus in &buses {
                        for &k in &stamped {
                            g.bump(bus, k);
                        }
                    }
                }
                let ch: HashMap<String, V> = ch.into_iter().map(|(k, v)| (k, conv(&v))).collect();
                buses
                    .into_iter()
                    .filter(|bus| ready.signalled(bus))
                    .map(|bus| {
                        for &k in &inv {
                            request(conn, out, gens, &bus, k, None);
                        }
                        Ev::Props(bus, ch.clone())
                    })
                    .collect()
            }
            "Seeked" => {
                let Ok(us) = m.body().deserialize::<i64>() else {
                    continue;
                };
                let buses = bus_of(&sender);
                let mut g = gens.lock().unwrap();
                for bus in &buses {
                    g.bump(bus, "Position");
                }
                buses.into_iter().map(|bus| Ev::Seeked(bus, us)).collect()
            }
            _ => continue,
        };
        for ev in evs {
            if !out.post(ev) {
                return false;
            }
        }
    }
    out.post(Ev::Closed)
}

/// Applies a NameOwnerChanged to `owners`; whether `name` is to be fetched now. Not when the
/// signal only announces the owner `list_players` already found: that name is being fetched
/// or is ready, and while a second fetch runs `Ready::signalled` leaves a PropertiesChanged to
/// it, whose `Ev::Ready` is then dropped for a player that exists.
fn owner_changed(
    owners: &mut HashMap<String, BTreeSet<String>>,
    name: &str,
    old: &str,
    new: &str,
) -> bool {
    let mut listed = false;
    owners.retain(|owner, names| {
        listed |= names.remove(name) && owner == new;
        !names.is_empty()
    });
    if new.is_empty() {
        return false;
    }
    owners
        .entry(new.to_string())
        .or_default()
        .insert(name.to_string());
    !listed || !old.is_empty()
}

/// MprisWatcher's initial ListNames: every MPRIS name present before the subscription.
fn list_players(ready: &Ready, owners: &Mutex<HashMap<String, BTreeSet<String>>>) {
    let Ok(dbus) = DBusProxy::new(&ready.conn) else {
        return;
    };
    let mut names: Vec<String> = dbus
        .list_names()
        .unwrap_or_default()
        .into_iter()
        .map(|n| n.to_string())
        .filter(|n| n.starts_with(PREFIX))
        .collect();
    names.sort();
    for bus in names {
        let Ok(name) = zbus::names::BusName::try_from(bus.as_str()) else {
            continue;
        };
        let Ok(owner) = dbus.get_name_owner(name) else {
            continue;
        };
        owners
            .lock()
            .unwrap()
            .entry(owner.to_string())
            .or_default()
            .insert(bus.clone());
        ready.appeared(bus);
    }
}

/// `MprisPlayer.positionChanged()` called from QML only re-notifies the live `position`.
fn position_changed(
    it: &mut crate::eval::Interp,
    this: &Value,
    _: &[Value],
) -> crate::eval::R<Value> {
    if let (Value::Item(i), Some(w)) = (this, crate::runtime::world_of(it)) {
        w.borrow_mut().notify(*i, "position");
    }
    Ok(Value::Undefined)
}

fn track_info(props: &HashMap<String, V>) -> (String, String, String) {
    let meta = match props.get("Metadata") {
        Some(V::M(m)) => m.as_slice(),
        _ => &[],
    };
    let mget = |k: &str| meta.iter().find(|(n, _)| n == k).map(|(_, v)| v);
    let s = |k: &str| match mget(k) {
        Some(V::S(s)) => s.clone(),
        _ => String::new(),
    };
    (s("mpris:trackid"), s("xesam:url"), s("xesam:title"))
}

/// Recompute the MprisPlayer properties derived from the D-Bus properties in `changed` (all of
/// them when `None`). `metadata` is a new JS object on every write, so it is rebuilt only when
/// `Metadata` itself changed.
fn write_fields(w: &mut World, p: &Player, changed: Option<&[&str]>) {
    let i = p.item;
    let pr = &p.props;
    let hit = |deps: &[&str]| changed.is_none_or(|c| deps.iter().any(|d| c.contains(d)));
    let meta = match pr.get("Metadata") {
        Some(V::M(m)) => m.as_slice(),
        _ => &[],
    };
    let mget = |k: &str| meta.iter().find(|(n, _)| n == k).map(|(_, v)| v);
    let state = match text(pr.get("PlaybackStatus")).as_str() {
        "Playing" => 1,
        "Paused" => 2,
        _ => 0,
    };
    let rate = number(pr.get("Rate")).unwrap_or(1.0);
    let length_us = number(mget("mpris:length")).map(|v| v as i64);
    if let Some(ps) = w.mpris_pos.get_mut(&i) {
        ps.retime(state, rate);
    }
    let live = w.mpris_pos.get(&i).map_or(0.0, |p| p.live());
    let ctl = flag(pr.get("CanControl"));
    let b = |k: &str| Value::Bool(flag(pr.get(k)));
    let cb = |k: &str| Value::Bool(ctl && flag(pr.get(k)));
    let can_play = ctl && flag(pr.get("CanPlay"));
    let can_pause = ctl && flag(pr.get("CanPause"));
    let loop_state = match text(pr.get("LoopStatus")).as_str() {
        "Track" => 1,
        "Playlist" => 2,
        _ => 0,
    };
    const CTL: &str = "CanControl";
    const META: &str = "Metadata";
    let mut fields: Vec<(&str, Value)> = vec![];
    let mut put = |deps: &[&str], k: &'static str, v: &dyn Fn() -> Value| {
        if hit(deps) {
            fields.push((k, v()));
        }
    };
    put(&["Identity"], "identity", &|| {
        Value::str(&text(pr.get("Identity")))
    });
    put(&["DesktopEntry"], "desktopEntry", &|| {
        Value::str(&text(pr.get("DesktopEntry")))
    });
    put(&[CTL], "canControl", &|| Value::Bool(ctl));
    put(&[CTL, "CanPlay"], "canPlay", &|| Value::Bool(can_play));
    put(&[CTL, "CanPause"], "canPause", &|| Value::Bool(can_pause));
    put(
        &[CTL, "CanPlay", "CanPause", "PlaybackStatus"],
        "canTogglePlaying",
        &|| Value::Bool(if state == 1 { can_pause } else { can_play }),
    );
    put(&[CTL, "CanSeek"], "canSeek", &|| cb("CanSeek"));
    put(&[CTL, "CanGoNext"], "canGoNext", &|| cb("CanGoNext"));
    put(&[CTL, "CanGoPrevious"], "canGoPrevious", &|| {
        cb("CanGoPrevious")
    });
    put(&["CanQuit"], "canQuit", &|| b("CanQuit"));
    put(&["CanRaise"], "canRaise", &|| b("CanRaise"));
    put(&["CanSetFullscreen"], "canSetFullscreen", &|| {
        b("CanSetFullscreen")
    });
    put(&["Position"], "positionSupported", &|| {
        Value::Bool(pr.contains_key("Position"))
    });
    // Without `mpris:length` the length is the position at the last update.
    let length_deps: &[&str] = if length_us.is_some() { &[META] } else { &[] };
    if length_us.is_none() || hit(length_deps) {
        fields.push((
            "length",
            // Whole milliseconds, in seconds, as MprisPlayer::length().
            Value::Num(length_us.map_or(live, |us| (us / 1000) as f64 / 1000.0)),
        ));
    }
    let mut put = |deps: &[&str], k: &'static str, v: &dyn Fn() -> Value| {
        if hit(deps) {
            fields.push((k, v()));
        }
    };
    put(&[META], "lengthSupported", &|| {
        Value::Bool(length_us.is_some())
    });
    put(&["Volume"], "volume", &|| {
        Value::Num(number(pr.get("Volume")).unwrap_or(1.0))
    });
    put(&["Volume"], "volumeSupported", &|| {
        Value::Bool(pr.contains_key("Volume"))
    });
    put(&[META], "metadata", &|| to_value(&V::M(meta.to_vec())));
    put(&[META], "uniqueId", &|| Value::Num(p.unique));
    put(&[META], "trackTitle", &|| {
        Value::str(&text(mget("xesam:title")))
    });
    put(&[META], "trackArtist", &|| {
        Value::str(&text(mget("xesam:artist")))
    });
    put(&[META], "trackArtists", &|| {
        Value::str(&text(mget("xesam:artist")))
    });
    put(&[META], "trackAlbum", &|| {
        Value::str(&text(mget("xesam:album")))
    });
    put(&[META], "trackAlbumArtist", &|| {
        Value::str(&text(mget("xesam:albumArtist")))
    });
    put(&[META], "trackArtUrl", &|| {
        Value::str(&text(mget("mpris:artUrl")))
    });
    put(&["PlaybackStatus"], "playbackState", &|| {
        Value::Num(state as f64)
    });
    put(&["PlaybackStatus"], "isPlaying", &|| {
        Value::Bool(state == 1)
    });
    put(&["LoopStatus"], "loopState", &|| {
        Value::Num(loop_state as f64)
    });
    put(&["LoopStatus"], "loopSupported", &|| {
        Value::Bool(pr.contains_key("LoopStatus"))
    });
    put(&["Rate"], "rate", &|| Value::Num(rate));
    put(&["MinimumRate"], "minRate", &|| {
        Value::Num(number(pr.get("MinimumRate")).unwrap_or(1.0))
    });
    put(&["MaximumRate"], "maxRate", &|| {
        Value::Num(number(pr.get("MaximumRate")).unwrap_or(1.0))
    });
    put(&["Shuffle"], "shuffle", &|| {
        Value::Bool(flag(pr.get("Shuffle")))
    });
    put(&["Shuffle"], "shuffleSupported", &|| {
        Value::Bool(pr.contains_key("Shuffle"))
    });
    put(&["Fullscreen"], "fullscreen", &|| b("Fullscreen"));
    for (k, v) in fields {
        w.write(i, k, v);
    }
}

/// `MprisPlayer::setPosition(qlonglong)` + `onPositionUpdated()`.
fn set_position(w: &mut World, i: usize, us: i64) {
    if let Some(ps) = w.mpris_pos.get_mut(&i) {
        let now = crate::builtins::now_ms();
        ps.us = us;
        ps.at = now;
        ps.paused_at = now;
        ps.supported = true;
    }
    w.notify(i, "position");
}

impl Mpris {
    /// MPRIS commands and property writes return nothing (their effect comes back as signals):
    /// sent NO_REPLY_EXPECTED, in order on the one connection, which the bus delivers in order.
    fn send(&self, bus: &str, r: Req) {
        let Some(conn) = self.conn.as_ref() else {
            return;
        };
        let (itf, m) = match &r {
            Req::Call(itf, m, _) => (*itf, *m),
            Req::Set(..) => ("org.freedesktop.DBus.Properties", "Set"),
        };
        let msg = zbus::Message::method_call(PATH, m)
            .and_then(|b| b.destination(bus))
            .and_then(|b| b.interface(itf))
            .and_then(|b| b.with_flags(zbus::message::Flags::NoReplyExpected))
            .and_then(|b| match r {
                Req::Call(_, _, Arg::None) => b.build(&()),
                Req::Call(_, _, Arg::I64(n)) => b.build(&(n,)),
                Req::Call(_, _, Arg::Path(id, n)) => {
                    b.build(&(zbus::zvariant::ObjectPath::try_from(id.as_str())?, n))
                }
                Req::Set(k, v) => b.build(&(PLAYER, k, v)),
            });
        if let Err(e) = msg.and_then(|msg| conn.send(&msg)) {
            elog::eprintln!("mpris {bus}: {m}: {e}");
        }
    }

    fn command(&mut self, w: &mut World, k: &str, a: &[Value]) {
        let Some(Value::Item(i)) = a.first() else {
            return;
        };
        let i = *i;
        let Some(bus) = self
            .players
            .iter()
            .find(|(_, p)| p.item == i)
            .map(|(b, _)| b.clone())
        else {
            return;
        };
        let cap = |w: &mut World, k: &str| w.read(i, k).is_some_and(|v| v.truthy());
        let num = |w: &mut World, k: &str| w.read(i, k).map_or(f64::NAN, |v| v.to_number());
        let warn =
            |what: &str, why: &str| elog::eprintln!("Cannot {what} of MprisPlayer({bus}) {why}");
        let m = k;
        let state = num(w, "playbackState") as i32;
        let set_state = |s: &mut Self, w: &mut World, target: i32| {
            if target == state {
                return;
            }
            let (method, ok) = match target {
                0 => ("Stop", cap(w, "canControl")),
                1 => ("Play", cap(w, "canPlay")),
                2 => ("Pause", cap(w, "canPause")),
                _ => return,
            };
            if ok {
                s.send(&bus, Req::Call(PLAYER, method, Arg::None));
            } else {
                warn("set playbackState", "(capability is false)");
            }
        };
        match m {
            "PlayPause" => {
                let t = if cap(w, "isPlaying") { 2 } else { 1 };
                set_state(self, w, t)
            }
            "Play" => set_state(self, w, 1),
            "Pause" => set_state(self, w, 2),
            "Stop" => set_state(self, w, 0),
            "Next" | "Previous" => {
                let c = if m == "Next" {
                    "canGoNext"
                } else {
                    "canGoPrevious"
                };
                if cap(w, c) {
                    let m = if m == "Next" { "Next" } else { "Previous" };
                    self.send(&bus, Req::Call(PLAYER, m, Arg::None));
                }
            }
            "Seek" => {
                if cap(w, "canSeek") {
                    let off = a.get(1).map_or(0.0, |v| v.to_number());
                    let t = (off * 1000.0) as i64 * 1000;
                    self.send(&bus, Req::Call(PLAYER, "Seek", Arg::I64(t)));
                }
            }
            "Raise" | "Quit" => {
                let c = if m == "Raise" { "canRaise" } else { "canQuit" };
                if cap(w, c) {
                    let m = if m == "Raise" { "Raise" } else { "Quit" };
                    self.send(&bus, Req::Call(APP, m, Arg::None));
                }
            }
            "set" => {
                let name = a.get(1).map(|v| v.to_js_string()).unwrap_or_default();
                let v = a.get(2).cloned().unwrap_or_default();
                let ctl = cap(w, "canControl");
                match name.as_str() {
                    "position" => {
                        if !cap(w, "positionSupported") {
                            return warn("set position", "because position is not supported.");
                        }
                        if !cap(w, "canSeek") {
                            return warn("set position", "because canSeek is false.");
                        }
                        let target = (v.to_number() * 1000.0) as i64 * 1000;
                        let id = self.players[&bus].trackid.clone();
                        // MPRIS SetPosition: NoTrack is not a valid TrackId (the call would be
                        // dropped as stale), so the move is a relative Seek.
                        let arg = if !id.is_empty() && id != NO_TRACK {
                            Req::Call(PLAYER, "SetPosition", Arg::Path(id, target))
                        } else {
                            let cur = w.mpris_pos.get(&i).map_or(0, |p| p.ms()) * 1000;
                            Req::Call(PLAYER, "Seek", Arg::I64(target - cur))
                        };
                        self.send(&bus, arg);
                        set_position(w, i, target);
                    }
                    "volume" | "shuffle" | "loopState" | "rate" | "fullscreen" => {
                        let (sup, ok) = match name.as_str() {
                            "volume" => (cap(w, "volumeSupported"), ctl),
                            "shuffle" => (cap(w, "shuffleSupported"), ctl),
                            "loopState" => (
                                cap(w, "loopSupported"),
                                ctl && v.to_number() != num(w, "loopState"),
                            ),
                            "rate" => {
                                let r = v.to_number();
                                (
                                    true,
                                    r != num(w, "rate")
                                        && r >= num(w, "minRate")
                                        && r <= num(w, "maxRate"),
                                )
                            }
                            _ => (cap(w, "canSetFullscreen"), true),
                        };
                        if !(sup && ok) {
                            return;
                        }
                        let (key, zv) = match name.as_str() {
                            "volume" => ("Volume", ZV::from(v.to_number())),
                            "shuffle" => ("Shuffle", ZV::from(v.truthy())),
                            "rate" => ("Rate", ZV::from(v.to_number())),
                            "fullscreen" => ("Fullscreen", ZV::from(v.truthy())),
                            _ => match v.to_number() as i64 {
                                0 => ("LoopStatus", ZV::from("None")),
                                1 => ("LoopStatus", ZV::from("Track")),
                                2 => ("LoopStatus", ZV::from("Playlist")),
                                _ => return warn("set loopState", "to unknown value"),
                            },
                        };
                        // The binding updates locally first (bLoopState = ...; pLoopStatus.write()).
                        let p = self.players.get_mut(&bus).unwrap();
                        p.props.insert(key.into(), conv(&zv));
                        write_fields(w, p, Some(&[key]));
                        self.send(&bus, Req::Set(key, zv));
                    }
                    "playbackState" => set_state(self, w, v.to_number() as i32),
                    "isPlaying" if v.truthy() != cap(w, "isPlaying") => {
                        let t = if v.truthy() { 1 } else { 2 };
                        set_state(self, w, t)
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn publish(&self, w: &mut World, root: usize) {
        let vals = self
            .order
            .iter()
            .filter_map(|b| self.players.get(b))
            .map(|p| Value::Item(p.item))
            .collect();
        if let Some(Value::Item(model)) = w.read(root, "players") {
            w.write(model, "values", Value::array(vals));
        }
    }

    /// The players of a connection that closed leave the list, as each does when its name
    /// loses its owner. Returns them, for [`Mpris::event`]'s caller to delete.
    fn clear(&mut self, w: &mut World, root: usize) -> Vec<usize> {
        self.order.clear();
        let gone: Vec<usize> = self.players.drain().map(|(_, p)| p.item).collect();
        if gone.is_empty() {
            return gone;
        }
        self.publish(w, root);
        for p in &gone {
            w.mpris_pos.remove(p);
        }
        gone
    }

    /// Returns the players that left the list: MprisWatcher::onServiceUnregistered takes one
    /// out of the model (`removeObject`) and deletes it later (`deleteLater`), so the caller
    /// destroys them once the model's change has been delivered.
    fn event(&mut self, w: &mut World, root: usize, session: u64, ev: Ev) -> Vec<usize> {
        match ev {
            Ev::Connected(conn) => {
                let gone = self.clear(w, root);
                self.conn = Some(conn);
                self.session = session;
                return gone;
            }
            // What a call on an earlier connection, or on one closed since, still reported.
            _ if session != self.session || self.conn.is_none() => {}
            Ev::Closed => {
                let gone = self.clear(w, root);
                self.conn = None;
                return gone;
            }
            Ev::Ready(bus, props) => {
                let live = self
                    .owners
                    .lock()
                    .unwrap()
                    .values()
                    .any(|b| b.contains(&bus));
                if self.players.contains_key(&bus) || !live {
                    return vec![];
                }
                let i = crate::types::native_singleton(w, "MprisPlayer");
                w.declare(i, "positionChanged", "function");
                w.items[i].props.get_mut("positionChanged").unwrap().value =
                    fn_value(Callable::Native("native", position_changed));
                w.write(i, "dbusName", Value::str(&bus));
                let now = crate::builtins::now_ms();
                let us = number(props.get("Position")).map_or(0, |v| v as i64);
                w.mpris_pos.insert(
                    i,
                    PosState {
                        us,
                        at: now,
                        paused_at: now,
                        rate: 1.0,
                        state: 0,
                        supported: props.contains_key("Position"),
                    },
                );
                let (trackid, url, title) = track_info(&props);
                let unique = if trackid.is_empty() && url.is_empty() && title.is_empty() {
                    0.0
                } else {
                    1.0
                };
                let p = Player {
                    item: i,
                    props,
                    trackid,
                    url,
                    title,
                    unique,
                };
                write_fields(w, &p, None);
                // MprisWatcher exposes a player only once ready: its initial values were set
                // before any binding could observe it, so they emit no change signals.
                w.changed.retain(|((it, _), _)| *it != i);
                self.players.insert(bus.clone(), p);
                self.order.push(bus);
                self.publish(w, root);
            }
            Ev::Gone(bus) => {
                self.order.retain(|b| *b != bus);
                if let Some(p) = self.players.remove(&bus) {
                    self.publish(w, root);
                    w.mpris_pos.remove(&p.item);
                    return vec![p.item];
                }
            }
            Ev::Seeked(bus, us) => {
                if let Some(p) = self.players.get(&bus) {
                    set_position(w, p.item, us);
                }
            }
            Ev::Pos(bus, us) => {
                if let Some(p) = self.players.get(&bus)
                    && w.mpris_pos.get(&p.item).is_some_and(|ps| ps.us != us)
                {
                    set_position(w, p.item, us);
                }
            }
            Ev::Props(bus, ch) => {
                let Some(conn) = self.conn.clone() else {
                    return vec![];
                };
                let out = Out {
                    tx: self.tx.clone(),
                    session,
                };
                let Some(p) = self.players.get_mut(&bus) else {
                    return vec![];
                };
                let i = p.item;
                let meta = ch.contains_key("Metadata");
                // Quickshell's bindable D-Bus properties signal only real changes.
                let status = ch
                    .get("PlaybackStatus")
                    .is_some_and(|v| p.props.get("PlaybackStatus") != Some(v));
                let announced = ch.contains_key("PlaybackStatus");
                let pos = number(ch.get("Position")).map(|v| v as i64);
                let keys: Vec<String> = ch.keys().cloned().collect();
                p.props.extend(ch);
                let mut track_changed = false;
                if meta {
                    let (id, url, title) = track_info(&p.props);
                    track_changed |= id != p.trackid;
                    p.trackid = id;
                    if !url.is_empty() {
                        track_changed |= url != p.url;
                        p.url = url;
                    }
                    if !title.is_empty() {
                        track_changed |= title != p.title;
                        p.title = title;
                    }
                    if track_changed {
                        crate::native::emit(w, i, "trackChanged", &[], vec![]);
                        p.unique += 1.0;
                    }
                }
                // MPRIS Position emits no PropertiesChanged: between Seeked signals it is
                // extrapolated by Rate, and read when the track changes or the player announces
                // its status, changed or not (`onPlaybackStatusUpdated` runs on every store):
                // at once and 100 ms later, for players that move the position after the status.
                if (track_changed || announced) && pos.is_none() {
                    request(&conn, &out, &self.gens, &bus, "Position", None);
                }
                if announced {
                    let late = Some(Duration::from_millis(100));
                    request(&conn, &out, &self.gens, &bus, "Position", late);
                }
                let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
                write_fields(w, p, Some(&keys));
                if let Some(us) = pos {
                    set_position(w, i, us);
                } else if status {
                    w.notify(i, "position");
                }
                if track_changed {
                    crate::native::emit(w, i, "postTrackChanged", &[], vec![]);
                }
            }
        }
        vec![]
    }
}

impl Runtime {
    /// Drive the `Mpris` singleton (if instantiated); returns whether anything changed.
    pub(crate) fn poll_mpris(&mut self, calls: Vec<crate::svc::Call>) -> bool {
        let Some(&root) = self.world.borrow().singletons.get("Mpris") else {
            return false;
        };
        let mut w = self.world.borrow_mut();
        if w.mpris.is_none() {
            w.mpris = start(&w.waker.handle(crate::svc::Svc::Mpris));
            if w.mpris.is_none() {
                return false;
            }
        }
        let mut m = w.mpris.take().unwrap();
        let mut changed = !calls.is_empty();
        while let Ok((session, ev)) = m.rx.try_recv() {
            let ready = matches!(ev, Ev::Ready(..));
            let gone = m.event(&mut w, root, session, ev);
            changed = true;
            if !gone.is_empty() {
                // Whoever hears the rows leave (an Instantiator's `objectRemoved`, its
                // delegate's `Component.onDestruction`) still reads the player.
                w.mpris = Some(m);
                drop(w);
                self.settle();
                w = self.world.borrow_mut();
                for p in gone {
                    w.destroy(p);
                }
                m = w.mpris.take().unwrap();
            }
            // Quickshell's MprisWatcher announces a ready player before its next
            // PropertiesChanged: QML settles on the new `players` first (MprisController sets
            // `activeTrack` in `onActivePlayerChanged`), the rest drains on the next wakeup.
            if ready {
                w.waker.wake(crate::svc::Svc::Mpris);
                break;
            }
        }
        for crate::svc::Call { method: k, args: a } in calls {
            m.command(&mut w, &k, &a);
        }
        w.mpris = Some(m);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u64_saturates() {
        assert_eq!(conv(&ZV::U64(u64::MAX)), V::I(i64::MAX));
        assert_eq!(conv(&ZV::U64(7)), V::I(7));
    }

    #[test]
    fn owner_changes_only_for_mpris_names() {
        let r = rules().unwrap();
        let noc = r
            .iter()
            .map(|r| r.to_string())
            .find(|r| r.contains("NameOwnerChanged"));
        assert!(
            noc.unwrap()
                .contains("arg0namespace='org.mpris.MediaPlayer2'")
        );
    }

    #[test]
    fn a_listed_name_is_not_fetched_again_when_announced() {
        let (a, b) = ("org.mpris.MediaPlayer2.a", "org.mpris.MediaPlayer2.b");
        let names = |o: &HashMap<String, BTreeSet<String>>, owner: &str| {
            o.get(owner).cloned().unwrap_or_default()
        };
        // What `list_players` leaves: `a` found at :1.2 and being fetched.
        let mut o = HashMap::from([(":1.2".to_string(), BTreeSet::from([a.to_string()]))]);
        assert!(!owner_changed(&mut o, a, "", ":1.2"), "already listed");
        assert!(owner_changed(&mut o, b, "", ":1.2"), "a new name");
        assert_eq!(names(&o, ":1.2").len(), 2);
        assert!(!owner_changed(&mut o, a, ":1.2", ""), "released");
        assert_eq!(names(&o, ":1.2"), BTreeSet::from([b.to_string()]));
        assert!(owner_changed(&mut o, a, "", ":1.2"), "taken again");
        assert!(owner_changed(&mut o, a, ":1.2", ":1.3"), "another owner");
        assert_eq!(names(&o, ":1.3"), BTreeSet::from([a.to_string()]));
        // Listed at :1.5 after it had been at :1.4: the signals queued meanwhile replay that.
        let mut o = HashMap::from([(":1.5".to_string(), BTreeSet::from([a.to_string()]))]);
        assert!(owner_changed(&mut o, a, "", ":1.4"));
        assert!(names(&o, ":1.5").is_empty(), "one owner per name");
        assert!(!owner_changed(&mut o, a, ":1.4", ""));
        assert!(owner_changed(&mut o, a, "", ":1.5"));
        // Handed over before it was listed: `Ev::Gone` follows, so it is fetched again.
        let mut o = HashMap::from([(":1.5".to_string(), BTreeSet::from([a.to_string()]))]);
        assert!(owner_changed(&mut o, a, ":1.4", ":1.5"));
    }

    #[test]
    fn a_signal_makes_an_earlier_get_stale() {
        let mut g = Gens::default();
        let stamp = g.current("p", "Metadata");
        assert_eq!(
            g.current("p", "Metadata"),
            stamp,
            "no signal, reply applies"
        );
        g.bump("p", "Metadata");
        assert_ne!(g.current("p", "Metadata"), stamp);
        let before = g.current("p", "Position");
        g.gone("p");
        assert_ne!(
            g.current("p", "Position"),
            before,
            "a returning name starts fresh"
        );
    }
}
