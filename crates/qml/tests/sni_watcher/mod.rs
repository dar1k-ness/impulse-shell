//! A private session `dbus-daemon`, a foreign StatusNotifierWatcher and an event-driven loop
//! for the SystemTray watcher hand-off tests (one test per binary: the Watcher name is
//! process-wide, as every runtime shares one bus connection).
// Shared by several test binaries, each using part of it.
#![allow(dead_code)]

use qml::runtime::Runtime;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use testenv::Wakeups;
use zbus::blocking::Connection;
use zbus::zvariant::OwnedValue;

pub const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

/// In `test`'s own process (see `testenv::run_in_own_process`): a private session bus at the
/// address the environment names, a socket in a directory the parent made, as everything the
/// test runs finds the session bus through the environment. `None` in the process that ran the
/// test there, or without dbus-daemon.
///
/// Closing the returned stdin (test exit) makes the wrapper shell kill the daemon.
pub fn bus(test: &str) -> Option<ChildStdin> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(test, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        return None;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let cfg = std::env::temp_dir().join(format!("qml-sni-watcher-{}.conf", std::process::id()));
    std::fs::write(
        &cfg,
        "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>\
         <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
    )
    .ok()?;
    let Ok(mut c) = Command::new("sh")
        .args(["-c", "dbus-daemon --config-file=\"$0\" --address=\"$1\" --nofork --nopidfile --print-address=1 & read _; kill $!"])
        .arg(&cfg)
        .arg(&addr)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    else {
        testenv::unavailable("no dbus-daemon");
        return None;
    };
    let mut line = String::new();
    let _ = BufReader::new(c.stdout.take().unwrap()).read_line(&mut line);
    if line.trim().is_empty() {
        testenv::unavailable("dbus-daemon not available");
        return None;
    }
    c.stdin.take()
}

/// The test's host loop for a runtime: every event the test waits for (the runtime's own
/// wakeups, a foreign watcher's calls, Watcher name owner changes) pings `wake`, and it sleeps
/// on the runtime's epoll set as well, the way the shell's loop does.
pub struct Loop {
    pub wake: Wakeups,
    ping: Arc<dyn Fn() + Send + Sync>,
    ep: Option<i32>,
}

impl Loop {
    pub fn new(rt: &mut Runtime) -> Loop {
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        let ping: Arc<dyn Fn() + Send + Sync> = Arc::new(wake.ping());
        Loop { wake, ping, ep }
    }

    pub fn ping(&self) -> Arc<dyn Fn() + Send + Sync> {
        self.ping.clone()
    }

    /// Runs loop turns, sleeping on the wakeups between them, until `done`.
    pub fn until(&self, rt: &mut Runtime, what: &str, mut done: impl FnMut(&mut Runtime) -> bool) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        loop {
            rt.poll_native();
            rt.settle();
            if done(rt) {
                return;
            }
            if self.wake.wait(self.ep, None).fd {
                rt.process_fd_ready();
            }
        }
    }

    /// Runs loop turns until `rx` (whose sender pings this loop) yields what `want` matches:
    /// what came before it, and the match.
    pub fn recv_until<T>(
        &self,
        rt: &mut Runtime,
        rx: &Receiver<T>,
        what: &str,
        mut want: impl FnMut(&T) -> bool,
    ) -> (Vec<T>, T) {
        let mut before = vec![];
        let mut hit = None;
        self.until(rt, what, |_| {
            for x in rx.try_iter() {
                if want(&x) {
                    hit = Some(x);
                    return true;
                }
                before.push(x);
            }
            false
        });
        (before, hit.unwrap())
    }
}

/// A StatusNotifierWatcher of another process: lists `items` and reports each host that
/// registers with it.
struct Fake {
    items: Vec<String>,
    hosts: Mutex<Sender<String>>,
    ping: Arc<dyn Fn() + Send + Sync>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Fake {
    fn register_status_notifier_host(&self, service: &str) {
        let _ = self.hosts.lock().unwrap().send(service.to_string());
        (self.ping)();
    }

    fn register_status_notifier_item(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

/// A foreign watcher that owns the Watcher name, taking it over if `replace`. It gives the
/// name up entirely when replaced (no queueing), as only the last owner matters here.
pub fn foreign_watcher(
    items: Vec<String>,
    replace: bool,
    ping: Arc<dyn Fn() + Send + Sync>,
) -> (Connection, Receiver<String>) {
    let (tx, hosts) = channel();
    let c = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at(
            WATCHER_PATH,
            Fake {
                items,
                hosts: Mutex::new(tx),
                ping,
            },
        )
        .unwrap()
        .build()
        .unwrap();
    use zbus::fdo::RequestNameFlags as F;
    let mut flags = F::AllowReplacement | F::DoNotQueue;
    if replace {
        flags |= F::ReplaceExisting;
    }
    let r = c.request_name_with_flags(WATCHER, flags).unwrap();
    assert_eq!(r, zbus::fdo::RequestNameReply::PrimaryOwner);
    (c, hosts)
}

/// Each new owner of the Watcher name ("" once nobody owns it), from a match rule added
/// before this returns.
pub fn watcher_owners(ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<String> {
    let c = Connection::session().unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&c).unwrap();
    let it = dbus
        .receive_name_owner_changed_with_args(&[(0, WATCHER)])
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
            ping();
        }
    });
    rx
}

/// A StatusNotifierItem with only an id, a status and an icon name.
pub struct Named(pub &'static str);

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Named {
    #[zbus(property)]
    fn id(&self) -> String {
        self.0.into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        format!("{}-icon", self.0)
    }
}

/// A reply the test holds back: `wait` resolves once `open` is called, from any thread.
#[derive(Clone, Default)]
pub struct Gate(Arc<Mutex<(bool, Option<std::task::Waker>)>>);

impl Gate {
    pub fn open(&self) {
        let w = {
            let mut g = self.0.lock().unwrap();
            g.0 = true;
            g.1.take()
        };
        if let Some(w) = w {
            w.wake();
        }
    }

    pub async fn wait(&self) {
        std::future::poll_fn(|cx| {
            let mut g = self.0.lock().unwrap();
            if g.0 {
                std::task::Poll::Ready(())
            } else {
                g.1 = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        })
        .await
    }
}

/// [`Named`] whose `Id` read (so its `GetAll`) is answered only once the test opens `gate`;
/// `entered` (pinging the test's loop) reports the read began.
pub struct Held {
    id: &'static str,
    gate: Gate,
    entered: Mutex<Option<Sender<()>>>,
    ping: Arc<dyn Fn() + Send + Sync>,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Held {
    #[zbus(property)]
    async fn id(&self) -> String {
        let entered = self.entered.lock().unwrap().take();
        if let Some(tx) = entered {
            let _ = tx.send(());
            (self.ping)();
        }
        self.gate.wait().await;
        self.id.into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        format!("{}-icon", self.id)
    }
}

/// A connection serving a [`Held`] item `id`, its gate, and the report of its first read.
pub fn held_item(
    id: &'static str,
    ping: Arc<dyn Fn() + Send + Sync>,
) -> (Connection, Gate, Receiver<()>) {
    let gate = Gate::default();
    let (tx, entered) = channel();
    let c = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at(
            "/StatusNotifierItem",
            Held {
                id,
                gate: gate.clone(),
                entered: Mutex::new(Some(tx)),
                ping,
            },
        )
        .unwrap()
        .build()
        .unwrap();
    (c, gate, entered)
}

/// Registers the item `c` serves with the Watcher, under `c`'s unique name.
pub fn register(c: &Connection) {
    c.call_method(
        Some(WATCHER),
        WATCHER_PATH,
        Some(WATCHER),
        "RegisterStatusNotifierItem",
        &(c.unique_name().unwrap().as_str(),),
    )
    .unwrap();
}

/// A connection serving item `id` at `/StatusNotifierItem`.
pub fn item(id: &'static str) -> Connection {
    zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at("/StatusNotifierItem", Named(id))
        .unwrap()
        .build()
        .unwrap()
}

/// A tray whose binding uses SystemTray at load, which starts the service.
pub const TRAY_QML: &str = "import QtQuick\nimport Rustshell.Services.SystemTray\nItem {\n\
    property string ids: SystemTray.items.values.map(i => i.id).join(',') }";

pub fn ids(rt: &Runtime, root: usize) -> String {
    rt.get(root, "ids").to_js_string()
}

/// A signal of the object at `/StatusNotifierWatcher`, as the tests wait for it.
#[derive(Debug, Clone, PartialEq)]
pub enum Sig {
    ItemRegistered(String),
    ItemUnregistered(String),
    HostRegistered,
    HostUnregistered,
    /// `PropertiesChanged` carrying `RegisteredStatusNotifierItems`.
    Items(Vec<String>),
    /// `PropertiesChanged` carrying `IsStatusNotifierHostRegistered`.
    HostPresent(bool),
}

/// The Watcher object's signals in the order its owner sent them, from a match rule added
/// before this returns.
pub fn watcher_signals(ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<Sig> {
    let c = Connection::session().unwrap();
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(WATCHER_PATH)
        .unwrap()
        .build();
    let it = zbus::blocking::MessageIterator::for_match_rule(rule, &c, None).unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _c = c;
        for m in it {
            let Ok(m) = m else { continue };
            let member = m.header().member().map(|n| n.to_string());
            let b = m.body();
            let sigs = match member.as_deref() {
                Some("StatusNotifierItemRegistered") => {
                    vec![Sig::ItemRegistered(b.deserialize().unwrap())]
                }
                Some("StatusNotifierItemUnregistered") => {
                    vec![Sig::ItemUnregistered(b.deserialize().unwrap())]
                }
                Some("StatusNotifierHostRegistered") => vec![Sig::HostRegistered],
                Some("StatusNotifierHostUnregistered") => vec![Sig::HostUnregistered],
                Some("PropertiesChanged") => {
                    let (_, changed, _): (String, HashMap<String, OwnedValue>, Vec<String>) =
                        b.deserialize().unwrap();
                    let mut v = vec![];
                    if let Some(x) = changed.get("RegisteredStatusNotifierItems") {
                        v.push(Sig::Items(x.try_clone().unwrap().try_into().unwrap()));
                    }
                    if let Some(x) = changed.get("IsStatusNotifierHostRegistered") {
                        v.push(Sig::HostPresent(x.try_clone().unwrap().try_into().unwrap()));
                    }
                    v
                }
                _ => vec![],
            };
            for s in sigs {
                if tx.send(s).is_err() {
                    return;
                }
                ping();
            }
        }
    });
    rx
}

/// `(name, old owner, new owner)` of each bus name starting with `prefix` that changes owner,
/// from a match rule added before this returns.
pub fn name_owners(
    prefix: &'static str,
    ping: Arc<dyn Fn() + Send + Sync>,
) -> Receiver<(String, String, String)> {
    let c = Connection::session().unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&c).unwrap();
    let it = dbus.receive_name_owner_changed().unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _c = c;
        let un = |o: &Option<zbus::names::UniqueName<'_>>| {
            o.as_ref().map_or(String::new(), |n| n.to_string())
        };
        for s in it {
            let Ok(a) = s.args() else { continue };
            if !a.name().starts_with(prefix) {
                continue;
            }
            let ev = (a.name().to_string(), un(a.old_owner()), un(a.new_owner()));
            if tx.send(ev).is_err() {
                return;
            }
            ping();
        }
    });
    rx
}

/// Receives from `rx` until `want` holds, with no runtime left to run: what came before it,
/// and the match.
pub fn recv_until<T: std::fmt::Debug>(
    rx: &Receiver<T>,
    what: &str,
    mut want: impl FnMut(&T) -> bool,
) -> (Vec<T>, T) {
    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for {what}");
    let mut before = vec![];
    loop {
        let x = rx.recv().unwrap();
        if want(&x) {
            return (before, x);
        }
        before.push(x);
    }
}

/// A property of the Watcher, read from `c`.
pub fn watcher_prop(c: &Connection, name: &str) -> OwnedValue {
    zbus::blocking::fdo::PropertiesProxy::builder(c)
        .destination(WATCHER)
        .unwrap()
        .path(WATCHER_PATH)
        .unwrap()
        .build()
        .unwrap()
        .get(WATCHER.try_into().unwrap(), name)
        .unwrap()
}

/// A StatusNotifierHost of another process: a connection owning `name`, registered with the
/// Watcher under it.
pub fn host(name: &str) -> Connection {
    let c = Connection::session().unwrap();
    c.request_name(name).unwrap();
    c.call_method(
        Some(WATCHER),
        WATCHER_PATH,
        Some(WATCHER),
        "RegisterStatusNotifierHost",
        &(name,),
    )
    .unwrap();
    c
}
