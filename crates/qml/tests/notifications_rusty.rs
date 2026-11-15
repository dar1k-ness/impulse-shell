//! The tree's Notifications service on a private session bus: discarded `Notif` wrappers and
//! their `NotifTimer`s are destroyed, a timer or `cancelTimeout` never dereferences a missing
//! wrapper or timer, and a `replaces_id` update pops the notification up again with a fresh
//! timeout and rewrites the history file. And `Impulse.Notify`, the shell's own sender, against
//! that service: what `send` puts on the bus and which button `actionInvoked` names. And against
//! a server that leaves one send's call unanswered: the sends after it are not held.
use qml::runtime::Runtime;
use qml::value::Value;
use std::io::BufRead;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

fn ri() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"))
}

const MAIN: &str = "import QtQuick
import ri.services
Item {
    property int count: Notifications.list.length
    function at(i) { return Notifications.list[i] }
    function discard(id) { Notifications.discardNotification(id) }
    function discardAll() { Notifications.discardAllNotifications() }
    function cancel(id) { Notifications.cancelTimeout(id) }
}
";

struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The test's host loop on the runtime's own test clock: the popup timeouts (NotifTimer) run
/// on `now`, which only [`T::advance`] moves, and the loop sleeps on the runtime's wakeups
/// and its epoll set between turns.
struct T {
    rt: Runtime,
    root: usize,
    wake: testenv::Wakeups,
    ep: Option<i32>,
    now: f64,
}

impl T {
    /// One loop turn at `now`, polling again while a poll's own writes changed the world.
    fn step(&mut self) {
        loop {
            let rev = self.rt.revision();
            self.rt.poll_native();
            self.rt.tick(self.now);
            if self.rt.revision() == rev {
                return;
            }
        }
    }
    /// Runs loop turns, sleeping on the wakeups between them, until `f`.
    fn wait(&mut self, what: &str, mut f: impl FnMut(&mut T) -> bool) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        loop {
            self.step();
            let rev = self.rt.revision();
            if f(self) {
                return;
            }
            // A call in `done` can create objects (a service singleton) that only the next
            // poll handles: turn again before sleeping.
            if self.rt.revision() != rev {
                continue;
            }
            if self.wake.wait(self.ep, None).fd {
                self.rt.process_fd_ready();
            }
        }
    }
    /// Moves the test clock on by `ms` and runs a turn there: every timer due by then fires.
    fn advance(&mut self, ms: f64) {
        self.now += ms;
        self.step();
    }
    fn notify(&mut self, replaces: u32, summary: &str, timeout: i32) -> u32 {
        let summary = summary.to_string();
        let ping: Arc<dyn Fn() + Send + Sync> = Arc::new(self.wake.ping());
        // Set before the ping: the thread itself finishes only after it.
        let done = std::sync::atomic::AtomicBool::new(false);
        let out = std::thread::scope(|sc| {
            let h = sc.spawn(|| {
                // The server takes the name on its bus lane, after the loop turn that made it:
                // waits for that NameOwnerChanged, as a client of a starting daemon does.
                // A failed wait shows in the call's own error below.
                let _ = Command::new("gdbus")
                    .args(["wait", "--session", "org.freedesktop.Notifications"])
                    .status();
                let out = Command::new("gdbus")
                    .args([
                        "call",
                        "--session",
                        "--dest",
                        "org.freedesktop.Notifications",
                        "--object-path",
                        "/org/freedesktop/Notifications",
                        "--method",
                        "org.freedesktop.Notifications.Notify",
                        "app",
                        &replaces.to_string(),
                        "",
                        &summary,
                        "body",
                        "[]",
                        "{}",
                        &format!("int32 {timeout}"),
                    ])
                    .output()
                    .unwrap();
                done.store(true, std::sync::atomic::Ordering::SeqCst);
                ping();
                out
            });
            self.wait("Notify", |_| done.load(std::sync::atomic::Ordering::SeqCst));
            h.join().unwrap()
        });
        let s = String::from_utf8_lossy(&out.stdout);
        s.trim()
            .trim_start_matches("(uint32 ")
            .trim_end_matches(",)")
            .parse()
            .unwrap_or_else(|_| panic!("Notify: {s} {}", String::from_utf8_lossy(&out.stderr)))
    }
    fn get(&mut self, i: usize, p: &str) -> Value {
        self.rt.get(i, p)
    }
    fn count(&mut self) -> usize {
        let r = self.root;
        self.get(r, "count").to_number() as usize
    }
    fn call(&mut self, f: &str, a: Vec<Value>) -> Value {
        let v = self.rt.call_method(self.root, f, a).unwrap().unwrap();
        self.step();
        v
    }
    /// The newest wrapper, once there are `n`.
    fn last(&mut self, n: usize) -> usize {
        self.wait("wrapper", |t| t.count() == n);
        match self.call("at", vec![Value::Num((n - 1) as f64)]) {
            Value::Item(i) => i,
            v => panic!("no wrapper: {v:?}"),
        }
    }
    fn timer(&mut self, w: usize) -> Option<usize> {
        match self.get(w, "timer") {
            Value::Item(i) => Some(i),
            _ => None,
        }
    }
    fn dead(&self, i: usize) -> bool {
        let w = self.rt.world.borrow();
        w.dead.contains(&i)
    }
    fn type_errors(&self) -> Vec<String> {
        let mut e = self.rt.errors();
        e.retain(|e| e.contains("TypeError"));
        e
    }
}

/// Everything finds its home, config, state, cache and session bus through the environment:
/// a test runs in a process of its own with them set there. `false` in the process that
/// only started that one.
fn in_own_home(test: &str) -> bool {
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let vars: [(&str, std::ffi::OsString); 5] = [
            ("HOME", h.into()),
            ("XDG_CONFIG_HOME", h.join("config").into()),
            ("XDG_STATE_HOME", h.join("state").into()),
            ("XDG_CACHE_HOME", h.join("cache").into()),
            (
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", h.join("bus").display()).into(),
            ),
        ];
        testenv::run_in_own_process(test, vars);
        return false;
    }
    true
}

/// The session bus of [`in_own_home`]'s environment and the shell's services with `main` as
/// the document, after `before` prepared the home.
fn start(main: &str, before: impl FnOnce(&std::path::Path)) -> (T, Daemon) {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let dir = home.join("qml");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(home.join("config/impulse-shell")).unwrap();
    std::fs::write(home.join("config/impulse-shell/config.json"), "{}").unwrap();
    std::fs::write(dir.join("main.qml"), main).unwrap();
    let mut d = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .arg(format!(
            "--address={}",
            std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap()
        ))
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut addr = String::new();
    std::io::BufReader::new(d.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    before(&home);
    let mut rt = Runtime::new(Some(ri()));
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let dirs = ["", "modules/common", "modules/common/functions", "services"].map(|d| ri().join(d));
    let dirs: Vec<PathBuf> = dirs.into_iter().collect();
    rt.load_singletons_lazy(&dirs);
    rt.set_screens(&[("Virtual-1".into(), 0, 0, 1920, 1080)]);
    let root = rt.load_file(&dir.join("main.qml")).unwrap();
    rt.settle();
    let t = T {
        rt,
        root,
        wake,
        ep,
        now: 0.0,
    };
    (t, Daemon(d))
}

#[test]
fn wrapper_lifetime_and_replace() {
    if !testenv::require("dbus-daemon") || !testenv::require("gdbus") {
        return;
    }
    if !in_own_home(concat!(module_path!(), "::wrapper_lifetime_and_replace")) {
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let history = home.join("cache/rustshell/notifications/notifications.json");
    // A notification restored from an earlier run's history, with its saved image copy.
    let png = home.join("cache/impulse-shell/notification-images/0-7-1.png");
    std::fs::create_dir_all(png.parent().unwrap()).unwrap();
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    let (mut t, _d) = start(MAIN, |_| {
        std::fs::write(&png, b"png").unwrap();
        std::fs::write(
            &history,
            r#"[{"notificationId":1,"appIcon":"","appName":"app","body":"","image":"image://qsimage/0-7/1","summary":"restored","time":0,"urgency":"normal"}]"#,
        )
        .unwrap();
    });

    // Discarding it from the history deletes the copy: no server notification is left to.
    let w = t.last(1);
    let id = t.get(w, "notificationId");
    t.call("discard", vec![id]);
    assert_eq!(t.count(), 0);
    qml::notifications::wait_disk_idle();
    assert!(!png.exists(), "saved copy deleted");

    // No timer (expireTimeout 0): cancelTimeout is a no-op; discarding destroys the wrapper.
    t.notify(0, "persistent", 0);
    let w = t.last(1);
    assert!(t.get(w, "popup").truthy());
    assert_eq!(t.timer(w), None);
    let id = t.get(w, "notificationId");
    t.call("cancel", vec![id.clone()]);
    t.call("discard", vec![id]);
    assert_eq!(t.count(), 0);
    assert!(t.dead(w), "a discarded wrapper is destroyed");

    // A wrapper discarded while its popup counts down takes its timer with it.
    t.notify(0, "short", 400);
    let w = t.last(1);
    let timer = t.timer(w).expect("popup timer");
    let id = t.get(w, "notificationId");
    t.call("discard", vec![id]);
    assert!(
        t.dead(w) && t.dead(timer),
        "wrapper and timer are destroyed"
    );
    // Past its timeout: nothing fires for the destroyed timer.
    t.advance(700.0);

    // A timeout hides the popup and the spent timer goes away; a replacement pops the same
    // wrapper up again with the new contents, a fresh timeout and an updated history file.
    let n = t.notify(0, "first", 300);
    let w = t.last(1);
    let timer = t.timer(w).expect("popup timer");
    assert!(t.get(w, "popup").truthy());
    t.advance(300.0);
    t.wait("timeout", |t| !t.get(w, "popup").truthy());
    t.step();
    assert!(t.dead(timer), "a fired timer is destroyed");
    assert_eq!(t.timer(w), None);
    assert_eq!(t.notify(n, "second", 300), n);
    t.wait("replaced", |t| {
        t.get(w, "summary").to_js_string() == "second"
    });
    assert_eq!(t.count(), 1);
    assert!(t.get(w, "popup").truthy(), "a replacement pops up again");
    let timer = t.timer(w).expect("a replacement gets a fresh timeout");
    assert!(!t.dead(timer));
    t.wait("history", |_| {
        std::fs::read_to_string(&history).is_ok_and(|s| s.contains("\"second\""))
    });
    let saved = std::fs::read_to_string(&history).unwrap();
    assert!(
        !saved.contains("\"first\""),
        "history kept the old summary: {saved}"
    );
    t.advance(300.0);
    t.wait("replacement timeout", |t| !t.get(w, "popup").truthy());

    // discardAllNotifications destroys every wrapper and timer.
    t.notify(0, "x", 5000);
    t.notify(0, "y", 0);
    let a = t.last(3);
    let x = match t.call("at", vec![Value::Num(1.0)]) {
        Value::Item(i) => i,
        v => panic!("{v:?}"),
    };
    let xt = t.timer(x).expect("popup timer");
    t.call("discardAll", vec![]);
    assert_eq!(t.count(), 0);
    assert!(t.dead(w) && t.dead(x) && t.dead(xt) && t.dead(a));

    assert_eq!(t.type_errors(), Vec::<String>::new());
}

const SENDER: &str = "import QtQuick
import Impulse
import ri.services
Item {
    property int count: Notifications.list.length
    property var pressed: []
    function at(i) { return Notifications.list[i] }
    function send(summary, body, options) { return Notify.send(summary, body, options) }
    function plain(summary, body) { return Notify.send(summary, body) }
    function press(i, k) { Notifications.list[i].notification.actions[k].invoke() }
    Connections {
        target: Notify
        function onActionInvoked(id, index) { pressed = pressed.concat([id + \":\" + index]) }
    }
}
";

/// `Notify.send`: the notification is on the bus when the call returns, with the app name,
/// urgency and transient hint asked for, and a pressed action button comes back as
/// `actionInvoked(id, index)` with the id `send` returned and the button's index from 0.
#[test]
fn notify_send_and_action_invoked() {
    if !testenv::require("dbus-daemon") || !testenv::require("gdbus") {
        return;
    }
    if !in_own_home(concat!(module_path!(), "::notify_send_and_action_invoked")) {
        return;
    }
    let (mut t, _d) = start(SENDER, |_| {});
    let text = |t: &mut T, w: usize, p: &str| t.get(w, p).to_js_string();
    let opts = |pairs: &[(&str, Value)]| {
        let o = qml::eval::new_obj();
        for (k, v) in pairs {
            qml::eval::set_own(&o, k, v.clone());
        }
        Value::Obj(o)
    };
    // The server exists before the first send: a client of a starting daemon waits for it.
    t.notify(0, "first", 0);
    let _ = t.last(1);

    let id = t.call(
        "plain",
        vec![Value::str("Weather"), Value::str("Refreshing")],
    );
    assert_eq!(id.to_number(), 1.0);
    let w = t.last(2);
    assert_eq!(text(&mut t, w, "appName"), "Shell");
    assert_eq!(text(&mut t, w, "summary"), "Weather");
    assert_eq!(text(&mut t, w, "body"), "Refreshing");
    // NotificationUrgency: Low 0, Normal 1, Critical 2.
    assert_eq!(text(&mut t, w, "urgency"), "1");
    assert!(!t.get(w, "isTransient").truthy());

    let o = opts(&[
        ("critical", Value::Bool(true)),
        ("transient", Value::Bool(true)),
        ("appName", Value::str("Files")),
    ]);
    let id = t.call(
        "send",
        vec![Value::str("Low battery"), Value::str("-x <b>y</b>\\n"), o],
    );
    assert_eq!(id.to_number(), 2.0);
    let w = t.last(3);
    assert_eq!(text(&mut t, w, "appName"), "Files");
    assert_eq!(text(&mut t, w, "summary"), "Low battery");
    assert_eq!(
        text(&mut t, w, "body"),
        "-x <b>y</b>\\n",
        "the body goes out as given"
    );
    assert_eq!(text(&mut t, w, "urgency"), "2");
    assert!(t.get(w, "isTransient").truthy());

    let labels = Value::array(vec![Value::str("Shazam"), Value::str("YouTube")]);
    let o = opts(&[("actions", labels)]);
    let id = t.call("send", vec![Value::str("Song"), Value::str("Artist"), o]);
    assert_eq!(id.to_number(), 3.0);
    let w = t.last(4);
    assert_eq!(text(&mut t, w, "summary"), "Song");
    let n = match t.get(w, "notification") {
        Value::Item(i) => i,
        v => panic!("{v:?}"),
    };
    let shown = t.get(n, "actions");
    let label = |t: &mut T, k: &str| {
        let a = t.rt.it.get(&shown, k).unwrap();
        t.rt.it.get(&a, "text").unwrap().to_js_string()
    };
    assert_eq!(
        [label(&mut t, "0"), label(&mut t, "1")],
        ["Shazam", "YouTube"]
    );
    t.call("press", vec![Value::Num(3.0), Value::Num(1.0)]);
    let root = t.root;
    t.wait("actionInvoked", |t| {
        t.get(root, "pressed").to_js_string() == "3:1"
    });

    assert_eq!(t.type_errors(), Vec::<String>::new());
}

/// Sends with buttons that nobody dismisses hold no slot of `workpool::long`: with as many of
/// them shown as the pool has slots, an unrelated long job (a download, a screenshot, a
/// program launch) still runs, and the button of any of them is still reported. A job that
/// stayed queued behind them would never answer, and nextest's slow-timeout ends the test.
#[test]
fn undismissed_sends_with_buttons_leave_the_long_pool_free() {
    if !testenv::require("dbus-daemon") || !testenv::require("gdbus") {
        return;
    }
    if !in_own_home(concat!(
        module_path!(),
        "::undismissed_sends_with_buttons_leave_the_long_pool_free"
    )) {
        return;
    }
    let (mut t, _d) = start(SENDER, |_| {});
    // The server exists before the first send: a client of a starting daemon waits for it.
    t.notify(0, "first", 0);
    let _ = t.last(1);

    let sends = workpool::LONG_MAX;
    for k in 1..=sends {
        let o = qml::eval::new_obj();
        let labels = Value::array(vec![Value::str("Shazam"), Value::str("YouTube")]);
        qml::eval::set_own(&o, "actions", labels);
        let id = t.call(
            "send",
            vec![
                Value::str("Music Recognized"),
                Value::str("Song"),
                Value::Obj(o),
            ],
        );
        assert_eq!(id.to_number(), k as f64);
        // Shown: the server has answered this send's `Notify`, and all that is left of it is
        // the wait for a button or the close.
        let _ = t.last(1 + k);
    }

    let (ran, done) = std::sync::mpsc::channel();
    workpool::long(move || {
        let _ = ran.send(());
    });
    eprintln!("waiting for an unrelated long job");
    done.recv().unwrap();

    // The last and the first of them: `list[0]` is the gdbus one.
    let root = t.root;
    t.call("press", vec![Value::Num(sends as f64), Value::Num(1.0)]);
    let last = format!("{sends}:1");
    t.wait("actionInvoked of the last send", |t| {
        t.get(root, "pressed").to_js_string() == last
    });
    t.call("press", vec![Value::Num(1.0), Value::Num(0.0)]);
    let both = format!("{sends}:1,1:0");
    t.wait("actionInvoked of the first send", |t| {
        t.get(root, "pressed").to_js_string() == both
    });
    assert_eq!(t.type_errors(), Vec::<String>::new());
}

const BUTTONS: &str = "import QtQuick
import Impulse
Item {
    property var pressed: []
    function send(summary) { return Notify.send(summary, \"\", { actions: [\"Open\", \"Later\"] }) }
    Connections {
        target: Notify
        function onActionInvoked(id, index) { pressed = pressed.concat([id + \":\" + index]) }
    }
}
";

const NOTIFICATIONS: &str = "org.freedesktop.Notifications";

/// The next method call the notification server `calls` reads for receives, which is `member`.
fn called(calls: &mut zbus::blocking::MessageIterator, member: &str) -> zbus::Message {
    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for the server to be called {member}");
    let m = calls
        .find_map(|m| {
            m.ok()
                .filter(|m| m.message_type() == zbus::message::Type::MethodCall)
        })
        .expect("the server's connection closed");
    assert_eq!(m.header().member().unwrap().as_str(), member);
    m
}

/// The arguments of a `Notify` call.
type NotifyArgs = (
    String,
    u32,
    String,
    String,
    String,
    Vec<String>,
    std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    i32,
);

/// The summary of the `Notify` call `m`.
fn summary_of(m: &zbus::Message) -> String {
    m.body().deserialize::<NotifyArgs>().unwrap().3
}

/// A notification server leaves a call of the first send with buttons, `held`, unanswered,
/// as one that hangs does. Each `notify-send -A` the original ran was a process of its own, so that
/// held no other notification: the second send is asked about, shown, and its button is
/// reported while the first still waits. When the server answers the first after all, that
/// one is shown and its button reported too. A second send queued behind the first's
/// answer would never reach the server, and nextest's slow-timeout ends the test.
fn a_send_the_server_holds_does_not_hold_the_next(test: &str, held: &str) {
    if !testenv::require("dbus-daemon") {
        return;
    }
    if !in_own_home(test) {
        return;
    }
    let (mut t, _d) = start(BUTTONS, |_| {});
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    // A connection with no object server answers nothing by itself: every reply is this
    // test's, in the order written here.
    let server = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .name(NOTIFICATIONS)
        .unwrap()
        .build()
        .unwrap();
    let mut calls = zbus::blocking::MessageIterator::from(&server);
    let caps = |m: &zbus::Message| server.reply(&m.header(), &vec!["actions"]).unwrap();
    let shown = |m: &zbus::Message, id: u32| server.reply(&m.header(), &id).unwrap();
    let press = |id: u32, key: &str| {
        server
            .emit_signal(
                None::<&str>,
                "/org/freedesktop/Notifications",
                NOTIFICATIONS,
                "ActionInvoked",
                &(id, key),
            )
            .unwrap()
    };
    let closed = |calls: &mut zbus::blocking::MessageIterator, id: u32| {
        let m = called(calls, "CloseNotification");
        assert_eq!(m.body().deserialize::<u32>().unwrap(), id);
    };
    let root = t.root;

    assert_eq!(t.call("send", vec![Value::str("first")]).to_number(), 1.0);
    let first = if held == "GetCapabilities" {
        called(&mut calls, "GetCapabilities")
    } else {
        caps(&called(&mut calls, "GetCapabilities"));
        let m = called(&mut calls, "Notify");
        assert_eq!(summary_of(&m), "first");
        m
    };

    // The second send, with the first's call still out.
    assert_eq!(t.call("send", vec![Value::str("second")]).to_number(), 2.0);
    caps(&called(&mut calls, "GetCapabilities"));
    let m = called(&mut calls, "Notify");
    assert_eq!(summary_of(&m), "second");
    shown(&m, 7);
    press(7, "1");
    t.wait("actionInvoked of the second send", |t| {
        t.get(root, "pressed").to_js_string() == "2:1"
    });
    closed(&mut calls, 7);

    // The answer the first send still waits for.
    let m = if held == "GetCapabilities" {
        caps(&first);
        let m = called(&mut calls, "Notify");
        assert_eq!(summary_of(&m), "first");
        m
    } else {
        first
    };
    shown(&m, 5);
    press(5, "0");
    t.wait("actionInvoked of the first send", |t| {
        t.get(root, "pressed").to_js_string() == "2:1,1:0"
    });
    closed(&mut calls, 5);
    assert_eq!(t.type_errors(), Vec::<String>::new());
}

#[test]
fn a_server_silent_on_notify_does_not_hold_the_next_send_with_buttons() {
    a_send_the_server_holds_does_not_hold_the_next(
        concat!(
            module_path!(),
            "::a_server_silent_on_notify_does_not_hold_the_next_send_with_buttons"
        ),
        "Notify",
    );
}

#[test]
fn a_server_silent_on_get_capabilities_does_not_hold_the_next_send_with_buttons() {
    a_send_the_server_holds_does_not_hold_the_next(
        concat!(
            module_path!(),
            "::a_server_silent_on_get_capabilities_does_not_hold_the_next_send_with_buttons"
        ),
        "GetCapabilities",
    );
}

/// A notification server never answers a call of the first send with buttons, `held`.
/// `notify-send -A` gave each of its two calls GDBus's 25 s: with no answer to
/// `GetCapabilities` it showed the notification without buttons, with none to `Notify` it
/// said "Failed to show notification" and never reported a button. So here: nothing ends
/// before the limit; at it the call is given up (the test names the moment, it does not wait
/// for it); a `Notify` given up is forgotten, so the server's late answer and a press of the
/// id it brings are nobody's; and the sends before and after are shown and report theirs.
/// A press signalled by a program that is not the server is nobody's either.
fn a_call_the_server_never_answers_ends_at_the_reply_limit(test: &str, held: &str) {
    use std::time::{Duration, Instant};
    if !testenv::require("dbus-daemon") {
        return;
    }
    if !in_own_home(test) {
        return;
    }
    let (mut t, _d) = start(BUTTONS, |_| {});
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let server = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .name(NOTIFICATIONS)
        .unwrap()
        .build()
        .unwrap();
    let mut calls = zbus::blocking::MessageIterator::from(&server);
    let caps = |m: &zbus::Message| server.reply(&m.header(), &vec!["actions"]).unwrap();
    let shown = |m: &zbus::Message, id: u32| server.reply(&m.header(), &id).unwrap();
    let press = |id: u32, key: &str| {
        server
            .emit_signal(
                None::<&str>,
                "/org/freedesktop/Notifications",
                NOTIFICATIONS,
                "ActionInvoked",
                &(id, key),
            )
            .unwrap()
    };
    let closed = |calls: &mut zbus::blocking::MessageIterator, id: u32| {
        let m = called(calls, "CloseNotification");
        assert_eq!(m.body().deserialize::<u32>().unwrap(), id);
    };
    let root = t.root;
    let limit = qml::notifier::REPLY_LIMIT;
    assert_eq!(limit, Duration::from_secs(25), "GDBus's default");

    // Every call of this test is written after `before`.
    let before = Instant::now();
    assert_eq!(t.call("send", vec![Value::str("first")]).to_number(), 1.0);
    let first = if held == "GetCapabilities" {
        called(&mut calls, "GetCapabilities")
    } else {
        caps(&called(&mut calls, "GetCapabilities"));
        let m = called(&mut calls, "Notify");
        assert_eq!(summary_of(&m), "first");
        m
    };
    assert_eq!(t.call("send", vec![Value::str("second")]).to_number(), 2.0);
    caps(&called(&mut calls, "GetCapabilities"));
    let m = called(&mut calls, "Notify");
    assert_eq!(summary_of(&m), "second");
    shown(&m, 7);
    press(7, "1");
    t.wait("actionInvoked of the second send", |t| {
        t.get(root, "pressed").to_js_string() == "2:1"
    });
    closed(&mut calls, 7);

    // Just short of the limit of the first call written, the held call is still waited for;
    // a limit after every call was written, it is the one call that ends.
    let conn = qml::dbus::session().expect("the shell's session bus");
    let short = before + limit - Duration::from_millis(1);
    assert_eq!(qml::dbus::limits_reached_at(&conn, short), 0);
    assert_eq!(
        qml::dbus::limits_reached_at(&conn, Instant::now() + limit),
        1
    );

    if held == "GetCapabilities" {
        // Shown without buttons, and no answer is asked for.
        let m = called(&mut calls, "Notify");
        assert_eq!(summary_of(&m), "first");
        let flags = m.primary_header().flags();
        assert!(flags.contains(zbus::message::Flags::NoReplyExpected));
        let actions = m.body().deserialize::<NotifyArgs>().unwrap().5;
        assert_eq!(actions, Vec::<String>::new());
    } else {
        // The server shows it after all: too late for anyone to know its id.
        shown(&first, 5);
        press(5, "0");
    }

    assert_eq!(t.call("send", vec![Value::str("third")]).to_number(), 3.0);
    caps(&called(&mut calls, "GetCapabilities"));
    let m = called(&mut calls, "Notify");
    assert_eq!(summary_of(&m), "third");
    shown(&m, 8);
    // Another program signals a press of that id: only the server's signals count, as for
    // libnotify's proxy. The bus hands it on before it answers the program's next call.
    let stranger = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .build()
        .unwrap();
    let path = "/org/freedesktop/Notifications";
    stranger
        .emit_signal(
            None::<&str>,
            path,
            NOTIFICATIONS,
            "ActionInvoked",
            &(8u32, "1"),
        )
        .unwrap();
    let dbus = "org.freedesktop.DBus";
    stranger
        .call_method(
            Some(dbus),
            "/org/freedesktop/DBus",
            Some(dbus),
            "GetId",
            &(),
        )
        .unwrap();
    press(8, "0");
    t.wait("actionInvoked of the third send", |t| {
        t.get(root, "pressed").to_js_string() == "2:1,3:0"
    });
    // The next call the server gets: the press of 5 closed nothing.
    closed(&mut calls, 8);
    assert_eq!(
        qml::dbus::limits_reached_at(&conn, Instant::now() + limit),
        0
    );
    assert_eq!(t.type_errors(), Vec::<String>::new());
}

#[test]
fn a_notify_the_server_never_answers_is_given_up_at_the_reply_limit() {
    a_call_the_server_never_answers_ends_at_the_reply_limit(
        concat!(
            module_path!(),
            "::a_notify_the_server_never_answers_is_given_up_at_the_reply_limit"
        ),
        "Notify",
    );
}

#[test]
fn a_send_whose_get_capabilities_is_never_answered_is_shown_without_buttons_at_the_reply_limit() {
    a_call_the_server_never_answers_ends_at_the_reply_limit(
        concat!(
            module_path!(),
            "::a_send_whose_get_capabilities_is_never_answered_is_shown_without_buttons_at_the_reply_limit"
        ),
        "GetCapabilities",
    );
}

/// A server may signal a press before it answers the `Notify` that showed the notification:
/// the shell's own server answers one turn after it showed it, as quickshell's does once its
/// `notification` slot has run. The press is reported when the answer brings the id; a press
/// of an id that is nobody's send is not.
#[test]
fn a_press_signalled_before_the_notify_answer_is_reported() {
    if !testenv::require("dbus-daemon") {
        return;
    }
    if !in_own_home(concat!(
        module_path!(),
        "::a_press_signalled_before_the_notify_answer_is_reported"
    )) {
        return;
    }
    let (mut t, _d) = start(BUTTONS, |_| {});
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let server = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .name(NOTIFICATIONS)
        .unwrap()
        .build()
        .unwrap();
    let mut calls = zbus::blocking::MessageIterator::from(&server);
    let press = |id: u32, key: &str| {
        server
            .emit_signal(
                None::<&str>,
                "/org/freedesktop/Notifications",
                NOTIFICATIONS,
                "ActionInvoked",
                &(id, key),
            )
            .unwrap()
    };
    let root = t.root;

    assert_eq!(t.call("send", vec![Value::str("first")]).to_number(), 1.0);
    let m = called(&mut calls, "GetCapabilities");
    server.reply(&m.header(), &vec!["actions"]).unwrap();
    let m = called(&mut calls, "Notify");
    press(9, "0");
    press(7, "1");
    server.reply(&m.header(), &7u32).unwrap();
    t.wait("actionInvoked of the send", |t| {
        t.get(root, "pressed").to_js_string() == "1:1"
    });
    let m = called(&mut calls, "CloseNotification");
    assert_eq!(m.body().deserialize::<u32>().unwrap(), 7);
    assert_eq!(t.type_errors(), Vec::<String>::new());
}

const BOTH: &str = "import QtQuick
import Impulse
Item {
    function plain(summary) { return Notify.send(summary, \"\") }
    function buttons(summary) { return Notify.send(summary, \"\", { actions: [\"Open\"] }) }
}
";

/// A session bus of the test's own and a notification server on it that answers nothing by
/// itself, as in `a_send_the_server_holds_does_not_hold_the_next`.
fn silent_server() -> (Daemon, zbus::blocking::Connection) {
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let mut d = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .arg(format!("--address={addr}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(d.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let server = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .name(NOTIFICATIONS)
        .unwrap()
        .build()
        .unwrap();
    (Daemon(d), server)
}

/// A document of [`BOTH`].
fn both() -> (Runtime, usize) {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(BOTH).unwrap();
    rt.settle();
    (rt, root)
}

/// Calls `method` of a [`both`] document with `summary`: the id the send returned.
fn send_as(rt: &mut Runtime, root: usize, method: &str, summary: &str) -> f64 {
    rt.call_method(root, method, vec![Value::str(summary)])
        .unwrap()
        .unwrap()
        .to_number()
}

/// The system refuses the shell's main thread new threads (the limit on threads is reached)
/// while it sends one notification without buttons and one with, before the session bus is
/// open: neither queue gets its thread. Both sends return with their ids and one line each in
/// the log, and the shell has opened no connection: its main thread sent nothing itself.
/// Neither waits for a later send: the next job any queue of the shell takes in (here one
/// that has nothing to do with notifications, on a thread the system refuses nothing) asks
/// for both threads again, and both sends are on their way before a notification sent after
/// them.
#[test]
fn sends_refused_their_threads_go_out_with_the_next_job_of_any_queue() {
    if !testenv::require("dbus-daemon") {
        return;
    }
    if !in_own_home(concat!(
        module_path!(),
        "::sends_refused_their_threads_go_out_with_the_next_job_of_any_queue"
    )) {
        return;
    }
    let (_d, server) = silent_server();
    let mut calls = zbus::blocking::MessageIterator::from(&server);
    let (sent, refused) = std::sync::mpsc::channel::<()>();
    let (shown, done) = std::sync::mpsc::channel::<()>();
    std::thread::scope(|s| {
        s.spawn(move || {
            let (mut rt, root) = both();
            testenv::refuse_new_threads();
            let log = testenv::stderr_of(|| {
                assert_eq!(send_as(&mut rt, root, "plain", "one"), 1.0);
                assert_eq!(send_as(&mut rt, root, "buttons", "two"), 2.0);
            });
            let lines: Vec<_> = log.lines().collect();
            assert_eq!(lines.len(), 2, "{log}");
            for l in lines {
                assert!(
                    l.starts_with("impulse-shell: cannot start a lane thread: ")
                        && l.ends_with("; 1 queued until one starts"),
                    "{log}"
                );
            }
            sent.send(()).unwrap();
            // The document outlives its sends, as the shell does.
            let _ = done.recv();
        });
        // Dropped when an assertion below fails, which ends the thread above.
        let shown = shown;
        refused.recv().expect("the refused sends");
        assert_eq!(
            qml::dbus::opened(),
            0,
            "a send opened the bus on its caller"
        );

        workpool::run(|| ());
        // Both queues have written what they had when this returns.
        workpool::wait_idle();
        let (mut rt, root) = both();
        assert_eq!(send_as(&mut rt, root, "plain", "three"), 1.0);
        // The bus is open and nothing is queued by now: this one is written here.
        let mut before = Vec::new();
        let mut asked = None;
        loop {
            let m = calls
                .find_map(|m| {
                    m.ok()
                        .filter(|m| m.message_type() == zbus::message::Type::MethodCall)
                })
                .expect("the server's connection closed");
            match m.header().member().unwrap().as_str() {
                "GetCapabilities" => {
                    before.push("asked for two".to_string());
                    asked = Some(m);
                }
                _ if summary_of(&m) == "three" => break,
                _ => before.push(summary_of(&m)),
            }
        }
        before.sort();
        assert_eq!(
            before,
            ["asked for two", "one"],
            "what reached the server before the later notification"
        );
        server
            .reply(&asked.unwrap().header(), &vec!["actions"])
            .unwrap();
        let m = called(&mut calls, "Notify");
        assert_eq!(summary_of(&m), "two");
        server.reply(&m.header(), &1u32).unwrap();
        shown.send(()).unwrap();
    });
}

/// A send with buttons that the system refused its thread, and no job after it: the shell
/// ends. Its exit asks for the thread again and waits until the notification is on the bus,
/// as it does for one without buttons: the `notify-send -A` it replaces outlived the shell.
#[test]
fn a_send_with_buttons_refused_its_thread_goes_out_before_the_exit() {
    if !testenv::require("dbus-daemon") {
        return;
    }
    if !in_own_home(concat!(
        module_path!(),
        "::a_send_with_buttons_refused_its_thread_goes_out_before_the_exit"
    )) {
        return;
    }
    let (_d, server) = silent_server();
    let mut calls = zbus::blocking::MessageIterator::from(&server);
    let (sent, refused) = std::sync::mpsc::channel::<()>();
    let (shown, done) = std::sync::mpsc::channel::<()>();
    std::thread::scope(|s| {
        s.spawn(move || {
            let (mut rt, root) = both();
            testenv::refuse_new_threads();
            assert_eq!(send_as(&mut rt, root, "buttons", "two"), 1.0);
            sent.send(()).unwrap();
            let _ = done.recv();
        });
        // Dropped when an assertion below fails, which ends the thread above.
        let shown = shown;
        refused.recv().expect("the refused send");
        let exit = s.spawn(|| {
            let flushed = qml::notifier::flush();
            // Sent after the flush returned: the server reads it after what the flush waited
            // for.
            let (mut rt, root) = both();
            send_as(&mut rt, root, "plain", "after the flush");
            assert!(qml::notifier::flush(), "the bus answers");
            flushed
        });
        let m = called(&mut calls, "GetCapabilities");
        server.reply(&m.header(), &vec!["actions"]).unwrap();
        let m = called(&mut calls, "Notify");
        assert_eq!(summary_of(&m), "two");
        assert_eq!(summary_of(&called(&mut calls, "Notify")), "after the flush");
        assert!(exit.join().unwrap(), "the server answered within the limit");
        shown.send(()).unwrap();
    });
}

const QUITTER: &str = "import QtQuick
import Impulse
Item {
    Component.onCompleted: {
        Notify.send(\"Bye\", \"sent right before Qt.quit()\", { appName: \"Welcome\" });
        Qt.quit();
    }
}
";

/// What [`quits_right_after_send`] exits with once its QML asked to quit.
const QUIT_SEEN: i32 = 42;

/// The process [`send_then_quit_reaches_the_server`] starts: its document sends a notification
/// and calls `Qt.quit()`, as welcome.qml's `onClosing` does, and the process ends there, with
/// no loop turn after the call. Does nothing as a test of its own.
#[test]
fn quits_right_after_send() {
    if !testenv::in_own_process(concat!(module_path!(), "::quits_right_after_send")) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.qml"), QUITTER).unwrap();
    let mut rt = Runtime::new(Some(ri()));
    rt.load_file(&dir.path().join("main.qml")).unwrap();
    let quit = rt.world.borrow().quit;
    assert_eq!(quit, Some(0), "the document called Qt.quit()");
    // What the shell's exit waits for: this process had not opened the session bus before
    // the send, which therefore went to a thread.
    assert!(qml::notifier::flush(), "the bus answers");
    std::process::exit(QUIT_SEEN);
}

/// A notification sent right before `Qt.quit()` is out of the process when it ends: the sender
/// has exited before the server's loop runs again, and the server still shows it.
#[test]
fn send_then_quit_reaches_the_server() {
    if !testenv::require("dbus-daemon") || !testenv::require("gdbus") {
        return;
    }
    if !in_own_home(concat!(
        module_path!(),
        "::send_then_quit_reaches_the_server"
    )) {
        return;
    }
    let (mut t, _d) = start(MAIN, |_| {});
    // The server exists before the send: a client of a starting daemon waits for it.
    t.notify(0, "first", 0);
    let _ = t.last(1);

    let status = testenv::own_process_command(concat!(module_path!(), "::quits_right_after_send"))
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(QUIT_SEEN), "the sender sent and quit");

    let w = t.last(2);
    assert_eq!(t.get(w, "appName").to_js_string(), "Welcome");
    assert_eq!(t.get(w, "summary").to_js_string(), "Bye");
    assert_eq!(
        t.get(w, "body").to_js_string(),
        "sent right before Qt.quit()"
    );
    assert_eq!(t.type_errors(), Vec::<String>::new());
}
