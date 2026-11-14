//! `Rustshell.Services.Notifications` against a private `dbus-daemon --session`, driven by
//! real clients (`notify-send`, `gdbus`) and a zbus client for signals/floods.
use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, MessageIterator};

const QML: &str = r#"import QtQuick
import Rustshell
import Rustshell.Services.Notifications
Item {
    id: root
    property string log: ""
    property bool track: true
    property var srv: server
    NotificationServer {
        id: server
        actionsSupported: true
        bodyMarkupSupported: true
        imageSupported: true
        persistenceSupported: true
        inlineReplySupported: true
        extraHints: ["x-test"]
        onNotification: root.got(notification)
    }
    property int publishes: 0
    Connections {
        target: server.trackedNotifications
        function onValuesChanged() { root.publishes++ }
    }
    Component {
        id: watcher
        Connections {
            function onClosed(reason) { root.log += "closed " + target.id + " " + reason + ";"; destroy() }
        }
    }
    function got(n) {
        root.log += "notif " + n.id + " " + n.summary + ";";
        watcher.createObject(root, { target: n });
        n.tracked = root.track && n.summary !== "untracked";
        if (n.summary === "expire-now") n.expire();
    }
    function count() { return server.trackedNotifications.values.length }
    function at(i) { return server.trackedNotifications.values[i] }
    function invoke(i, j) { at(i).actions[j].invoke() }
    function dismiss(i) { at(i).dismiss() }
    function expire(i) { at(i).expire() }
    function reply(i, t) { at(i).sendInlineReply(t) }
    function untrack(i) { at(i).tracked = false }
    function dismissAll() { const v = server.trackedNotifications.values.slice(); for (const n of v) n.dismiss() }
}
"#;

struct Bus(Child);

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Whether this is `test`'s own process (see `testenv::run_in_own_process`). The process that
/// runs the test starts it there with XDG data dirs holding one app
/// (`org.telegram.desktop.desktop`, Icon=org.telegram.desktop), a cache directory and a session
/// bus address (a socket), all in a fresh directory: the server and its icon lookups find them
/// through the environment.
fn own_process(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let apps = d.join("applications");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(
        apps.join("org.telegram.desktop.desktop"),
        "[Desktop Entry]\nType=Application\nName=Telegram\nExec=telegram-desktop\nIcon=org.telegram.desktop\n",
    )
    .unwrap();
    let vars: [(&str, std::ffi::OsString); 4] = [
        ("XDG_DATA_HOME", d.join("home").into()),
        ("XDG_DATA_DIRS", d.into()),
        ("XDG_CACHE_HOME", d.join("cache").into()),
        (
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", d.join("bus").display()).into(),
        ),
    ];
    testenv::run_in_own_process(test, vars);
    false
}

/// The private session bus, at the address the environment names.
fn bus() -> Option<Bus> {
    // No <servicedir>: a system notification daemon must not be bus-activated. The size of a
    // message is the session's (/usr/share/dbus-1/session.conf), not dbus-daemon's own 32 MiB.
    let cfg = std::env::temp_dir().join(format!("qml-notif-bus-{}.conf", std::process::id()));
    std::fs::write(
        &cfg,
        "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>\
         <limit name=\"max_message_size\">1000000000</limit>\
         <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
    )
    .ok()?;
    let mut c = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", cfg.display()))
        .arg(format!(
            "--address={}",
            std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap()
        ))
        .args(["--nofork", "--nopidfile", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| testenv::unavailable(format!("no dbus-daemon ({e})")))
        .ok()?;
    let mut addr = String::new();
    BufReader::new(c.stdout.take().unwrap())
        .read_line(&mut addr)
        .ok()?;
    Some(Bus(c))
}

type Ping = Arc<dyn Fn() + Send + Sync>;

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

    fn ping(&self) -> Ping {
        Arc::new(self.wake.ping())
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
            self.wait_wakeup(rt);
        }
    }

    /// Sleeps until a wakeup.
    fn wait_wakeup(&self, rt: &mut Runtime) {
        if self.wake.wait(self.ep, None).fd {
            rt.process_fd_ready();
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

/// The server has asked for its name (and owns it unless someone else does) and watches it.
fn watching(rt: &mut Runtime) -> bool {
    rt.world
        .borrow()
        .notifications
        .as_ref()
        .is_some_and(|n| n.watching())
}

/// Member of the signal [`T::wait_sigs`] sends itself behind the server's signals.
const MARK: &str = "TestMark";

/// What a [`T::client`] thread returns, handed over before it wakes the loop.
struct Answer<R>(std::sync::mpsc::Receiver<R>);

struct T {
    rt: Runtime,
    lp: Loop,
    root: usize,
    sigs: Arc<Mutex<Vec<String>>>,
    _bus: Bus,
}

fn setup(qml: &str) -> Option<T> {
    let b = bus()?;
    let sigs = Arc::new(Mutex::new(vec![]));
    let conn = Connection::session().unwrap();
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.Notifications")
        .unwrap()
        .build();
    let it = MessageIterator::for_match_rule(rule, &conn, None).unwrap();
    let s = sigs.clone();
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let ping = lp.ping();
    std::thread::spawn(move || {
        let _conn = conn;
        for m in it.flatten() {
            let h = m.header();
            let member = h.member().map(|m| m.to_string()).unwrap_or_default();
            let body = match member.as_str() {
                "NotificationClosed" => m
                    .body()
                    .deserialize::<(u32, u32)>()
                    .map(|(a, b)| format!("{a} {b}")),
                _ => m
                    .body()
                    .deserialize::<(u32, String)>()
                    .map(|(a, b)| format!("{a} {b}")),
            };
            s.lock()
                .unwrap()
                .push(format!("{member} {}", body.unwrap_or_default()));
            ping();
        }
    });
    let root = rt.load_str(qml).unwrap();
    rt.settle();
    let mut t = T {
        rt,
        lp,
        root,
        sigs,
        _bus: b,
    };
    t.until("the server to own its name", |t| watching(&mut t.rt));
    Some(t)
}

impl T {
    /// Runs loop turns, sleeping on the wakeups between them, until `done`.
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut T) -> bool) {
        eprintln!("waiting for {what}");
        loop {
            turn(&mut self.rt);
            let rev = self.rt.revision();
            if done(self) {
                return;
            }
            // A call in `done` can create objects (a service singleton) that only the next
            // poll starts: turn again before sleeping.
            if self.rt.revision() != rev {
                continue;
            }
            self.lp.wait_wakeup(&mut self.rt);
        }
    }
    /// Run a D-Bus client call on another thread, running the loop meanwhile: `Notify` replies
    /// only once the shell has handled it.
    fn run<R: Send>(&mut self, f: impl FnOnce() -> R + Send) -> R {
        let ping = self.lp.ping();
        // Set before the ping: the thread itself finishes only after it.
        let done = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|sc| {
            // Also when `f` panics: the join below then fails the test, instead of the loop
            // waiting for a call that ended.
            struct Ended<'a>(&'a std::sync::atomic::AtomicBool, &'a Ping);
            impl Drop for Ended<'_> {
                fn drop(&mut self) {
                    self.0.store(true, std::sync::atomic::Ordering::SeqCst);
                    (self.1)();
                }
            }
            let ended = Ended(&done, &ping);
            let h = sc.spawn(move || {
                let _ended = ended;
                f()
            });
            self.until("the client call", |_| {
                done.load(std::sync::atomic::Ordering::SeqCst)
            });
            h.join().unwrap()
        })
    }
    fn notify(
        &mut self,
        replaces: u32,
        summary: &str,
        actions: &str,
        hints: &str,
        timeout: i32,
    ) -> u32 {
        self.run(|| notify(replaces, summary, actions, hints, timeout))
    }
    /// `Notify` with no actions, hints or expiry.
    fn send(&mut self, replaces: u32, summary: &str) -> u32 {
        self.notify(replaces, summary, "[]", "{}", 0)
    }
    /// Turns the loop until no wakeup is pending: the shell has nothing left to do.
    fn idle(&mut self) {
        loop {
            turn(&mut self.rt);
            let woke = self.lp.wake.wait(self.lp.ep, Some(Duration::ZERO));
            if woke.fd {
                self.rt.process_fd_ready();
            }
            if !woke.any() {
                return;
            }
        }
    }
    /// A D-Bus client on another thread; it hands over its result, then wakes the loop.
    fn client<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send + 'static) -> Answer<R> {
        let ping = self.lp.ping();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ping();
        });
        Answer(rx)
    }
    /// Turns the loop until the client's answer is in.
    fn answer<R>(&mut self, what: &str, a: Answer<R>) -> R {
        let mut got = None;
        self.until(what, |_| {
            got = got.take().or_else(|| a.0.try_recv().ok());
            got.is_some()
        });
        got.unwrap()
    }
    /// `CloseNotification(id)`: gdbus' output, once the loop handled it and it replied.
    fn close(&mut self, id: u32) -> String {
        let a = self.client(move || gdbus("CloseNotification", &[&id.to_string()]));
        self.answer("CloseNotification's reply", a)
    }
    /// `count()` without turning the loop.
    fn tracked(&mut self) -> usize {
        let v = self.rt.call_method(self.root, "count", vec![]).unwrap();
        v.unwrap().to_number() as usize
    }
    fn call(&mut self, f: &str, a: Vec<Value>) -> Value {
        let v = self.rt.call_method(self.root, f, a).unwrap().unwrap();
        self.rt.poll_native();
        v
    }
    fn count(&mut self) -> usize {
        self.call("count", vec![]).to_number() as usize
    }
    fn at(&mut self, i: usize) -> usize {
        match self.call("at", vec![Value::Num(i as f64)]) {
            Value::Item(i) => i,
            v => panic!("no notification {i}: {v:?}"),
        }
    }
    fn get(&mut self, item: usize, p: &str) -> Value {
        self.rt
            .world
            .borrow_mut()
            .read(item, p)
            .unwrap_or(Value::Undefined)
    }
    fn s(&mut self, item: usize, p: &str) -> String {
        self.get(item, p).to_js_string()
    }
    fn n(&mut self, item: usize, p: &str) -> f64 {
        self.get(item, p).to_number()
    }
    fn log(&mut self) -> String {
        let r = self.root;
        self.s(r, "log")
    }
    fn wait_count(&mut self, n: usize) {
        self.until(&format!("{n} notifications"), |t| t.count() == n);
    }
    /// The server's signals once there are `n`, and every one it sent before then: a signal
    /// the test sends from the server's connection, as a task on its executor behind the
    /// server's, comes after all of them (the bus keeps one sender's messages in order).
    fn wait_sigs(&mut self, n: usize) -> Vec<String> {
        let s = self.sigs.clone();
        self.until(&format!("{n} signals"), |_| s.lock().unwrap().len() >= n);
        self.signals_so_far()
    }
    /// Every signal the server has sent so far: see [`T::wait_sigs`].
    fn signals_so_far(&mut self) -> Vec<String> {
        let s = self.sigs.clone();
        turn(&mut self.rt);
        let c = qml::dbus::session().unwrap().inner().clone();
        let on = c.clone();
        let task = c.executor().spawn(
            async move {
                on.emit_signal(
                    None::<&str>,
                    "/mark",
                    "org.freedesktop.Notifications",
                    MARK,
                    &(),
                )
                .await
                .unwrap();
            },
            "test mark",
        );
        zbus::block_on(task).expect("mark task");
        let marked = |s: &Arc<Mutex<Vec<String>>>| {
            s.lock()
                .unwrap()
                .iter()
                .any(|x| x.starts_with(&format!("{MARK} ")))
        };
        self.until("the mark signal", |_| marked(&s));
        let mut got = std::mem::take(&mut *self.sigs.lock().unwrap());
        got.retain(|x| !x.starts_with(&format!("{MARK} ")));
        got
    }
    fn dead(&self, i: usize) -> bool {
        self.rt.world.borrow().dead.contains(&i)
    }
}

fn gdbus(method: &str, args: &[&str]) -> String {
    let o = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.Notifications",
            "--object-path",
            "/org/freedesktop/Notifications",
            "--method",
        ])
        .arg(format!("org.freedesktop.Notifications.{method}"))
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
        + String::from_utf8_lossy(&o.stderr).trim()
}

/// `Notify` via gdbus; returns the id.
fn notify(replaces: u32, summary: &str, actions: &str, hints: &str, timeout: i32) -> u32 {
    let r = gdbus(
        "Notify",
        &[
            "app",
            &replaces.to_string(),
            "",
            summary,
            "body",
            actions,
            hints,
            &format!("int32 {timeout}"),
        ],
    );
    r.trim_start_matches("(uint32 ")
        .trim_end_matches(",)")
        .parse()
        .unwrap_or_else(|_| panic!("Notify: {r}"))
}

/// The test's `T`, in its own process (`own_process`); returns from the test elsewhere.
macro_rules! setup {
    ($test:expr) => {{
        if !own_process($test) {
            return;
        }
        let Some(t) = setup(QML) else { return };
        t
    }};
}

#[test]
fn capabilities_and_server_information() {
    let mut t = setup!(concat!(
        module_path!(),
        "::capabilities_and_server_information"
    ));
    assert_eq!(
        gdbus("GetCapabilities", &[]),
        "(['persistence', 'body', 'body-markup', 'actions', 'icon-static', 'inline-reply', 'x-test'],)"
    );
    assert_eq!(
        gdbus("GetServerInformation", &[]),
        "('rustshell', 'rustshell', '', '1.2')"
    );
    let srv = match t.get(t.root, "srv") {
        Value::Item(i) => i,
        v => panic!("{v:?}"),
    };
    // bodySupported defaults to true like NotificationServerSupport.
    assert!(t.get(srv, "bodySupported").truthy());
    t.rt.world
        .borrow_mut()
        .write(srv, "bodySupported", Value::Bool(false));
    t.rt.world
        .borrow_mut()
        .write(srv, "actionsSupported", Value::Bool(false));
    // The poll after the writes hands them to the server.
    turn(&mut t.rt);
    assert_eq!(
        gdbus("GetCapabilities", &[]),
        "(['persistence', 'icon-static', 'inline-reply', 'x-test'],)"
    );
}

#[test]
fn notify_send_properties_urgency_expire_and_hints() {
    let mut t = setup!(concat!(
        module_path!(),
        "::notify_send_properties_urgency_expire_and_hints"
    ));
    // libnotify >= 0.8.4 sends `-i` as the image hint and has `--app-icon` for the app_icon
    // argument; older versions send `-i` as app_icon and have no `--app-icon`.
    let app_icon_flag = match Command::new("notify-send").arg("--help").output() {
        Ok(o) if String::from_utf8_lossy(&o.stdout).contains("--app-icon") => "--app-icon",
        _ => "-i",
    };
    let st = t.run(|| {
        Command::new("notify-send")
            .args([
                "-a",
                "My App",
                "-u",
                "critical",
                "-t",
                "1234",
                app_icon_flag,
                "/tmp/icon.png",
                "-h",
                "string:desktop-entry:org.example.App",
            ])
            .args([
                "-h",
                "string:image-path:/tmp/pic.png",
                "-h",
                "int:x-num:5",
                "-c",
                "im.received",
                "Hello",
                "<b>World</b>",
            ])
            .status()
    });
    let Ok(st) = st else {
        testenv::unavailable("no notify-send");
        return;
    };
    assert!(st.success());
    t.wait_count(1);
    let n = t.at(0);
    assert_eq!(t.n(n, "id"), 1.0);
    assert_eq!(t.s(n, "appName"), "My App");
    assert_eq!(t.s(n, "summary"), "Hello");
    assert_eq!(t.s(n, "body"), "<b>World</b>");
    assert_eq!(t.n(n, "urgency"), 2.0);
    assert_eq!(t.n(n, "expireTimeout"), 1234.0);
    assert_eq!(t.s(n, "appIcon"), "/tmp/icon.png");
    assert_eq!(t.s(n, "desktopEntry"), "org.example.App");
    assert_eq!(t.s(n, "image"), "image://icon//tmp/pic.png");
    assert!(t.get(n, "tracked").truthy());
    assert!(!t.get(n, "resident").truthy());
    let hints = t.get(n, "hints");
    let mut it = qml::eval::Interp::default();
    let hint = |it: &mut qml::eval::Interp, k: &str| it.get(&hints, k).unwrap_or(Value::Undefined);
    assert_eq!(hint(&mut it, "x-num").to_number(), 5.0);
    assert_eq!(hint(&mut it, "urgency").to_number(), 2.0);
    assert_eq!(hint(&mut it, "category").to_js_string(), "im.received");
    assert_eq!(t.log(), "notif 1 Hello;");

    t.run(|| {
        Command::new("notify-send")
            .args([
                "-u",
                "low",
                "-h",
                "string:image-path:file:///tmp/a.png",
                "-h",
                "byte:resident:1",
                "Low",
            ])
            .status()
    })
    .unwrap();
    t.wait_count(2);
    let n = t.at(1);
    assert_eq!(t.n(n, "id"), 2.0);
    assert_eq!(t.n(n, "urgency"), 0.0);
    assert_eq!(t.n(n, "expireTimeout"), -1.0);
    assert_eq!(t.s(n, "image"), "file:///tmp/a.png");
    assert!(t.get(n, "resident").truthy());
    t.run(|| Command::new("notify-send").args(["Normal"]).status())
        .unwrap();
    t.wait_count(3);
    let n = t.at(2);
    assert_eq!(t.n(n, "urgency"), 1.0);
}

#[test]
fn replace_updates_in_place() {
    let mut t = setup!(concat!(module_path!(), "::replace_updates_in_place"));
    assert_eq!(t.notify(0, "first", "['a', 'A']", "{}", -1), 1);
    t.wait_count(1);
    let n = t.at(0);
    let a0 = t.get(n, "actions");
    assert_eq!(
        t.notify(
            1,
            "second",
            "['a', 'Aa', 'b', 'B']",
            "{'urgency': <byte 2>}",
            5000
        ),
        1
    );
    t.until("the replacement", |t| t.s(n, "summary") == "second");
    assert_eq!(t.count(), 1);
    assert_eq!(t.at(0), n, "replace must keep the same object");
    assert_eq!(t.n(n, "urgency"), 2.0);
    assert_eq!(t.n(n, "expireTimeout"), 5000.0);
    assert_eq!(
        t.log(),
        "notif 1 first;",
        "replace must not emit notification()"
    );
    let mut it = qml::eval::Interp::default();
    let acts = t.get(n, "actions");
    let first_old = it.get(&a0, "0").unwrap();
    let first_new = it.get(&acts, "0").unwrap();
    let Value::Item(fa) = first_new.clone() else {
        panic!()
    };
    assert!(
        matches!((first_old, first_new), (Value::Item(x), Value::Item(y)) if x == y),
        "same-identifier action is kept"
    );
    assert_eq!(t.s(fa, "text"), "Aa");
    let Value::Item(b) = it.get(&acts, "1").unwrap() else {
        panic!()
    };
    assert_eq!(
        (t.s(b, "identifier"), t.s(b, "text")),
        ("b".into(), "B".into())
    );
    // A replaces_id never handed out shows a new notification under that id (spec: the reply
    // to a non-zero replaces_id is that id).
    assert_eq!(t.notify(77, "third", "[]", "{}", -1), 77);
    t.wait_count(2);
    assert!(t.sigs.lock().unwrap().is_empty());
}

#[test]
fn actions_invoke_and_resident() {
    let mut t = setup!(concat!(module_path!(), "::actions_invoke_and_resident"));
    t.notify(0, "act", "['ok', 'OK', 'no', 'No']", "{}", -1);
    t.notify(0, "res", "['ok', 'OK']", "{'resident': <true>}", -1);
    t.wait_count(2);
    let n = t.at(0);
    let r = t.at(1);
    t.call("invoke", vec![Value::Num(1.0), Value::Num(0.0)]);
    assert_eq!(t.wait_sigs(1), vec!["ActionInvoked 2 ok"]);
    assert_eq!(t.count(), 2, "resident stays tracked");
    t.call("invoke", vec![Value::Num(0.0), Value::Num(1.0)]);
    assert_eq!(
        t.wait_sigs(2),
        vec!["ActionInvoked 1 no", "NotificationClosed 1 2"]
    );
    t.wait_count(1);
    assert!(t.dead(n));
    assert!(!t.dead(r));
    assert!(t.log().ends_with("closed 1 2;"), "{}", t.log());
}

#[test]
fn close_reasons() {
    let mut t = setup!(concat!(module_path!(), "::close_reasons"));
    for s in ["a", "b", "c", "d"] {
        t.notify(0, s, "[]", "{}", -1);
    }
    t.wait_count(4);
    let items: Vec<usize> = (0..4).map(|i| t.at(i)).collect();
    assert_eq!(t.close(1), "()");
    assert_eq!(t.wait_sigs(1), vec!["NotificationClosed 1 3"]);
    t.call("dismiss", vec![Value::Num(0.0)]);
    assert_eq!(t.wait_sigs(1), vec!["NotificationClosed 2 2"]);
    t.call("expire", vec![Value::Num(0.0)]);
    assert_eq!(t.wait_sigs(1), vec!["NotificationClosed 3 1"]);
    t.call("untrack", vec![Value::Num(0.0)]);
    assert_eq!(t.wait_sigs(1), vec!["NotificationClosed 4 2"]);
    t.wait_count(0);
    assert!(items.iter().all(|i| t.dead(*i)));
    assert_eq!(
        t.log(),
        "notif 1 a;notif 2 b;notif 3 c;notif 4 d;closed 1 3;closed 2 2;closed 3 1;closed 4 2;"
    );
    // Unknown or already closed id: no signal, and (Desktop Notifications 1.3) "an empty D-BUS
    // Error message is sent back".
    for id in [42, 1] {
        assert_eq!(
            t.close(id),
            "Error: GDBus.Error:org.freedesktop.DBus.Error.Failed:"
        );
    }
    assert_eq!(t.signals_so_far(), Vec::<String>::new());
    // Untracked in onNotification → destroyed, Dismissed; expire() inside → Expired.
    assert_eq!(t.notify(0, "untracked", "[]", "{}", -1), 5);
    assert_eq!(t.wait_sigs(1), vec!["NotificationClosed 5 2"]);
    assert_eq!(t.notify(0, "expire-now", "[]", "{}", -1), 6);
    assert_eq!(t.wait_sigs(1), vec!["NotificationClosed 6 1"]);
    assert_eq!(t.count(), 0);
    // Replacing a closed id shows a new notification under that id.
    assert_eq!(t.notify(5, "x", "[]", "{}", -1), 5);
    t.wait_count(1);
}

#[test]
fn inline_reply() {
    let mut t = setup!(concat!(module_path!(), "::inline_reply"));
    t.notify(0, "chat", "['inline-reply', 'Type…', 'ok', 'OK']", "{}", -1);
    t.wait_count(1);
    let n = t.at(0);
    assert!(t.get(n, "hasInlineReply").truthy());
    assert_eq!(t.s(n, "inlineReplyPlaceholder"), "Type…");
    let mut it = qml::eval::Interp::default();
    let acts = t.get(n, "actions");
    assert_eq!(it.get(&acts, "length").unwrap().to_number(), 1.0);
    t.call("reply", vec![Value::Num(0.0), Value::str("hi there")]);
    assert_eq!(
        t.wait_sigs(2),
        vec!["NotificationReplied 1 hi there", "NotificationClosed 1 2"]
    );
    t.wait_count(0);
}

#[test]
fn image_hints_served_via_qsimage() {
    let mut t = setup!(concat!(module_path!(), "::image_hints_served_via_qsimage"));
    let img = "<(2, 1, 8, true, 8, 4, [byte 255, 0, 0, 255, 0, 255, 0, 128])>";
    t.notify(0, "img", "[]", &format!("{{'image-data': {img}}}"), -1);
    t.wait_count(1);
    let n = t.at(0);
    let src = t.s(n, "image");
    assert!(
        src.starts_with("image://qsimage/") && src.ends_with("/1"),
        "{src}"
    );
    let (w, h, px) = qml::notifications::pixmap(&src).unwrap();
    assert_eq!((w, h), (2, 1));
    assert_eq!(px, vec![[255, 0, 0, 255], [0, 128, 0, 128]]);
    let hints = t.get(n, "hints");
    let mut it = qml::eval::Interp::default();
    assert!(
        matches!(
            it.get(&hints, "image-data").unwrap_or(Value::Undefined),
            Value::Undefined
        ),
        "image bytes are stripped from hints"
    );
    // Replacing bumps the change index; RGB (no alpha) through the legacy key.
    t.notify(
        1,
        "img2",
        "[]",
        "{'image_data': <(1, 1, 3, false, 8, 3, [byte 1, 2, 3])>}",
        -1,
    );
    t.until("the second image", |t| t.s(n, "image").ends_with("/2"));
    let src2 = t.s(n, "image");
    assert_eq!(
        qml::notifications::pixmap(&src2).unwrap(),
        (1, 1, vec![[1, 2, 3, 255]])
    );
    t.notify(
        0,
        "icon",
        "[]",
        "{'icon_data': <(1, 1, 4, true, 8, 4, [byte 0, 0, 255, 255])>}",
        -1,
    );
    t.wait_count(2);
    let m = t.at(1);
    assert!(t.s(m, "image").starts_with("image://qsimage/"));
    // Empty image data falls back to image-path.
    t.notify(
        0,
        "fallback",
        "[]",
        "{'image-data': <(0, 0, 0, true, 8, 4, @ay [])>, 'image-path': <'dialog-information'>}",
        -1,
    );
    t.wait_count(3);
    let f = t.at(2);
    assert_eq!(t.s(f, "image"), "image://icon/dialog-information");
    // A closed notification leaves the original's history, so its saved copy is deleted with the pixels;
    // the copy of the image its replacement superseded is already gone.
    let png = |src: &str| {
        let (key, change) = src
            .strip_prefix("image://qsimage/")
            .unwrap()
            .split_once('/')
            .unwrap();
        std::path::PathBuf::from(std::env::var("XDG_CACHE_HOME").unwrap()).join(format!(
            "rusty-impulse/notification-images/{key}-{change}.png"
        ))
    };
    wait_disk();
    assert!(!png(&src).exists(), "replaced image's copy deleted");
    assert!(png(&src2).exists());
    assert_eq!(t.close(1), "()");
    t.wait_count(2);
    wait_disk();
    assert!(!png(&src2).exists(), "closed notification's copy deleted");
    assert!(qml::notifications::pixmap(&src2).is_none());
    assert!(
        png(&t.s(m, "image")).exists(),
        "an open notification keeps its copy"
    );
}

/// Dismissing a batch ("clear all") rewrites trackedNotifications once, not once per id.
#[test]
fn batch_close_publishes_once() {
    let mut t = setup!(concat!(module_path!(), "::batch_close_publishes_once"));
    for i in 0..5 {
        t.notify(0, &format!("n{i}"), "[]", "{}", -1);
    }
    t.wait_count(5);
    let r = t.root;
    let before = t.n(r, "publishes");
    t.call("dismissAll", vec![]);
    t.wait_count(0);
    assert_eq!(t.n(r, "publishes") - before, 1.0);
    let sigs = t.wait_sigs(5);
    let closed: Vec<_> = sigs
        .iter()
        .filter(|s| s.starts_with("NotificationClosed"))
        .collect();
    assert_eq!(closed.len(), 5, "{sigs:?}");
    assert!(
        t.log()
            .ends_with("closed 1 2;closed 2 2;closed 3 2;closed 4 2;closed 5 2;"),
        "{}",
        t.log()
    );
}

/// Saved copies are written (and deleted) by a pool job off the main loop.
fn wait_disk() {
    qml::notifications::wait_disk_idle();
}

/// An image hint is decoded on a pool worker, not on the session bus executor; requests sent
/// back to back still land in the order they came, the plain one behind the decoding one.
#[test]
fn image_decode_keeps_request_order() {
    let mut t = setup!(concat!(
        module_path!(),
        "::image_decode_keeps_request_order"
    ));
    let (w, h) = (1024i32, 1024i32);
    let data = vec![200u8; (w * h * 4) as usize];
    t.run(move || {
        let c = Connection::session().unwrap();
        let mut hints: std::collections::HashMap<&str, zbus::zvariant::Value> = Default::default();
        hints.insert("image-data", (w, h, w * 4, true, 8i32, 4i32, data).into());
        let call =
            |summary: &str, hints: &std::collections::HashMap<&str, zbus::zvariant::Value>| {
                zbus::Message::method_call("/org/freedesktop/Notifications", "Notify")
                    .unwrap()
                    .destination("org.freedesktop.Notifications")
                    .unwrap()
                    .interface("org.freedesktop.Notifications")
                    .unwrap()
                    .build(&(
                        "app",
                        0u32,
                        "",
                        summary,
                        "",
                        Vec::<&str>::new(),
                        hints,
                        -1i32,
                    ))
                    .unwrap()
            };
        // Sent without waiting for the first reply.
        c.send(&call("big", &hints)).unwrap();
        c.send(&call("plain", &Default::default())).unwrap();
    });
    t.wait_count(2);
    let (a, b) = (t.at(0), t.at(1));
    assert_eq!(
        (t.s(a, "summary"), t.s(b, "summary")),
        ("big".into(), "plain".into())
    );
    assert!(t.n(a, "id") < t.n(b, "id"));
    let (iw, ih, _) = qml::notifications::pixmap(&t.s(a, "image")).unwrap();
    assert_eq!((iw, ih), (1024, 1024));
    assert_eq!(t.s(b, "image"), "");
    assert!(
        t.log().starts_with("notif 1 big;notif 2 plain;"),
        "{}",
        t.log()
    );
}

/// The original saves `image` to notifications.json and shows saved notifications again after a restart,
/// when the handle behind the old `image://qsimage` source is gone; the image comes from disk.
#[test]
fn saved_image_source_survives_restart() {
    let mut t = setup!(concat!(
        module_path!(),
        "::saved_image_source_survives_restart"
    ));
    let img = "<(2, 1, 8, true, 8, 4, [byte 255, 0, 0, 255, 0, 255, 0, 128])>";
    t.notify(0, "avatar", "[]", &format!("{{'image-data': {img}}}"), -1);
    t.wait_count(1);
    let n = t.at(0);
    let src = t.s(n, "image");
    let want = (2, 1, vec![[255, 0, 0, 255], [0, 128, 0, 128]]);
    assert_eq!(qml::notifications::pixmap(&src).unwrap(), want);
    // The same source as an earlier process left it: another run prefix, no live handle.
    let (key, change) = src
        .strip_prefix("image://qsimage/")
        .unwrap()
        .split_once('/')
        .unwrap();
    let (run, handle) = key.split_once('-').unwrap();
    let dir = std::path::PathBuf::from(std::env::var("XDG_CACHE_HOME").unwrap())
        .join("rusty-impulse/notification-images");
    // The PNG copy is written off the main loop.
    let saved = dir.join(format!("{run}-{handle}-{change}.png"));
    wait_disk();
    std::fs::copy(&saved, dir.join(format!("0-{handle}-{change}.png"))).unwrap();
    let old = format!("image://qsimage/0-{handle}/{change}");
    assert_eq!(qml::notifications::pixmap(&old).unwrap(), want);
    // A handle number reused by this run is not mistaken for the saved one.
    assert!(qml::notifications::pixmap(&format!("image://qsimage/0-{handle}/99")).is_none());
    assert!(qml::notifications::pixmap("image://qsimage/../x/1").is_none());
}

/// Telegram Desktop sends `app_icon` "" whenever it attaches the sender's userpic as
/// image-data, so its icon only comes from the `desktop-entry` hint; that lookup must not depend
/// on the shell's QML having used `DesktopEntries` first (the original only does in the launcher/dock).
#[test]
fn app_icon_from_desktop_entry_without_desktop_entries_in_qml() {
    let mut t = setup!(concat!(
        module_path!(),
        "::app_icon_from_desktop_entry_without_desktop_entries_in_qml"
    ));
    let img = "<(1, 1, 4, true, 8, 4, [byte 10, 20, 30, 255])>";
    t.notify(
        0,
        "линусь",
        "[]",
        &format!("{{'desktop-entry': <'org.telegram.desktop'>, 'image-data': {img}}}"),
        -1,
    );
    t.wait_count(1);
    let n = t.at(0);
    assert_eq!(t.s(n, "desktopEntry"), "org.telegram.desktop");
    assert_eq!(t.s(n, "appIcon"), "org.telegram.desktop");
    assert!(t.s(n, "image").starts_with("image://qsimage/"));
    // An unknown entry leaves it empty, like DesktopEntryManager::byId returning null.
    t.notify(
        0,
        "other",
        "[]",
        "{'desktop-entry': <'org.example.Missing'>}",
        -1,
    );
    t.wait_count(2);
    let m = t.at(1);
    assert_eq!(t.s(m, "appIcon"), "");
}

/// Another server owns the name: ours asks for it (without queueing), watches it, and takes it
/// once the other owner leaves (QDBusServiceWatcher → tryRegister again).
#[test]
fn name_taken_then_freed() {
    const NAME: &str = "org.freedesktop.Notifications";
    if !own_process(concat!(module_path!(), "::name_taken_then_freed")) {
        return;
    }
    let Some(b) = bus() else { return };
    let other = Connection::session().unwrap();
    other.request_name(NAME).unwrap();
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    // Each new owner of the name, from a match rule added before the server starts.
    let probe = Connection::session().unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&probe).unwrap();
    let it = dbus
        .receive_name_owner_changed_with_args(&[(0, NAME)])
        .unwrap();
    let (tx, owners) = std::sync::mpsc::channel();
    let ping = lp.ping();
    std::thread::spawn(move || {
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
    let owner = || {
        dbus.get_name_owner(NAME.try_into().unwrap())
            .unwrap()
            .to_string()
    };
    rt.load_str(QML).unwrap();
    rt.settle();
    lp.until(&mut rt, "the server to ask for its name", watching);
    let theirs = other.unique_name().unwrap().to_string();
    assert_eq!(owner(), theirs, "the server took the name from its owner");
    drop(other);
    let ours = qml::dbus::session()
        .unwrap()
        .unique_name()
        .unwrap()
        .to_string();
    lp.until(&mut rt, "the server to take the freed name", |_| {
        owners.try_iter().any(|o| o == ours)
    });
    assert_eq!(owner(), ours);
    drop(b);
}

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches(" kB").trim().parse().ok())
        .unwrap_or(0)
}

/// Live malloc bytes (all arenas, incl. mmapped chunks) in kB. Unlike RSS it
/// does not count freed pages the allocator keeps, so it measures retention.
fn heap_kb() -> i64 {
    // SAFETY: mallinfo2(3) takes no arguments; it only reads the allocator's statistics.
    let m = unsafe { libc::mallinfo2() };
    ((m.uordblks + m.hblkhd) / 1024) as i64
}

#[test]
fn flood_500() {
    let mut t = setup!(concat!(module_path!(), "::flood_500"));
    let client = Connection::session().unwrap();
    let px: Vec<u8> = vec![200; 64 * 64 * 4];
    let mut rss = vec![];
    let mut heap = vec![];
    let mut last = 0;
    let rounds: usize = std::env::var("RI_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    for round in 0..rounds {
        let t0 = Instant::now();
        t.run(|| {
            for i in 0..500 {
                let mut hints = std::collections::HashMap::<&str, zbus::zvariant::Value>::new();
                hints.insert(
                    "image-data",
                    zbus::zvariant::Value::from((
                        64i32,
                        64i32,
                        256i32,
                        true,
                        8i32,
                        4i32,
                        px.clone(),
                    )),
                );
                let r = client
                    .call_method(
                        Some("org.freedesktop.Notifications"),
                        "/org/freedesktop/Notifications",
                        Some("org.freedesktop.Notifications"),
                        "Notify",
                        &(
                            "flood",
                            0u32,
                            "",
                            format!("n{i}"),
                            "body",
                            Vec::<&str>::new(),
                            hints,
                            -1i32,
                        ),
                    )
                    .unwrap();
                let id: u32 = r.body().deserialize().unwrap();
                assert!(id > last);
                last = id;
            }
        });
        let sent = t0.elapsed();
        t.wait_count(500);
        eprintln!("sent in {sent:?}, processed by {:?}", t0.elapsed());
        let full = rss_kb();
        t.call("dismissAll", vec![]);
        t.wait_count(0);
        // Host maintenance the shell runs every frame.
        t.rt.reap();
        t.rt.world.borrow_mut().purge_dead();
        qml::eval::collect_cycles();
        let s = t.wait_sigs(500);
        assert_eq!(s.len(), 500);
        assert!(
            s.iter()
                .all(|s| s.starts_with("NotificationClosed") && s.ends_with(" 2"))
        );
        rss.push((full, rss_kb()));
        heap.push(heap_kb());
        eprintln!(
            "flood round {round}: 500 notifs in {:?}, rss with 500 tracked {full} kB, after dismiss {} kB, heap {} kB",
            t0.elapsed(),
            rss_kb(),
            heap_kb()
        );
    }
    // Untracked flood: nothing kept.
    let r = t.root;
    t.rt.world
        .borrow_mut()
        .write(r, "track", Value::Bool(false));
    t.rt.settle();
    let t0 = Instant::now();
    t.run(|| {
        for i in 0..500 {
            client
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "Notify",
                    &(
                        "flood",
                        0u32,
                        "",
                        format!("u{i}"),
                        "",
                        Vec::<&str>::new(),
                        std::collections::HashMap::<&str, zbus::zvariant::Value>::new(),
                        -1i32,
                    ),
                )
                .unwrap();
        }
    });
    let s = t.wait_sigs(500);
    eprintln!(
        "untracked flood: 500 notifs (no image) closed in {:?}",
        t0.elapsed()
    );
    assert_eq!(s.len(), 500);
    assert_eq!(t.count(), 0);
    let growth = heap[3] - heap[1];
    eprintln!(
        "heap growth rounds 1→3 after dismiss: {growth} kB (rss {} kB)",
        rss[3].1 as i64 - rss[1].1 as i64
    );
    assert!(
        growth < 2 * 1024,
        "memory not bounded: heap {heap:?} rss {rss:?}"
    );
}

/// The stored picture of a notification whose `image-data` is `w` by `h`, opaque red in its
/// left half and nothing in its right one.
fn kept(t: &mut T, w: i32, h: i32) -> (usize, usize, Vec<[u8; 4]>) {
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w / 2 {
            let o = ((y * w + x) * 4) as usize;
            data[o] = 255;
            data[o + 3] = 255;
        }
    }
    t.run(move || {
        let c = Connection::session().unwrap();
        let mut hints: std::collections::HashMap<&str, zbus::zvariant::Value> = Default::default();
        hints.insert("image-data", (w, h, w * 4, true, 8i32, 4i32, data).into());
        c.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("app", 0u32, "", "big", "", Vec::<&str>::new(), hints, -1i32),
        )
        .unwrap();
    });
    t.wait_count(1);
    let n = t.at(0);
    let src = t.s(n, "image");
    qml::notifications::pixmap(&src).unwrap()
}

/// A huge `image-data` is decoded typed and kept averaged down to at most 1024 px a side, not
/// stored (and copied on every texture load) at full size.
#[test]
fn oversized_image_kept_downscaled() {
    let mut t = setup!(concat!(module_path!(), "::oversized_image_kept_downscaled"));
    let (iw, ih, px) = kept(&mut t, 2048, 1024);
    assert_eq!((iw, ih), (1024, 512));
    assert_eq!(
        (px[0], px[511], px[512], px[1023]),
        ([255, 0, 0, 255], [255, 0, 0, 255], [0; 4], [0; 4])
    );
}

/// One of 4096x2048 is a message of 32 MiB and 304 bytes: `dbus-daemon` with no
/// `max_message_size` in its configuration takes at most 32 MiB and closes the sender's
/// connection, the session's configuration (`session.conf`, and [`bus`]) lets it through.
/// It is kept as the smaller one is.
#[test]
fn an_image_of_32_mib_is_kept_downscaled() {
    let mut t = setup!(concat!(
        module_path!(),
        "::an_image_of_32_mib_is_kept_downscaled"
    ));
    let (iw, ih, px) = kept(&mut t, 4096, 2048);
    assert_eq!((iw, ih), (1024, 512));
    assert_eq!(
        (px[0], px[511], px[512], px[1023]),
        ([255, 0, 0, 255], [255, 0, 0, 255], [0; 4], [0; 4])
    );
}

/// The largest picture a bus carries is 4096x4095: a notification's hints are one D-Bus array,
/// of at most 64 MiB. It is kept with its longer side 1024, 4 MiB and not 64.
#[test]
fn the_largest_image_of_a_bus_is_kept_downscaled() {
    let mut t = setup!(concat!(
        module_path!(),
        "::the_largest_image_of_a_bus_is_kept_downscaled"
    ));
    let (iw, ih, px) = kept(&mut t, 4096, 4095);
    assert_eq!((iw, ih), (1024, 1024));
    assert_eq!(px.len(), 1024 * 1024);
    let last = 1023 * 1024;
    assert_eq!(
        (px[0], px[511], px[512], px[1023]),
        ([255, 0, 0, 255], [255, 0, 0, 255], [0; 4], [0; 4])
    );
    assert_eq!(
        (px[last], px[last + 511], px[last + 512], px[last + 1023]),
        ([255, 0, 0, 255], [255, 0, 0, 255], [0; 4], [0; 4])
    );
}

/// A replacement already queued when the shell closes the notification it names (dismissed in
/// the same loop turn): the close is handled first, so the replacement shows a new notification,
/// under the same id as Desktop Notifications 1.3 requires of a non-zero `replaces_id`, and that
/// one stays replaceable in place (no second popup).
#[test]
fn replace_queued_behind_its_close_keeps_its_id() {
    let mut t = setup!(concat!(
        module_path!(),
        "::replace_queued_behind_its_close_keeps_its_id"
    ));
    let x = t.send(0, "a");
    t.idle();
    let h = t.client(move || notify(x, "b", "[]", "{}", 0));
    eprintln!("waiting for the replacement to be queued");
    // Its send is the only thing that can wake the loop now.
    assert!(t.lp.wake.wait(None, None).ping);
    // One turn: the dismiss (a queued effect) runs before the queued request.
    t.rt.call_method(t.root, "dismiss", vec![Value::Num(0.0)])
        .unwrap()
        .unwrap();
    t.rt.poll_native();
    let y = t.answer("the replacement's reply", h);
    assert_eq!(y, x, "{}", t.log());
    let closed = format!("NotificationClosed {x} 2");
    t.until("x's NotificationClosed", |t| {
        t.sigs.lock().unwrap().contains(&closed)
    });
    assert_eq!(t.log(), format!("notif {x} a;closed {x} 2;notif {x} b;"));
    assert_eq!(t.tracked(), 1);
    let z = t.send(x, "c");
    let shown = t.at(0);
    assert_eq!(
        (
            z,
            t.tracked(),
            t.n(shown, "id") as u32,
            t.s(shown, "summary")
        ),
        (x, 1, x, "c".to_string()),
        "{}",
        t.log()
    );
    assert_eq!(t.log(), format!("notif {x} a;closed {x} 2;notif {x} b;"));
    assert_eq!(*t.sigs.lock().unwrap(), vec![closed]);
}

/// A notification closed while it is shown (not tracked) is gone: replacing its id shows a new
/// notification under that id.
#[test]
fn closed_id_is_shown_again_under_its_id() {
    let mut t = setup!(concat!(
        module_path!(),
        "::closed_id_is_shown_again_under_its_id"
    ));
    let u = t.send(0, "untracked");
    let v = t.send(u, "b");
    assert_eq!(v, u, "{}", t.log());
    assert_eq!(t.tracked(), 1);
    assert_eq!(t.log(), format!("notif {u} untracked;notif {u} b;"));
}

/// A `replaces_id` the server never handed out is shown under that id, and new ids then skip it
/// while it is live, so two live notifications never share an id.
#[test]
fn new_ids_skip_a_live_replaces_id() {
    let mut t = setup!(concat!(module_path!(), "::new_ids_skip_a_live_replaces_id"));
    assert_eq!(t.send(3, "a"), 3, "{}", t.log());
    let ids: Vec<u32> = ["b", "c", "d"].iter().map(|s| t.send(0, s)).collect();
    assert_eq!(ids, [1, 2, 4], "{}", t.log());
    assert_eq!(t.tracked(), 4);
    assert_eq!(t.log(), "notif 3 a;notif 1 b;notif 2 c;notif 4 d;");
}

/// Like Qt delivering one D-Bus message per event, queued requests are handled one per loop
/// turn whatever the machine's speed, each further one waking the loop for the next turn.
#[test]
fn queued_requests_take_one_turn_each() {
    let mut t = setup!(concat!(
        module_path!(),
        "::queued_requests_take_one_turn_each"
    ));
    let ids: Vec<u32> = (0..3).map(|i| t.send(0, &format!("n{i}"))).collect();
    assert_eq!(t.tracked(), 3);
    t.idle();
    // The closes are sent at once, not waiting for their replies (they come once the loop has
    // handled each). The server runs every call as a task of its own, in the order they came, so
    // GetServerInformation's reply means each close before it is queued.
    let closes = t.client(move || {
        let c = Connection::session().unwrap();
        let call = |m| {
            zbus::Message::method_call("/org/freedesktop/Notifications", m)
                .unwrap()
                .destination("org.freedesktop.Notifications")
                .unwrap()
                .interface("org.freedesktop.Notifications")
                .unwrap()
        };
        for id in ids {
            let m = call("CloseNotification").build(&(id,)).unwrap();
            zbus::block_on(c.inner().send(&m)).unwrap();
        }
        c.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "GetServerInformation",
            &(),
        )
        .unwrap();
        c
    });
    let _c = closes.0.recv().unwrap();
    for left in (0..3).rev() {
        t.rt.poll_native();
        let again = t.rt.world.borrow().waker.peek().has(qml::svc::Svc::Notif);
        assert_eq!((t.tracked(), again), (left, left > 0), "{}", t.log());
    }
}
