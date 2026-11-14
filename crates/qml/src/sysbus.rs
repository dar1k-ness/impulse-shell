//! Shared plumbing for system-bus services (UPower, power-profiles-daemon, BlueZ, NetworkManager):
//! a worker thread per service on the shared system connection that receives its signals in order,
//! reports snapshots and resubscribes once the bus is back. A session-bus service that follows
//! its connection the same way (MPRIS) runs on the same loop ([`spawn_on`]).

use crate::mpris::{V, conv};
use crate::svc::Stop;
use std::collections::{BTreeMap, HashMap};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::MatchRule;
use zbus::blocking::Connection;
use zbus::message::{Message, Type as MsgType};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

pub(crate) const PROPS: &str = "org.freedesktop.DBus.Properties";

pub(crate) type Props = HashMap<String, V>;
/// An object's properties by interface.
pub(crate) type Ifaces = BTreeMap<String, Props>;

/// Run `session` on the shared system-bus connection until `stop`; signals matching `rules`
/// arrive in bus order on the receiver. `session` returns false to stop for good (its consumer is
/// gone) or true when the bus connection dropped, which reconnects (see [`run`]). `stop` ends the
/// current session's subscription (its receiver disconnects, so the session returns), wakes a
/// wait for the bus socket, and the thread ends: its owner sets it when it goes. Where the
/// system refuses the thread, `session` is dropped without having run.
pub(crate) fn spawn<F>(rules: Vec<MatchRule<'static>>, stop: Arc<Stop>, session: F)
where
    F: FnMut(&Connection, &Receiver<Message>) -> bool + Send + 'static,
{
    let thread = std::thread::Builder::new();
    if let Err(e) = spawn_on(crate::dbus::Bus::System, thread, rules, stop, session) {
        elog::eprintln!("sysbus: cannot start a service thread: {e}");
    }
}

/// [`spawn`] on the shared connection of `bus`, on the thread `thread` describes.
pub(crate) fn spawn_on<F>(
    bus: crate::dbus::Bus,
    thread: std::thread::Builder,
    rules: Vec<MatchRule<'static>>,
    stop: Arc<Stop>,
    mut session: F,
) -> std::io::Result<()>
where
    F: FnMut(&Connection, &Receiver<Message>) -> bool + Send + 'static,
{
    let socket = match bus {
        crate::dbus::Bus::System => system_socket(),
        crate::dbus::Bus::Session => session_socket(),
    };
    let stopped = {
        let stop = stop.clone();
        move || stop.load(Ordering::SeqCst)
    };
    // The live session's subscription, ended by the stop.
    let current: Arc<Mutex<Option<crate::dbus::Cancel>>> = Arc::default();
    let cancel_current = {
        let current = current.clone();
        stop.on_stop_owned(move || {
            if let Some(c) = current.lock().unwrap_or_else(|e| e.into_inner()).take() {
                c.cancel();
            }
        })
    };
    // Subscribed before AddMatch, so nothing between AddMatch and the sync is lost.
    let subscribe = {
        let stopped = stopped.clone();
        move || {
            if stopped() {
                return None;
            }
            let (conn, rx, cancel) = crate::dbus::subscribe_cancellable(bus, rules.clone())?;
            *current.lock().unwrap_or_else(|e| e.into_inner()) = Some(cancel);
            // The stop's wakeup ran before the subscription was stored there.
            if stopped()
                && let Some(c) = current.lock().unwrap_or_else(|e| e.into_inner()).take()
            {
                c.cancel();
            }
            Some((conn, rx))
        }
    };
    thread.spawn(move || {
        let _cancel_current = cancel_current;
        // Set up before the first attempt so a socket created in between is not missed. Without
        // a watchable socket (no inotify, abstract or TCP address) a failed connect is final, as
        // in Qt.
        let mut watch = socket.as_deref().and_then(SocketWatch::new);
        if let Some(w) = watch.as_mut() {
            w.stop_on(&stop);
        }
        run(
            subscribe,
            || !stopped() && watch.as_mut().is_some_and(SocketWatch::wait),
            |(conn, rx)| session(conn, rx) && !stopped(),
        );
    })?;
    Ok(())
}

/// The system bus socket file: `DBUS_SYSTEM_BUS_ADDRESS`'s `unix:path=` or the default.
fn system_socket() -> Option<PathBuf> {
    system_socket_of(std::env::var("DBUS_SYSTEM_BUS_ADDRESS").ok().as_deref())
}

fn system_socket_of(addr: Option<&str>) -> Option<PathBuf> {
    match addr {
        Some(a) => unix_path(a),
        None => Some("/run/dbus/system_bus_socket".into()),
    }
}

/// The session bus socket file: `DBUS_SESSION_BUS_ADDRESS`'s `unix:path=`, or with no address
/// `$XDG_RUNTIME_DIR/bus`, where libdbus and zbus look then.
pub(crate) fn session_socket() -> Option<PathBuf> {
    session_socket_of(
        std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref(),
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
    )
}

fn session_socket_of(addr: Option<&str>, runtime_dir: Option<PathBuf>) -> Option<PathBuf> {
    match addr {
        Some(a) => unix_path(a),
        None => runtime_dir.map(|d| d.join("bus")),
    }
}

/// The first `unix:path=` of a D-Bus address list.
fn unix_path(addr: &str) -> Option<PathBuf> {
    addr.split(';')
        .find_map(|a| a.strip_prefix("unix:"))
        .and_then(|kv| kv.split(',').find_map(|kv| kv.strip_prefix("path=")))
        .map(PathBuf::from)
}

/// Watches the socket's directory for the socket being created, moved in or changing attributes,
/// the events sd-bus `watch_bind` waits on. bind(2) creates the file before listen(2), so a
/// connect on its creation can be refused; the attribute change announces the listen: systemd
/// touches a socket unit's node after listen(2) (`socket_address_listen`) and dbus-daemon
/// chmods its socket after listen(2) (`_dbus_listen_unix_socket`). A daemon that announces
/// nothing after listen(2) is reached on its next socket event.
pub(crate) struct SocketWatch {
    ino: inotify::Inotify,
    names: Vec<std::ffi::OsString>,
    /// An eventfd its stop writes: [`SocketWatch::wait`] then returns false.
    stop: Option<Arc<OwnedFd>>,
    /// The stop's wakeup writing `stop`, unregistered with the watch.
    on_stop: Option<crate::svc::OnStop<'static>>,
}

impl SocketWatch {
    pub(crate) fn new(socket: &Path) -> Option<SocketWatch> {
        let ino = inotify::Inotify::init().ok()?;
        let mask =
            inotify::WatchMask::CREATE | inotify::WatchMask::MOVED_TO | inotify::WatchMask::ATTRIB;
        ino.watches().add(socket.parent()?, mask).ok()?;
        Some(SocketWatch {
            ino,
            names: vec![socket.file_name()?.to_owned()],
            stop: None,
            on_stop: None,
        })
    }

    /// Also wake on events for `name` in the socket's directory: a socket its daemon creates
    /// only once the watched one listens announces that listen.
    pub(crate) fn also(mut self, name: std::ffi::OsString) -> SocketWatch {
        self.names.push(name);
        self
    }

    /// `stop` ends a [`SocketWatch::wait`] in progress and every later one.
    pub(crate) fn stop_on(&mut self, stop: &Arc<Stop>) {
        let Some(fd) = eventfd() else {
            return;
        };
        let fd = Arc::new(fd);
        let w = Arc::downgrade(&fd);
        self.stop = Some(fd);
        self.on_stop = Some(stop.on_stop_owned(move || {
            if let Some(w) = w.upgrade() {
                signal(&w);
            }
        }));
    }

    /// Whether the stop arrived before the inotify fd became readable; blocks until either.
    fn stopped(&self) -> bool {
        self.stop
            .as_ref()
            .is_some_and(|stop| readable_or_stopped(stop, self.ino.as_fd()))
    }

    /// Block until the socket is created, moved in or touched after the previous call (events
    /// queue meanwhile); false once stopped ([`SocketWatch::stop_on`]).
    pub(crate) fn wait(&mut self) -> bool {
        let mut buf = [0u8; 4096];
        loop {
            if self.stopped() {
                return false;
            }
            let Ok(evs) = self.ino.read_events_blocking(&mut buf) else {
                return false;
            };
            if evs
                .into_iter()
                .any(|e| e.name.is_some_and(|n| self.names.iter().any(|m| m == n)))
            {
                return true;
            }
        }
    }
}

/// A nonblocking eventfd a thread's stop writes ([`signal`]); None at the fd limit.
pub(crate) fn eventfd() -> Option<OwnedFd> {
    // SAFETY: eventfd(2) takes plain integers and touches no memory of ours.
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    // SAFETY: eventfd just returned this descriptor, owned by nothing else.
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Makes [`eventfd`] `fd` readable for good (nothing reads it).
pub(crate) fn signal(fd: &OwnedFd) {
    let one = 1u64;
    // SAFETY: `one` is the 8 readable bytes an eventfd write takes; `fd` is open for the call.
    unsafe { libc::write(fd.as_raw_fd(), (&one as *const u64).cast(), 8) };
}

/// Blocks until `stop` (an [`eventfd`]) or `fd` is readable; true if `stop` is (or poll(2)
/// failed), so the waiting thread ends.
pub(crate) fn readable_or_stopped(stop: &OwnedFd, fd: std::os::fd::BorrowedFd<'_>) -> bool {
    let mut fds = [stop.as_raw_fd(), fd.as_raw_fd()].map(|fd| libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    });
    loop {
        // SAFETY: `fds` is two writable pollfds whose descriptors the caller keeps open.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if n >= 0 {
            return fds[0].revents != 0;
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return true;
        }
    }
}

/// The connection loop: `subscribe` connects, `session` runs on the connection until it ends
/// (false: stop for good), `wait` blocks until the next socket event (false: none will come).
/// A session that ended is followed by a connect at once: under socket activation
/// (`dbus.socket` owns the path) a restarted bus daemon keeps the socket, so no event marks
/// its return, and systemd starts it on that connect. A connect that fails, refused or with no
/// socket, waits for the next socket event, never a timer.
fn run<C>(
    mut subscribe: impl FnMut() -> Option<C>,
    mut wait: impl FnMut() -> bool,
    mut session: impl FnMut(&C) -> bool,
) {
    loop {
        match subscribe() {
            Some(c) => {
                if !session(&c) {
                    return;
                }
            }
            None => {
                if !wait() {
                    return;
                }
            }
        }
    }
}

/// A socket at `path` bound but not listening yet, as a daemon's is between bind(2) and
/// listen(2); it appears there at once (renamed from a scratch name).
#[cfg(test)]
pub(crate) fn bound_unlistened(path: &Path) -> std::os::fd::OwnedFd {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    let scratch = path.with_extension("new");
    // SAFETY: socket(2) takes plain integers.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    assert!(fd >= 0);
    // SAFETY: `fd` is the socket created above, open and owned by nothing else.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: sockaddr_un is integers and a byte array, for which all zeros is valid.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (d, s) in addr.sun_path.iter_mut().zip(scratch.as_os_str().as_bytes()) {
        *d = *s as libc::c_char;
    }
    let len = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    // SAFETY: `addr` is a live sockaddr_un and `len` its size.
    let r = unsafe { libc::bind(fd.as_raw_fd(), (&raw const addr).cast(), len) };
    assert_eq!(r, 0);
    std::fs::rename(&scratch, path).unwrap();
    fd
}

fn sig() -> zbus::match_rule::Builder<'static> {
    MatchRule::builder().msg_type(MsgType::Signal)
}

/// Signals from the current owner of `service`: without the sender, any local user could forge
/// them (the system bus policy lets everyone emit signals). The router checks the unique owner.
fn from(service: &'static str) -> Option<zbus::match_rule::Builder<'static>> {
    sig().sender(service).ok()
}

/// `PropertiesChanged` from `service`'s objects under `path`.
pub(crate) fn props_rule(service: &'static str, path: &'static str) -> Option<MatchRule<'static>> {
    Some(
        from(service)?
            .interface(PROPS)
            .ok()?
            .member("PropertiesChanged")
            .ok()?
            .path_namespace(path)
            .ok()?
            .build(),
    )
}

/// Every signal of `iface` that `service` emits at `path`.
pub(crate) fn iface_rule(
    service: &'static str,
    iface: &'static str,
    path: &'static str,
) -> Option<MatchRule<'static>> {
    Some(
        from(service)?
            .interface(iface)
            .ok()?
            .path(path)
            .ok()?
            .build(),
    )
}

/// `NameOwnerChanged` for the well-known `name` (daemon start/exit).
pub(crate) fn owner_rule(name: &'static str) -> Option<MatchRule<'static>> {
    Some(
        sig()
            .sender("org.freedesktop.DBus")
            .ok()?
            .interface("org.freedesktop.DBus")
            .ok()?
            .member("NameOwnerChanged")
            .ok()?
            .arg(0, name)
            .ok()?
            .build(),
    )
}

/// How long a call to a daemon waits for its reply: the 25 s that libdbus
/// (`_DBUS_DEFAULT_TIMEOUT_VALUE`), GDBus and QtDBus give a call that names no timeout, so the
/// limit of every call Quickshell and libnm make. A daemon that keeps its bus name and never
/// answers ends the call with an error then, and the calls queued behind it go on.
pub(crate) const REPLY_TIMEOUT: Duration = Duration::from_secs(25);

static REPLY_MS: AtomicU64 = AtomicU64::new(REPLY_TIMEOUT.as_millis() as u64);

/// Sets how long a daemon's reply is waited for instead of [`REPLY_TIMEOUT`]: tests of a daemon
/// that never answers.
pub fn set_reply_timeout(t: Duration) {
    REPLY_MS.store(t.as_millis() as u64, Ordering::SeqCst);
}

fn reply_timeout() -> Duration {
    Duration::from_millis(REPLY_MS.load(Ordering::SeqCst))
}

fn timed_out() -> zbus::Error {
    zbus::Error::InputOutput(Arc::new(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "no reply",
    )))
}

/// A method call on a daemon, from a thread that may wait: its reply, or an error once
/// [`REPLY_TIMEOUT`] has passed without one.
pub(crate) fn call<B>(
    conn: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    method: &str,
    body: &B,
) -> zbus::Result<Message>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    let limit = reply_timeout();
    crate::dbus::call_timeout(conn, limit, Some(dest), path, Some(iface), method, body)
}

/// [`call`] that says when the call is on the bus: `sent` runs once the call was written (or
/// could not be), before the wait for its reply. The shell's exit waits for `sent`, not for the
/// reply, as Qt's `asyncCall` leaves a call on the bus when the process ends.
pub(crate) fn call_sent<B>(
    conn: &Connection,
    to: (&str, &str, &str, &str),
    body: &B,
    sent: impl FnOnce(),
) -> zbus::Result<Message>
where
    B: serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    let (dest, path, iface, method) = to;
    let (tx, rx) = std::sync::mpsc::channel();
    let limit = reply_timeout();
    let written =
        crate::dbus::call_then_within(conn, limit, dest, path, iface, method, body, move |r| {
            let _ = tx.send(r);
        });
    sent();
    written?;
    // The router answers every call it took, or drops it with the connection.
    rx.recv().unwrap_or_else(|_| Err(timed_out()))
}

/// [`call`] for a request to the bus daemon that zbus makes itself (`RequestName`,
/// `ReleaseName`): `f`'s result, or an error once [`REPLY_TIMEOUT`] has passed.
pub(crate) fn within<T>(f: impl Future<Output = zbus::Result<T>>) -> zbus::Result<T> {
    async_io::block_on(within_async(f))
}

/// `f`'s result, or an error once [`REPLY_TIMEOUT`] has passed.
pub(crate) async fn within_async<T>(f: impl Future<Output = zbus::Result<T>>) -> zbus::Result<T> {
    limited(reply_timeout(), f).await
}

async fn limited<T>(limit: Duration, f: impl Future<Output = zbus::Result<T>>) -> zbus::Result<T> {
    futures_lite::future::or(f, async move {
        async_io::Timer::after(limit).await;
        Err(timed_out())
    })
    .await
}

pub(crate) fn conv_map(m: HashMap<String, OwnedValue>) -> Props {
    m.into_iter().map(|(k, v)| (k, conv(&v))).collect()
}

pub(crate) fn get_all(conn: &Connection, dest: &str, path: &str, iface: &str) -> Option<Props> {
    let r = call(conn, dest, path, PROPS, "GetAll", &(iface,)).ok()?;
    r.body()
        .deserialize::<HashMap<String, OwnedValue>>()
        .ok()
        .map(conv_map)
}

pub(crate) fn object_path(r: zbus::Result<Message>) -> Option<String> {
    r.ok()?
        .body()
        .deserialize::<OwnedObjectPath>()
        .ok()
        .map(|p| p.to_string())
}

pub(crate) fn object_paths(r: zbus::Result<Message>) -> Vec<String> {
    r.ok()
        .and_then(|m| m.body().deserialize::<Vec<OwnedObjectPath>>().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.to_string())
        .collect()
}

/// `org.freedesktop.DBus.Properties.Set`, off the main thread on `lane`, the daemon's queue of
/// calls: a daemon that never answers holds only that lane, and only for [`REPLY_TIMEOUT`].
/// `failed` runs on an error reply or when none came: the daemon kept its value (as far as
/// anyone knows) and sends no PropertiesChanged, so the caller restores it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn set_prop(
    lane: &'static workpool::Lane,
    conn: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    prop: &str,
    v: zbus::zvariant::Value<'static>,
    failed: impl FnOnce() + Send + 'static,
) {
    set_prop_sent(lane, conn, dest, path, iface, prop, v, || (), failed);
}

/// [`set_prop`] that says when the write is on the bus ([`call_sent`]): `sent` runs on the
/// lane's thread, or is dropped unrun with a write the lane never ran.
#[allow(clippy::too_many_arguments)]
pub(crate) fn set_prop_sent(
    lane: &'static workpool::Lane,
    conn: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    prop: &str,
    v: zbus::zvariant::Value<'static>,
    sent: impl FnOnce() + Send + 'static,
    failed: impl FnOnce() + Send + 'static,
) {
    let (conn, dest, path, iface, prop) = (
        conn.clone(),
        dest.to_string(),
        path.to_string(),
        iface.to_string(),
        prop.to_string(),
    );
    lane.run(move || {
        let to = (dest.as_str(), path.as_str(), PROPS, "Set");
        let r = call_sent(&conn, to, &(iface.as_str(), prop.as_str(), v), sent);
        if let Err(e) = r {
            elog::eprintln!("{dest}: failed to set {prop} on {path}: {e}");
            failed();
        }
    });
}

pub(crate) fn member(m: &Message) -> (String, String, String) {
    let h = m.header();
    (
        h.interface().map(|s| s.to_string()).unwrap_or_default(),
        h.member().map(|s| s.to_string()).unwrap_or_default(),
        h.path().map(|s| s.to_string()).unwrap_or_default(),
    )
}

/// `NameOwnerChanged(name, old, new)` → (name, now owned).
pub(crate) fn owner_changed(m: &Message) -> Option<(String, bool)> {
    let (i, mem, _) = member(m);
    if i != "org.freedesktop.DBus" || mem != "NameOwnerChanged" {
        return None;
    }
    let (n, _, new) = m.body().deserialize::<(String, String, String)>().ok()?;
    Some((n, !new.is_empty()))
}

pub(crate) struct Changed {
    pub path: String,
    pub iface: String,
    pub props: Props,
    /// Properties changed without their value; after [`resolve`], those that no longer exist.
    pub invalidated: Vec<String>,
}

pub(crate) fn props_changed(m: &Message) -> Option<Changed> {
    let (i, mem, path) = member(m);
    if i != PROPS || mem != "PropertiesChanged" {
        return None;
    }
    let (iface, props, invalidated) = m
        .body()
        .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
        .ok()?;
    Some(Changed {
        path,
        iface,
        props: conv_map(props),
        invalidated,
    })
}

/// Fetch the new values of a `PropertiesChanged`'s invalidated properties with one `GetAll`
/// of the interface (D-Bus spec, org.freedesktop.DBus.Properties: invalidated properties are
/// changed, their value is not sent). Those `GetAll` no longer returns are gone and stay in
/// `invalidated`; if the call fails nothing is known about them and the list is emptied.
pub(crate) fn resolve(conn: &Connection, dest: &str, c: Changed) -> Changed {
    if c.invalidated.is_empty() {
        return c;
    }
    let now = get_all(conn, dest, &c.path, &c.iface);
    resolved(c, now)
}

/// [`resolve`] given the `GetAll` reply `now` (`None`: the call failed). A reply with no
/// properties is an answer too: every invalidated property is gone.
fn resolved(mut c: Changed, now: Option<Props>) -> Changed {
    let Some(mut now) = now else {
        c.invalidated.clear();
        return c;
    };
    c.invalidated.retain(|k| match now.remove(k) {
        Some(v) => {
            c.props.insert(k.clone(), v);
            false
        }
        None => true,
    });
    c
}

/// Apply a [`resolve`]d change to an interface's properties.
pub(crate) fn apply_changed(target: &mut Props, c: &Changed) {
    target.extend(c.props.iter().map(|(k, v)| (k.clone(), v.clone())));
    for k in &c.invalidated {
        target.remove(k);
    }
}

/// What a service worker reports: its whole state after (re)connecting or the daemon
/// (re)starting, then only what each signal changed, so the UI thread touches just the objects
/// concerned (as Quickshell's per-object property groups do).
pub(crate) enum Delta<S> {
    Reset(S),
    Changed(Changed),
    /// Interfaces added to an object (ObjectManager.InterfacesAdded, UPower.DeviceAdded).
    Added(String, Ifaces),
    /// Interfaces removed from an object.
    Removed(String, Vec<String>),
}

/// Queue `d` after `ds`: a Reset supersedes everything before it, and a change of an
/// interface merges into its earlier change in the batch unless the object was added or
/// removed in between (a burst of RSSI updates becomes one delta per device).
pub(crate) fn push<S>(ds: &mut Vec<Delta<S>>, d: Delta<S>) {
    let c = match d {
        Delta::Reset(_) => {
            ds.clear();
            ds.push(d);
            return;
        }
        Delta::Changed(c) => c,
        d => {
            ds.push(d);
            return;
        }
    };
    for e in ds.iter_mut().rev() {
        match e {
            Delta::Changed(p) if p.path == c.path && p.iface == c.iface => {
                for k in &c.invalidated {
                    p.props.remove(k);
                }
                p.invalidated.retain(|k| !c.props.contains_key(k));
                for k in c.invalidated {
                    if !p.invalidated.contains(&k) {
                        p.invalidated.push(k);
                    }
                }
                p.props.extend(c.props);
                return;
            }
            Delta::Added(p, _) | Delta::Removed(p, _) if *p == c.path => break,
            Delta::Reset(_) => break,
            _ => {}
        }
    }
    ds.push(Delta::Changed(c));
}

/// A worker session: report `first` as a Reset, then for every burst of signals the deltas
/// `handle` makes of them. False when the consumer is gone, true when the bus dropped.
pub(crate) fn relay<S>(
    rx: &Receiver<Message>,
    first: S,
    mut handle: impl FnMut(&Message) -> Option<Delta<S>>,
    mut send: impl FnMut(Vec<Delta<S>>) -> bool,
) -> bool {
    if !send(vec![Delta::Reset(first)]) {
        return false;
    }
    loop {
        let Some(ms) = batch(rx) else {
            return true;
        };
        let mut ds = vec![];
        for m in &ms {
            if let Some(d) = handle(m) {
                push(&mut ds, d);
            }
        }
        if !ds.is_empty() && !send(ds) {
            return false;
        }
    }
}

pub(crate) fn num(v: Option<&V>) -> Option<f64> {
    match v? {
        V::I(i) => Some(*i as f64),
        V::F(f) => Some(*f),
        _ => None,
    }
}

pub(crate) fn boolean(v: Option<&V>) -> Option<bool> {
    match v? {
        V::B(b) => Some(*b),
        _ => None,
    }
}

pub(crate) fn string(v: Option<&V>) -> Option<String> {
    match v? {
        V::S(s) => Some(s.clone()),
        _ => None,
    }
}

/// Properties of `new` that differ from `old` (all of `new` when `old` is None).
pub(crate) fn diff<'a>(old: Option<&Props>, new: &'a Props) -> Props {
    new.iter()
        .filter(|(k, v)| old.and_then(|o| o.get(*k)) != Some(*v))
        .map(|(k, v): (&'a String, &'a V)| (k.clone(), v.clone()))
        .collect()
}

/// Drain everything queued after the first message (bursts are handled in one snapshot).
pub(crate) fn batch(rx: &Receiver<Message>) -> Option<Vec<Message>> {
    let first = rx.recv().ok()?;
    let mut v = vec![first];
    while let Ok(m) = rx.try_recv() {
        v.push(m);
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::{BufRead, BufReader};
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{Receiver, channel};

    /// A request that is never answered ends with an error at the limit, so what waits behind
    /// it runs; an answered one gives its answer.
    #[test]
    fn a_request_ends_with_its_answer_or_at_the_limit() {
        use std::time::Duration;
        let never = std::future::pending::<zbus::Result<()>>();
        let r = async_io::block_on(super::limited(Duration::from_millis(50), never));
        assert!(
            matches!(&r, Err(zbus::Error::InputOutput(e)) if e.kind() == std::io::ErrorKind::TimedOut),
            "{r:?}"
        );
        let r = async_io::block_on(super::limited(Duration::from_secs(3600), async { Ok(7) }));
        assert_eq!(r.unwrap(), 7);
    }

    struct Daemon(Child);

    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ri-sysbus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A bus daemon on `socket`: bound by itself (`unix:path=`), or, given `activated`, on that
    /// listening socket passed as fd 3 as systemd passes a socket unit's (`systemd:`).
    fn daemon(socket: &Path, activated: Option<&UnixListener>) -> Option<Daemon> {
        let cfg = socket.with_extension("conf");
        let listen = match activated {
            Some(_) => "systemd:".to_string(),
            None => format!("unix:path={}", socket.display()),
        };
        std::fs::write(
            &cfg,
            format!(
                "<busconfig><type>session</type><listen>{listen}</listen><auth>EXTERNAL</auth>\
                 <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>"
            ),
        )
        .ok()?;
        let mut c = Command::new("sh");
        // sd_listen_fds(3) takes the fds only when LISTEN_PID is the daemon's own pid.
        c.args(["-c", r#"LISTEN_PID=$$ exec "$0" "$@""#, "dbus-daemon"])
            .arg(format!("--config-file={}", cfg.display()))
            .args(["--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped());
        if let Some(l) = activated {
            let fd = l.as_raw_fd();
            c.env("LISTEN_FDS", "1");
            // Runs in the forked child before exec: fcntl(2) and dup2(2) are async-signal-safe
            // and nothing is allocated. Copied above 3 first so that `fd` being 3 still works.
            let hook = move || {
                // SAFETY: F_DUPFD_CLOEXEC duplicates the open `fd`; the copy closes at exec.
                let m = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 4) };
                // SAFETY: dup2 of the open `m` onto 3 leaves 3 without FD_CLOEXEC, so the
                // daemon inherits it.
                if m < 0 || unsafe { libc::dup2(m, 3) } < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            };
            // SAFETY: `hook` is async-signal-safe and allocation-free, as pre_exec requires.
            unsafe { c.pre_exec(hook) };
        } else {
            c.env_remove("LISTEN_FDS");
        }
        let mut c = c.spawn().ok()?;
        let out = c.stdout.take();
        let d = Daemon(c);
        // Printed once it accepts connections.
        let mut addr = String::new();
        BufReader::new(out?).read_line(&mut addr).ok()?;
        Some(d)
    }

    /// Bound and listening at a scratch name, then renamed onto `socket`: it is listening
    /// before it appears at the path, as a systemd socket unit's is to a later watcher.
    fn publish(socket: &Path) -> UnixListener {
        let scratch = socket.with_extension("new");
        let _ = std::fs::remove_file(&scratch);
        let l = UnixListener::bind(&scratch).unwrap();
        std::fs::rename(&scratch, socket).unwrap();
        l
    }

    /// Runs [`super::run`] on the bus at `socket` with the real watch. Each session reports
    /// its start and lasts until the bus drops the connection; each connect is reported.
    fn follow(socket: &Path) -> (Receiver<()>, Receiver<()>) {
        let addr = format!("unix:path={}", socket.display());
        let (tx, sessions) = channel();
        let (atx, attempts) = channel();
        let mut watch = super::SocketWatch::new(socket);
        std::thread::spawn(move || {
            let subscribe = || {
                let conn = zbus::blocking::connection::Builder::address(addr.as_str())
                    .and_then(|b| b.build());
                let _ = atx.send(());
                let conn = conn.ok()?;
                let it = zbus::blocking::MessageIterator::from(&conn);
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    for m in it {
                        if m.is_err() || tx.send(m.unwrap()).is_err() {
                            return;
                        }
                    }
                });
                Some((conn, rx))
            };
            super::run(
                subscribe,
                || watch.as_mut().is_some_and(super::SocketWatch::wait),
                |(_, sigs)| {
                    let _ = tx.send(());
                    // The session ends when the bus goes away.
                    while sigs.recv().is_ok() {}
                    true
                },
            );
        });
        (sessions, attempts)
    }

    #[test]
    fn reconnects_when_the_bus_socket_is_created() {
        // The old loop retried every 3 s; now a (re)started daemon's socket wakes the worker.
        // dbus-daemon binds the socket itself: its chmod after listen(2) is what a connect
        // refused between bind(2) and listen(2) waits for.
        let dir = tmp("created");
        let socket = dir.join("bus");
        let (rx, attempts) = follow(&socket);
        // The first attempt (no bus yet) has failed: from here on only the socket wakes it.
        attempts.recv().expect("no first attempt");
        for round in 0..2 {
            let Some(d) = daemon(&socket, None) else {
                crate::testenv::unavailable("no dbus-daemon");
                return;
            };
            // Nothing but the socket's events wakes the worker after the failed attempt.
            rx.recv().expect("no session");
            // The attempt that started the session was reported before it (the daemon's socket
            // events may have woken an earlier one too, before it listened).
            assert!(
                attempts.try_iter().count() >= 1,
                "round {round}: no attempt"
            );
            drop(d);
            // The session ended with the bus and the attempt after it failed: any second
            // session on this daemon would have been reported before.
            attempts.recv().expect("no attempt after the session");
            assert!(rx.try_recv().is_err(), "round {round}: extra session");
            let _ = std::fs::remove_file(&socket);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reconnects_at_once_when_a_socket_activated_bus_restarts() {
        // dbus.socket owns the path and keeps it across a dbus-broker restart: no inotify
        // event marks the new daemon, the connect after the session ended reaches it.
        let dir = tmp("activated");
        let socket = dir.join("bus");
        let unit = publish(&socket);
        let (rx, _attempts) = follow(&socket);
        let Some(d) = daemon(&socket, Some(&unit)) else {
            crate::testenv::unavailable("no dbus-daemon");
            return;
        };
        rx.recv().expect("no session");
        drop(d);
        let _d = daemon(&socket, Some(&unit)).expect("dbus-daemon");
        rx.recv()
            .expect("no session after the bus restarted on its socket");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Runs `run` with connects answered by `connects` (then failing) and socket events by
    /// `events` (then none can come); every session ends with the bus dropping it. Returns
    /// what it did, in order.
    fn trace(connects: &[bool], events: &[bool]) -> Vec<&'static str> {
        let log = RefCell::new(Vec::new());
        let (mut c, mut e) = (connects.iter(), events.iter());
        super::run(
            || {
                log.borrow_mut().push("connect");
                c.next().copied().unwrap_or(false).then_some(())
            },
            || {
                log.borrow_mut().push("wait");
                e.next().copied().unwrap_or(false)
            },
            |()| {
                log.borrow_mut().push("session");
                true
            },
        );
        log.into_inner()
    }

    #[test]
    fn a_session_that_ended_reconnects_at_once() {
        assert_eq!(
            trace(&[true, true, false], &[]),
            [
                "connect", "session", "connect", "session", "connect", "wait"
            ]
        );
    }

    #[test]
    fn a_failed_connect_waits_for_the_next_socket_event() {
        // Refused or no socket: the next connect follows the next socket event, never a timer.
        assert_eq!(
            trace(&[false, false, true], &[true, true]),
            [
                "connect", "wait", "connect", "wait", "connect", "session", "connect", "wait"
            ]
        );
    }

    /// A socket bound but not listening yet sits at `path`; the watch on it is set up after
    /// it appeared.
    fn bound_not_listening(path: &Path) -> (OwnedFd, super::SocketWatch) {
        let fd = super::bound_unlistened(path);
        let watch = super::SocketWatch::new(path).unwrap();
        assert_eq!(
            UnixStream::connect(path).unwrap_err().kind(),
            std::io::ErrorKind::ConnectionRefused
        );
        (fd, watch)
    }

    /// listen(2) on the socket, then `announce` as its daemon does: the watch wakes and the
    /// connect succeeds.
    fn wakes_once_listening(name: &str, announce: impl FnOnce(&Path)) {
        let dir = tmp(name);
        let socket = dir.join("bus");
        let (fd, mut watch) = bound_not_listening(&socket);
        // SAFETY: listen(2) on the open socket `fd`.
        assert_eq!(unsafe { libc::listen(fd.as_raw_fd(), 8) }, 0);
        announce(&socket);
        let (tx, rx) = channel();
        std::thread::spawn(move || tx.send(watch.wait()));
        assert!(rx.recv().expect("no socket event once listening"));
        UnixStream::connect(&socket).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn socket_watch_wakes_when_a_socket_unit_touches_its_node() {
        // systemd's socket_address_listen: touch(path) after listen(2).
        wakes_once_listening("touch", |p| {
            use std::os::unix::ffi::OsStrExt;
            let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
            // SAFETY: `c` is a NUL-terminated path; null times means now, as touch(1) sets.
            let r = unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), std::ptr::null(), 0) };
            assert_eq!(r, 0);
        });
    }

    #[test]
    fn socket_watch_wakes_when_dbus_daemon_chmods_its_socket() {
        // dbus-daemon's _dbus_listen_unix_socket: chmod(path, 0777) after listen(2).
        wakes_once_listening("chmod", |p| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o777)).unwrap();
        });
    }

    fn changed(path: &str, iface: &str, kv: &[(&str, i64)], inv: &[&str]) -> super::Delta<()> {
        super::Delta::Changed(super::Changed {
            path: path.into(),
            iface: iface.into(),
            props: kv
                .iter()
                .map(|(k, v)| (k.to_string(), crate::mpris::V::I(*v)))
                .collect(),
            invalidated: inv.iter().map(|s| s.to_string()).collect(),
        })
    }

    fn shape(ds: &[super::Delta<()>]) -> Vec<String> {
        ds.iter()
            .map(|d| match d {
                super::Delta::Reset(()) => "reset".into(),
                super::Delta::Changed(c) => {
                    let mut p: Vec<_> = c
                        .props
                        .iter()
                        .map(|(k, v)| format!("{k}={}", super::num(Some(v)).unwrap()))
                        .collect();
                    p.sort();
                    format!("{} {} {p:?} -{:?}", c.path, c.iface, c.invalidated)
                }
                super::Delta::Added(p, _) => format!("added {p}"),
                super::Delta::Removed(p, _) => format!("removed {p}"),
            })
            .collect()
    }

    /// The stop ends a wait in progress and every later one, with no socket event.
    #[test]
    fn socket_watch_ends_on_stop() {
        let dir = tmp("stop");
        let socket = dir.join("bus");
        let mut watch = super::SocketWatch::new(&socket).unwrap();
        let stop = std::sync::Arc::new(crate::svc::Stop::default());
        watch.stop_on(&stop);
        let (tx, rx) = channel();
        let t = std::thread::spawn(move || {
            tx.send(()).unwrap();
            let first = watch.wait();
            (first, watch.wait())
        });
        rx.recv().unwrap();
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(t.join().unwrap(), (false, false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bursts_coalesce_per_interface() {
        let mut ds = vec![];
        for d in [
            changed("/a", "D", &[("RSSI", -40)], &[]),
            changed("/b", "D", &[("RSSI", -70)], &[]),
            changed("/a", "D", &[("RSSI", -41), ("TxPower", 4)], &[]),
            changed("/a", "B", &[("Percentage", 50)], &[]),
            changed("/b", "D", &[], &["RSSI"]),
            changed("/a", "D", &[("RSSI", -42)], &[]),
        ] {
            super::push(&mut ds, d);
        }
        assert_eq!(
            shape(&ds),
            [
                r#"/a D ["RSSI=-42", "TxPower=4"] -[]"#,
                r#"/b D [] -["RSSI"]"#,
                r#"/a B ["Percentage=50"] -[]"#,
            ]
        );
        // Not merged across the object being added or removed; a Reset drops what came before.
        super::push(
            &mut ds,
            super::Delta::Removed("/b".into(), vec!["D".into()]),
        );
        super::push(&mut ds, changed("/b", "D", &[("RSSI", -1)], &[]));
        assert_eq!(shape(&ds).len(), 5);
        super::push(&mut ds, super::Delta::Reset(()));
        super::push(&mut ds, changed("/a", "D", &[("RSSI", 1)], &[]));
        assert_eq!(shape(&ds), ["reset", r#"/a D ["RSSI=1"] -[]"#]);
    }

    /// `GetAll` after a `PropertiesChanged` with invalidated properties: what it returns is
    /// their new value, what it no longer returns is gone (also when it returns nothing at
    /// all), and a failed call says nothing about them.
    #[test]
    fn invalidated_properties_follow_the_get_all_reply() {
        let inv = || {
            let super::Delta::Changed(c) = changed("/a", "D", &[("Kept", 1)], &["Gone", "New"])
            else {
                unreachable!()
            };
            c
        };
        let reply = |kv: &[(&str, i64)]| -> Option<super::Props> {
            Some(
                kv.iter()
                    .map(|(k, v)| (k.to_string(), crate::mpris::V::I(*v)))
                    .collect(),
            )
        };
        let show = |c: super::Changed| shape(&[super::Delta::Changed(c)]).remove(0);
        assert_eq!(
            show(super::resolved(inv(), reply(&[("New", 2), ("Other", 3)]))),
            r#"/a D ["Kept=1", "New=2"] -["Gone"]"#
        );
        assert_eq!(
            show(super::resolved(inv(), reply(&[]))),
            r#"/a D ["Kept=1"] -["Gone", "New"]"#,
            "an empty reply: every invalidated property is gone"
        );
        assert_eq!(
            show(super::resolved(inv(), None)),
            r#"/a D ["Kept=1"] -[]"#,
            "a failed call: nothing is known"
        );
    }

    #[test]
    fn system_socket_from_address() {
        assert_eq!(
            super::system_socket_of(Some("unix:path=/x/y,guid=1;tcp:host=a")),
            Some("/x/y".into())
        );
        assert_eq!(super::system_socket_of(Some("unix:abstract=/z")), None);
        assert_eq!(
            super::system_socket_of(None),
            Some("/run/dbus/system_bus_socket".into())
        );
    }

    #[test]
    fn session_socket_from_address() {
        assert_eq!(
            super::session_socket_of(Some("unix:path=/run/user/1/bus"), Some("/x".into())),
            Some("/run/user/1/bus".into())
        );
        assert_eq!(
            super::session_socket_of(Some("unix:abstract=/z"), None),
            None
        );
        assert_eq!(
            super::session_socket_of(None, Some("/run/user/1".into())),
            Some("/run/user/1/bus".into())
        );
        assert_eq!(super::session_socket_of(None, None), None);
    }

    /// The system refuses the service's thread (the limit on threads is reached): `spawn`
    /// returns, instead of panicking, and has dropped the session it never ran, with what the
    /// session holds (a service's channel to the loop). The refusal is in the log. In a
    /// process of its own: the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_thread_drops_the_session_unrun() {
        const TEST: &str = concat!(module_path!(), "::a_refused_thread_drops_the_session_unrun");
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        struct Held(std::sync::mpsc::Sender<()>);
        impl Drop for Held {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let (tx, dropped) = channel();
        let held = Held(tx);
        crate::testenv::refuse_new_threads();
        let ((), log) = crate::testenv::stderr_with(|| {
            super::spawn(vec![], Default::default(), move |_, _| {
                let _ = &held;
                true
            });
        });
        assert!(
            log.contains("sysbus: cannot start a service thread:"),
            "stderr: {log:?}"
        );
        assert_eq!(dropped.try_recv(), Ok(()));
    }
}
