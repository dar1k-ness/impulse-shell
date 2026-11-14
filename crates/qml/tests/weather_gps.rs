//! The tree's Weather service against fake GeoClue2, NetworkManager and notification services on
//! a private `dbus-daemon` (standing in for both buses), with a proxy that refuses every fetch.
//! Native `PositionSource` runs a GeoClue2 client: no service, or GeoClue refusing or dropping the
//! client, falls back to the city with one notification; a fix drives the fetch. A failed fetch
//! is repeated once NetworkManager reports connectivity, and left to Weather's timer while online;
//! the subscription to NetworkManager outlives the system-bus connection it was made on.
//! The runtime runs on a clock that stands still (see `Host`): each wait blocks until the event
//! it waits for wakes the runtime or a fake service, never for wall time.

use qml::runtime::Runtime;
use qml::value::Value;
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use testenv::Wakeups;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};

/// The test's host loop (as `event_loop::Loop`) on a clock that stands still, which can also
/// block until the runtime is woken: by its waker's ping (a worker's reply) or its epoll set
/// (process I/O).
struct Host {
    wake: Wakeups,
    ep: Option<i32>,
}

impl Host {
    fn new(rt: &mut Runtime) -> Host {
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Host { wake, ep }
    }

    /// Runs loop turns until nothing is ready: no wakeup, no event on the epoll set and no
    /// timer due. Pool work still in flight (a fetch the proxy holds) wakes the runtime when it
    /// reports.
    fn drain(&mut self, rt: &mut Runtime) {
        loop {
            rt.poll_native();
            rt.tick(0.0);
            rt.poll_native();
            rt.settle();
            let woke = self.wake.wait(self.ep, Some(Duration::ZERO));
            if woke.fd {
                rt.process_fd_ready();
            }
            if !woke.any() && rt.next_deadline(0.0) > 0.0 {
                return;
            }
        }
    }

    /// Blocks until the runtime is woken.
    fn block(&mut self, rt: &mut Runtime) {
        if self.wake.wait(self.ep, None).fd {
            rt.process_fd_ready();
        }
    }
}

fn ri() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"))
}

/// As the shell: the weather widget, and with it the Weather singleton, only once Config has
/// read config.json (Weather reads `enableGPS` when it completes). The widget is its own file:
/// a singleton named in main.qml is created with main.qml.
const MAIN: &str = "import QtQuick
import ri.modules.common
Item {
    property var probe: l.item
    Loader {
        id: l
        active: Config.ready && Config.options.bar.weather.enable
        sourceComponent: WeatherProbe {}
    }
}
";

const PROBE: &str = "import QtQuick
import ri.services
Item {
    property bool gps: Weather.gpsActive
    property bool gpsAtStart
    property var weather: Weather
    Component.onCompleted: gpsAtStart = Weather.gpsActive
}
";

/// The private `dbus-daemon`, ended with the test.
struct Bus(Child);

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A proxy that refuses every fetch, appending a byte to `log` for each before it does. With
/// `hold`, it keeps each fetch's connection open until a connection to `release` lets it go.
struct Proxy {
    addr: SocketAddr,
    release: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Proxy {
    fn start(log: PathBuf, release: PathBuf, hold: bool) -> Proxy {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let releases = UnixListener::bind(&release).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut f = std::fs::File::create(log).unwrap();
            for s in l.incoming() {
                if done.load(Ordering::SeqCst) {
                    break;
                }
                f.write_all(b".").unwrap();
                if hold {
                    let _ = releases.accept();
                }
                drop(s);
            }
        });
        Proxy {
            addr,
            release,
            stop,
            thread: Some(thread),
        }
    }
}

/// Wakes a held fetch and the accept loop with one last connection each, which it sees as the
/// stop.
impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.release);
        let _ = TcpStream::connect(self.addr);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// What a test sees in its own process.
struct Env {
    fetches: PathBuf,
    release: PathBuf,
}

impl Env {
    /// The fetches the proxy has refused (or holds) so far.
    fn fetches(&self) -> usize {
        std::fs::metadata(&self.fetches).map_or(0, |m| m.len() as usize)
    }

    /// Lets the proxy refuse the oldest fetch it holds.
    fn release(&self) {
        UnixStream::connect(&self.release).unwrap();
    }
}

/// In `test`'s own process (see `testenv::run_in_own_process`): a private bus as both buses,
/// a fresh home, every fetch through the refusing proxy (holding each with `hold`). The bus and
/// the proxy run in the process that ran it there, which gets `None`, as a machine without
/// dbus-daemon.
fn env(test: &str, hold: bool) -> Option<Env> {
    env_with(test, hold, false)
}

/// [`env`]; with `relayed`, the system bus address is a socket nothing listens at yet, for
/// [`relay`] to forward to the bus: the test can then end just the shell's system-bus
/// connection.
fn env_with(test: &str, hold: bool, relayed: bool) -> Option<Env> {
    if testenv::in_own_process(test) {
        let fetches = std::env::var_os("WEATHER_FETCHES").unwrap().into();
        let release = std::env::var_os("WEATHER_RELEASE").unwrap().into();
        return Some(Env { fetches, release });
    }
    if !testenv::require("dbus-daemon") {
        return None;
    }
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join("bus.conf");
    std::fs::write(
        &cfg,
        "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>\
         <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
    )
    .unwrap();
    let mut c = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", cfg.display()))
        .arg(format!(
            "--address=unix:path={}",
            tmp.path().join("bus").display()
        ))
        .args(["--nofork", "--nopidfile", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut addr = String::new();
    BufReader::new(c.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    let _bus = Bus(c);
    let fetches = tmp.path().join("fetches");
    let release = tmp.path().join("release");
    let proxy = Proxy::start(fetches.clone(), release.clone(), hold);
    let url = OsString::from(format!("http://{}", proxy.addr));
    let mut env = testenv::home_vars(&tmp.path().join("home"));
    let system = if relayed {
        format!("unix:path={}", tmp.path().join("system-relay").display())
    } else {
        addr.trim().to_string()
    };
    env.extend([
        ("DBUS_SESSION_BUS_ADDRESS", addr.trim().into()),
        ("DBUS_SYSTEM_BUS_ADDRESS", system.into()),
        ("WEATHER_FETCHES", fetches.into()),
        ("WEATHER_RELEASE", release.into()),
    ]);
    env.extend(
        ["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY"].map(|k| (k, url.clone())),
    );
    // Empty counts as unset (curl's `detect_proxy`, as `qml::http`): nothing bypasses the proxy.
    env.extend(["no_proxy", "NO_PROXY"].map(|k| (k, OsString::new())));
    testenv::run_in_own_process(test, env);
    None
}

/// The socket file of a `unix:path=` bus address.
fn socket_of(addr: &str) -> String {
    let a = addr.strip_prefix("unix:path=").unwrap();
    a.split(',').next().unwrap().to_string()
}

/// Relays every connection to the system bus address (see [`env_with`]) to the private bus;
/// the returned list holds both ends of each relayed connection, for [`cut`].
fn relay() -> Arc<Mutex<Vec<UnixStream>>> {
    let listen = socket_of(&std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap());
    let bus = socket_of(&std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap());
    let ends = Arc::new(Mutex::new(Vec::new()));
    let l = UnixListener::bind(listen).unwrap();
    let e = ends.clone();
    std::thread::spawn(move || {
        for client in l.incoming() {
            let Ok(client) = client else { return };
            let up = UnixStream::connect(&bus).unwrap();
            e.lock()
                .unwrap()
                .extend([client.try_clone().unwrap(), up.try_clone().unwrap()]);
            for (mut from, mut to) in [
                (client.try_clone().unwrap(), up.try_clone().unwrap()),
                (up, client),
            ] {
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut from, &mut to);
                    let _ = to.shutdown(std::net::Shutdown::Write);
                });
            }
        }
    });
    ends
}

/// Ends every connection relayed so far, as a bus dropping its client does.
fn cut(ends: &Mutex<Vec<UnixStream>>) {
    for s in ends.lock().unwrap().drain(..) {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
}

#[derive(Default)]
struct Notes(Arc<Mutex<Vec<String>>>);

// Calls are handled one at a time in the order they arrive (see `quiet`).
#[zbus::interface(name = "org.freedesktop.Notifications", spawn = false)]
impl Notes {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        _app: String,
        _replaces: u32,
        _icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        _hints: std::collections::HashMap<String, OwnedValue>,
        _timeout: i32,
    ) -> u32 {
        let mut v = self.0.lock().unwrap();
        v.push(format!("{summary}: {body}"));
        v.len() as u32
    }
    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".into(), "actions".into()]
    }
    fn get_server_information(&self) -> (String, String, String, String) {
        ("fake".into(), "test".into(), "1".into(), "1.2".into())
    }
}

/// How the fake GeoClue behaves, and the calls it got.
#[derive(Clone, Default)]
struct Gc {
    /// LocationUpdated on Start, for a location GeoClue already has.
    fix: bool,
    /// GetClient answers AccessDenied, as GeoClue with location services off.
    deny_client: bool,
    /// Start answers AccessDenied, as when the agent refuses the app.
    deny_start: bool,
    /// The calls so far, notified after each.
    calls: Arc<(Mutex<Vec<String>>, Condvar)>,
}

impl Gc {
    fn log(&self, call: &str) {
        self.calls.0.lock().unwrap().push(call.into());
        self.calls.1.notify_all();
    }
    fn called(&self, call: &str) -> bool {
        self.calls().iter().any(|c| c == call)
    }
    fn calls(&self) -> Vec<String> {
        self.calls.0.lock().unwrap().clone()
    }
    /// Blocks until GeoClue got `call`, woken by each call it gets.
    fn wait_called(&self, call: &str) {
        let calls = self.calls.0.lock().unwrap();
        drop(
            self.calls
                .1
                .wait_while(calls, |v| !v.iter().any(|c| c == call))
                .unwrap(),
        );
    }
}

struct Manager(Gc);

#[zbus::interface(name = "org.freedesktop.GeoClue2.Manager")]
impl Manager {
    fn get_client(&self) -> zbus::fdo::Result<OwnedObjectPath> {
        self.0.log("GetClient");
        if self.0.deny_client {
            return Err(zbus::fdo::Error::AccessDenied("location disabled".into()));
        }
        Ok(ObjectPath::try_from(CLIENT).unwrap().into())
    }
    fn delete_client(&self, client: OwnedObjectPath) {
        assert_eq!(client.as_str(), CLIENT);
        self.0.log("DeleteClient");
    }
}

const CLIENT: &str = "/org/freedesktop/GeoClue2/Client/1";
const LOCATION: &str = "/org/freedesktop/GeoClue2/Location/1";

struct Client {
    gc: Gc,
    desktop_id: String,
}

#[zbus::interface(name = "org.freedesktop.GeoClue2.Client")]
impl Client {
    async fn start(
        &mut self,
        #[zbus(signal_emitter)] e: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gc.log("Start");
        if self.gc.deny_start {
            return Err(zbus::fdo::Error::AccessDenied("not allowed".into()));
        }
        if self.gc.fix {
            let _ = Self::location_updated(
                &e,
                ObjectPath::try_from("/").unwrap(),
                ObjectPath::try_from(LOCATION).unwrap(),
            )
            .await;
        }
        Ok(())
    }
    fn stop(&mut self) {
        self.gc.log("Stop");
    }
    #[zbus(property)]
    fn location(&self) -> OwnedObjectPath {
        ObjectPath::try_from("/").unwrap().into()
    }
    #[zbus(property)]
    fn active(&self) -> bool {
        self.gc.called("Start")
    }
    #[zbus(property)]
    fn desktop_id(&self) -> String {
        self.desktop_id.clone()
    }
    #[zbus(property)]
    fn set_desktop_id(&mut self, v: String) {
        self.desktop_id = v;
    }
    #[zbus(property)]
    fn requested_accuracy_level(&self) -> u32 {
        0
    }
    #[zbus(property)]
    fn set_requested_accuracy_level(&mut self, _v: u32) {}
    #[zbus(property)]
    fn time_threshold(&self) -> u32 {
        0
    }
    #[zbus(property)]
    fn set_time_threshold(&mut self, _v: u32) {}
    #[zbus(signal)]
    async fn location_updated(
        e: &SignalEmitter<'_>,
        old: ObjectPath<'_>,
        new: ObjectPath<'_>,
    ) -> zbus::Result<()>;
}

struct Location;

#[zbus::interface(name = "org.freedesktop.GeoClue2.Location")]
impl Location {
    #[zbus(property)]
    fn latitude(&self) -> f64 {
        48.85
    }
    #[zbus(property)]
    fn longitude(&self) -> f64 {
        2.35
    }
}

fn notes() -> (zbus::blocking::Connection, Arc<Mutex<Vec<String>>>) {
    let n = Notes::default();
    let log = n.0.clone();
    let c = zbus::blocking::connection::Builder::session()
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .serve_at("/org/freedesktop/Notifications", n)
        .unwrap()
        .build()
        .unwrap();
    (c, log)
}

fn geoclue(gc: &Gc) -> zbus::blocking::Connection {
    zbus::blocking::connection::Builder::session()
        .unwrap()
        .name("org.freedesktop.GeoClue2")
        .unwrap()
        .serve_at("/org/freedesktop/GeoClue2/Manager", Manager(gc.clone()))
        .unwrap()
        .serve_at(
            CLIENT,
            Client {
                gc: gc.clone(),
                desktop_id: String::new(),
            },
        )
        .unwrap()
        .serve_at(LOCATION, Location)
        .unwrap()
        .build()
        .unwrap()
}

/// A fake NetworkManager whose `State` starts at `state`.
struct Nm {
    conn: zbus::blocking::Connection,
    state: Arc<AtomicU32>,
}

struct NmIface(Arc<AtomicU32>);

#[zbus::interface(name = "org.freedesktop.NetworkManager")]
impl NmIface {
    #[zbus(property)]
    fn state(&self) -> u32 {
        self.0.load(Ordering::SeqCst)
    }
}

const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_IFACE: &str = "org.freedesktop.NetworkManager";

impl Nm {
    fn start(state: u32) -> Nm {
        let state = Arc::new(AtomicU32::new(state));
        let conn = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name(NM_IFACE)
            .unwrap()
            .serve_at(NM_PATH, NmIface(state.clone()))
            .unwrap()
            .build()
            .unwrap();
        Nm { conn, state }
    }

    /// NM's state turns `s`: the property, then `StateChanged`, as NetworkManager does.
    fn set(&self, s: u32) {
        self.state.store(s, Ordering::SeqCst);
        self.conn
            .emit_signal(None::<&str>, NM_PATH, NM_IFACE, "StateChanged", &(s,))
            .unwrap();
    }
}

/// The original with `config.json` in the fresh config dir, Weather loaded; returns the weather probe.
fn start(config: &str) -> (Runtime, Host, usize) {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let cfg = PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").unwrap()).join("rusty-impulse");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("config.json"), config).unwrap();
    std::fs::write(home.join("main.qml"), MAIN).unwrap();
    std::fs::write(home.join("WeatherProbe.qml"), PROBE).unwrap();
    let mut rt = Runtime::new(Some(ri()));
    let mut lp = Host::new(&mut rt);
    let dirs = ["modules/common", "modules/common/functions", "services"].map(|d| ri().join(d));
    rt.load_singletons_lazy(&dirs);
    rt.set_screens(&[("Virtual-1".into(), 0, 0, 1920, 1080)]);
    let main = rt.load_file(&home.join("main.qml")).unwrap();
    rt.settle();
    let mut r = None;
    wait(&mut lp, &mut rt, |rt| match rt.get(main, "probe") {
        Value::Item(p) => {
            r = Some(p);
            true
        }
        _ => false,
    });
    (rt, lp, r.unwrap())
}

/// Runs the loop until `done`: it waits for the events that can make it (process exits, D-Bus
/// replies and signals waking the runtime), never for wall time; nextest's slow-timeout ends a
/// hang.
fn wait(lp: &mut Host, rt: &mut Runtime, mut done: impl FnMut(&mut Runtime) -> bool) {
    loop {
        lp.drain(rt);
        if done(rt) {
            return;
        }
        lp.block(rt);
    }
}

/// Runs the loop until no `Process` is left running and the pool is idle: every fetch has
/// exited and been handled, and every detached `notify-send` has exited, so what it sent has
/// reached the notification server.
fn quiet(lp: &mut Host, rt: &mut Runtime) {
    wait(lp, rt, |rt| rt.world.borrow().procs.is_empty());
    // The detached `notify-send`s: the pool starts and reaps them.
    workpool::wait_idle();
    // `Notify.send` posts without waiting for a reply. The bus keeps one sender's messages in
    // order and the fake server handles them in that order, so once it answered this call on
    // the same connection it has handled every notification sent before it.
    if let Some(c) = qml::dbus::session() {
        let _ = c.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "GetServerInformation",
            &(),
        );
    }
}

fn command(rt: &mut Runtime, r: usize) -> String {
    let Value::Item(weather) = rt.get(r, "weather") else {
        panic!("no Weather")
    };
    let w = rt.world.borrow();
    let fetcher = (0..w.items.len())
        .filter(|&i| w.items[i].parent == Some(weather))
        .find(|&i| w.ctxs.iter().any(|c| c.ids.get("fetcher") == Some(&i)))
        .expect("fetcher");
    let place = w.items[fetcher]
        .props
        .get("place")
        .unwrap()
        .value
        .to_js_string();
    format!("wttr.in/{place}?")
}

thread_local! {
    /// The console lines of this thread's runtime.
    static CONSOLE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Starts keeping the console lines the runtime of this thread prints.
fn keep_console() {
    let _ = qml::builtins::CONSOLE_SINK.set(|_, line| {
        CONSOLE.with_borrow_mut(|c| c.push(line.to_string()));
    });
}

fn log_count(s: &str) -> usize {
    CONSOLE.with_borrow(|c| c.iter().filter(|l| l.contains(s)).count())
}

const GPS_CONFIG: &str =
    r#"{"bar": {"weather": {"enable": true, "enableGPS": true, "city": "Paris"}}}"#;
const NOTE: &str = "Weather Service: Cannot find a GPS service. Using the fallback method instead.";

/// `GeoClueCheck` (Weather.qml `geoclueCheck`, ServicesConfig.qml's probe): the `bash` busctl
/// pipeline the original ran vs native `probe::geoclue`, with no GeoClue2 on the system bus, with it,
/// and after it left.
#[test]
fn geoclue_check_matches_busctl() {
    let Some(_e) = env(
        concat!(module_path!(), "::geoclue_check_matches_busctl"),
        false,
    ) else {
        return;
    };
    if !["bash", "busctl", "grep"]
        .iter()
        .all(|t| testenv::require(t))
    {
        return;
    }
    let cmd = "busctl --system --no-pager list 2>/dev/null | grep -q org.freedesktop.GeoClue2";
    for f in [
        "services/Weather.qml",
        "modules/settings/ServicesConfig.qml",
    ] {
        let src = std::fs::read_to_string(ri().join(f)).unwrap();
        assert!(src.contains("GeoClueCheck {"), "{f}: no GeoClueCheck");
    }
    let original = || {
        Command::new("bash")
            .args(["-c", cmd])
            .status()
            .unwrap()
            .code()
            .unwrap()
    };
    let both = |want: i32| {
        assert_eq!(original(), want, "original");
        assert_eq!(qml::probe::geoclue().code, want, "native");
    };
    both(1);
    let gc = geoclue(&Gc::default());
    both(0);
    // The name is gone once the bus says so: NameOwnerChanged with no new owner, watched
    // before the connection closes.
    let bus = zbus::blocking::Connection::session().unwrap();
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")
        .unwrap()
        .interface("org.freedesktop.DBus")
        .unwrap()
        .member("NameOwnerChanged")
        .unwrap()
        .arg(0, "org.freedesktop.GeoClue2")
        .unwrap()
        .build();
    let mut owners = zbus::blocking::MessageIterator::for_match_rule(rule, &bus, None).unwrap();
    drop(gc);
    loop {
        let m = owners.next().expect("bus connection closed").unwrap();
        let (_, _, new): (String, String, String) = m.body().deserialize().unwrap();
        if new.is_empty() {
            break;
        }
    }
    both(1);
}

/// No GeoClue2 on the system bus (`geoclueCheck` fails): this run uses the city without
/// starting the position source, so no "cannot find a GPS service" notification, and the
/// saved `enableGPS` stays on for when GeoClue is up.
#[test]
fn gps_without_geoclue_uses_the_city_quietly() {
    let Some(_e) = env(
        concat!(
            module_path!(),
            "::gps_without_geoclue_uses_the_city_quietly"
        ),
        false,
    ) else {
        return;
    };
    let (_n, notes) = notes();
    keep_console();
    let (mut rt, mut lp, r) = start(GPS_CONFIG);
    assert!(rt.get(r, "gpsAtStart").truthy(), "GPS mode starts active");
    wait(&mut lp, &mut rt, |rt| !rt.get(r, "gps").truthy());
    quiet(&mut lp, &mut rt);
    assert!(
        notes.lock().unwrap().is_empty(),
        "{:?}",
        notes.lock().unwrap()
    );
    assert_eq!(log_count("Starting the GPS service"), 0);
    assert_eq!(log_count("Could not aquire a valid backend plugin"), 0);
    assert!(command(&mut rt, r).contains("wttr.in/Paris?"));
    let cfg = PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").unwrap());
    let saved = std::fs::read_to_string(cfg.join("rusty-impulse/config.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved["bar"]["weather"]["enableGPS"], true, "{saved}");
}

/// How the fake GeoClue fails the client.
enum Failure {
    /// GetClient answers AccessDenied (location services off).
    GetClient,
    /// Start answers AccessDenied (the agent refuses the app).
    Start,
    /// The client's `Active` turns false after Start.
    Inactive,
    /// GeoClue leaves the bus after Start.
    Leave,
}

/// GeoClue failing the client: the source turns invalid, Weather uses the city with one
/// notification, and a client GeoClue still holds is stopped and deleted.
fn gps_failure(test: &str, failure: Failure) {
    let Some(_e) = env(test, false) else {
        return;
    };
    let (_n, notes) = notes();
    let gc = Gc {
        deny_client: matches!(failure, Failure::GetClient),
        deny_start: matches!(failure, Failure::Start),
        ..Gc::default()
    };
    let conn = geoclue(&gc);
    let (mut rt, mut lp, r) = start(GPS_CONFIG);
    let mut conn = Some(conn);
    if let Failure::Inactive | Failure::Leave = failure {
        // The position source's GeoClue client runs on its own thread once started.
        wait(&mut lp, &mut rt, |rt| !rt.world.borrow().geo.is_empty());
        gc.wait_called("Start");
        assert!(rt.get(r, "gps").truthy(), "gave up before GeoClue failed");
        if let Failure::Leave = failure {
            conn = None;
        } else {
            let changed = HashMap::from([("Active", zbus::zvariant::Value::from(false))]);
            conn.as_ref()
                .unwrap()
                .emit_signal(
                    None::<&str>,
                    CLIENT,
                    "org.freedesktop.DBus.Properties",
                    "PropertiesChanged",
                    &(
                        "org.freedesktop.GeoClue2.Client",
                        changed,
                        Vec::<String>::new(),
                    ),
                )
                .unwrap();
        }
    }
    wait(&mut lp, &mut rt, |rt| !rt.get(r, "gps").truthy());
    quiet(&mut lp, &mut rt);
    assert_eq!(*notes.lock().unwrap(), [NOTE], "{:?}", gc.calls());
    assert!(command(&mut rt, r).contains("wttr.in/Paris?"));
    if let Failure::Start | Failure::Inactive = failure {
        gc.wait_called("DeleteClient");
        let calls = gc.calls();
        let at = |c: &str| calls.iter().position(|x| x == c);
        assert!(at("Stop") < at("DeleteClient"), "{calls:?}");
    }
    assert!(
        rt.world.borrow().geo.is_empty(),
        "the invalid source was not dropped"
    );
    drop(conn);
}

#[test]
fn gps_client_denied_falls_back_once() {
    gps_failure(
        concat!(module_path!(), "::gps_client_denied_falls_back_once"),
        Failure::GetClient,
    );
}

#[test]
fn gps_start_denied_falls_back_once() {
    gps_failure(
        concat!(module_path!(), "::gps_start_denied_falls_back_once"),
        Failure::Start,
    );
}

#[test]
fn gps_client_deactivated_falls_back_once() {
    gps_failure(
        concat!(module_path!(), "::gps_client_deactivated_falls_back_once"),
        Failure::Inactive,
    );
}

#[test]
fn gps_geoclue_leaving_falls_back_once() {
    gps_failure(
        concat!(module_path!(), "::gps_geoclue_leaving_falls_back_once"),
        Failure::Leave,
    );
}

/// A GeoClue2 fix sets the location and the fetch is for the coordinates.
#[test]
fn gps_fix_drives_the_fetch() {
    let Some(_e) = env(concat!(module_path!(), "::gps_fix_drives_the_fetch"), false) else {
        return;
    };
    let (_n, notes) = notes();
    let _gc = geoclue(&Gc {
        fix: true,
        ..Gc::default()
    });
    let (mut rt, mut lp, r) = start(GPS_CONFIG);
    wait(&mut lp, &mut rt, |rt| {
        command(rt, r).contains("wttr.in/48.85,2.35?")
    });
    assert!(rt.get(r, "gps").truthy());
    quiet(&mut lp, &mut rt);
    assert!(rt.get(r, "gps").truthy());
    assert!(
        notes.lock().unwrap().is_empty(),
        "{:?}",
        notes.lock().unwrap()
    );
}

const POSITION_QML: &str = "import QtQuick\nimport QtPositioning\nItem {\n\
    property alias active: src.active\n\
    property int fixes: 0\n\
    property real lat: 0\n\
    PositionSource {\n\
        id: src\n\
        onPositionChanged: { fixes++; lat = position.coordinate.latitude }\n\
    }\n\
    function start() { src.start() }\n\
    function update() { src.update() }\n\
    function stop() { src.stop() }\n\
}";

/// `PositionSource.update()` is Qt's single update: the source is active until the fix
/// arrives and inactive after it, unless `start()` asked for regular updates; `stop()` during
/// a single update leaves it its fix.
#[test]
fn position_update_is_a_single_update() {
    let Some(_e) = env(
        concat!(module_path!(), "::position_update_is_a_single_update"),
        false,
    ) else {
        return;
    };
    let gc = Gc {
        fix: true,
        ..Gc::default()
    };
    let _geoclue = geoclue(&gc);
    let mut rt = Runtime::new(None);
    let mut lp = Host::new(&mut rt);
    let root = rt.load_str(POSITION_QML).unwrap();
    rt.settle();
    let active = |rt: &Runtime| rt.get(root, "active").truthy();
    let fixes = |rt: &Runtime| rt.get(root, "fixes").to_number();
    let call = |rt: &mut Runtime, f: &str| {
        rt.call_method(root, f, vec![]).unwrap().unwrap();
        rt.settle();
    };
    assert!(!active(&rt));

    call(&mut rt, "update");
    assert!(active(&rt), "active while the single update runs");
    wait(&mut lp, &mut rt, |rt| !active(rt));
    assert_eq!((fixes(&rt), rt.get(root, "lat").to_number()), (1.0, 48.85));
    // The client went with the single update.
    gc.wait_called("Stop");

    // `stop()` ends regular updates only: the single update still gets its fix.
    call(&mut rt, "update");
    call(&mut rt, "stop");
    assert!(active(&rt), "stop() during a single update");
    wait(&mut lp, &mut rt, |rt| !active(rt));
    assert_eq!(fixes(&rt), 2.0);

    // With regular updates running, the single update's fix leaves the source active.
    call(&mut rt, "start");
    call(&mut rt, "update");
    wait(&mut lp, &mut rt, |rt| fixes(rt) == 3.0);
    assert!(active(&rt), "regular updates go on");
    call(&mut rt, "stop");
    assert!(!active(&rt));
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

const CITY_CONFIG: &str =
    r#"{"bar": {"weather": {"enable": true, "enableGPS": false, "city": "Paris"}}}"#;

fn status(rt: &Runtime) -> Option<qml::weather::Status> {
    qml::weather::status(&rt.world.borrow())
}

fn fetch_is(rt: &Runtime, f: qml::weather::Fetch) -> bool {
    status(rt).is_some_and(|s| s.fetch == f)
}

/// NM state values (NMState).
const DISCONNECTED: u32 = 20;
const CONNECTED_GLOBAL: u32 = 70;

/// A fetch failed while offline is repeated when NetworkManager reports connectivity, not on
/// other states.
#[test]
fn failed_fetch_offline_refetches_on_connect() {
    let Some(e) = env(
        concat!(
            module_path!(),
            "::failed_fetch_offline_refetches_on_connect"
        ),
        false,
    ) else {
        return;
    };
    use qml::weather::Fetch;
    let nm = Nm::start(DISCONNECTED);
    let (mut rt, mut lp, _r) = start(CITY_CONFIG);
    // The fetch's exit and NM's `State` reply each wake the runtime.
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.state == Some(DISCONNECTED)) && fetch_is(rt, Fetch::Offline)
    });
    quiet(&mut lp, &mut rt);
    assert_eq!(e.fetches(), 1);
    nm.set(40);
    nm.set(CONNECTED_GLOBAL);
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.refetches == 1) && fetch_is(rt, Fetch::Idle)
    });
    quiet(&mut lp, &mut rt);
    let s = status(&rt).unwrap();
    assert_eq!(
        (s.refetches, e.fetches(), s.fetch),
        (1, 2, Fetch::Idle),
        "{s:?}"
    );
    assert_eq!(s.state, Some(CONNECTED_GLOBAL));
    drop(rt);
}

/// A `WeatherFetch` anywhere in a document, next to no function of any name: what Weather.qml
/// does on its own is not assumed.
const NESTED_FETCH: &str = "import QtQuick
import Impulse
Item {
    id: probe
    property int restored: 0
    property int exits: 0
    Item {
        Item {
            WeatherFetch {
                id: fetch
                place: 'Paris'
                running: true
                onExited: probe.exits += 1
                onNetworkRestored: {
                    probe.restored += 1;
                    fetch.running = true;
                }
            }
        }
    }
}
";

/// The network coming back after a fetch failed offline is `WeatherFetch.networkRestored`:
/// the QML that owns the fetch decides what to do. Nothing is looked up by a QML name (the
/// engine called `getData` of the fetch's parent).
#[test]
fn network_coming_back_is_a_signal_of_the_fetch() {
    let Some(e) = env(
        concat!(
            module_path!(),
            "::network_coming_back_is_a_signal_of_the_fetch"
        ),
        false,
    ) else {
        return;
    };
    use qml::weather::Fetch;
    let nm = Nm::start(DISCONNECTED);
    let mut rt = Runtime::new(None);
    let mut lp = Host::new(&mut rt);
    let r = rt.load_str(NESTED_FETCH).unwrap();
    assert_eq!(rt.errors(), Vec::<String>::new());
    rt.settle();
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.state == Some(DISCONNECTED)) && fetch_is(rt, Fetch::Offline)
    });
    quiet(&mut lp, &mut rt);
    assert_eq!((e.fetches(), rt.get(r, "restored").to_number()), (1, 0.0));
    nm.set(CONNECTED_GLOBAL);
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.refetches == 1) && fetch_is(rt, Fetch::Idle)
    });
    quiet(&mut lp, &mut rt);
    assert_eq!(
        (
            rt.get(r, "restored").to_number(),
            rt.get(r, "exits").to_number(),
            e.fetches()
        ),
        (1.0, 2.0, 2)
    );
    assert_eq!(rt.errors(), Vec::<String>::new());
    drop(rt);
}

/// A NetworkManager whose `State` cannot be read; each read is reported on the channel.
struct NmUnreadable(Mutex<std::sync::mpsc::Sender<()>>);

#[zbus::interface(name = "org.freedesktop.NetworkManager")]
impl NmUnreadable {
    #[zbus(property)]
    fn state(&self) -> zbus::fdo::Result<u32> {
        let _ = self.0.lock().unwrap().send(());
        Err(zbus::fdo::Error::Failed("not ready".into()))
    }
}

/// A NetworkManager whose `State` cannot be read (the read fails with an error) and that then
/// reports connectivity: nothing said it was connected when the fetch failed, so its first
/// `StateChanged` to connected repeats the fetch at once.
#[test]
fn failed_fetch_refetches_on_connect_after_a_failed_state_read() {
    let Some(e) = env(
        concat!(
            module_path!(),
            "::failed_fetch_refetches_on_connect_after_a_failed_state_read"
        ),
        false,
    ) else {
        return;
    };
    use qml::weather::Fetch;
    let (read_tx, read) = std::sync::mpsc::channel();
    let nm = zbus::blocking::connection::Builder::session()
        .unwrap()
        .name(NM_IFACE)
        .unwrap()
        .serve_at(NM_PATH, NmUnreadable(Mutex::new(read_tx)))
        .unwrap()
        .build()
        .unwrap();
    let (mut rt, mut lp, _r) = start(CITY_CONFIG);
    wait(&mut lp, &mut rt, |rt| fetch_is(rt, Fetch::Offline));
    quiet(&mut lp, &mut rt);
    assert_eq!(e.fetches(), 1);
    // The subscription is made before the read: a signal sent from here on reaches it.
    read.recv().unwrap();
    nm.emit_signal(
        None::<&str>,
        NM_PATH,
        NM_IFACE,
        "StateChanged",
        &(CONNECTED_GLOBAL,),
    )
    .unwrap();
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.refetches == 1) && fetch_is(rt, Fetch::Idle)
    });
    quiet(&mut lp, &mut rt);
    let s = status(&rt).unwrap();
    assert_eq!(
        (s.refetches, e.fetches(), s.fetch),
        (1, 2, Fetch::Idle),
        "{s:?}"
    );
    assert_eq!(s.state, Some(CONNECTED_GLOBAL));
    drop(rt);
}

/// The system bus drops the shell after a fetch failed offline: the subscription is made again
/// on the next connection, so NetworkManager reporting connectivity there still repeats the
/// fetch (whether its signal or the new connection's first read of `State` says so).
#[test]
fn failed_fetch_offline_refetches_on_connect_after_the_bus_reconnects() {
    let Some(e) = env_with(
        concat!(
            module_path!(),
            "::failed_fetch_offline_refetches_on_connect_after_the_bus_reconnects"
        ),
        false,
        true,
    ) else {
        return;
    };
    use qml::weather::Fetch;
    let ends = relay();
    let nm = Nm::start(DISCONNECTED);
    let (mut rt, mut lp, _r) = start(CITY_CONFIG);
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.state == Some(DISCONNECTED)) && fetch_is(rt, Fetch::Offline)
    });
    quiet(&mut lp, &mut rt);
    assert_eq!(e.fetches(), 1);
    cut(&ends);
    nm.set(CONNECTED_GLOBAL);
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.refetches == 1) && fetch_is(rt, Fetch::Idle)
    });
    quiet(&mut lp, &mut rt);
    let s = status(&rt).unwrap();
    assert_eq!(
        (s.refetches, e.fetches(), s.fetch),
        (1, 2, Fetch::Idle),
        "{s:?}"
    );
    assert_eq!(s.state, Some(CONNECTED_GLOBAL));
    // The connection goes once more; a later loss of connectivity and its return are seen too.
    cut(&ends);
    nm.set(DISCONNECTED);
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.state == Some(DISCONNECTED))
    });
    drop(rt);
}

/// NetworkManager reaching connectivity while the fetch is still in flight: the fetch failing
/// afterwards is repeated at once, with no further signal.
#[test]
fn failed_fetch_refetches_when_network_connected_mid_fetch() {
    let Some(e) = env(
        concat!(
            module_path!(),
            "::failed_fetch_refetches_when_network_connected_mid_fetch"
        ),
        true,
    ) else {
        return;
    };
    use qml::weather::Fetch;
    let nm = Nm::start(DISCONNECTED);
    let (mut rt, mut lp, _r) = start(CITY_CONFIG);
    // The proxy holds the fetch until released: it stays in flight.
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.state == Some(DISCONNECTED))
    });
    assert!(
        fetch_is(&rt, Fetch::Running { rose: false }),
        "{:?}",
        status(&rt)
    );
    nm.set(CONNECTED_GLOBAL);
    wait(&mut lp, &mut rt, |rt| {
        fetch_is(rt, Fetch::Running { rose: true })
    });
    e.release();
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.refetches == 1) && fetch_is(rt, Fetch::Running { rose: false })
    });
    e.release();
    wait(&mut lp, &mut rt, |rt| fetch_is(rt, Fetch::Idle));
    quiet(&mut lp, &mut rt);
    let s = status(&rt).unwrap();
    assert_eq!(
        (s.refetches, e.fetches(), s.fetch),
        (1, 2, Fetch::Idle),
        "{s:?}"
    );
    drop(rt);
}

/// A fetch failing while online throughout starts no retry: Weather's timer covers it.
#[test]
fn failed_fetch_while_online_waits_for_the_timer() {
    let Some(e) = env(
        concat!(
            module_path!(),
            "::failed_fetch_while_online_waits_for_the_timer"
        ),
        false,
    ) else {
        return;
    };
    use qml::weather::Fetch;
    let _nm = Nm::start(CONNECTED_GLOBAL);
    let (mut rt, mut lp, r) = start(CITY_CONFIG);
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.state == Some(CONNECTED_GLOBAL)) && fetch_is(rt, Fetch::Idle)
    });
    quiet(&mut lp, &mut rt);
    let (n0, r0) = (e.fetches(), status(&rt).unwrap().refetches);
    assert_eq!((n0, r0), (1, 0), "{:?}", status(&rt));
    let Value::Item(weather) = rt.get(r, "weather") else {
        panic!("no Weather")
    };
    assert!(matches!(
        rt.call_method(weather, "getData", vec![]),
        Some(Ok(_))
    ));
    assert!(
        fetch_is(&rt, Fetch::Running { rose: false }),
        "{:?}",
        status(&rt)
    );
    wait(&mut lp, &mut rt, |rt| fetch_is(rt, Fetch::Idle));
    quiet(&mut lp, &mut rt);
    let s = status(&rt).unwrap();
    assert_eq!(
        (s.refetches, e.fetches(), s.fetch),
        (r0, n0 + 1, Fetch::Idle),
        "{s:?}"
    );
    drop(rt);
}

/// The match rules of Weather's NetworkManager subscription that this process's system bus
/// connection has, once the bus has handled everything it sent so far.
fn nm_rules(probe: &zbus::blocking::Connection) -> Vec<String> {
    let c = qml::dbus::system().unwrap();
    c.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus.Peer"),
        "Ping",
        &(),
    )
    .unwrap();
    let all: std::collections::HashMap<String, Vec<String>> = probe
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Debug.Stats"),
            "GetAllMatchRules",
            &(),
        )
        .unwrap()
        .body()
        .deserialize()
        .unwrap();
    all.get(c.unique_name().unwrap().as_str())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.contains("member='StateChanged'"))
        .collect()
}

/// Weather's subscription to NetworkManager's `StateChanged` ends at once when the Weather
/// object goes, also when NetworkManager never signals again: the thread that made it ended
/// after reading `State`.
#[test]
fn nm_subscription_ends_with_weather() {
    let Some(_e) = env(
        concat!(module_path!(), "::nm_subscription_ends_with_weather"),
        false,
    ) else {
        return;
    };
    let _nm = Nm::start(CONNECTED_GLOBAL);
    let (mut rt, mut lp, r) = start(CITY_CONFIG);
    wait(&mut lp, &mut rt, |rt| {
        status(rt).is_some_and(|s| s.state == Some(CONNECTED_GLOBAL))
    });
    quiet(&mut lp, &mut rt);
    let probe = zbus::blocking::Connection::session().unwrap();
    assert_eq!(nm_rules(&probe).len(), 1, "{:?}", nm_rules(&probe));
    let Value::Item(weather) = rt.get(r, "weather") else {
        panic!("no Weather")
    };
    rt.world.borrow_mut().destroy(weather);
    wait(&mut lp, &mut rt, |rt| status(rt).is_none());
    assert_eq!(
        nm_rules(&probe),
        Vec::<String>::new(),
        "the subscription outlived Weather"
    );
}
