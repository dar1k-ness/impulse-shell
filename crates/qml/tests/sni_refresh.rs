//! SystemTray against fake StatusNotifierItems on a private session `dbus-daemon`: a signal from
//! one item re-reads only that item, and only the properties the signal names (NewStatus
//! carries its value), while registrations still add and remove items.

use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use zbus::object_server::SignalEmitter;

/// Closing it (test process exit) makes the wrapper shell kill the daemon.
struct Bus(#[allow(dead_code)] ChildStdin);

/// In `test`'s own process (see `testenv::run_in_own_process`): a private session bus at the
/// address the environment names, a socket in a directory the parent made, as everything the
/// test runs finds the session bus through the environment. `None` in the process that ran the
/// test there, or without dbus-daemon.
fn bus(test: &str) -> Option<Bus> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(test, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        return None;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let cfg = std::env::temp_dir().join(format!("qml-sni-refresh-{}.conf", std::process::id()));
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

/// The test's host loop: sleeps on the runtime's wakeups and its epoll set between turns, the
/// way the shell's loop does.
struct Loop {
    wake: testenv::Wakeups,
    ep: Option<i32>,
}

impl Loop {
    fn new(rt: &mut Runtime) -> Loop {
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Loop { wake, ep }
    }

    /// Runs loop turns, sleeping on the wakeups between them, until `done`.
    fn until(&self, rt: &mut Runtime, what: &str, mut done: impl FnMut(&mut Runtime) -> bool) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        loop {
            turn(rt);
            let rev = rt.revision();
            if done(rt) {
                return;
            }
            // A call in `done` can create objects (a service singleton) that only the next
            // poll starts: turn again before sleeping.
            if rt.revision() != rev {
                continue;
            }
            if self.wake.wait(self.ep, None).fd {
                rt.process_fd_ready();
            }
        }
    }
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

/// Blocks until the tray's Watcher has an owner.
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

/// A StatusNotifierItem that logs every property it is asked for.
struct Item {
    id: String,
    title: String,
    status: String,
    icon: String,
    reads: Arc<Mutex<Vec<&'static str>>>,
}

impl Item {
    fn read(&self, p: &'static str) {
        self.reads.lock().unwrap().push(p);
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn id(&self) -> String {
        self.read("Id");
        self.id.clone()
    }
    #[zbus(property)]
    fn title(&self) -> String {
        self.read("Title");
        self.title.clone()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        self.read("Status");
        self.status.clone()
    }
    #[zbus(property)]
    fn category(&self) -> String {
        self.read("Category");
        "ApplicationStatus".into()
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        self.read("IconName");
        self.icon.clone()
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        self.read("IconPixmap");
        vec![(1, 1, vec![255, 1, 2, 3])]
    }
    #[zbus(signal)]
    async fn new_title(e: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn new_icon(e: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn new_status(e: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

struct Fake {
    conn: zbus::blocking::Connection,
    reads: Arc<Mutex<Vec<&'static str>>>,
}

impl Fake {
    fn new(id: &str) -> Fake {
        let reads = Arc::new(Mutex::new(vec![]));
        let item = Item {
            id: id.into(),
            title: format!("{id}0"),
            status: "Active".into(),
            icon: format!("{id}-icon"),
            reads: reads.clone(),
        };
        let conn = zbus::blocking::connection::Builder::session()
            .unwrap()
            .serve_at("/StatusNotifierItem", item)
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
        Fake { conn, reads }
    }

    fn iface(&self) -> zbus::blocking::object_server::InterfaceRef<Item> {
        self.conn
            .object_server()
            .interface::<_, Item>("/StatusNotifierItem")
            .unwrap()
    }

    fn take_reads(&self) -> Vec<&'static str> {
        std::mem::take(&mut *self.reads.lock().unwrap())
    }
}

/// An item whose `NewStatus` (which carries the status, so the tray reads nothing) flips the
/// tray's view of it: once the flip shows, the tray's fetch loop, which takes signals in bus
/// order and finishes a pass's reads before it publishes, has handled every signal sent before.
struct Marker {
    item: Fake,
    on: bool,
}

impl Marker {
    fn new() -> Marker {
        Marker {
            item: Fake::new("marker"),
            on: true,
        }
    }

    /// Returns once the tray has handled everything signalled before this call.
    fn sync(&mut self, lp: &Loop, rt: &mut Runtime, root: usize) {
        self.on = !self.on;
        let (st, want) = if self.on {
            ("Active", "1")
        } else {
            ("Passive", "0")
        };
        let i = self.item.iface();
        i.get_mut().status = st.into();
        zbus::block_on(Item::new_status(i.signal_emitter(), st)).unwrap();
        lp.until(rt, "the marker's status", |rt| marker(rt, root) == want);
    }
}

const TRAY_QML: &str = "import QtQuick\nimport Rustshell.Services.SystemTray\nItem {\n\
    function dump() { return SystemTray.items.values.filter(i => i.id !== 'marker').map(i => \
    i.id + ':' + i.title + ':' + i.status + ':' + i.icon).sort().join('|') }\n\
    function marker() { const m = SystemTray.items.values.find(i => i.id === 'marker'); \
    return m ? '' + m.status : '' } }";

fn marker(rt: &mut Runtime, root: usize) -> String {
    rt.call_method(root, "marker", vec![])
        .unwrap()
        .unwrap()
        .to_js_string()
}

fn dump(rt: &mut Runtime, root: usize) -> String {
    rt.call_method(root, "dump", vec![])
        .unwrap()
        .unwrap()
        .to_js_string()
}

/// `id:title:status` of every tray item, sorted.
fn rows(rt: &mut Runtime, root: usize) -> Vec<String> {
    dump(rt, root)
        .split('|')
        .filter(|r| !r.is_empty())
        .map(|r| r.splitn(4, ':').take(3).collect::<Vec<_>>().join(":"))
        .collect()
}

#[test]
fn item_signal_rereads_only_that_item_and_property() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::item_signal_rereads_only_that_item_and_property"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt.load_str(TRAY_QML).unwrap();
    rt.settle();
    // Instantiates the SystemTray singleton, which starts the watcher on the next poll.
    assert_eq!(dump(&mut rt, root), "");
    turn(&mut rt);
    watcher_up();
    let mut m = Marker::new();
    let a = Fake::new("a");
    let b = Fake::new("b");
    lp.until(&mut rt, "items a and b", |rt| {
        rows(rt, root) == ["a:a0:1", "b:b0:1"]
    });
    m.sync(&lp, &mut rt, root);
    a.take_reads();
    b.take_reads();

    // NewTitle from A: A's Title alone, nothing of B.
    let ia = a.iface();
    ia.get_mut().title = "a1".into();
    zbus::block_on(Item::new_title(ia.signal_emitter())).unwrap();
    lp.until(&mut rt, "A's new title", |rt| {
        rows(rt, root) == ["a:a1:1", "b:b0:1"]
    });
    m.sync(&lp, &mut rt, root);
    assert_eq!(a.take_reads(), ["Title"]);
    assert_eq!(
        b.take_reads(),
        Vec::<&str>::new(),
        "B re-read on A's signal"
    );

    // NewStatus carries the status: no read at all.
    ia.get_mut().status = "NeedsAttention".into();
    zbus::block_on(Item::new_status(ia.signal_emitter(), "NeedsAttention")).unwrap();
    lp.until(&mut rt, "A's new status", |rt| {
        rows(rt, root) == ["a:a1:2", "b:b0:1"]
    });
    m.sync(&lp, &mut rt, root);
    assert_eq!(a.take_reads(), Vec::<&str>::new());
    assert_eq!(b.take_reads(), Vec::<&str>::new());

    // NewIcon from B: B's icon group only; the visible icon follows.
    let ib = b.iface();
    ib.get_mut().icon = "b-icon2".into();
    ib.get_mut().status = "Passive".into();
    let before = dump(&mut rt, root);
    zbus::block_on(Item::new_icon(ib.signal_emitter())).unwrap();
    lp.until(&mut rt, "B's new icon", |rt| dump(rt, root) != before);
    m.sync(&lp, &mut rt, root);
    let mut got = b.take_reads();
    got.sort();
    assert_eq!(got, ["IconName", "IconPixmap"]);
    assert_eq!(a.take_reads(), Vec::<&str>::new());
    // Status was not re-read on NewIcon.
    assert_eq!(rows(&mut rt, root), ["a:a1:2", "b:b0:1"]);

    // Registration and unregistration still add and remove items, without re-reading others.
    let c = Fake::new("c");
    lp.until(&mut rt, "item c", |rt| rows(rt, root).len() == 3);
    drop(ib);
    drop(b);
    lp.until(&mut rt, "item b gone", |rt| {
        rows(rt, root) == ["a:a1:2", "c:c0:1"]
    });
    m.sync(&lp, &mut rt, root);
    assert_eq!(a.take_reads(), Vec::<&str>::new());
    drop(c);
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// A burst of change signals costs at most two fetches: the one in flight when it began, and
/// one for every signal that arrived meanwhile.
#[test]
fn signal_burst_costs_at_most_two_fetches() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::signal_burst_costs_at_most_two_fetches"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt.load_str(TRAY_QML).unwrap();
    rt.settle();
    assert_eq!(dump(&mut rt, root), "");
    turn(&mut rt);
    watcher_up();
    let mut m = Marker::new();
    let a = Fake::new("burst");
    lp.until(&mut rt, "the item", |rt| {
        rows(rt, root) == ["burst:burst0:1"]
    });
    m.sync(&lp, &mut rt, root);
    a.take_reads();
    let ia = a.iface();
    {
        // The item answers no Get until the whole burst is out.
        let mut it = ia.get_mut();
        it.title = "burst50".into();
        for _ in 0..50 {
            zbus::block_on(Item::new_title(ia.signal_emitter())).unwrap();
        }
    }
    lp.until(&mut rt, "the burst's title", |rt| {
        rows(rt, root) == ["burst:burst50:1"]
    });
    m.sync(&lp, &mut rt, root);
    let n = a.take_reads().iter().filter(|r| **r == "Title").count();
    assert!((1..=2).contains(&n), "{n} Title fetches for a burst of 50");
}
