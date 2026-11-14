//! One session-bus and one system-bus connection for the whole shell, as Quickshell uses
//! `QDBusConnection::sessionBus()` / `systemBus()` everywhere. Subsystems keep their own worker
//! threads; only the connections are shared.
//!
//! Signals reach subsystems through `subscribe`: one router thread per connection reads every
//! incoming message once and forwards it, in bus order, to each subscriber whose rules match. The
//! subscriber queues are unbounded, so a busy worker never stalls the shared socket for the
//! others. When the connection closes, every subscription ends and the next `session()` /
//! `system()` reconnects. The router also hands the answer of a call made with [`call_then`],
//! which is written without waiting for it, to that call's closure; a call made with
//! [`call_then_within`] is given up at its limit, by one more thread per connection.

// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

use std::collections::{BTreeSet, HashMap};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator, connection::Builder};
use zbus::message::{Flags, Message, Sequence, Type as MsgType};
use zbus::names::BusName;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bus {
    Session,
    System,
}

const DBUS: &str = "org.freedesktop.DBus";

/// Takes a matching message; `false` ends the subscription (its receiver is gone).
pub type Sink = Box<dyn Fn(&Message) -> bool + Send>;

/// Takes the answer to a [`call_then`] call.
type Answer = Box<dyn FnOnce(zbus::Result<Message>) + Send>;

struct Sub {
    id: u64,
    rules: Vec<MatchRule<'static>>,
    tx: Sink,
}

static NEXT_SUB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Default)]
struct Routes {
    subs: Vec<Sub>,
    /// Bus-side AddMatch reference counts.
    matches: HashMap<String, usize>,
    /// Well-known sender names used in rules: their owner watch.
    owners: HashMap<String, Owner>,
    /// [`call_then`] calls not answered yet, by serial, each with its limit if it has one.
    pending: HashMap<NonZeroU32, (Answer, Option<Instant>)>,
    /// The limits of the calls of `pending` that have one, the nearest first.
    limits: BTreeSet<(Instant, NonZeroU32)>,
    /// The wakeup of the thread that ends those calls at their limits: started by the first
    /// call with a limit.
    limiter: Option<Arc<Condvar>>,
}

/// The `NameOwnerChanged` watch of a well-known sender name some subscription's rules use.
struct Owner {
    /// Current unique owner ("" when unowned).
    unique: String,
    /// The receive position of what `unique` was learned from: a `NameOwnerChanged` or a
    /// `GetNameOwner` reply.
    at: Option<Sequence>,
    /// Rules naming it, over every subscription: the watch's match leaves the bus with the last.
    refs: usize,
}

impl Owner {
    /// `unique` as of the message received at `at`, unless what it holds came later.
    fn learn(&mut self, at: Sequence, unique: String) {
        if self.at.is_none_or(|a| a < at) {
            self.unique = unique;
            self.at = Some(at);
        }
    }
}

#[derive(Clone)]
struct Shared {
    addr: Option<String>,
    conn: Connection,
    routes: Arc<Mutex<Routes>>,
    closed: Arc<AtomicBool>,
    routed: Arc<Routed>,
}

/// How far the router has got: the receive position of the last message every matching sink
/// has been handed. `None` once the connection has closed.
struct Routed {
    last: Mutex<Option<Sequence>>,
    cv: Condvar,
}

static SESSION: Mutex<Option<Shared>> = Mutex::new(None);
static SYSTEM: Mutex<Option<Shared>> = Mutex::new(None);
static OPENED: AtomicUsize = AtomicUsize::new(0);

fn slot(bus: Bus) -> &'static Mutex<Option<Shared>> {
    match bus {
        Bus::Session => &SESSION,
        Bus::System => &SYSTEM,
    }
}

fn address(bus: Bus) -> Option<String> {
    std::env::var(match bus {
        Bus::Session => "DBUS_SESSION_BUS_ADDRESS",
        Bus::System => "DBUS_SYSTEM_BUS_ADDRESS",
    })
    .ok()
}

/// A connection builder for the D-Bus address `addr`.
pub fn builder(addr: &str) -> zbus::Result<Builder<'static>> {
    builder_for(addr.parse()?)
}

/// Whether async-io's own thread runs.
static DRIVEN: AtomicBool = AtomicBool::new(false);

/// async-io starts a thread of its own at its first use in the process, and panics when the
/// system refuses it (the limit on threads is reached). Inside `Builder::build` that panic
/// comes in `block_on`, whose guard asks for the thread again while it unwinds, and a second
/// panic there aborts the process. So the first use is made here, before any connection is
/// built: a refusal is the error of the connection being opened, as an unreachable bus is,
/// and the next one opened asks again.
fn driven() -> std::io::Result<()> {
    if DRIVEN.load(Ordering::Acquire) {
        return Ok(());
    }
    // Asked of the system first, so that a refusal is an error and no panic is reported.
    std::thread::Builder::new()
        .name("ri-dbus-threads".into())
        .spawn(|| ())?;
    first_use()?;
    DRIVEN.store(true, Ordering::Release);
    Ok(())
}

/// Uses async-io outside `block_on`, where its panic for a refused thread unwinds to here: the
/// limit can be reached between the thread [`driven`] was given and the one async-io asks for.
fn first_use() -> std::io::Result<()> {
    let (a, _b) = std::os::unix::net::UnixStream::pair()?;
    std::panic::catch_unwind(|| async_io::Async::new(a).map(drop)).unwrap_or_else(|p| {
        let why = p
            .downcast_ref::<String>()
            .map_or("async-io panicked", |s| s);
        Err(std::io::Error::other(why.to_owned()))
    })
}

/// A unix-socket address is connected here, on the caller's thread: zbus would connect it on the
/// `blocking` crate's pool, whose thread then stays parked for the life of the process, waking
/// every 500 ms.
fn builder_for(addr: zbus::Address) -> zbus::Result<Builder<'static>> {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixStream};
    use zbus::address::transport::{Transport, UnixSocket};
    driven().map_err(|e| {
        let e = std::io::Error::new(e.kind(), format!("no thread for a connection: {e}"));
        zbus::Error::InputOutput(Arc::new(e))
    })?;
    let sa = match addr.transport() {
        Transport::Unix(u) => match u.path() {
            UnixSocket::File(p) => Some(SocketAddr::from_pathname(p)?),
            UnixSocket::Abstract(n) => Some(SocketAddr::from_abstract_name(n.as_encoded_bytes())?),
            _ => None,
        },
        _ => None,
    };
    let Some(sa) = sa else {
        return Builder::address(addr);
    };
    let s = UnixStream::connect_addr(&sa)
        .map_err(|e| zbus::Error::Connection(Arc::new(e), addr.clone()))?;
    Ok(Builder::async_io_unix_stream(s))
}

/// Bus connections opened by this process so far (reconnects included).
pub fn opened() -> usize {
    OPENED.load(Ordering::Relaxed)
}

/// The connection in `bus`'s slot, if it is open and to `addr`.
fn open_to(bus: Bus, addr: &Option<String>) -> Option<Shared> {
    let g = slot(bus).lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref()
        .filter(|s| s.addr == *addr && !s.closed.load(Ordering::Acquire))
        .cloned()
}

fn shared(bus: Bus) -> Option<Shared> {
    let addr = address(bus);
    if let Some(s) = open_to(bus, &addr) {
        return Some(s);
    }
    // One thread opens a bus at a time and the others wait for it here, not on the slot: a
    // bus that never answers would hold whoever looks a connection up ([`shared_of`]), a
    // call on the other bus included.
    static OPENING: (Mutex<()>, Mutex<()>) = (Mutex::new(()), Mutex::new(()));
    let opening = match bus {
        Bus::Session => &OPENING.0,
        Bus::System => &OPENING.1,
    };
    let _opening = opening.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = open_to(bus, &addr) {
        return Some(s);
    }
    let conn = match bus {
        Bus::Session => zbus::Address::session(),
        Bus::System => zbus::Address::system(),
    }
    .and_then(builder_for)
    // An interface served by the builder starts the object server before the connection reads
    // its first message; started later by `object_server()`, its task subscribes to method
    // calls on its own time and drops the calls that come first (a Watcher's
    // `RegisterStatusNotifierItem` right after its name is claimed). The root ObjectManager
    // also lists every object the process serves.
    .and_then(|b| b.serve_at("/", zbus::fdo::ObjectManager))
    .and_then(|b| b.build())
    .map_err(|e| elog::eprintln!("dbus: cannot connect to the {bus:?} bus: {e}"))
    .ok()?;
    OPENED.fetch_add(1, Ordering::Relaxed);
    let name = match bus {
        Bus::Session => "ri-dbus-session",
        Bus::System => "ri-dbus-system",
    };
    let s = open(addr, conn, name)
        .map_err(|e| elog::eprintln!("dbus: cannot start the {bus:?} router: {e}"))
        .ok()?;
    *slot(bus).lock().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
    Some(s)
}

/// `conn` shared, with its router thread `name` started.
fn open(addr: Option<String>, conn: Connection, name: &str) -> std::io::Result<Shared> {
    let s = Shared {
        addr,
        conn: conn.clone(),
        routes: Default::default(),
        closed: Default::default(),
        routed: Arc::new(Routed {
            last: Mutex::new(Some(Sequence::default())),
            cv: Condvar::new(),
        }),
    };
    // Created before any AddMatch, so nothing a subscriber asked for can be missed.
    let it = MessageIterator::from(&conn);
    let (routes, closed, routed, rconn) = (
        s.routes.clone(),
        s.closed.clone(),
        s.routed.clone(),
        conn.clone(),
    );
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            for m in it.flatten() {
                route(&rconn, &routes, &m);
                *routed.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(m.recv_position());
                routed.cv.notify_all();
            }
            closed.store(true, Ordering::Release);
            *routed.last.lock().unwrap_or_else(|e| e.into_inner()) = None;
            routed.cv.notify_all();
            let pending = {
                let mut g = routes.lock().unwrap_or_else(|e| e.into_inner());
                g.subs.clear();
                g.limits.clear();
                // Under the lock the limiter looks at `closed` with: it ends here.
                if let Some(limiter) = &g.limiter {
                    limiter.notify_all();
                }
                std::mem::take(&mut g.pending)
            };
            // Run after the lock: what they own may end subscriptions.
            for (_, (answer, _)) in pending {
                answer(Err(closed_error()));
            }
        })?;
    Ok(s)
}

/// The shared session-bus connection.
pub fn session() -> Option<Connection> {
    shared(Bus::Session).map(|s| s.conn)
}

/// The shared system-bus connection.
pub fn system() -> Option<Connection> {
    shared(Bus::System).map(|s| s.conn)
}

pub fn connection(bus: Bus) -> Option<Connection> {
    shared(bus).map(|s| s.conn)
}

/// The shared connection if it is open now. Never dials and never waits for a thread that is
/// dialing, so the main thread can ask: `None` there means "send from a thread that may wait".
pub fn connected(bus: Bus) -> Option<Connection> {
    use std::sync::TryLockError;
    let g = match slot(bus).try_lock() {
        Ok(g) => g,
        Err(TryLockError::Poisoned(e)) => e.into_inner(),
        Err(TryLockError::WouldBlock) => return None,
    };
    let s = g.as_ref()?;
    (s.addr == address(bus) && !s.closed.load(Ordering::Acquire)).then(|| s.conn.clone())
}

fn owner_rule(name: &str) -> Option<MatchRule<'static>> {
    Some(
        MatchRule::builder()
            .msg_type(MsgType::Signal)
            .sender(DBUS)
            .ok()?
            .interface(DBUS)
            .ok()?
            .member("NameOwnerChanged")
            .ok()?
            .arg(0, name.to_string())
            .ok()?
            .build(),
    )
}

fn well_known(r: &MatchRule<'_>) -> Option<String> {
    match r.sender()? {
        BusName::WellKnown(n) if n.as_str() != DBUS => Some(n.to_string()),
        _ => None,
    }
}

fn matches(owners: &HashMap<String, Owner>, r: &MatchRule<'_>, m: &Message) -> bool {
    if !r.matches(m).unwrap_or(false) {
        return false;
    }
    let Some(name) = well_known(r) else {
        return true;
    };
    // The bus delivers well-known senders as their unique owner name.
    let h = m.header();
    let sender = h.sender().map(|s| s.as_str());
    owners
        .get(&name)
        .is_some_and(|o| !o.unique.is_empty() && Some(o.unique.as_str()) == sender)
}

fn closed_error() -> zbus::Error {
    zbus::Error::InputOutput(Arc::new(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "the bus connection closed",
    )))
}

fn route(conn: &Connection, routes: &Mutex<Routes>, m: &Message) {
    let mut g = routes.lock().unwrap_or_else(|e| e.into_inner());
    let answered = match m.message_type() {
        MsgType::MethodReturn | MsgType::Error => m.header().reply_serial(),
        _ => None,
    };
    if let Some(serial) = answered
        && let Some((answer, limit)) = g.pending.remove(&serial)
    {
        if let Some(at) = limit {
            g.limits.remove(&(at, serial));
        }
        drop(g);
        answer(match m.message_type() {
            MsgType::Error => Err(m.clone().into()),
            _ => Ok(m.clone()),
        });
        g = routes.lock().unwrap_or_else(|e| e.into_inner());
    }
    if m.message_type() == MsgType::Signal {
        let h = m.header();
        if h.sender().is_some_and(|s| s.as_str() == DBUS)
            && h.member().is_some_and(|s| s.as_str() == "NameOwnerChanged")
            && let Ok((name, _, new)) = m.body().deserialize::<(String, String, String)>()
            && let Some(o) = g.owners.get_mut(&name)
        {
            o.learn(m.recv_position(), new);
        }
    }
    let Routes { subs, owners, .. } = &mut *g;
    let mut gone: Vec<usize> = vec![];
    for (i, s) in subs.iter().enumerate() {
        if s.rules.iter().any(|r| matches(owners, r, m)) && !(s.tx)(m) {
            gone.push(i);
        }
    }
    if !gone.is_empty() {
        remove(conn, g, gone);
    }
}

/// Removes the subscriptions at `gone` (ascending) and the bus-side matches only they held.
/// Their sinks are dropped, so their receivers disconnect.
fn remove(conn: &Connection, mut g: std::sync::MutexGuard<'_, Routes>, gone: Vec<usize>) {
    let mut removed = vec![];
    for i in gone.into_iter().rev() {
        removed.push(g.subs.remove(i));
    }
    for s in &removed {
        for r in signal_rules(&s.rules) {
            let k = r.to_string();
            if let Some(n) = g.matches.get_mut(&k) {
                *n -= 1;
                if *n == 0 {
                    g.matches.remove(&k);
                    // Under the lock: a later subscriber's AddMatch for the same rule goes out
                    // after it.
                    remove_match(conn, &k);
                }
            }
        }
        for n in s.rules.iter().filter_map(well_known) {
            if let Some(o) = g.owners.get_mut(&n) {
                o.refs -= 1;
                if o.refs == 0 {
                    g.owners.remove(&n);
                    if let Some(r) = owner_rule(&n) {
                        remove_match(conn, &r.to_string());
                    }
                }
            }
        }
    }
    // The sinks are dropped after the lock: what they own may end other subscriptions.
    drop(g);
}

/// AddMatch without waiting for the reply, as `dbus_bus_add_match` with no error does: see
/// [`remove_match`]. Sent under the routes lock, it is in order with the other subscribers'
/// matches and calls.
fn add_match(conn: &Connection, rule: &str) {
    match_call(conn, "AddMatch", rule);
}

/// RemoveMatch without waiting for the reply, as `dbus_bus_remove_match` with no error does. It
/// only writes to the socket, so the router thread can send it too (it must keep reading: the
/// bus's replies reach zbus through it), and it is in order with the connection's later
/// messages, which the bus handles one after another: once a later call is answered, the match
/// is gone.
fn remove_match(conn: &Connection, rule: &str) {
    match_call(conn, "RemoveMatch", rule);
}

fn match_call(conn: &Connection, member: &str, rule: &str) {
    let m = Message::method_call("/org/freedesktop/DBus", member)
        .and_then(|b| b.destination(DBUS))
        .and_then(|b| b.interface(DBUS))
        .and_then(|b| b.with_flags(Flags::NoReplyExpected))
        .and_then(|b| b.build(&(rule,)));
    if let Err(e) = m.and_then(|m| conn.send(&m)) {
        elog::eprintln!("dbus: {member} {rule}: {e}");
    }
}

fn signal_rules<'a>(
    rules: &'a [MatchRule<'static>],
) -> impl Iterator<Item = &'a MatchRule<'static>> {
    rules
        .iter()
        .filter(|r| r.msg_type().is_none_or(|t| t == MsgType::Signal))
}

/// Messages matching any of `rules`, in bus order, with the connection they arrive on. Signal
/// rules are added to the bus; other message types (method calls to our objects) need no match.
/// The receiver disconnects when the connection closes.
pub fn subscribe(
    bus: Bus,
    rules: Vec<MatchRule<'static>>,
) -> Option<(Connection, Receiver<Message>)> {
    let (tx, rx) = channel();
    let c = subscribe_to(bus, rules, Box::new(move |m| tx.send(m.clone()).is_ok()))?;
    Some((c, rx))
}

/// [`subscribe`] into `tx`, for a subscriber that also waits on other events in one place. The
/// sink is dropped when the subscription ends.
pub fn subscribe_to(bus: Bus, rules: Vec<MatchRule<'static>>, tx: Sink) -> Option<Connection> {
    attach(shared(bus)?, rules, tx).map(|c| c.conn)
}

/// [`subscribe`] for a subscriber that can go away before the connection does: [`Cancel`] ends
/// the subscription at once instead of at the next matching message.
pub fn subscribe_cancellable(
    bus: Bus,
    rules: Vec<MatchRule<'static>>,
) -> Option<(Connection, Receiver<Message>, Cancel)> {
    let (tx, rx) = channel();
    let c = attach(
        shared(bus)?,
        rules,
        Box::new(move |m| tx.send(m.clone()).is_ok()),
    )?;
    Some((c.conn.clone(), rx, c))
}

/// [`subscribe_to`] with a [`Cancel`], for a sink whose owner can go away before the connection
/// does.
pub fn subscribe_to_cancellable(
    bus: Bus,
    rules: Vec<MatchRule<'static>>,
    tx: Sink,
) -> Option<(Connection, Cancel)> {
    let c = attach(shared(bus)?, rules, tx)?;
    Some((c.conn.clone(), c))
}

/// Ends one subscription: its sink is dropped (the receiver disconnects) and the bus-side
/// matches only it held are removed, in order with the connection's later messages. A no-op
/// once the connection has closed, which ended it. Not for a sink to cancel (sinks run under the
/// routes lock).
pub struct Cancel {
    conn: Connection,
    routes: std::sync::Weak<Mutex<Routes>>,
    id: u64,
}

impl Cancel {
    pub fn cancel(self) {
        let Some(routes) = self.routes.upgrade() else {
            return;
        };
        let g = routes.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = g.subs.iter().position(|s| s.id == self.id) {
            remove(&self.conn, g, vec![i]);
        }
    }
}

/// A subscription owned by its holder: dropping it ends it at once, as [`Cancel::cancel`] does,
/// without waiting for a matching message to find the sink gone.
pub struct Subscription {
    conn: Connection,
    cancel: Option<Cancel>,
}

impl Subscription {
    /// The connection the messages arrive on.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(c) = self.cancel.take() {
            c.cancel();
        }
    }
}

/// [`subscribe_to`] that ends when the returned guard is dropped.
pub fn subscription(bus: Bus, rules: Vec<MatchRule<'static>>, tx: Sink) -> Option<Subscription> {
    let (conn, cancel) = subscribe_to_cancellable(bus, rules, tx)?;
    Some(Subscription {
        conn,
        cancel: Some(cancel),
    })
}

/// `None` (and `tx` dropped) once `s` has closed: the router clears the routes only after
/// marking it closed, so a sub added after that would never be served nor ended.
///
/// The bus-side matches a subscription is the first to need go out under the routes lock, so a
/// later subscriber of the same rule, which sends none, sends everything else after them; each
/// subscriber then asks for the owners of the sender names its rules use itself. Those replies,
/// or a `Ping` when there are none, come after the bus has handled the matches: on return the
/// subscription is installed, whichever subscriber added its rules.
fn attach(s: Shared, rules: Vec<MatchRule<'static>>, tx: Sink) -> Option<Cancel> {
    let mut names: Vec<String> = rules.iter().filter_map(well_known).collect();
    let id = NEXT_SUB.fetch_add(1, Ordering::Relaxed);
    {
        let mut g = s.routes.lock().unwrap_or_else(|e| e.into_inner());
        if s.closed.load(Ordering::Acquire) {
            return None;
        }
        for n in &names {
            let o = g.owners.entry(n.clone()).or_insert(Owner {
                unique: String::new(),
                at: None,
                refs: 0,
            });
            o.refs += 1;
            if o.refs == 1
                && let Some(r) = owner_rule(n)
            {
                add_match(&s.conn, &r.to_string());
            }
        }
        for r in signal_rules(&rules) {
            let k = r.to_string();
            let n = g.matches.entry(k.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                add_match(&s.conn, &k);
            }
        }
        g.subs.push(Sub { id, rules, tx });
    }
    pause("attached");
    names.sort_unstable();
    names.dedup();
    for n in &names {
        seed_owner(&s, n);
    }
    if names.is_empty() {
        let _ = s.conn.call_method(
            Some(DBUS),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Peer"),
            "Ping",
            &(),
        );
    }
    Some(Cancel {
        conn: s.conn,
        routes: Arc::downgrade(&s.routes),
        id,
    })
}

/// `GetNameOwner(name)`, kept as `name`'s owner unless the router has handled a newer
/// `NameOwnerChanged` for it meanwhile (or another subscriber a newer reply): received later,
/// it says who owns the name after this reply.
fn seed_owner(s: &Shared, name: &str) {
    let reply = s.conn.call_method(
        Some(DBUS),
        "/org/freedesktop/DBus",
        Some(DBUS),
        "GetNameOwner",
        &(name,),
    );
    let (at, unique) = match reply {
        Ok(m) => match m.body().deserialize::<String>() {
            Ok(o) => (m.recv_position(), o),
            Err(_) => return,
        },
        Err(zbus::Error::MethodError(e, _, m))
            if e.as_str() == "org.freedesktop.DBus.Error.NameHasNoOwner" =>
        {
            (m.recv_position(), String::new())
        }
        Err(_) => return,
    };
    pause("owner reply");
    let mut g = s.routes.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(o) = g.owners.get_mut(name) {
        o.learn(at, unique);
    }
}

/// Where [`attach`] stops for a test that set `tests::PAUSE` on its thread: the interleaving
/// with other threads that the test needs, without a time window.
#[cfg(test)]
fn pause(at: &'static str) {
    tests::PAUSE.with(|p| {
        if let Some((_, reached, go)) = p.borrow_mut().take_if(|(w, ..)| *w == at) {
            let _ = reached.send(());
            let _ = go.recv();
        }
    });
}

#[cfg(not(test))]
fn pause(_: &'static str) {}

/// A round trip to the bus, like `wl_display_roundtrip`: returns once every message the bus sent
/// this process before answering has been through the router, so each subscriber's sink has been
/// handed the signals that preceded any reply already received (the bus keeps one sender's
/// messages in order). Replies reach their callers directly, not through the router; this orders
/// them back against the subscribers' signals. The bus itself answers the `Ping`, so this waits on
/// no other client; `false` if the connection closed.
pub fn sync(bus: Bus) -> bool {
    shared(bus).is_some_and(|s| sync_on(&s))
}

fn sync_on(s: &Shared) -> bool {
    let Ok(r) = s.conn.call_method(
        Some(DBUS),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus.Peer"),
        "Ping",
        &(),
    ) else {
        return false;
    };
    let at = r.recv_position();
    let mut g = s.routed.last.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        match *g {
            None => return false,
            Some(last) if last >= at => return true,
            Some(_) => g = s.routed.cv.wait(g).unwrap_or_else(|e| e.into_inner()),
        }
    }
}

/// `call_method` that gives up after `timeout`, for callers that need a shorter limit than the
/// shared connection's (a per-connection `method_timeout` would apply to every subsystem).
#[allow(clippy::too_many_arguments)]
pub fn call_timeout<'d, 'p, 'i, 'm, D, P, I, M, B>(
    conn: &Connection,
    timeout: std::time::Duration,
    destination: Option<D>,
    path: P,
    interface: Option<I>,
    method: M,
    body: &B,
) -> zbus::Result<Message>
where
    D: TryInto<BusName<'d>>,
    P: TryInto<zbus::zvariant::ObjectPath<'p>>,
    I: TryInto<zbus::names::InterfaceName<'i>>,
    M: TryInto<zbus::names::MemberName<'m>>,
    D::Error: Into<zbus::Error>,
    P::Error: Into<zbus::Error>,
    I::Error: Into<zbus::Error>,
    M::Error: Into<zbus::Error>,
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    let call = conn
        .inner()
        .call_method(destination, path, interface, method, body);
    async_io::block_on(within(call, timeout))
}

/// `call` or, once it has run for `timeout`, a `TimedOut` error.
async fn within(
    call: impl Future<Output = zbus::Result<Message>>,
    timeout: std::time::Duration,
) -> zbus::Result<Message> {
    futures_lite::future::or(call, async move {
        async_io::Timer::after(timeout).await;
        Err(timed_out())
    })
    .await
}

/// The error of a call given up at its limit.
fn timed_out() -> zbus::Error {
    zbus::Error::InputOutput(Arc::new(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "Method call timed out",
    )))
}

/// [`call_timeout`] for a caller whose need for the answer can end first (a menu closed while
/// its app holds the call): `None` as soon as `cancel` completes. Either way the call is
/// forgotten: an answer that comes later is dropped.
#[allow(clippy::too_many_arguments)]
pub fn call_until<'d, 'p, 'i, 'm, D, P, I, M, B>(
    conn: &Connection,
    timeout: std::time::Duration,
    cancel: impl Future<Output = ()>,
    destination: Option<D>,
    path: P,
    interface: Option<I>,
    method: M,
    body: &B,
) -> Option<zbus::Result<Message>>
where
    D: TryInto<BusName<'d>>,
    P: TryInto<zbus::zvariant::ObjectPath<'p>>,
    I: TryInto<zbus::names::InterfaceName<'i>>,
    M: TryInto<zbus::names::MemberName<'m>>,
    D::Error: Into<zbus::Error>,
    P::Error: Into<zbus::Error>,
    I::Error: Into<zbus::Error>,
    M::Error: Into<zbus::Error>,
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    let call = conn
        .inner()
        .call_method(destination, path, interface, method, body);
    // `cancel` is looked at first: a call nobody wants any more is not even written.
    async_io::block_on(futures_lite::future::or(
        async {
            cancel.await;
            None
        },
        async { Some(within(call, timeout).await) },
    ))
}

/// The shared connection `conn` is, with its router, while it is open.
fn shared_of(conn: &Connection) -> Option<Shared> {
    [Bus::Session, Bus::System].into_iter().find_map(|bus| {
        let g = slot(bus).lock().unwrap_or_else(|e| e.into_inner());
        g.as_ref()
            .filter(|s| {
                s.conn.unique_name() == conn.unique_name()
                    && s.conn.server_guid() == conn.server_guid()
                    && !s.closed.load(Ordering::Acquire)
            })
            .cloned()
    })
}

/// A method call on the shared connection `conn` that is only written here, as Qt's
/// `asyncCall` is: nothing waits for the peer. `answer` gets the reply, or the error the peer or
/// the bus answered (the bus answers `NoReply` for a peer that left without replying), or an
/// I/O error if the connection closes first. It runs on the connection's router thread, which
/// reads for every subscriber: it must not wait on the bus.
///
/// A peer that stays on the bus and never answers leaves `answer` waiting for as long as the
/// connection is open: [`call_then_within`] is the call with an end.
///
/// `Err`: the call was not sent, and `answer` is dropped unrun.
pub fn call_then<B>(
    conn: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
    answer: impl FnOnce(zbus::Result<Message>) + Send + 'static,
) -> zbus::Result<()>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    let to = (destination, path, interface, method);
    write_call(conn, None, to, body, Box::new(answer))
}

/// [`call_then`] that gives the peer `limit` to answer, as libdbus and GDBus give every call
/// one: at the limit the call is forgotten and `answer` gets a `TimedOut` I/O error, on the
/// connection's limiter thread, where it must not wait on the bus either. An answer that comes
/// after that is dropped.
///
/// If the system refuses that thread, the call is written all the same; its limit is kept
/// from the moment a later call with a limit gets the thread ([`limiter_of`]).
#[allow(clippy::too_many_arguments)]
pub fn call_then_within<B>(
    conn: &Connection,
    limit: Duration,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
    answer: impl FnOnce(zbus::Result<Message>) + Send + 'static,
) -> zbus::Result<()>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    let to = (destination, path, interface, method);
    write_call(conn, Some(limit), to, body, Box::new(answer))
}

/// Writes the call `to` (destination, path, interface, method) for `answer`, which `limit`
/// ends if no answer does.
fn write_call<B>(
    conn: &Connection,
    limit: Option<Duration>,
    to: (&str, &str, &str, &str),
    body: &B,
    answer: Answer,
) -> zbus::Result<()>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    let (destination, path, interface, method) = to;
    let s = shared_of(conn).ok_or_else(closed_error)?;
    let mut b = Message::method_call(path, method)?
        .destination(destination)?
        .interface(interface)?;
    if let Some(me) = conn.unique_name() {
        b = b.sender(me)?;
    }
    let m = b.build(body)?;
    let serial = m.primary_header().serial_num();
    {
        let mut g = s.routes.lock().unwrap_or_else(|e| e.into_inner());
        // The router marks the connection closed before it fails what is pending: looked at
        // under its lock, nothing is added after that.
        if s.closed.load(Ordering::Acquire) {
            return Err(closed_error());
        }
        // A limit past the end of the clock is no limit.
        let at = limit.and_then(|l| Instant::now().checked_add(l));
        if let Some(at) = at {
            let limiter = limiter_of(&s, &mut g);
            g.limits.insert((at, serial));
            // The limiter sleeps until the nearest limit it knew of: this one is nearer.
            if let Some(limiter) = limiter
                && g.limits.first() == Some(&(at, serial))
            {
                limiter.notify_all();
            }
        }
        // Before the call is written, so its answer cannot come first.
        g.pending.insert(serial, (answer, at));
    }
    if let Err(e) = conn.send(&m) {
        let mut g = s.routes.lock().unwrap_or_else(|e| e.into_inner());
        // Gone already: the connection closed, and the router has told `answer` so.
        let Some((_, at)) = g.pending.remove(&serial) else {
            return Ok(());
        };
        if let Some(at) = at {
            g.limits.remove(&(at, serial));
        }
        return Err(e);
    }
    Ok(())
}

/// The wakeup of the thread that ends the calls of `s` at their limits, started here for the
/// first of them. `None`: the system gave no thread (the limit on threads is reached), which
/// is logged. The call is written all the same and keeps its limit: the next call with a
/// limit asks for the thread again, and the thread ends every call whose limit has come.
fn limiter_of(s: &Shared, g: &mut Routes) -> Option<Arc<Condvar>> {
    if let Some(limiter) = &g.limiter {
        return Some(limiter.clone());
    }
    let limiter = Arc::new(Condvar::new());
    let (routes, closed, wake) = (s.routes.clone(), s.closed.clone(), limiter.clone());
    let started = std::thread::Builder::new()
        .name("ri-dbus-limits".into())
        .spawn(move || end_at_limits(&routes, &closed, &wake));
    if let Err(e) = started {
        elog::eprintln!("dbus: no thread to end calls at their limits yet: {e}");
        return None;
    }
    g.limiter = Some(limiter.clone());
    Some(limiter)
}

/// The limiter thread of a connection: sleeps until the nearest limit of its unanswered
/// calls, ends the calls whose limit has come, and ends itself when the connection closes.
/// An answer takes its call's limit away; a nearer limit and the close wake `wake`.
fn end_at_limits(routes: &Mutex<Routes>, closed: &AtomicBool, wake: &Condvar) {
    let mut g = routes.lock().unwrap_or_else(|e| e.into_inner());
    while !closed.load(Ordering::Acquire) {
        let now = Instant::now();
        let due = take_due(&mut g, now);
        if !due.is_empty() {
            // Run after the lock: they make calls and end subscriptions.
            drop(g);
            for answer in due {
                answer(Err(timed_out()));
            }
            g = routes.lock().unwrap_or_else(|e| e.into_inner());
            continue;
        }
        g = match g.limits.first() {
            Some(&(at, _)) => {
                wake.wait_timeout(g, at.saturating_duration_since(now))
                    .unwrap_or_else(|e| e.into_inner())
                    .0
            }
            None => wake.wait(g).unwrap_or_else(|e| e.into_inner()),
        };
    }
}

/// Takes out of `g` the unanswered calls whose limit is `now` or earlier: their closures.
fn take_due(g: &mut Routes, now: Instant) -> Vec<Answer> {
    let mut due = Vec::new();
    while let Some(&(at, serial)) = g.limits.first()
        && at <= now
    {
        g.limits.pop_first();
        if let Some((answer, _)) = g.pending.remove(&serial) {
            due.push(answer);
        }
    }
    due
}

/// What the limiter thread of `conn` does when its clock reads `now`, done here: the
/// [`call_then_within`] calls whose limit is `now` or earlier end with `TimedOut`. How many
/// did. For the test of a limit too long to wait out: it names the moment instead.
#[doc(hidden)]
pub fn limits_reached_at(conn: &Connection, now: Instant) -> usize {
    let Some(s) = shared_of(conn) else {
        return 0;
    };
    let due = take_due(&mut s.routes.lock().unwrap_or_else(|e| e.into_inner()), now);
    let ended = due.len();
    for answer in due {
        answer(Err(timed_out()));
    }
    ended
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use zbus::blocking::fdo::DBusProxy;

    /// The router ends between `shared()` handing out the connection and the subscriber
    /// attaching: the late subscription must end at once, not dangle with a live receiver.
    #[test]
    fn attach_after_close_ends_the_subscription() {
        let d = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| testenv::equiv_missing(format_args!("dbus-daemon ({e})")));
        let Ok(mut d) = d else { return };
        let mut addr = String::new();
        BufReader::new(d.stdout.take().unwrap())
            .read_line(&mut addr)
            .unwrap();
        let conn = builder(addr.trim()).unwrap().build().unwrap();
        let s = Shared {
            addr: None,
            conn,
            routes: Default::default(),
            closed: Arc::new(AtomicBool::new(true)),
            routed: Arc::new(Routed {
                last: Mutex::new(None),
                cv: Condvar::new(),
            }),
        };
        let (tx, rx) = channel::<Message>();
        let rule = MatchRule::builder()
            .msg_type(MsgType::Signal)
            .interface("org.example.I")
            .unwrap()
            .build();
        let sink: Sink = Box::new(move |m| tx.send(m.clone()).is_ok());
        assert!(attach(s.clone(), vec![rule], sink).is_none());
        assert!(s.routes.lock().unwrap().subs.is_empty());
        assert!(s.routes.lock().unwrap().matches.is_empty());
        assert!(
            rx.recv().is_err(),
            "the sink outlived the closed connection"
        );
        let _ = d.kill();
        let _ = d.wait();
    }

    /// A cancelled subscription ends at once, before any message matches it: its receiver
    /// disconnects and its bus-side match goes, while a subscription sharing the rule stays.
    #[test]
    fn cancel_ends_the_subscription_at_once() {
        let d = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| testenv::equiv_missing(format_args!("dbus-daemon ({e})")));
        let Ok(mut d) = d else { return };
        let mut addr = String::new();
        BufReader::new(d.stdout.take().unwrap())
            .read_line(&mut addr)
            .unwrap();
        let conn = builder(addr.trim()).unwrap().build().unwrap();
        let s = Shared {
            addr: None,
            conn,
            routes: Default::default(),
            closed: Default::default(),
            routed: Arc::new(Routed {
                last: Mutex::new(Some(Sequence::default())),
                cv: Condvar::new(),
            }),
        };
        let rule = || {
            MatchRule::builder()
                .msg_type(MsgType::Signal)
                .interface("org.example.I")
                .unwrap()
                .build()
        };
        let sub = || {
            let (tx, rx) = channel::<Message>();
            let sink: Sink = Box::new(move |m| tx.send(m.clone()).is_ok());
            (attach(s.clone(), vec![rule()], sink).unwrap(), rx)
        };
        let (a, arx) = sub();
        let (b, brx) = sub();
        a.cancel();
        assert!(arx.recv().is_err(), "the cancelled sink is still held");
        assert_eq!(s.routes.lock().unwrap().subs.len(), 1);
        assert_eq!(
            s.routes.lock().unwrap().matches.get(&rule().to_string()),
            Some(&1)
        );
        b.cancel();
        assert!(brx.recv().is_err(), "the cancelled sink is still held");
        assert!(s.routes.lock().unwrap().subs.is_empty());
        assert!(s.routes.lock().unwrap().matches.is_empty());
        let _ = d.kill();
        let _ = d.wait();
    }

    type Pause = (&'static str, std::sync::mpsc::Sender<()>, Receiver<()>);

    thread_local! {
        pub(super) static PAUSE: std::cell::RefCell<Option<Pause>> =
            const { std::cell::RefCell::new(None) };
    }

    struct Daemon(std::process::Child, String);

    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn daemon() -> Option<Daemon> {
        let mut d = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| testenv::equiv_missing(format_args!("dbus-daemon ({e})")))
            .ok()?;
        let mut addr = String::new();
        BufReader::new(d.stdout.take()?).read_line(&mut addr).ok()?;
        Some(Daemon(d, addr.trim().to_string()))
    }

    fn peer(d: &Daemon) -> Connection {
        builder(&d.1).unwrap().build().unwrap()
    }

    /// async-io is refused its thread at its first use: an error each time it is asked, not
    /// a panic that reaches the caller, and the thread starts once the system gives one.
    #[test]
    fn async_io_refused_its_thread_is_an_error_until_one_is_given() {
        const TEST: &str = concat!(
            module_path!(),
            "::async_io_refused_its_thread_is_an_error_until_one_is_given"
        );
        if !testenv::in_own_process(TEST) {
            testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        std::thread::scope(|s| {
            let refused = s.spawn(|| {
                testenv::refuse_new_threads();
                for _ in 0..2 {
                    let e = first_use().unwrap_err().to_string();
                    assert!(e.starts_with("cannot spawn async-io thread"), "{e}");
                    assert!(driven().is_err());
                }
            });
            refused.join().unwrap();
        });
        driven().unwrap();
        assert!(DRIVEN.load(Ordering::Acquire));
    }

    /// A shared connection to `d` with its router running.
    fn routed(d: &Daemon) -> Shared {
        open(None, peer(d), "test-router").unwrap()
    }

    fn rule(sender: Option<&'static str>) -> MatchRule<'static> {
        let b = MatchRule::builder()
            .msg_type(MsgType::Signal)
            .interface("org.example.I")
            .unwrap();
        match sender {
            Some(n) => b.sender(n).unwrap().build(),
            None => b.build(),
        }
    }

    type Subbed = (Cancel, Receiver<Message>);

    fn sub(s: &Shared, r: MatchRule<'static>) -> Subbed {
        let (tx, rx) = channel::<Message>();
        let sink: Sink = Box::new(move |m| tx.send(m.clone()).is_ok());
        (attach(s.clone(), vec![r], sink).unwrap(), rx)
    }

    /// [`sub`] on a thread of its own that stops at [`pause`] point `at`: `reached` once it is
    /// there, and it goes on when `go` is sent.
    struct Paused {
        reached: Receiver<()>,
        go: std::sync::mpsc::Sender<()>,
        t: std::thread::JoinHandle<Subbed>,
    }

    fn paused_sub(s: &Shared, r: MatchRule<'static>, at: &'static str) -> Paused {
        let (reached_tx, reached) = channel();
        let (go, go_rx) = channel();
        let s = s.clone();
        let t = std::thread::spawn(move || {
            PAUSE.with(|p| *p.borrow_mut() = Some((at, reached_tx, go_rx)));
            sub(&s, r)
        });
        reached
            .recv()
            .expect("attach never reached its pause point");
        Paused { reached, go, t }
    }

    impl Paused {
        fn finish(self) -> Subbed {
            self.go.send(()).unwrap();
            drop(self.reached);
            self.t.join().unwrap()
        }
    }

    /// `c` emits `Ping(n)`; the bus has handled it once this returns, so it was queued to every
    /// connection it matches before the bus answers anything those send later.
    fn ping(c: &Connection, n: u32) {
        c.emit_signal(None::<&str>, "/t", "org.example.I", "Ping", &(n,))
            .unwrap();
        DBusProxy::new(c).unwrap().get_id().unwrap();
    }

    /// What `rx` holds now: call after [`sync_on`].
    fn got(rx: &Receiver<Message>) -> Vec<u32> {
        rx.try_iter()
            .map(|m| m.body().deserialize().unwrap())
            .collect()
    }

    /// The owner's reply to a subscriber's `GetNameOwner` is older than a `NameOwnerChanged`
    /// the router has already handled (the owner left meanwhile): the subscription must follow
    /// the signal, not reinstate the owner that left and take its signals as the name's.
    #[test]
    fn owner_reply_older_than_a_routed_change_is_not_seeded() {
        let Some(d) = daemon() else { return };
        let s = routed(&d);
        let (x, y) = (peer(&d), peer(&d));
        x.request_name("org.example.N").unwrap();
        // Brings the former owner's signals to the connection, as a sender-less rule does.
        let (_all_sub, all) = sub(&s, rule(None));
        let p = paused_sub(&s, rule(Some("org.example.N")), "owner reply");
        x.release_name("org.example.N").unwrap();
        // The release's NameOwnerChanged has been routed.
        assert!(sync_on(&s));
        let (_n_sub, from_n) = p.finish();

        ping(&x, 1);
        assert!(sync_on(&s));
        assert_eq!(got(&all), [1]);
        assert_eq!(
            got(&from_n),
            [0u32; 0],
            "routed from the name's former owner"
        );

        y.request_name("org.example.N").unwrap();
        ping(&y, 2);
        assert!(sync_on(&s));
        assert_eq!(got(&from_n), [2], "not routed from the name's new owner");
    }

    /// A second subscriber of a rule and sender name returns while the first is still between
    /// taking its routes entry and the bus calls: on return, the rule must be on the bus and the
    /// name's owner known, as for the first, so a signal the owner sends next reaches it.
    #[test]
    fn second_subscriber_returns_with_the_match_installed() {
        let Some(d) = daemon() else { return };
        let s = routed(&d);
        let y = peer(&d);
        y.request_name("org.example.N").unwrap();
        let first = paused_sub(&s, rule(Some("org.example.N")), "attached");
        let (_second_sub, second) = sub(&s, rule(Some("org.example.N")));
        ping(&y, 1);
        assert!(sync_on(&s));
        assert_eq!(
            got(&second),
            [1],
            "lost while the first subscriber was installing"
        );
        let (_first_sub, first) = first.finish();
        ping(&y, 2);
        assert!(sync_on(&s));
        // Routed to since it took its entry.
        assert_eq!(got(&first), [1, 2]);
        assert_eq!(got(&second), [2]);
    }
}
