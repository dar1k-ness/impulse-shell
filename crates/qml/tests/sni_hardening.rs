//! The native StatusNotifierWatcher/host against hostile items on a private session bus:
//! ownerless and excess registrations are refused, an item whose GetAll fails is asked again
//! only once it signals or its name changes owner (not on a timer or on others' signals), and
//! huge icon pixmaps are kept downscaled.

use qml::runtime::Runtime;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use zbus::blocking::Connection;

/// In `test`'s own process (see `testenv::run_in_own_process`): a private session bus at the
/// address the environment names, a socket in a directory the parent made, as everything the
/// test runs finds the session bus through the environment. `None` in the process that ran the
/// test there, or without dbus-daemon.
///
/// Closing the returned stdin (test exit) makes the wrapper shell kill the daemon.
fn bus(test: &str) -> Option<std::process::ChildStdin> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(test, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
        return None;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let cfg = std::env::temp_dir().join(format!("qml-sni-hard-{}.conf", std::process::id()));
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

/// An item with only a 512×512 opaque blue pixmap for an icon.
struct Big;

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Big {
    #[zbus(property)]
    fn id(&self) -> String {
        "big".into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![(512, 512, [255u8, 0, 0, 255].repeat(512 * 512))]
    }
}

/// A healthy item with only an id, a status and an icon name.
struct Named(&'static str);

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

/// An item whose `NewStatus` (which carries the status, so the tray reads nothing) flips the
/// tray's view of it: once the flip shows, the tray's fetch loop, which takes signals in bus
/// order, has handled every signal sent before. A fetch it started for one may still be in
/// flight: each runs as a task of its own.
struct Marker {
    conn: Connection,
    on: bool,
}

struct Flip(&'static str);

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Flip {
    #[zbus(property)]
    fn id(&self) -> String {
        "marker".into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        self.0.into()
    }
}

impl Marker {
    fn new() -> Marker {
        let conn = zbus::blocking::connection::Builder::session()
            .unwrap()
            .serve_at("/StatusNotifierItem", Flip("Active"))
            .unwrap()
            .build()
            .unwrap();
        register(&conn, conn.unique_name().unwrap().as_str()).unwrap();
        Marker { conn, on: true }
    }

    /// Returns once the tray has handled everything signalled before this call.
    fn sync(&mut self, lp: &Loop, rt: &mut Runtime, root: usize) {
        self.on = !self.on;
        let (st, want) = if self.on {
            ("Active", "1")
        } else {
            ("Passive", "0")
        };
        let i = self
            .conn
            .object_server()
            .interface::<_, Flip>("/StatusNotifierItem")
            .unwrap();
        i.get_mut().0 = st;
        self.conn
            .emit_signal(
                None::<&str>,
                "/StatusNotifierItem",
                "org.kde.StatusNotifierItem",
                "NewStatus",
                &(st,),
            )
            .unwrap();
        lp.until(rt, "the marker's status", |rt| status(rt, root) == want);
    }
}

/// A connection answering every GetAll itself: with an error until `healthy` is set, then as
/// item `id`. Returns the number of GetAll calls it got, and a receiver that gets one message
/// per call once it is counted.
fn raw_item(
    c: &Connection,
    id: &'static str,
    healthy: Arc<AtomicBool>,
) -> (Arc<AtomicUsize>, mpsc::Receiver<()>) {
    let n = Arc::new(AtomicUsize::new(0));
    let (asked, calls) = mpsc::channel();
    // Created before the thread: a call arriving while `c` has no stream is dropped.
    let it = zbus::blocking::MessageIterator::from(c);
    let (c, n2) = (c.clone(), n.clone());
    std::thread::spawn(move || {
        for m in it.flatten() {
            let h = m.header();
            if h.message_type() != zbus::message::Type::MethodCall
                || !h.member().is_some_and(|m| m.as_str() == "GetAll")
            {
                continue;
            }
            n2.fetch_add(1, Ordering::SeqCst);
            let _ = asked.send(());
            if healthy.load(Ordering::SeqCst) {
                let icon = format!("{id}-icon");
                let props: HashMap<&str, zbus::zvariant::Value> = HashMap::from([
                    ("Id", id.into()),
                    ("Status", "Active".into()),
                    ("IconName", icon.as_str().into()),
                ]);
                let _ = c.reply(&h, &props);
            } else {
                let _ = c.reply_error(&h, "org.freedesktop.DBus.Error.Failed", &"broken");
            }
        }
    });
    (n, calls)
}

fn register(c: &Connection, service: &str) -> zbus::Result<()> {
    c.call_method(
        Some("org.kde.StatusNotifierWatcher"),
        "/StatusNotifierWatcher",
        Some("org.kde.StatusNotifierWatcher"),
        "RegisterStatusNotifierItem",
        &(service,),
    )
    .map(|_| ())
}

fn registered(c: &Connection) -> Vec<String> {
    let p = zbus::blocking::fdo::PropertiesProxy::builder(c)
        .destination("org.kde.StatusNotifierWatcher")
        .unwrap()
        .path("/StatusNotifierWatcher")
        .unwrap()
        .build()
        .unwrap();
    let v = p
        .get(
            "org.kde.StatusNotifierWatcher".try_into().unwrap(),
            "RegisteredStatusNotifierItems",
        )
        .unwrap();
    Vec::<String>::try_from(v).unwrap()
}

const TRAY_QML: &str = "import QtQuick\nimport Rustshell.Services.SystemTray\nItem {\n\
    function icon(id) { const i = SystemTray.items.values.find(i => i.id === id); return i ? i.icon : '' }\n\
    function status() { const i = SystemTray.items.values.find(i => i.id === 'marker'); return i ? '' + i.status : '' } }";

fn status(rt: &mut Runtime, root: usize) -> String {
    rt.call_method(root, "status", vec![])
        .unwrap()
        .unwrap()
        .to_js_string()
}

fn icon(rt: &mut Runtime, root: usize, id: &str) -> String {
    rt.call_method(root, "icon", vec![qml::value::Value::str(id)])
        .unwrap()
        .unwrap()
        .to_js_string()
}

#[test]
fn hostile_items_are_bounded() {
    let Some(_bus) = bus(concat!(module_path!(), "::hostile_items_are_bounded")) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt.load_str(TRAY_QML).unwrap();
    rt.settle();
    assert_eq!(icon(&mut rt, root, "big"), "");
    // Instantiates the SystemTray singleton, which starts the watcher on that poll.
    turn(&mut rt);
    watcher_up();
    let me = Connection::session().unwrap();
    {
        // Gone again right away: it serves no item and its GetAll would only time out.
        let probe = Connection::session().unwrap();
        register(&probe, "/probe").unwrap();
    }
    let mut m = Marker::new();

    // A 512×512 pixmap is kept as it came, as in Quickshell.
    let big = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at("/StatusNotifierItem", Big)
        .unwrap()
        .build()
        .unwrap();
    register(&big, big.unique_name().unwrap().as_str()).unwrap();
    lp.until(&mut rt, "the big icon", |rt| {
        !icon(rt, root, "big").is_empty()
    });
    let src = icon(&mut rt, root, "big");
    assert!(src.starts_with("image://service.sni/"), "{src}");
    let (w, h, px) = qml::sni::pixmap(&src, 0, 0).unwrap();
    assert_eq!((w, h, px[0]), (512, 512, [0, 0, 255, 255]));

    // A name nobody owns is refused and never listed.
    assert!(register(&me, "org.nobody.Ghost").is_err());
    assert!(
        !registered(&me)
            .iter()
            .any(|k| k.starts_with("org.nobody.Ghost"))
    );

    // One connection registers at most 16 items.
    let spam = Connection::session().unwrap();
    let ok = (0..20)
        .filter(|i| register(&spam, &format!("/spam{i}")).is_ok())
        .count();
    assert_eq!(ok, 16);
    drop(spam);

    // A failing GetAll is asked once, and not again on other items' signals: each of them
    // handled by a fetch pass of its own.
    let healthy = Arc::new(AtomicBool::new(false));
    let broken = Connection::session().unwrap();
    let (n, asked) = raw_item(&broken, "late", healthy.clone());
    register(&broken, broken.unique_name().unwrap().as_str()).unwrap();
    asked.recv().unwrap();
    assert_eq!(n.load(Ordering::SeqCst), 1, "a broken item's GetAll");
    for _ in 0..40 {
        big.emit_signal(
            None::<&str>,
            "/StatusNotifierItem",
            "org.kde.StatusNotifierItem",
            "NewTitle",
            &(),
        )
        .unwrap();
        m.sync(&lp, &mut rt, root);
    }
    let calls = n.load(Ordering::SeqCst);
    assert_eq!(
        calls, 1,
        "GetAll of a broken item {calls} times in 40 passes"
    );
    assert_eq!(icon(&mut rt, root, "late"), "");

    // Its own signal says it is up again: it is fetched then, and shows.
    healthy.store(true, Ordering::SeqCst);
    broken
        .emit_signal(
            None::<&str>,
            "/StatusNotifierItem",
            "org.kde.StatusNotifierItem",
            "NewIcon",
            &(),
        )
        .unwrap();
    lp.until(&mut rt, "the healed item's icon", |rt| {
        icon(rt, root, "late") == "image://icon/late-icon"
    });
    assert_eq!(n.load(Ordering::SeqCst), 2);

    // A well-known name whose owner fails GetAll: a new owner of the name is asked at once.
    let old = Connection::session().unwrap();
    let (old_n, old_asked) = raw_item(&old, "swap", Arc::new(AtomicBool::new(false)));
    old.request_name_with_flags(
        "org.test.Swap",
        zbus::fdo::RequestNameFlags::AllowReplacement.into(),
    )
    .unwrap();
    register(&old, "org.test.Swap").unwrap();
    old_asked.recv().unwrap();
    assert_eq!(old_n.load(Ordering::SeqCst), 1);
    let new = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at("/StatusNotifierItem", Named("swap"))
        .unwrap()
        .build()
        .unwrap();
    new.request_name_with_flags(
        "org.test.Swap",
        zbus::fdo::RequestNameFlags::ReplaceExisting.into(),
    )
    .unwrap();
    lp.until(&mut rt, "the item under its new owner", |rt| {
        icon(rt, root, "swap") == "image://icon/swap-icon"
    });
    assert_eq!(old_n.load(Ordering::SeqCst), 1);
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
