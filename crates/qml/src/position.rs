//! QtPositioning `PositionSource` on GeoClue2 (the backend Qt's geoclue2 plugin uses), behind
//! The original's services/Weather.qml GPS mode. As Qt's `startUpdates()`, `positionChanged` fires on
//! GeoClue's `LocationUpdated` only and there is no fix deadline. The source turns invalid on
//! GeoClue's own failures: an error reply to GetClient, Set or Start (AccessDenied while location
//! services are off), the client's `Active` turning false, or GeoClue leaving the bus. Weather
//! then falls back to the configured city (its `onValidityChanged` notifies, once per source,
//! since an invalid source does not restart). `update()` is Qt's single update: the source is
//! active until one fix arrives or the request times out, then inactive again unless `start()`
//! asked for updates meanwhile.

use crate::runtime::World;
use crate::svc::{Sender, Stop, Svc};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::message::{Flags, Message};
use zbus::zvariant::{DynamicType, ObjectPath, OwnedObjectPath, OwnedValue};

const GC: &str = "org.freedesktop.GeoClue2";
const MANAGER_PATH: &str = "/org/freedesktop/GeoClue2/Manager";
const MANAGER: &str = "org.freedesktop.GeoClue2.Manager";
const CLIENT: &str = "org.freedesktop.GeoClue2.Client";
const LOCATION: &str = "org.freedesktop.GeoClue2.Location";
const PROPS: &str = "org.freedesktop.DBus.Properties";
const DBUS: &str = "org.freedesktop.DBus";
/// GClueAccuracyLevel EXACT: what Qt requests with the default positioning methods.
const ACCURACY_EXACT: u32 = 8;

/// Qt's geoclue2 plugin (`UPDATE_TIMEOUT_COLD_START`): how long a single update waits for a
/// fix when `update()` names no timeout.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(120);
/// The plugin's `minimumUpdateInterval()`, in ms: `update()` with a shorter timeout fails at
/// once.
const MINIMUM_UPDATE: f64 = 1000.0;

enum Ev {
    Fix(f64, f64),
    /// A single update's deadline passed with no fix.
    TimedOut,
    Failed,
}

/// The GeoClue client thread of one active `PositionSource`; dropping it stops the client.
pub struct Source {
    stop: Arc<Stop>,
    rx: Receiver<Ev>,
    /// `start()` asked for updates until `stop()`.
    regular: bool,
    /// `update()` asked for one fix, which has not arrived or timed out yet.
    single: bool,
    /// The deadline of each single update, for the client thread.
    requests: async_channel::Sender<Instant>,
}

impl Drop for Source {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

pub fn new_item(w: &mut World, parent: Option<usize>) -> usize {
    let id = w.new_inst("PositionSource", parent);
    for (k, ty, v) in [
        ("valid", "bool", Value::Bool(true)),
        ("active", "bool", Value::Bool(false)),
        ("updateInterval", "int", Value::Num(0.0)),
        ("position", "var", position(None)),
    ] {
        w.declare(id, k, ty);
        w.items[id].props.get_mut(k).unwrap().value = v;
    }
    id
}

/// QGeoPositionInfo as QML sees it (`position.coordinate.latitude`, `position.latitudeValid`).
fn position(fix: Option<(f64, f64)>) -> Value {
    let (lat, lon) = fix.unwrap_or((f64::NAN, f64::NAN));
    let c = crate::eval::new_obj();
    crate::eval::set_own(&c, "latitude", Value::Num(lat));
    crate::eval::set_own(&c, "longitude", Value::Num(lon));
    crate::eval::set_own(&c, "altitude", Value::Num(f64::NAN));
    crate::eval::set_own(&c, "isValid", Value::Bool(fix.is_some()));
    let p = crate::eval::new_obj();
    crate::eval::set_own(&p, "coordinate", Value::Obj(c));
    crate::eval::set_own(&p, "latitudeValid", Value::Bool(fix.is_some()));
    crate::eval::set_own(&p, "longitudeValid", Value::Bool(fix.is_some()));
    crate::eval::set_own(&p, "altitudeValid", Value::Bool(false));
    Value::Obj(p)
}

fn is(w: &World, item: usize) -> bool {
    w.items[item]
        .types
        .first()
        .is_some_and(|t| *t == "PositionSource")
}

/// The GeoClue client of `item`, started if it has none; `None` for an invalid source.
fn client(w: &mut World, item: usize) -> Option<&mut Source> {
    if !w.geo.contains_key(&item) {
        if !w.read(item, "valid").is_some_and(|v| v.truthy()) {
            return None;
        }
        let ms = w
            .read(item, "updateInterval")
            .map_or(0.0, |v| v.to_number());
        // GeoClue's TimeThreshold, in seconds: Qt's geoclue2 plugin maps updateInterval to it.
        let threshold = if ms.is_finite() && ms > 0.0 {
            (ms / 1000.0).min(f64::from(u32::MAX)) as u32
        } else {
            0
        };
        let (tx, rx) = w.waker.handle(Svc::Position).channel();
        let stop = Arc::new(Stop::default());
        let s = stop.clone();
        let (requests, deadlines) = async_channel::unbounded();
        let r = std::thread::Builder::new()
            .name("geoclue".into())
            .spawn(move || run(&tx, &s, threshold, &deadlines));
        if let Err(e) = r {
            // The source stays inactive; its next start() or update() tries again.
            elog::eprintln!("PositionSource: cannot start the GeoClue thread: {e}");
            return None;
        }
        let source = Source {
            stop,
            rx,
            regular: false,
            single: false,
            requests,
        };
        w.geo.insert(item, source);
        w.write(item, "active", Value::Bool(true));
    }
    w.geo.get_mut(&item)
}

/// How long `update(ms)` waits for its fix: `ms`, [`UPDATE_TIMEOUT`] when it names none, and
/// `None` for one under [`MINIMUM_UPDATE`], which the plugin refuses.
fn single_timeout(ms: f64) -> Option<Duration> {
    if ms.is_finite() && ms > 0.0 {
        (ms >= MINIMUM_UPDATE).then(|| Duration::from_secs_f64(ms / 1000.0))
    } else {
        Some(UPDATE_TIMEOUT)
    }
}

/// Whether [`call`] answers `name` on `item`.
pub fn has(w: &World, item: usize, name: &str) -> bool {
    matches!(name, "start" | "update" | "stop") && is(w, item)
}

/// `start()`, `update(timeout)` and `stop()`, as QDeclarativePositionSource on Qt's geoclue2
/// plugin.
pub fn call(w: &mut World, item: usize, name: &str, args: &[Value]) -> Option<Value> {
    if !has(w, item, name) {
        return None;
    }
    match name {
        "start" => {
            if let Some(s) = client(w, item) {
                s.regular = true;
            }
        }
        "update" => {
            let ms = args.first().map_or(0.0, |v| v.to_number());
            // The plugin fails such a request with UpdateTimeoutError before asking GeoClue.
            let Some(timeout) = single_timeout(ms) else {
                return Some(Value::Undefined);
            };
            // A request while one is in progress is ignored, as the plugin does.
            if let Some(s) = client(w, item)
                && !std::mem::replace(&mut s.single, true)
            {
                let _ = s.requests.try_send(Instant::now() + timeout);
            }
        }
        // `stop()` ends the regular updates; a single update in progress still gets its fix.
        _ => {
            if let Some(s) = w.geo.get_mut(&item) {
                s.regular = false;
                if !s.single {
                    w.geo.remove(&item);
                    w.write(item, "active", Value::Bool(false));
                }
            }
        }
    }
    Some(Value::Undefined)
}

/// A single update of `i` ended (with a fix or at its deadline): the source goes inactive
/// unless regular updates run; whether it did.
fn single_done(w: &mut World, i: usize) -> bool {
    let Some(s) = w.geo.get_mut(&i) else {
        return false;
    };
    if !std::mem::take(&mut s.single) || s.regular {
        return false;
    }
    w.geo.remove(&i);
    w.write(i, "active", Value::Bool(false));
    true
}

pub fn poll(w: &mut World) -> bool {
    let mut changed = false;
    let ids: Vec<usize> = w.geo.keys().copied().collect();
    for i in ids {
        if w.dead.contains(&i) {
            w.geo.remove(&i);
            continue;
        }
        while let Some(ev) = w.geo.get(&i).and_then(|s| s.rx.try_recv().ok()) {
            changed = true;
            match ev {
                Ev::Fix(lat, lon) => {
                    w.write(i, "position", position(Some((lat, lon))));
                    if single_done(w, i) {
                        break;
                    }
                }
                Ev::TimedOut => {
                    if single_done(w, i) {
                        break;
                    }
                }
                Ev::Failed => {
                    w.geo.remove(&i);
                    w.write(i, "active", Value::Bool(false));
                    w.write(i, "valid", Value::Bool(false));
                    crate::native::emit(w, i, "validityChanged", &[], vec![]);
                    break;
                }
            }
        }
    }
    changed
}

/// How a GeoClue client session ended.
#[derive(PartialEq)]
enum End {
    /// The source was stopped (or its item went away).
    Stopped,
    /// GeoClue refused or dropped the client: the source turns invalid.
    Failed,
}

fn run(tx: &Sender<Ev>, stop: &Stop, threshold: u32, deadlines: &async_channel::Receiver<Instant>) {
    // Closed by the stop: every wait below ends with it.
    let (stopped_tx, stopped) = async_channel::bounded::<()>(1);
    let _close = stop.on_stop(move || drop(stopped_tx));
    let (sig_tx, sigs) = async_channel::unbounded::<Message>();
    let sink: crate::dbus::Sink = Box::new(move |m| sig_tx.try_send(m.clone()).is_ok());
    // Subscribed before GetClient, so no failure signal between the calls is missed.
    let end = match crate::dbus::subscribe_to_cancellable(crate::dbus::Bus::System, rules(), sink) {
        Some((c, cancel)) => {
            let end = Client::new(&c, &stopped).track(tx, &sigs, threshold, deadlines);
            cancel.cancel();
            end
        }
        None => End::Failed,
    };
    if end == End::Failed && !stop.load(Ordering::Acquire) {
        let _ = tx.send(Ev::Failed);
    }
}

/// GeoClue leaving the bus, and the client's `LocationUpdated` and `PropertiesChanged` (filtered
/// by client path once GetClient answered).
fn rules() -> Vec<zbus::MatchRule<'static>> {
    let b = || zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal);
    [
        b().sender(DBUS)
            .and_then(|b| b.interface(DBUS))
            .and_then(|b| b.member("NameOwnerChanged"))
            .and_then(|b| b.arg(0, GC)),
        b().sender(GC)
            .and_then(|b| b.interface(CLIENT))
            .and_then(|b| b.member("LocationUpdated")),
        b().sender(GC)
            .and_then(|b| b.interface(PROPS))
            .and_then(|b| b.member("PropertiesChanged"))
            .and_then(|b| b.arg(0, CLIENT)),
    ]
    .into_iter()
    .filter_map(|r| r.ok().map(|b| b.build()))
    .collect()
}

/// What the client waits for once started.
enum Wake {
    Stopped,
    /// A signal, or the end of the subscription with its bus connection.
    Signal(Result<Message, async_channel::RecvError>),
    /// `update()` asked for one fix by this deadline.
    Request(Instant),
    /// That deadline passed.
    TimedOut,
}

/// Blocks until the source is stopped, a signal arrives, a single update is requested or the
/// one in progress reaches `deadline`: no fix by then is not an event, so a clock ends it.
fn next(
    stopped: &async_channel::Receiver<()>,
    sigs: &async_channel::Receiver<Message>,
    deadlines: &async_channel::Receiver<Instant>,
    deadline: Option<Instant>,
) -> Wake {
    use futures_lite::future::{or, pending};
    let request = async {
        match deadlines.recv().await {
            Ok(at) => Wake::Request(at),
            // The source is going: its stop ends the wait.
            Err(_) => pending().await,
        }
    };
    let timeout = async {
        match deadline {
            Some(at) => {
                async_io::Timer::at(at).await;
                Wake::TimedOut
            }
            None => pending().await,
        }
    };
    let stop = async {
        let _ = stopped.recv().await;
        Wake::Stopped
    };
    let signal = async { Wake::Signal(sigs.recv().await) };
    zbus::block_on(or(stop, or(signal, or(request, timeout))))
}

/// `f`, or `None` once the source is stopped.
fn until<T>(
    stopped: &async_channel::Receiver<()>,
    f: impl std::future::Future<Output = T>,
) -> Option<T> {
    zbus::block_on(futures_lite::future::or(async { Some(f.await) }, async {
        let _ = stopped.recv().await;
        None
    }))
}

struct Client<'a> {
    c: &'a Connection,
    stopped: &'a async_channel::Receiver<()>,
}

impl Client<'_> {
    fn new<'a>(c: &'a Connection, stopped: &'a async_channel::Receiver<()>) -> Client<'a> {
        Client { c, stopped }
    }

    /// A plain call to GeoClue: it answers or the bus reports the error. `None` once stopped.
    fn call<B>(
        &self,
        path: &str,
        iface: &str,
        method: &str,
        body: &B,
    ) -> Option<zbus::Result<Message>>
    where
        B: serde::ser::Serialize + DynamicType,
    {
        until(
            self.stopped,
            self.c
                .inner()
                .call_method(Some(GC), path, Some(iface), method, body),
        )
    }

    /// Sent without waiting for a reply, as the source is going away.
    fn send<B>(&self, path: &str, iface: &str, method: &str, body: &B)
    where
        B: serde::ser::Serialize + DynamicType,
    {
        let m = Message::method_call(path, method)
            .and_then(|b| b.destination(GC))
            .and_then(|b| b.interface(iface))
            .and_then(|b| b.with_flags(Flags::NoReplyExpected))
            .and_then(|b| b.build(body));
        if let Ok(m) = m {
            let _ = self.c.send(&m);
        }
    }

    /// The coordinates of the Location object at `path`; `None` once stopped.
    fn fix(&self, path: &str) -> Option<Option<(f64, f64)>> {
        if path == "/" {
            return Some(None);
        }
        let r = self.call(path, PROPS, "GetAll", &(LOCATION,))?;
        Some(r.ok().and_then(|m| {
            let props = m.body().deserialize::<HashMap<String, OwnedValue>>().ok()?;
            let n = |k: &str| {
                props
                    .get(k)
                    .and_then(|v| f64::try_from(v).ok())
                    .filter(|v| v.is_finite())
            };
            Some((n("Latitude")?, n("Longitude")?))
        }))
    }

    fn track(
        &self,
        tx: &Sender<Ev>,
        sigs: &async_channel::Receiver<Message>,
        threshold: u32,
        deadlines: &async_channel::Receiver<Instant>,
    ) -> End {
        let Some(r) = self.call(MANAGER_PATH, MANAGER, "GetClient", &()) else {
            return End::Stopped;
        };
        let Some(client) = crate::sysbus::object_path(r) else {
            return End::Failed;
        };
        let end = self.session(&client, tx, sigs, threshold, deadlines);
        // As Qt's geoclue2 plugin on stop: Stop, then DeleteClient.
        self.send(&client, CLIENT, "Stop", &());
        if let Ok(p) = ObjectPath::try_from(client.as_str()) {
            self.send(MANAGER_PATH, MANAGER, "DeleteClient", &(p,));
        }
        end
    }

    fn session(
        &self,
        client: &str,
        tx: &Sender<Ev>,
        sigs: &async_channel::Receiver<Message>,
        threshold: u32,
        deadlines: &async_channel::Receiver<Instant>,
    ) -> End {
        let set = |prop: &str, v: zbus::zvariant::Value<'_>| {
            self.call(client, PROPS, "Set", &(CLIENT, prop, v))
                .map(|r| r.is_ok())
        };
        for (prop, v) in [
            ("DesktopId", "org.rustshell".into()),
            ("RequestedAccuracyLevel", ACCURACY_EXACT.into()),
            ("TimeThreshold", threshold.into()),
        ] {
            match set(prop, v) {
                None => return End::Stopped,
                Some(false) => return End::Failed,
                Some(true) => {}
            }
        }
        match self.call(client, CLIENT, "Start", &()) {
            None => return End::Stopped,
            Some(Err(_)) => return End::Failed,
            Some(Ok(_)) => {}
        }
        // The deadline of the single update in progress.
        let mut deadline = None;
        loop {
            let m = match next(self.stopped, sigs, deadlines, deadline) {
                Wake::Stopped => return End::Stopped,
                Wake::Request(at) => {
                    deadline = Some(at);
                    continue;
                }
                Wake::TimedOut => {
                    deadline = None;
                    if tx.send(Ev::TimedOut).is_err() {
                        return End::Stopped;
                    }
                    continue;
                }
                // The subscription ended with the bus connection: GeoClue is out of reach.
                Wake::Signal(Err(_)) => return End::Failed,
                Wake::Signal(Ok(m)) => m,
            };
            let h = m.header();
            let member = h.member().map(|m| m.as_str());
            let path = h.path().map(|p| p.as_str());
            match member {
                Some("NameOwnerChanged") => {
                    // The name left the owner that holds our client (GeoClue exited or was
                    // replaced): the client is gone with it.
                    if m.body()
                        .deserialize::<(String, String, String)>()
                        .is_ok_and(|(_, old, _)| !old.is_empty())
                    {
                        return End::Failed;
                    }
                }
                Some("PropertiesChanged") if path == Some(client) => {
                    let active = m
                        .body()
                        .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                        .ok()
                        .and_then(|(_, changed, _)| {
                            changed.get("Active").and_then(|v| bool::try_from(v).ok())
                        });
                    if active == Some(false) {
                        return End::Failed;
                    }
                }
                // GeoClue also sends it on Start for a location it already has.
                Some("LocationUpdated") if path == Some(client) => {
                    let new = m.body().deserialize::<(OwnedObjectPath, OwnedObjectPath)>();
                    let Ok((_, p)) = new else {
                        continue;
                    };
                    match self.fix(p.as_str()) {
                        None => return End::Stopped,
                        Some(Some(f)) => {
                            // The fix a single update waited for.
                            deadline = None;
                            if tx.send(Ev::Fix(f.0, f.1)).is_err() {
                                return End::Stopped;
                            }
                        }
                        Some(None) => {}
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source whose GeoClue thread the system refuses stays inactive, with the refusal in
    /// the log, each time it is asked. In a process of its own: the test reads what that
    /// process wrote to its stderr.
    #[test]
    fn a_refused_thread_is_logged_and_leaves_the_source_inactive() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_thread_is_logged_and_leaves_the_source_inactive"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let mut rt = crate::runtime::Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport QtPositioning\nItem {\n\
                 property alias active: src.active\n\
                 PositionSource { id: src }\n\
                 function start() { src.start() }\n\
                 function update() { src.update() } }",
            )
            .unwrap();
        rt.settle();
        crate::testenv::refuse_new_threads();
        let text = crate::testenv::stderr_of(|| {
            for method in ["start", "update"] {
                let called = rt.call_method(r, method, vec![]);
                assert!(matches!(called, Some(Ok(_))), "{called:?}");
                rt.poll_native();
                rt.settle();
                assert!(!rt.get(r, "active").truthy(), "active after {method}()");
                assert!(rt.world.borrow().geo.is_empty());
            }
        });
        assert_eq!(
            text.matches("PositionSource: cannot start the GeoClue thread:")
                .count(),
            2,
            "stderr: {text:?}"
        );
        assert_eq!(rt.errors(), Vec::<String>::new());
    }

    /// `update(timeout)` waits the milliseconds it names, the plugin's two minutes when it
    /// names none, and is refused under the plugin's minimum.
    #[test]
    fn a_single_update_waits_the_timeout_it_names() {
        assert_eq!(single_timeout(5000.0), Some(Duration::from_secs(5)));
        assert_eq!(single_timeout(1000.0), Some(Duration::from_secs(1)));
        assert_eq!(
            single_timeout(1500.5),
            Some(Duration::from_micros(1_500_500))
        );
        assert_eq!(single_timeout(999.0), None);
        for none in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(single_timeout(none), Some(UPDATE_TIMEOUT), "{none}");
        }
    }

    /// The client's wait ends with what happens first: a signal's end, a request, the stop, or
    /// the deadline of the single update in progress when none of them does.
    #[test]
    fn the_wait_ends_with_an_event_or_at_the_single_updates_deadline() {
        let (stop_tx, stopped) = async_channel::bounded::<()>(1);
        let (sig_tx, sigs) = async_channel::unbounded::<Message>();
        let (requests, deadlines) = async_channel::unbounded::<Instant>();
        let far = Instant::now() + Duration::from_secs(3600);
        requests.try_send(far).unwrap();
        assert!(matches!(
            next(&stopped, &sigs, &deadlines, None),
            Wake::Request(at) if at == far
        ));
        // Nothing else happens: the deadline, already reached, ends the wait.
        assert!(matches!(
            next(&stopped, &sigs, &deadlines, Some(Instant::now())),
            Wake::TimedOut
        ));
        drop(sig_tx);
        assert!(matches!(
            next(&stopped, &sigs, &deadlines, Some(far)),
            Wake::Signal(Err(_))
        ));
        drop(stop_tx);
        assert!(matches!(
            next(&stopped, &sigs, &deadlines, Some(Instant::now())),
            Wake::Stopped
        ));
    }
}
