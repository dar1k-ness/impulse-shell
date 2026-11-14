//! Tray menus against a fake StatusNotifierItem with a `com.canonical.dbusmenu` menu on a
//! private session `dbus-daemon`: `AboutToShow` goes out once per open, not on every refetch
//! (an app answering it with `LayoutUpdated` used to loop); only the shown menu's own signals
//! refetch or patch it; the menu's bus match ends with its last opener and with the tray; the
//! main loop never waits on the bus for it; and an app that does not answer (`Event`, or any
//! menu call at all) holds neither its menu's later calls nor, with the system refusing the
//! menu its thread, the main loop.
//! Every wait blocks on the runtime's wakeups; nextest's slow-timeout is the hang guard.

use qml::runtime::Runtime;
use qml::value::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use testenv::Wakeups;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value as ZV};

mod sni_watcher;

const MENU: &str = "com.canonical.dbusmenu";

/// Closing it (test process exit) makes the wrapper shell kill the daemon.
struct Bus(#[allow(dead_code)] ChildStdin);

/// A private bus for `test` (its exact libtest name): the test runs in a process of its own
/// (the services find the bus through the environment and start once per process) with
/// `DBUS_SESSION_BUS_ADDRESS` set to a socket in a fresh directory, where the daemon listens.
/// `None` in the parent, which has run the test there, or without dbus-daemon.
fn bus(test: &str) -> Option<Bus> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(test, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        return None;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let cfg = std::env::temp_dir().join(format!("qml-sni-menu-{}.conf", std::process::id()));
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
    Some(Bus(c.stdin.take().unwrap()))
}

#[derive(Default)]
struct State {
    labels: Vec<String>,
    revision: u32,
    /// `AboutToShow` answers with `LayoutUpdated`, as libdbusmenu apps that rebuild on it do.
    echo: bool,
    calls: Vec<String>,
    /// Holds the next `GetLayout` until the test releases it, after saying it started.
    gate: Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
    /// Wakes the test's loop on each `Event`.
    ping: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The next `GetLayout` is answered once the gate opens; the call reports it began.
    hold: Option<(sni_watcher::Gate, Box<dyn Fn() + Send>)>,
    /// `GetLayout` fails (an app whose menu is broken).
    fail_layout: bool,
    /// The methods a call of which is never answered, from the call on.
    mute: Vec<&'static str>,
}

impl State {
    /// Notes a call of `method` that is never to be answered, and wakes the test's loop.
    fn muted(&mut self, method: &str, call: String) -> bool {
        if !self.mute.contains(&method) {
            return false;
        }
        self.calls.push(format!("{call} unanswered"));
        if let Some(p) = &self.ping {
            p();
        }
        true
    }
}

/// Never ends: the caller gets no reply, as from an app whose loop is stuck.
async fn unanswered() {
    std::future::pending::<()>().await
}

type Shared = Arc<Mutex<State>>;

struct Menu(Shared);

fn label(l: &str) -> HashMap<String, OwnedValue> {
    HashMap::from([(
        "label".to_string(),
        OwnedValue::from(zbus::zvariant::Str::from(l.to_string())),
    )])
}

type Node = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    async fn about_to_show(&self, id: i32, #[zbus(signal_emitter)] e: SignalEmitter<'_>) -> bool {
        let muted = self
            .0
            .lock()
            .unwrap()
            .muted("AboutToShow", format!("AboutToShow {id}"));
        if muted {
            unanswered().await;
        }
        let (echo, rev) = {
            let mut s = self.0.lock().unwrap();
            s.calls.push(format!("AboutToShow {id}"));
            if s.echo {
                s.revision += 1;
            }
            (s.echo, s.revision)
        };
        if echo {
            Menu::layout_updated(&e, rev, 0).await.unwrap();
        }
        echo
    }

    /// Answers the layout as it was when the call came in, however long a gate holds it.
    async fn get_layout(
        &self,
        parent: i32,
        _depth: i32,
        _props: Vec<String>,
    ) -> zbus::fdo::Result<(u32, Node)> {
        let muted = self
            .0
            .lock()
            .unwrap()
            .muted("GetLayout", format!("GetLayout {parent}"));
        if muted {
            unanswered().await;
        }
        let (revision, n) = {
            let mut s = self.0.lock().unwrap();
            if s.fail_layout {
                s.calls.push(format!("GetLayout {parent} failed"));
                if let Some(p) = &s.ping {
                    p();
                }
                return Err(zbus::fdo::Error::Failed("no menu".into()));
            }
            (s.revision, s.labels.len() as i32)
        };
        let gate = self.0.lock().unwrap().gate.take();
        if let Some((started, release)) = gate {
            started.send(()).unwrap();
            if let Some(p) = &self.0.lock().unwrap().ping {
                p();
            }
            let _ = release.recv();
        }
        let hold = self.0.lock().unwrap().hold.take();
        if let Some((gate, entered)) = hold {
            entered();
            gate.wait().await;
        }
        self.0
            .lock()
            .unwrap()
            .calls
            .push(format!("GetLayout {parent}"));
        let kids = (1..=n)
            .map(|i| {
                let node: Node = (i, HashMap::new(), vec![]);
                OwnedValue::try_from(ZV::from(zbus::zvariant::Structure::from(node))).unwrap()
            })
            .collect();
        Ok((revision, (parent, HashMap::new(), kids)))
    }

    async fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _props: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let muted = self
            .0
            .lock()
            .unwrap()
            .muted("GetGroupProperties", "GetGroupProperties".into());
        if muted {
            unanswered().await;
        }
        let mut s = self.0.lock().unwrap();
        s.calls.push("GetGroupProperties".into());
        ids.into_iter()
            .filter_map(|i| s.labels.get(i as usize - 1).map(|l| (i, label(l))))
            .collect()
    }

    async fn event(&self, id: i32, name: String, _data: OwnedValue, _ts: u32) {
        let muted = {
            let mut s = self.0.lock().unwrap();
            s.calls.push(format!("Event {id} {name}"));
            if let Some(p) = &s.ping {
                p();
            }
            s.mute.contains(&"Event")
        };
        if muted {
            unanswered().await;
        }
    }

    #[zbus(signal)]
    async fn layout_updated(e: &SignalEmitter<'_>, revision: u32, parent: i32) -> zbus::Result<()>;
}

struct Item;

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn id(&self) -> String {
        "app".into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn menu(&self) -> zbus::zvariant::OwnedObjectPath {
        zbus::zvariant::ObjectPath::try_from("/MenuBar")
            .unwrap()
            .into()
    }
}

/// A tray app: its item, registered with the Watcher, and its menu at `/MenuBar`.
struct App {
    conn: zbus::blocking::Connection,
    state: Shared,
}

impl App {
    fn new(labels: &[&str], echo: bool) -> App {
        App::with(State {
            labels: labels.iter().map(|s| s.to_string()).collect(),
            echo,
            ..Default::default()
        })
    }

    fn with(state: State) -> App {
        let state: Shared = Arc::new(Mutex::new(state));
        let conn = zbus::blocking::connection::Builder::session()
            .unwrap()
            .serve_at("/StatusNotifierItem", Item)
            .unwrap()
            .serve_at("/MenuBar", Menu(state.clone()))
            .unwrap()
            .build()
            .unwrap();
        let me = conn.unique_name().unwrap().to_string();
        conn.call_method(
            Some("org.kde.StatusNotifierWatcher"),
            "/StatusNotifierWatcher",
            Some("org.kde.StatusNotifierWatcher"),
            "RegisterStatusNotifierItem",
            &(me.as_str(),),
        )
        .unwrap();
        App { conn, state }
    }

    /// Counts of each method called so far, by name.
    fn count(&self, method: &str) -> usize {
        let s = self.state.lock().unwrap();
        s.calls
            .iter()
            .filter(|c| c.split(' ').next() == Some(method))
            .count()
    }

    /// The `Event` calls so far, in order.
    fn events(&self) -> Vec<String> {
        let s = self.state.lock().unwrap();
        s.calls
            .iter()
            .filter(|c| c.starts_with("Event"))
            .cloned()
            .collect()
    }

    /// Changes the layout and announces it, as the spec has it: a new revision for the root.
    fn relabel(&self, labels: &[&str]) {
        let rev = {
            let mut s = self.state.lock().unwrap();
            s.labels = labels.iter().map(|s| s.to_string()).collect();
            s.revision += 1;
            s.revision
        };
        layout_updated(&self.conn, "/MenuBar", rev, 0);
    }

    /// Changes one entry's label and announces it with `ItemsPropertiesUpdated`.
    fn set_label(&self, id: i32, text: &str) {
        self.state.lock().unwrap().labels[id as usize - 1] = text.to_string();
        props_updated(&self.conn, "/MenuBar", id, text);
    }
}

/// Blocks until the StatusNotifierWatcher name has an owner.
fn watcher_up() {
    const WATCHER: &str = "org.kde.StatusNotifierWatcher";
    let c = zbus::blocking::Connection::session().unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&c).unwrap();
    // Subscribed before looking, so a claim in between is not missed.
    let mut claims = dbus
        .receive_name_owner_changed_with_args(&[(0, WATCHER)])
        .unwrap();
    if dbus.name_has_owner(WATCHER.try_into().unwrap()).unwrap() {
        return;
    }
    eprintln!("waiting for the Watcher");
    assert!(
        claims.any(|s| s.args().is_ok_and(|a| a.new_owner().is_some())),
        "the bus closed"
    );
}

fn layout_updated(c: &zbus::blocking::Connection, path: &str, rev: u32, parent: i32) {
    c.emit_signal(None::<&str>, path, MENU, "LayoutUpdated", &(rev, parent))
        .unwrap();
}

fn props_updated(c: &zbus::blocking::Connection, path: &str, id: i32, text: &str) {
    let updated = vec![(id, label(text))];
    let removed: Vec<(i32, Vec<String>)> = vec![];
    c.emit_signal(
        None::<&str>,
        path,
        MENU,
        "ItemsPropertiesUpdated",
        &(updated, removed),
    )
    .unwrap();
}

/// Returns once the bus has handled every message `c` sent before (one sender's messages are
/// handled in order), so its signals are queued to their receivers.
fn flushed(c: &zbus::blocking::Connection) {
    zbus::blocking::fdo::DBusProxy::new(c)
        .unwrap()
        .get_id()
        .unwrap();
}

const TRAY_QML: &str = "import QtQuick\nimport Quickshell\nimport Rustshell.Services.SystemTray\n\
    Item {\n property bool open: true\n property var kept: null\n\
    readonly property var item: SystemTray.items.values.length ? SystemTray.items.values[0] : null\n\
    QsMenuOpener { id: opener; menu: open && item ? item.menu : null }\n\
    function count() { return SystemTray.items.values.length }\n\
    function labels() { return opener.children.values.map(e => e.text).join(',') }\n\
    function keep() { kept = opener.children.values[0] }\n\
    function same() { return kept === opener.children.values[0] }\n}";

/// A runtime with its host loop: every wait blocks until the runtime is woken.
struct Host {
    rt: Runtime,
    r: usize,
    wake: Wakeups,
}

impl Drop for Host {
    /// Ends the tray (the world's `Sni`) with its host, as the shell's world does when it goes;
    /// a test's world may outlive its `Runtime` (values the interpreter still holds).
    fn drop(&mut self) {
        drop(self.rt.world.borrow_mut().sni.take());
    }
}

impl Host {
    fn new() -> Host {
        let mut rt = Runtime::new(None);
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let r = rt.load_str(TRAY_QML).unwrap();
        rt.settle();
        // Starts the tray, which serves the Watcher from its own thread.
        rt.poll_native();
        watcher_up();
        Host { rt, r, wake }
    }

    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Host) -> bool) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        loop {
            self.turn();
            if done(self) {
                return;
            }
            self.wake.wait(None, None);
        }
    }

    /// Runs what is queued for the loop now, without waiting. Like the shell's loop, it polls
    /// the services again while a poll's own writes changed the world (an opener bound to a
    /// tray item that poll created), since those count for the next poll.
    fn turn(&mut self) {
        loop {
            let rev = self.rt.revision();
            self.rt.poll_native();
            self.rt.settle();
            if self.rt.revision() == rev {
                return;
            }
        }
    }

    fn call(&mut self, f: &str) -> Value {
        self.rt.call_method(self.r, f, vec![]).unwrap().unwrap()
    }

    fn labels(&mut self) -> String {
        self.call("labels").to_js_string()
    }

    fn wait_labels(&mut self, want: &str) {
        self.until(&format!("menu {want:?}"), |h| h.labels() == want);
    }

    /// Gives the opener its item's menu, or takes it away, and runs what that queues.
    fn open(&mut self, open: bool) {
        self.rt
            .world
            .borrow_mut()
            .write(self.r, "open", Value::Bool(open));
        self.turn();
    }
}

/// The match rules of the shell's (this process's) session connection on the bus.
fn our_menu_rules(other: &zbus::blocking::Connection) -> Vec<String> {
    // The tray makes and ends menu matches off the main loop: every one asked for is sent.
    workpool::wait_idle();
    // Everything the shell sent before (an AddMatch or RemoveMatch) is handled first.
    assert!(qml::dbus::sync(qml::dbus::Bus::Session));
    let me = qml::dbus::session()
        .unwrap()
        .unique_name()
        .unwrap()
        .to_string();
    let reply = other
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Debug.Stats"),
            "GetAllMatchRules",
            &(),
        )
        .unwrap();
    let all: HashMap<String, Vec<String>> = reply.body().deserialize().unwrap();
    all.get(&me)
        .map(|rs| rs.iter().filter(|r| r.contains(MENU)).cloned().collect())
        .unwrap_or_default()
}

/// An app that answers `AboutToShow` with `LayoutUpdated` gets one `AboutToShow` per open and
/// one GetLayout per real change; property updates patch the entries in place.
#[test]
fn about_to_show_once_per_open_not_per_refetch() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::about_to_show_once_per_open_not_per_refetch"
    )) else {
        return;
    };
    let mut h = Host::new();
    let app = App::new(&["A", "B"], true);
    h.wait_labels("A,B");
    h.call("keep");
    // The echo preceded the GetLayout reply: already in it, no refetch.
    app.relabel(&["A", "B", "C"]);
    h.wait_labels("A,B,C");
    assert_eq!(
        (app.count("AboutToShow"), app.count("GetLayout")),
        (1, 2),
        "a refetch re-sent AboutToShow (or refetched the echo): {:?}",
        app.state.lock().unwrap().calls
    );
    assert!(h.call("same").truthy(), "a kept entry was recreated");

    app.set_label(1, "A2");
    h.wait_labels("A2,B,C");
    assert!(
        h.call("same").truthy(),
        "a property update recreated the entry"
    );
    assert_eq!(
        (app.count("AboutToShow"), app.count("GetLayout")),
        (1, 2),
        "a property update refetched the menu"
    );

    // Reopening is a new show: one more AboutToShow.
    h.rt.world
        .borrow_mut()
        .write(h.r, "open", Value::Bool(false));
    h.until("menu closed", |h| h.labels().is_empty());
    h.rt.world
        .borrow_mut()
        .write(h.r, "open", Value::Bool(true));
    h.wait_labels("A2,B,C");
    assert_eq!(app.count("AboutToShow"), 2);
    let errs = h.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// Another app's menu signals, and signals of another object of the same app, neither
/// refetch nor patch the shown menu.
#[test]
fn foreign_menu_signals_leave_the_open_menu_alone() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::foreign_menu_signals_leave_the_open_menu_alone"
    )) else {
        return;
    };
    let mut h = Host::new();
    let app = App::new(&["A", "B"], false);
    h.wait_labels("A,B");
    let other = zbus::blocking::Connection::session().unwrap();
    for i in 0..5 {
        layout_updated(&other, "/Other/Menu", 100 + i, 0);
        layout_updated(&other, "/MenuBar", 100 + i, 0);
        layout_updated(&app.conn, "/Other/Menu", 100 + i, 0);
    }
    props_updated(&other, "/MenuBar", 1, "X");
    props_updated(&app.conn, "/Other/Menu", 1, "Y");
    flushed(&other);
    flushed(&app.conn);
    // Every signal the bus sent the shell is now in its queues; run them.
    assert!(qml::dbus::sync(qml::dbus::Bus::Session));
    h.turn();
    app.relabel(&["A", "B", "C"]);
    h.wait_labels("A,B,C");
    assert_eq!(
        (app.count("AboutToShow"), app.count("GetLayout")),
        (1, 2),
        "foreign signals refetched the menu: {:?}",
        app.state.lock().unwrap().calls
    );
}

/// The menu's match is on the bus only while an opener shows it, and goes with the tray.
#[test]
fn menu_match_ends_with_its_last_opener_and_with_the_tray() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::menu_match_ends_with_its_last_opener_and_with_the_tray"
    )) else {
        return;
    };
    let other = zbus::blocking::Connection::session().unwrap();
    let mut h = Host::new();
    let app = App::new(&["A"], false);
    h.wait_labels("A");
    let rules = our_menu_rules(&other);
    let me = app.conn.unique_name().unwrap().to_string();
    assert!(
        rules.len() == 1 && rules[0].contains(&me) && rules[0].contains("/MenuBar"),
        "one match, for this menu's sender and path: {rules:?}"
    );

    h.rt.world
        .borrow_mut()
        .write(h.r, "open", Value::Bool(false));
    h.until("menu closed", |h| h.labels().is_empty());
    assert_eq!(
        our_menu_rules(&other),
        Vec::<String>::new(),
        "closed menu kept its match"
    );

    h.rt.world
        .borrow_mut()
        .write(h.r, "open", Value::Bool(true));
    h.wait_labels("A");
    assert_eq!(our_menu_rules(&other).len(), 1);
    // The tray goes away with the opener still showing the menu.
    drop(h);
    assert_eq!(
        our_menu_rules(&other),
        Vec::<String>::new(),
        "the dropped tray's menu match stayed on the bus"
    );
}

/// The bus daemon's process (the bus driver's own connection).
fn daemon_pid(other: &zbus::blocking::Connection) -> libc::pid_t {
    let pid: u32 = other
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetConnectionUnixProcessID",
            &("org.freedesktop.DBus",),
        )
        .unwrap()
        .body()
        .deserialize()
        .unwrap();
    pid as libc::pid_t
}

fn signal(pid: libc::pid_t, sig: libc::c_int) {
    // SAFETY: kill(2) takes any pid and signal number; it touches no memory of ours.
    assert_eq!(unsafe { libc::kill(pid, sig) }, 0);
}

/// Opening a menu while the bus daemon answers nothing (stopped) never blocks the main loop:
/// its match (AddMatch, a round trip) is made off it, and the menu loads once the bus is back.
#[test]
fn opening_a_menu_never_waits_on_the_bus() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::opening_a_menu_never_waits_on_the_bus"
    )) else {
        return;
    };
    let other = zbus::blocking::Connection::session().unwrap();
    let mut h = Host::new();
    let _app = App::new(&["A"], false);
    h.wait_labels("A");
    h.rt.world
        .borrow_mut()
        .write(h.r, "open", Value::Bool(false));
    h.until("menu closed", |h| h.labels().is_empty());
    assert_eq!(our_menu_rules(&other), Vec::<String>::new());

    let daemon = daemon_pid(&other);
    signal(daemon, libc::SIGSTOP);
    h.rt.world
        .borrow_mut()
        .write(h.r, "open", Value::Bool(true));
    // Hangs here (nextest's slow-timeout ends it) if the main loop waits for the bus.
    eprintln!("polling with the bus stopped");
    h.turn();
    assert_eq!(h.labels(), "", "the menu loaded with the bus stopped");
    signal(daemon, libc::SIGCONT);
    h.wait_labels("A");
    assert_eq!(our_menu_rules(&other).len(), 1);
    let errs = h.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// A menu served under a well-known name (as KDE apps register their items) leaves no
/// `NameOwnerChanged` match naming it on the bus once its last opener has gone.
#[test]
fn well_known_menu_owner_match_ends_with_the_menu() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::well_known_menu_owner_match_ends_with_the_menu"
    )) else {
        return;
    };
    const NAME: &str = "org.kde.StatusNotifierItem-4242-1";
    let other = zbus::blocking::Connection::session().unwrap();
    let mut h = Host::new();
    let state: Shared = Arc::new(Mutex::new(State {
        labels: vec!["A".into()],
        ..Default::default()
    }));
    let conn = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at("/StatusNotifierItem", Item)
        .unwrap()
        .serve_at("/MenuBar", Menu(state.clone()))
        .unwrap()
        .name(NAME)
        .unwrap()
        .build()
        .unwrap();
    conn.call_method(
        Some("org.kde.StatusNotifierWatcher"),
        "/StatusNotifierWatcher",
        Some("org.kde.StatusNotifierWatcher"),
        "RegisterStatusNotifierItem",
        &(NAME,),
    )
    .unwrap();
    h.wait_labels("A");
    h.rt.world
        .borrow_mut()
        .write(h.r, "open", Value::Bool(false));
    h.until("menu closed", |h| h.labels().is_empty());
    // The menu's match ends on its lane, off the main loop.
    workpool::wait_idle();
    assert!(qml::dbus::sync(qml::dbus::Bus::Session));
    let me = qml::dbus::session()
        .unwrap()
        .unique_name()
        .unwrap()
        .to_string();
    let reply = other
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Debug.Stats"),
            "GetAllMatchRules",
            &(),
        )
        .unwrap();
    let all: HashMap<String, Vec<String>> = reply.body().deserialize().unwrap();
    let left: Vec<String> = all
        .get(&me)
        .map(|rs| rs.iter().filter(|r| r.contains(NAME)).cloned().collect())
        .unwrap_or_default();
    assert_eq!(
        left,
        Vec::<String>::new(),
        "rules naming the closed menu's app"
    );
}

/// A menu closed and opened again while its first fetch is still out: that fetch goes with the
/// menu (Quickshell deletes the `DBusMenu`, its pending calls with it), so the app hears of no
/// menu that never loaded, and the reopened one loads and is `opened` once. Closed and opened
/// again once loaded, it gets `closed` and `opened` in that order.
#[test]
fn menu_events_keep_their_order_across_a_fetch() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::menu_events_keep_their_order_across_a_fetch"
    )) else {
        return;
    };
    let mut h = Host::new();
    let (started_tx, started) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let app = App::with(State {
        labels: vec!["A".into()],
        gate: Some((started_tx, release_rx)),
        ping: Some(Arc::new(h.wake.ping())),
        ..Default::default()
    });
    h.until("the first fetch in GetLayout", |_| {
        started.try_recv().is_ok()
    });

    h.open(false);
    h.open(true);
    drop(release);
    h.wait_labels("A");
    h.until("the opened event", |_| !app.events().is_empty());
    // Every menu call queued so far has been made.
    workpool::wait_idle();
    assert_eq!(
        app.events(),
        ["Event 0 opened"],
        "{:?}",
        app.state.lock().unwrap().calls
    );
    assert_eq!(app.count("AboutToShow"), 2, "one per open");

    h.open(false);
    h.open(true);
    h.until("three menu events", |_| app.events().len() >= 3);
    assert_eq!(
        app.events(),
        ["Event 0 opened", "Event 0 closed", "Event 0 opened"],
        "{:?}",
        app.state.lock().unwrap().calls
    );
    h.wait_labels("A");
    let errs = h.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// The `QsMenuHandle` of an item's menu goes with the item.
#[test]
fn a_gone_items_menu_handle_goes_with_it() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::a_gone_items_menu_handle_goes_with_it"
    )) else {
        return;
    };
    let mut h = Host::new();
    let handles = |h: &mut Host| h.rt.world.borrow_mut().of_type("QsMenuHandle").len();
    let app = App::new(&["A"], false);
    h.wait_labels("A");
    assert_eq!(handles(&mut h), 1);
    drop(app);
    h.until("the item gone", |h| {
        h.rt.world.borrow_mut().of_type("SystemTrayItem").is_empty()
    });
    assert_eq!(handles(&mut h), 0, "the gone item's menu handle stayed");
    let errs = h.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// A menu whose `GetLayout` is answered only after another app has been listed meanwhile is
/// still filled: the fetch waits for the answer (the reply limit, 25 s, is far off), and the
/// test's gate alone ends the hold.
#[test]
fn a_menu_answering_late_is_filled() {
    let Some(_bus) = bus(concat!(module_path!(), "::a_menu_answering_late_is_filled")) else {
        return;
    };
    let mut h = Host::new();
    let gate = sni_watcher::Gate::default();
    let (tx, entered) = std::sync::mpsc::channel();
    let ping = h.wake.ping();
    let app = App::with(State {
        labels: vec!["A".into(), "B".into()],
        hold: Some((
            gate.clone(),
            Box::new(move || {
                let _ = tx.send(());
                ping();
            }),
        )),
        ..Default::default()
    });
    h.until("the menu's GetLayout held", |_| entered.try_recv().is_ok());
    // The marker: an app registered after the held menu was asked for is listed meanwhile.
    let _marker = App::new(&["X"], false);
    h.until("the marker app listed", |h| {
        h.call("count").to_number() == 2.0
    });
    assert_eq!(h.labels(), "", "a menu was shown before its layout came");

    gate.open();
    h.wait_labels("A,B");
    assert_eq!(app.count("AboutToShow"), 1);
    let errs = h.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// The opener's menu leaves and comes back while its first `GetLayout` is still out (the app
/// changed the layout meanwhile): the first reply, for the same key but an earlier fetch, is
/// dropped and the second one, sent after the change, is shown.
#[test]
fn a_reply_for_an_earlier_fetch_of_the_same_menu_is_dropped() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::a_reply_for_an_earlier_fetch_of_the_same_menu_is_dropped"
    )) else {
        return;
    };
    let mut h = Host::new();
    let (started_tx, started) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let app = App::with(State {
        labels: vec!["A".into()],
        gate: Some((started_tx, release_rx)),
        ping: Some(Arc::new(h.wake.ping())),
        ..Default::default()
    });
    h.until("the first fetch in GetLayout", |_| {
        started.try_recv().is_ok()
    });
    // The first GetLayout answers one entry: the app grows its menu after it was asked.
    app.relabel(&["A", "B"]);
    for open in [false, true] {
        h.rt.world
            .borrow_mut()
            .write(h.r, "open", Value::Bool(open));
        h.turn();
    }
    drop(release);
    // Both fetches answered and handed to the loop.
    workpool::wait_idle();
    h.turn();
    assert_eq!(
        app.count("GetLayout"),
        2,
        "{:?}",
        app.state.lock().unwrap().calls
    );
    assert_eq!(h.labels(), "A,B", "the earlier fetch's layout is shown");
    let errs = h.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// A menu whose layout never loaded was never `opened` for the app, so closing it sends no
/// `closed`: com.canonical.dbusmenu's events come in pairs.
#[test]
fn a_menu_that_never_opened_is_not_closed() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::a_menu_that_never_opened_is_not_closed"
    )) else {
        return;
    };
    let mut h = Host::new();
    let app = App::with(State {
        labels: vec!["A".into()],
        fail_layout: true,
        ping: Some(Arc::new(h.wake.ping())),
        ..Default::default()
    });
    h.until("the failed GetLayout", |_| app.count("GetLayout") == 1);
    // The fetch's job is done: its outcome reached the loop.
    workpool::wait_idle();
    h.turn();
    h.rt.world
        .borrow_mut()
        .write(h.r, "open", Value::Bool(false));
    h.turn();
    // Every menu call the close queued has been made.
    workpool::wait_idle();
    let calls = app.state.lock().unwrap().calls.clone();
    assert!(
        !calls.iter().any(|c| c.starts_with("Event")),
        "events without a loaded menu: {calls:?}"
    );
    let errs = h.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// An app that never answers `Event` (it returns nothing, and some apps do not reply): its
/// menu is shown all the same, and its later events (`closed`, the next `opened`) go out behind
/// the unanswered one, as Quickshell's do, whose `Event` is a pending call nothing waits for.
#[test]
fn a_menu_shows_without_an_answer_to_its_opened_event() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::a_menu_shows_without_an_answer_to_its_opened_event"
    )) else {
        return;
    };
    let mut h = Host::new();
    let app = App::with(State {
        labels: vec!["A".into()],
        mute: vec!["Event"],
        ping: Some(Arc::new(h.wake.ping())),
        ..Default::default()
    });
    h.wait_labels("A");
    h.until("the opened event", |_| app.events().len() == 1);
    // The menu's queue is not held by the event: everything on it has been done.
    workpool::wait_idle();

    h.open(false);
    h.until("menu closed", |h| h.labels().is_empty());
    h.until("the closed event", |_| app.events().len() == 2);
    h.open(true);
    h.wait_labels("A");
    h.until("the second opened event", |_| app.events().len() == 3);
    assert_eq!(
        app.events(),
        ["Event 0 opened", "Event 0 closed", "Event 0 opened"],
        "{:?}",
        app.state.lock().unwrap().calls
    );
    workpool::wait_idle();
    let errs = h.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// An app that never answers a menu call (`AboutToShow`, `GetLayout` or `GetGroupProperties`,
/// each in turn): the main loop goes on, the call ends with the menu's opener (its queue is
/// free: the next open asks again), no event is sent for a menu that never loaded, and the menu
/// is filled once the app answers. A call still out when the tray goes ends with it.
#[test]
fn a_menu_whose_app_never_answers_holds_nothing() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::a_menu_whose_app_never_answers_holds_nothing"
    )) else {
        return;
    };
    let mut h = Host::new();
    h.open(false);
    let app = App::with(State {
        labels: vec!["A".into()],
        ping: Some(Arc::new(h.wake.ping())),
        ..Default::default()
    });
    h.until("the app listed", |h| h.call("count").to_number() == 1.0);
    for (n, method) in ["AboutToShow", "GetLayout", "GetGroupProperties"]
        .into_iter()
        .enumerate()
    {
        app.state.lock().unwrap().mute = vec![method];
        h.open(true);
        h.until(&format!("the unanswered {method}"), |_| {
            let s = app.state.lock().unwrap();
            s.calls.iter().filter(|c| c.ends_with("unanswered")).count() == n + 1
        });
        assert_eq!(h.labels(), "", "a menu was shown without its {method}");
        h.open(false);
        // Hangs here (nextest's slow-timeout ends it) while the unanswered call holds the
        // menu's queue: the queue's thread ends only when the queue is empty.
        eprintln!("waiting for the menu's queue to empty after {method}");
        workpool::wait_idle();
        // One `AboutToShow` per open so far: each open asked the app again.
        assert_eq!(app.count("AboutToShow"), n + 1);
    }
    assert_eq!(
        app.events(),
        Vec::<String>::new(),
        "events of a menu that never loaded"
    );

    app.state.lock().unwrap().mute.clear();
    h.open(true);
    h.wait_labels("A");
    h.until("the opened event", |_| app.events() == ["Event 0 opened"]);
    h.open(false);
    h.until("the closed event", |_| app.events().len() == 2);
    let errs = h.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");

    app.state.lock().unwrap().mute = vec!["AboutToShow"];
    h.open(true);
    h.until("the last unanswered AboutToShow", |_| {
        app.count("AboutToShow") == 5
    });
    drop(h);
    eprintln!("waiting for the menu's queue to empty after the tray");
    workpool::wait_idle();
}

/// The system refuses the menu's queue its thread (the limit on threads is reached): the menu
/// is not asked for on the main loop's thread instead, where an app that does not answer would
/// stop the whole shell. Nothing is called and nothing is left queued; the loop goes on.
#[test]
fn a_menu_refused_its_thread_is_not_fetched_on_the_main_loop() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::a_menu_refused_its_thread_is_not_fetched_on_the_main_loop"
    )) else {
        return;
    };
    let mut h = Host::new();
    h.open(false);
    let app = App::new(&["A"], false);
    h.until("the app listed", |h| h.call("count").to_number() == 1.0);
    // The tray's and the bus's threads run already; the main loop's thread gets no new one.
    testenv::refuse_new_threads();
    h.open(true);
    // A second pass asks again, and is refused again.
    h.turn();
    let calls = app.state.lock().unwrap().calls.clone();
    assert_eq!(
        calls,
        Vec::<String>::new(),
        "menu calls made on the main loop's thread"
    );
    assert_eq!(h.labels(), "");
    // Nothing was queued for a thread that does not exist.
    workpool::wait_idle();
    h.open(false);
    workpool::wait_idle();
    assert_eq!(app.state.lock().unwrap().calls, Vec::<String>::new());
    let errs = h.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
