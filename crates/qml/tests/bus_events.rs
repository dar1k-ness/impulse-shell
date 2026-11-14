//! MPRIS and SystemTray against fake services (zbus) on a private session `dbus-daemon`: a
//! player that exports its objects after taking its name, and how long a tray signal takes to
//! reach the item. Run with `--nocapture` for the measured latencies.

use qml::runtime::Runtime;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value as ZV};

/// Closing it (test process exit) makes the wrapper shell kill the daemon.
struct Bus(#[allow(dead_code)] ChildStdin);

/// In `test`'s own process (see `testenv::run_in_own_process`): a private session bus at the
/// address the environment names, a socket in a directory the parent made, as everything the
/// test runs finds the session bus through the environment. `None` in the process that ran the
/// test there, or without dbus-daemon.
fn bus(test: &str) -> Option<Bus> {
    bus_with(test, false)
}

/// [`bus`]; with `home`, the test's process also has a fresh home (`testenv::home_vars`), for
/// tests that load the original's own singletons.
fn bus_with(test: &str, home: bool) -> Option<Bus> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        let mut vars = vec![("DBUS_SESSION_BUS_ADDRESS", addr.into())];
        if home {
            vars.extend(testenv::home_vars(&dir.path().join("home")));
        }
        testenv::run_in_own_process(test, vars);
        return None;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let cfg = std::env::temp_dir().join(format!("qml-bus-events-{}.conf", std::process::id()));
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
            if self.wake.wait(self.ep, None).fd {
                rt.process_fd_ready();
            }
        }
    }
}

type Ping = Arc<dyn Fn() + Send + Sync>;

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

const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

/// A connection owning an MPRIS name before it exports any object (GLib's bus_own_name, then
/// register in `name_acquired`). It answers its Properties calls itself, on one thread, in the
/// order they arrive: UnknownObject until [`Unexported::export`] (reporting each failed GetAll
/// of the Player interface once sent), the exported player's values after it. Each GetAll of
/// the Player interface is counted before it is answered, so a count read once the service had
/// that answer includes the call. Not zbus's ObjectServer: the first `object_server()` starts
/// its dispatch task without waiting for it to subscribe, so a call arriving before that is
/// lost, and its answers would race this thread's count.
struct Unexported {
    conn: zbus::blocking::Connection,
    gets: Arc<AtomicUsize>,
    /// What each Get of Position was answered with, µs, in the order they arrived.
    positions: Arc<Mutex<Vec<i64>>>,
    player: Arc<Mutex<Option<Player>>>,
    failed: Receiver<()>,
}

impl Unexported {
    fn new(name: &str) -> Unexported {
        let conn = zbus::blocking::connection::Builder::session()
            .unwrap()
            .build()
            .unwrap();
        // Created before the name is taken: no call to it can be missed.
        let it = zbus::blocking::MessageIterator::from(&conn);
        let gets = Arc::new(AtomicUsize::new(0));
        let positions: Arc<Mutex<Vec<i64>>> = Arc::default();
        let player: Arc<Mutex<Option<Player>>> = Arc::default();
        let (tx, failed) = channel();
        let (c, g, p, read) = (
            conn.clone(),
            gets.clone(),
            player.clone(),
            positions.clone(),
        );
        std::thread::spawn(move || {
            for m in it.flatten() {
                let h = m.header();
                if h.message_type() != zbus::message::Type::MethodCall
                    || h.interface().map(|i| i.as_str()) != Some("org.freedesktop.DBus.Properties")
                {
                    continue;
                }
                let member = h.member().map(|m| m.to_string()).unwrap_or_default();
                let itf = match member.as_str() {
                    "GetAll" => m.body().deserialize::<String>().ok(),
                    "Get" => m
                        .body()
                        .deserialize::<(String, String)>()
                        .ok()
                        .map(|(i, _)| i),
                    _ => None,
                };
                let getall_player = member == "GetAll" && itf.as_deref() == Some(PLAYER_IFACE);
                if getall_player {
                    g.fetch_add(1, Ordering::SeqCst);
                }
                let mut p = p.lock().unwrap();
                let props = match (p.as_ref(), itf.as_deref()) {
                    (None, _) => None,
                    (Some(p), Some(PLAYER_IFACE)) => Some(p.props()),
                    (Some(_), Some("org.mpris.MediaPlayer2")) => Some(Root.props()),
                    (Some(_), _) => {
                        let _ = c.reply_error(
                            &h,
                            "org.freedesktop.DBus.Error.UnknownInterface",
                            &"no such interface",
                        );
                        continue;
                    }
                };
                let Some(props) = props else {
                    let _ = c.reply_error(
                        &h,
                        "org.freedesktop.DBus.Error.UnknownObject",
                        &"not exported yet",
                    );
                    if getall_player {
                        let _ = tx.send(());
                    }
                    continue;
                };
                if member == "GetAll" {
                    let _ = c.reply(&h, &props);
                    continue;
                }
                let k = m.body().deserialize::<(String, String)>().map(|(_, k)| k);
                let k = k.ok();
                match k.as_ref().and_then(|k| props.get(k).cloned()) {
                    Some(v) => {
                        if itf.as_deref() == Some(PLAYER_IFACE)
                            && k.as_deref() == Some("Position")
                            && let Some(p) = p.as_mut()
                        {
                            read.lock().unwrap().push(p.pos);
                            p.pos = p.then.take().unwrap_or(p.pos);
                        }
                        let _ = c.reply(&h, &ZV::from(v));
                    }
                    None => {
                        let _ = c.reply_error(
                            &h,
                            "org.freedesktop.DBus.Error.UnknownProperty",
                            &"no such property",
                        );
                    }
                }
            }
        });
        conn.request_name(format!("org.mpris.MediaPlayer2.{name}"))
            .unwrap();
        Unexported {
            conn,
            gets,
            positions,
            player,
            failed,
        }
    }

    /// Blocks until a GetAll was answered with an error.
    fn failed(&self) {
        eprintln!("waiting for a failed GetAll");
        self.failed.recv().unwrap();
    }

    /// GetAll calls of the Player interface so far.
    fn gets(&self) -> usize {
        self.gets.load(Ordering::SeqCst)
    }

    /// What the Gets of Position so far were answered with, µs.
    fn positions(&self) -> Vec<i64> {
        self.positions.lock().unwrap().clone()
    }

    /// Every call after this gets the player's values.
    fn export(&self, player: Player) {
        *self.player.lock().unwrap() = Some(player);
    }

    /// Sets the exported player's PlaybackStatus and signals it, as its ObjectServer would.
    fn set_status(&self, status: &str) {
        let v = {
            let mut p = self.player.lock().unwrap();
            let p = p.as_mut().unwrap();
            p.status = status.into();
            p.playback_status()
        };
        let changed = HashMap::from([("PlaybackStatus", ZV::from(v))]);
        self.conn
            .emit_signal(
                None::<&str>,
                "/org/mpris/MediaPlayer2",
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(PLAYER_IFACE, changed, Vec::<String>::new()),
            )
            .unwrap();
    }
}

/// The MPRIS service is subscribed and has listed the bus: it fetched a name taken after it
/// started, so every later name reaches it once, through NameOwnerChanged. Returns that name's
/// connection (a name with no player object, which the service then leaves alone).
fn mpris_up() -> Unexported {
    let u = Unexported::new("warmup");
    u.failed();
    u
}

fn call(rt: &mut Runtime, root: usize, f: &str) -> String {
    rt.call_method(root, f, vec![])
        .unwrap()
        .unwrap()
        .to_js_string()
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

struct Root;

impl Root {
    /// What zbus's ObjectServer answers GetAll of the interface with.
    fn props(&self) -> HashMap<String, OwnedValue> {
        HashMap::from([(
            "Identity".into(),
            OwnedValue::try_from(ZV::from(self.identity())).unwrap(),
        )])
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        "Fake Player".into()
    }
}

struct Player {
    status: String,
    title: String,
    art: String,
    /// Position, µs.
    pos: i64,
    /// Where it moves once it has answered the next Get of Position ([`Unexported`]), µs.
    then: Option<i64>,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.status.clone()
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let mut m = HashMap::from([(
            "xesam:title".to_string(),
            OwnedValue::try_from(ZV::from(self.title.as_str())).unwrap(),
        )]);
        if !self.art.is_empty() {
            m.insert(
                "mpris:artUrl".to_string(),
                OwnedValue::try_from(ZV::from(self.art.as_str())).unwrap(),
            );
        }
        m
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        self.pos
    }
}

impl Player {
    /// What zbus's ObjectServer answers GetAll of the interface with.
    fn props(&self) -> HashMap<String, OwnedValue> {
        let v = |v: ZV| OwnedValue::try_from(v).unwrap();
        HashMap::from([
            ("PlaybackStatus".into(), v(ZV::from(self.playback_status()))),
            ("Metadata".into(), v(ZV::from(self.metadata()))),
            ("CanControl".into(), v(ZV::from(self.can_control()))),
            ("Position".into(), v(ZV::from(self.position()))),
        ])
    }
}

const MPRIS_QML: &str = "import QtQuick\nimport Rustshell.Services.Mpris\nItem {\n\
    function title() { const p = Mpris.players.values; return p.length ? p[0].trackTitle : '' }\n\
    function playing() { const p = Mpris.players.values; return p.length ? '' + p[0].isPlaying : '' } }";

#[test]
fn mpris_player_exported_after_its_name_becomes_ready() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::mpris_player_exported_after_its_name_becomes_ready"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt.load_str(MPRIS_QML).unwrap();
    rt.settle();
    // Instantiates the Mpris singleton; its service starts on the next poll.
    assert_eq!(call(&mut rt, root, "title"), "");
    turn(&mut rt);
    let _warmup = mpris_up();
    // Name first, objects later: the GetAll on the name's appearance fails; the player's first
    // PropertiesChanged fetches again.
    let fake = Unexported::new("fake");
    fake.failed();
    fake.export(Player {
        status: "Paused".into(),
        title: "Song".into(),
        art: String::new(),
        pos: 0,
        then: None,
    });
    turn(&mut rt);
    assert_eq!(
        call(&mut rt, root, "title"),
        "",
        "GetAll repeated without an event"
    );
    let asked = fake.gets();
    fake.set_status("Paused");
    lp.until(&mut rt, "the late-exported player", |rt| {
        call(rt, root, "title") == "Song"
    });
    assert_eq!(
        fake.gets(),
        asked + 1,
        "one GetAll for the signal, none without an event"
    );
    assert_eq!(call(&mut rt, root, "playing"), "false");
    let mut lat = vec![];
    for i in 0..20 {
        let on = i % 2 == 0;
        let t = Instant::now();
        fake.set_status(if on { "Playing" } else { "Paused" });
        let want = if on { "true" } else { "false" };
        lp.until(&mut rt, "isPlaying", |rt| call(rt, root, "playing") == want);
        lat.push(t.elapsed());
    }
    eprintln!(
        "mpris PlaybackStatus -> isPlaying: median {:?}",
        median(lat)
    );
}

/// Quickshell reads Position again each time a player announces its PlaybackStatus, changed or
/// not (`MprisPlayer::onPlaybackStatusUpdated`, the updated callback of the D-Bus property), at
/// once and 100 ms later: a player that moved without a Seeked signal, or refused a SetPosition
/// and announced its state, is shown where it is, also when it moves after it has announced.
#[test]
fn mpris_position_read_again_when_the_status_is_announced() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::mpris_position_read_again_when_the_status_is_announced"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Services.Mpris\nItem {\n\
             function pos() { const p = Mpris.players.values; return p.length ? '' + p[0].position : '' } }",
        )
        .unwrap();
    rt.settle();
    assert_eq!(call(&mut rt, root, "pos"), "");
    turn(&mut rt);
    let _warmup = mpris_up();
    let fake = Unexported::new("seeker");
    fake.failed();
    fake.export(Player {
        status: "Paused".into(),
        title: "Song".into(),
        art: String::new(),
        pos: 0,
        then: None,
    });
    // The first sign of its objects: its values come with the GetAll this triggers.
    fake.set_status("Paused");
    lp.until(&mut rt, "the player", |rt| call(rt, root, "pos") == "0");
    assert_eq!(fake.positions(), [0i64; 0], "Position came with GetAll");
    // It moves without a Seeked signal and announces the status it already had; it moves again
    // once it has answered the read that follows, and says nothing more.
    {
        let mut p = fake.player.lock().unwrap();
        let p = p.as_mut().unwrap();
        p.pos = 42_000_000;
        p.then = Some(77_000_000);
    }
    fake.set_status("Paused");
    lp.until(&mut rt, "the position read 100 ms later", |rt| {
        call(rt, root, "pos") == "77"
    });
    assert_eq!(
        fake.positions(),
        [42_000_000, 77_000_000],
        "read at once, and once more later"
    );
}

fn stub(root: &std::path::Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn player(name: &str, status: &str) -> zbus::blocking::Connection {
    let path = "/org/mpris/MediaPlayer2";
    let p = Player {
        status: status.into(),
        title: name.into(),
        art: String::new(),
        pos: 0,
        then: None,
    };
    // Exported before the name is taken, as real players do: no GetAll retry window.
    zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at(path, Root)
        .unwrap()
        .serve_at(path, p)
        .unwrap()
        .name(format!("org.mpris.MediaPlayer2.{name}"))
        .unwrap()
        .build()
        .unwrap()
}

fn set_status(conn: &zbus::blocking::Connection, status: &str) {
    let iface = conn
        .object_server()
        .interface::<_, Player>("/org/mpris/MediaPlayer2")
        .unwrap();
    iface.get_mut().status = status.into();
    zbus::block_on(iface.get().playback_status_changed(iface.signal_emitter())).unwrap();
}

/// end-4/dots-hyprland #3099: the tree's `MprisController.qml` took any player whose PlaybackStatus
/// changed, so a player that stopped took `trackedPlayer` from the one still playing.
#[test]
fn mpris_controller_keeps_the_playing_player() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::mpris_controller_keeps_the_playing_player"
    )) else {
        return;
    };
    let tree = std::env::temp_dir().join(format!("qml-bus-events-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tree);
    let ri = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    stub(
        &tree,
        "services/MprisController.qml",
        &std::fs::read_to_string(ri.join("services/MprisController.qml")).unwrap(),
    );
    let single = |props: &str| {
        format!("pragma Singleton\nimport QtQuick\nimport Quickshell\nSingleton {{\n{props}\n}}\n")
    };
    stub(
        &tree,
        "modules/common/Config.qml",
        &single("property var options: ({ media: { filterDuplicatePlayers: false } })"),
    );
    stub(
        &tree,
        "services/Translation.qml",
        &single("function tr(s) { return s }"),
    );
    let mut rt = Runtime::new(Some(tree.clone()));
    let lp = Loop::new(&mut rt);
    rt.load_singletons(&[tree.join("modules/common"), tree.join("services")]);
    rt.settle();
    let root = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Services.Mpris\nimport ri.services\nItem {\n\
             function tracked() { return MprisController.trackedPlayer?.trackTitle ?? '' }\n\
             function states() { return Mpris.players.values.map(p => p.trackTitle + ':' + p.playbackState).sort().join(',') } }",
        )
        .unwrap();
    rt.settle();
    let a = player("a", "Playing");
    lp.until(&mut rt, "a tracked", |rt| call(rt, root, "tracked") == "a");
    let b = player("b", "Paused");
    lp.until(&mut rt, "player b", |rt| {
        call(rt, root, "states").contains("b:")
    });
    assert_eq!(call(&mut rt, root, "tracked"), "a");
    let before = call(&mut rt, root, "states");
    set_status(&b, "Stopped");
    lp.until(&mut rt, "b stopped", |rt| {
        call(rt, root, "states") != before
    });
    turn(&mut rt);
    assert_eq!(
        call(&mut rt, root, "tracked"),
        "a",
        "a stopped player took over"
    );
    set_status(&b, "Playing");
    // A player that starts playing takes over.
    lp.until(&mut rt, "b tracked", |rt| call(rt, root, "tracked") == "b");
    set_status(&a, "Paused");
    let before = call(&mut rt, root, "states");
    lp.until(&mut rt, "a paused", |rt| call(rt, root, "states") != before);
    assert_eq!(call(&mut rt, root, "tracked"), "b");
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// A player that changes its art right after appearing (mpv-mpris, cantata) reached QML in the
/// same drain as its readiness: `MprisController`'s `onTrackArtUrlChanged` ran before
/// `onActivePlayerChanged` had set `activeTrack` (TypeError at MprisController.qml:89).
#[test]
fn mpris_controller_sees_a_new_player_before_its_changes() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::mpris_controller_sees_a_new_player_before_its_changes"
    )) else {
        return;
    };
    let tree = std::env::temp_dir().join(format!("qml-bus-events-art-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tree);
    let ri = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    stub(
        &tree,
        "services/MprisController.qml",
        &std::fs::read_to_string(ri.join("services/MprisController.qml")).unwrap(),
    );
    let single = |props: &str| {
        format!("pragma Singleton\nimport QtQuick\nimport Quickshell\nSingleton {{\n{props}\n}}\n")
    };
    stub(
        &tree,
        "modules/common/Config.qml",
        &single("property var options: ({ media: { filterDuplicatePlayers: false } })"),
    );
    stub(
        &tree,
        "services/Translation.qml",
        &single("function tr(s) { return s }"),
    );
    let mut rt = Runtime::new(Some(tree.clone()));
    let lp = Loop::new(&mut rt);
    rt.load_singletons_lazy(&[tree.join("modules/common"), tree.join("services")]);
    rt.settle();
    let root = rt
        .load_str(
            "import QtQuick\nimport ri.services\nItem {\n\
             function art() { return MprisController.activeTrack?.artUrl ?? 'undefined' } }",
        )
        .unwrap();
    rt.settle();
    assert_eq!(call(&mut rt, root, "art"), "undefined");
    turn(&mut rt);
    let _warmup = mpris_up();
    // Everything the service queued so far is handled; no ping is left over.
    turn(&mut rt);
    lp.wake.wait(None, Some(Duration::ZERO));
    // Not polled meanwhile: readiness and the art change are queued together. The service's
    // first ping after the turn is the player's readiness (the only thing it queues).
    let a = player("art", "Playing");
    lp.wake.wait(None, None);
    let iface = a
        .object_server()
        .interface::<_, Player>("/org/mpris/MediaPlayer2")
        .unwrap();
    iface.get_mut().art = "file:///cover.png".into();
    zbus::block_on(iface.get().metadata_changed(iface.signal_emitter())).unwrap();
    // The change is queued once the service read the signal: the bus has routed it when it
    // answers the player's round trip, and the service reads its messages in order, so it saw
    // the change before a name that appears after that.
    zbus::blocking::fdo::DBusProxy::new(&a)
        .unwrap()
        .get_id()
        .unwrap();
    let marker = Unexported::new("marker");
    marker.failed();
    lp.until(&mut rt, "the new art", |rt| {
        call(rt, root, "art") == "file:///cover.png"
    });
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// One connection owning several MPRIS names (KDE Connect: one per phone/player) gets a player
/// per name, as Quickshell's MprisWatcher does; releasing one name keeps the others, and the
/// connection's PropertiesChanged still reaches them.
#[test]
fn mpris_one_connection_with_several_names() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::mpris_one_connection_with_several_names"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Services.Mpris\nItem {\n\
             function names() { return Mpris.players.values.map(p => p.dbusName + ':' + p.isPlaying).sort().join(',') } }",
        )
        .unwrap();
    rt.settle();
    // Instantiates the Mpris singleton; its service starts on the next poll.
    assert_eq!(call(&mut rt, root, "names"), "");
    turn(&mut rt);
    let conn = player("multi.a", "Paused");
    conn.request_name("org.mpris.MediaPlayer2.multi.b").unwrap();
    let both = "org.mpris.MediaPlayer2.multi.a:false,org.mpris.MediaPlayer2.multi.b:false";
    lp.until(&mut rt, "both names", |rt| call(rt, root, "names") == both);
    conn.release_name("org.mpris.MediaPlayer2.multi.a").unwrap();
    let b = "org.mpris.MediaPlayer2.multi.b:false";
    lp.until(&mut rt, "name a released", |rt| {
        call(rt, root, "names") == b
    });
    set_status(&conn, "Playing");
    let b = "org.mpris.MediaPlayer2.multi.b:true";
    lp.until(&mut rt, "b playing", |rt| call(rt, root, "names") == b);
}

struct Item {
    title: String,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn id(&self) -> String {
        "fake".into()
    }
    #[zbus(property)]
    fn title(&self) -> String {
        self.title.clone()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn category(&self) -> String {
        "ApplicationStatus".into()
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        "folder".into()
    }
    #[zbus(signal)]
    async fn new_title(e: &SignalEmitter<'_>) -> zbus::Result<()>;
}

const TRAY_QML: &str = "import QtQuick\nimport Rustshell.Services.SystemTray\nItem {\n\
    function title() { const v = SystemTray.items.values; return v.length ? v[0].title : '' } }";

#[test]
fn tray_item_title_follows_new_title() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::tray_item_title_follows_new_title"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt.load_str(TRAY_QML).unwrap();
    rt.settle();
    // Instantiates the SystemTray singleton, which starts the watcher on the next poll.
    assert_eq!(call(&mut rt, root, "title"), "");
    turn(&mut rt);
    watcher_up();
    let conn = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at("/StatusNotifierItem", Item { title: "t0".into() })
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
    lp.until(&mut rt, "the item", |rt| call(rt, root, "title") == "t0");
    let iface = conn
        .object_server()
        .interface::<_, Item>("/StatusNotifierItem")
        .unwrap();
    let mut lat = vec![];
    for i in 1..=20 {
        let want = format!("t{i}");
        iface.get_mut().title = want.clone();
        let t = Instant::now();
        zbus::block_on(Item::new_title(iface.signal_emitter())).unwrap();
        lp.until(&mut rt, "the new title", |rt| {
            call(rt, root, "title") == want
        });
        lat.push(t.elapsed());
    }
    eprintln!("tray NewTitle -> title: median {:?}", median(lat));
}

type Calls = Arc<Mutex<Vec<(String, Instant)>>>;
/// What reached the probe's connection, in arrival order: `Get <property>` and `Mark`.
type Arrivals = Arc<Mutex<Vec<String>>>;

const PROBE: &str = "org.mpris.MediaPlayer2.probe";

/// A player whose method calls and hung `Next` the tests observe.
struct Probe {
    status: String,
    volume: f64,
    calls: Calls,
    ping: Ping,
}

impl Probe {
    /// Logs `c`, marked when the caller asked for a reply.
    fn log(&self, c: String, h: &zbus::message::Header<'_>) {
        let flags = h.primary().flags();
        let c = if flags.contains(zbus::message::Flags::NoReplyExpected) {
            c
        } else {
            format!("{c} (reply expected)")
        };
        self.calls.lock().unwrap().push((c, Instant::now()));
        (self.ping)();
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Probe {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.status.clone()
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let path = zbus::zvariant::ObjectPath::from_static_str_unchecked(
            "/org/mpris/MediaPlayer2/TrackList/NoTrack",
        );
        HashMap::from([
            (
                "mpris:trackid".into(),
                OwnedValue::try_from(ZV::from(path)).unwrap(),
            ),
            (
                "xesam:title".into(),
                OwnedValue::try_from(ZV::from("probe")).unwrap(),
            ),
            // `t` where the spec says `x` (some players do): saturates, never negative.
            (
                "mpris:length".into(),
                OwnedValue::try_from(ZV::from(u64::MAX)).unwrap(),
            ),
        ])
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.volume
    }
    /// `&self`: the never-answered `Next` holds the interface read lock, so a `&mut self`
    /// setter would wait on the probe, not on the shell.
    #[zbus(property)]
    fn set_volume(&self, v: f64, #[zbus(header)] h: Option<zbus::message::Header<'_>>) {
        if let Some(h) = h {
            self.log(format!("Set Volume {v}"), &h);
        }
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        0
    }
    fn set_position(
        &self,
        _id: zbus::zvariant::ObjectPath<'_>,
        x: i64,
        #[zbus(header)] h: zbus::message::Header<'_>,
    ) {
        self.log(format!("SetPosition {x}"), &h);
    }
    fn seek(&self, x: i64, #[zbus(header)] h: zbus::message::Header<'_>) {
        self.log(format!("Seek {x}"), &h);
    }
    /// Never answers.
    async fn next(&self, #[zbus(header)] h: zbus::message::Header<'_>) {
        self.log("Next".into(), &h);
        std::future::pending::<()>().await
    }
    fn previous(&self, #[zbus(header)] h: zbus::message::Header<'_>) {
        self.log("Previous".into(), &h);
    }
}

const PROBE_QML: &str = "import QtQuick\nimport Rustshell.Services.Mpris\nItem {\n\
    property var m0: null\n\
    function pl() { for (const p of Mpris.players.values) if (p.trackTitle === 'probe') return p; return null }\n\
    function ready() { return '' + (pl() !== null) }\n\
    function length() { return '' + pl().length }\n\
    function volume() { return '' + pl().volume }\n\
    function playing() { return '' + pl().isPlaying }\n\
    function snap() { m0 = pl().metadata; return '' }\n\
    function same() { return '' + (m0 === pl().metadata) }\n\
    function seek() { pl().position = 30; return '' }\n\
    function next() { pl().next(); return '' }\n\
    function setVolume() { pl().volume = 0.75; return '' }\n\
    function previous() { pl().previous(); return '' } }";

/// The probe player; each method call it logs and each arrival pings `ping`.
fn probe(ping: Ping) -> (zbus::blocking::Connection, Arrivals, Calls) {
    let calls = Calls::default();
    let p = Probe {
        status: "Playing".into(),
        volume: 0.5,
        calls: calls.clone(),
        ping: ping.clone(),
    };
    let path = "/org/mpris/MediaPlayer2";
    let conn = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at(path, Root)
        .unwrap()
        .serve_at(path, p)
        .unwrap()
        .build()
        .unwrap();
    // Created before the name is taken: no call to it can be missed.
    let it = zbus::blocking::MessageIterator::from(&conn);
    let arrivals = Arrivals::default();
    let a = arrivals.clone();
    std::thread::spawn(move || {
        for m in it.flatten() {
            let h = m.header();
            if h.message_type() != zbus::message::Type::MethodCall {
                continue;
            }
            let seen = match h.member().map(|m| m.to_string()).as_deref() {
                Some("Get") => match m.body().deserialize::<(String, String)>() {
                    Ok((_, p)) => format!("Get {p}"),
                    Err(_) => continue,
                },
                Some("Mark") => "Mark".into(),
                _ => continue,
            };
            a.lock().unwrap().push(seen);
            ping();
        }
    });
    conn.request_name(PROBE).unwrap();
    (conn, arrivals, calls)
}

/// A `Mark` call to `dest` from the runtime's own session connection, run as a task on that
/// connection's executor: it goes out behind every call the runtime's tasks queued before it,
/// and the bus keeps one sender's messages in order. Returns once answered (with an error: no
/// one serves it).
fn mark(dest: &'static str) {
    let c = qml::dbus::session().unwrap().inner().clone();
    let on = c.clone();
    let task = c.executor().spawn(
        async move {
            let _ = on
                .call_method(Some(dest), "/mark", Some("org.test.Mark"), "Mark", &())
                .await;
        },
        "test mark",
    );
    zbus::block_on(task).expect("mark task");
}

/// `Get Position` calls that reached the probe before its last `Mark`.
fn position_gets(a: &Arrivals) -> usize {
    let a = a.lock().unwrap();
    let end = a.iter().rposition(|x| x == "Mark").unwrap_or(a.len());
    a[..end].iter().filter(|x| *x == "Get Position").count()
}

fn marked(a: &Arrivals) -> usize {
    a.lock().unwrap().iter().filter(|x| *x == "Mark").count()
}

fn probe_iface(
    conn: &zbus::blocking::Connection,
) -> zbus::blocking::object_server::InterfaceRef<Probe> {
    conn.object_server()
        .interface::<_, Probe>("/org/mpris/MediaPlayer2")
        .unwrap()
}

/// MPRIS Player spec: Position is read once per PlaybackStatus change (no second timed read),
/// a Volume change leaves `metadata` the same object, NoTrack positions by `Seek`, commands and
/// property writes go out NO_REPLY_EXPECTED in order (a player that never answers holds
/// nothing back), and a `t` `mpris:length` saturates.
#[test]
fn mpris_player_follows_the_spec() {
    let Some(_bus) = bus(concat!(module_path!(), "::mpris_player_follows_the_spec")) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt.load_str(PROBE_QML).unwrap();
    rt.settle();
    // Instantiates the Mpris singleton; its service starts on the next poll.
    assert_eq!(call(&mut rt, root, "ready"), "false");
    turn(&mut rt);
    let (conn, arrivals, calls) = probe(lp.ping());
    lp.until(&mut rt, "the probe player", |rt| {
        call(rt, root, "ready") == "true"
    });
    let len: f64 = call(&mut rt, root, "length").parse().unwrap();
    assert!(len > 1e12, "u64 mpris:length saturates: {len}");

    // Readiness came with one GetAll: no Position read is outstanding.
    let before = arrivals
        .lock()
        .unwrap()
        .iter()
        .filter(|x| *x == "Get Position")
        .count();
    let iface = probe_iface(&conn);
    iface.get_mut().status = "Paused".into();
    zbus::block_on(iface.get().playback_status_changed(iface.signal_emitter())).unwrap();
    lp.until(&mut rt, "isPlaying false", |rt| {
        call(rt, root, "playing") == "false"
    });
    lp.until(&mut rt, "the Position read", |_| {
        arrivals
            .lock()
            .unwrap()
            .iter()
            .filter(|x| *x == "Get Position")
            .count()
            > before
    });
    // Whatever else the status change made the runtime ask arrived before this mark.
    turn(&mut rt);
    let n = marked(&arrivals);
    mark(PROBE);
    lp.until(&mut rt, "the mark", |_| marked(&arrivals) > n);
    assert_eq!(
        position_gets(&arrivals) - before,
        1,
        "one Position Get per status change"
    );

    call(&mut rt, root, "snap");
    iface.get_mut().volume = 0.25;
    zbus::block_on(iface.get().volume_changed(iface.signal_emitter())).unwrap();
    lp.until(&mut rt, "the new volume", |rt| {
        call(rt, root, "volume") == "0.25"
    });
    assert_eq!(
        call(&mut rt, root, "same"),
        "true",
        "metadata rebuilt on a Volume change"
    );

    call(&mut rt, root, "seek");
    let seeked = || {
        calls
            .lock()
            .unwrap()
            .iter()
            .any(|(c, _)| c.starts_with("Seek "))
    };
    lp.until(&mut rt, "Seek", |_| seeked());
    assert!(
        !calls
            .lock()
            .unwrap()
            .iter()
            .any(|(c, _)| c.starts_with("SetPosition")),
        "SetPosition with NoTrack"
    );

    call(&mut rt, root, "next");
    call(&mut rt, root, "previous");
    call(&mut rt, root, "setVolume");
    let done = || {
        calls
            .lock()
            .unwrap()
            .iter()
            .any(|(c, _)| c.starts_with("Set Volume"))
    };
    // `Next` never answers: the later commands go out without waiting for its reply.
    lp.until(&mut rt, "Set Volume behind the hung Next", |_| done());
    let log: Vec<String> = calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
    let tail = &log[log.len() - 3..];
    assert_eq!(
        tail,
        ["Next", "Previous", "Set Volume 0.75"],
        "commands out of order or waiting for replies: {log:?}"
    );
    assert!(
        log.iter().all(|c| !c.ends_with("(reply expected)")),
        "{log:?}"
    );
    // The hung call keeps the connection alive past its drop: give the name up explicitly.
    conn.release_name(PROBE).unwrap();
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// Resumes a SIGSTOPped process when dropped.
struct Stopped(u32);

impl Drop for Stopped {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-CONT", &self.0.to_string()])
            .status();
    }
}

/// Starting MPRIS waits on the bus daemon (AddMatch, ListNames, GetNameOwner) off the main
/// loop: the first poll returns while the daemon is frozen (were it to wait on the daemon, it
/// would never return, and nextest's slow-timeout ends the test), and players appear once the
/// daemon resumes.
#[test]
fn mpris_start_does_not_block_the_main_loop() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::mpris_start_does_not_block_the_main_loop"
    )) else {
        return;
    };
    let a = player("frozen", "Playing");
    let pid = zbus::blocking::fdo::DBusProxy::new(&a)
        .unwrap()
        .get_connection_unix_process_id(
            zbus::names::BusName::try_from("org.freedesktop.DBus").unwrap(),
        )
        .unwrap();
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt.load_str(MPRIS_QML).unwrap();
    rt.settle();
    // Instantiates the Mpris singleton; its service starts on the next poll.
    call(&mut rt, root, "title");
    {
        assert!(
            Command::new("kill")
                .args(["-STOP", &pid.to_string()])
                .status()
                .unwrap()
                .success()
        );
        let _resume = Stopped(pid);
        rt.settle();
        rt.poll_native();
    }
    lp.until(&mut rt, "the player after the daemon resumed", |rt| {
        call(rt, root, "title") == "frozen"
    });
    drop(a);
}

type Gets = std::sync::Arc<std::sync::atomic::AtomicUsize>;

/// A player whose next bus Get of `Metadata` (once `hold` is armed) reads the title, then
/// answers only when the test releases it. Each Get it answers pings the test's loop.
struct Slow {
    title: std::sync::Arc<Mutex<String>>,
    gets: Gets,
    seen: Ping,
    hold: std::sync::Arc<std::sync::atomic::AtomicBool>,
    release: async_channel::Receiver<()>,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Slow {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        "Playing".into()
    }
    #[zbus(property)]
    async fn metadata(
        &self,
        #[zbus(header)] h: Option<zbus::message::Header<'_>>,
    ) -> HashMap<String, OwnedValue> {
        let title = self.title.lock().unwrap().clone();
        if h.is_some() {
            self.gets.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            (self.seen)();
            if self.hold.swap(false, std::sync::atomic::Ordering::SeqCst) {
                let _ = self.release.recv().await;
            }
        }
        HashMap::from([(
            "xesam:title".to_string(),
            OwnedValue::try_from(ZV::from(title.as_str())).unwrap(),
        )])
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// A Get started by an invalidation and answered after a newer PropertiesChanged carried the
/// property is stale: the newer value stays and the property is read again.
#[test]
fn mpris_stale_get_does_not_overwrite_a_newer_signal() {
    use std::sync::atomic::Ordering::SeqCst;
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::mpris_stale_get_does_not_overwrite_a_newer_signal"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let root = rt.load_str(MPRIS_QML).unwrap();
    rt.settle();
    let lp = Loop::new(&mut rt);
    // Instantiates the Mpris singleton; its service starts on the next poll.
    assert_eq!(call(&mut rt, root, "title"), "");
    turn(&mut rt);
    let _warmup = mpris_up();
    let (release_tx, release) = async_channel::unbounded();
    let slow = Slow {
        title: std::sync::Arc::new(Mutex::new("A".into())),
        gets: Default::default(),
        seen: lp.ping(),
        hold: Default::default(),
        release,
    };
    let (title, gets, hold) = (slow.title.clone(), slow.gets.clone(), slow.hold.clone());
    let path = "/org/mpris/MediaPlayer2";
    let conn = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at(path, Root)
        .unwrap()
        .serve_at(path, slow)
        .unwrap()
        .name("org.mpris.MediaPlayer2.slow")
        .unwrap()
        .build()
        .unwrap();
    lp.until(&mut rt, "the first title", |rt| {
        call(rt, root, "title") == "A"
    });
    let iface = conn.object_server().interface::<_, Slow>(path).unwrap();
    let base = gets.load(SeqCst);

    hold.store(true, SeqCst);
    zbus::block_on(zbus::fdo::Properties::properties_changed(
        iface.signal_emitter(),
        "org.mpris.MediaPlayer2.Player".try_into().unwrap(),
        HashMap::new(),
        std::borrow::Cow::Borrowed(&["Metadata"]),
    ))
    .unwrap();
    lp.until(&mut rt, "the Get after the invalidation", |_| {
        gets.load(SeqCst) == base + 1
    });
    *title.lock().unwrap() = "B".into();
    zbus::block_on(iface.get().metadata_changed(iface.signal_emitter())).unwrap();
    lp.until(&mut rt, "the newer title", |rt| {
        call(rt, root, "title") == "B"
    });

    // The held reply carries "A"; reading again means it was recognised as stale.
    release_tx.send_blocking(()).unwrap();
    lp.until(&mut rt, "the stale reply read again", |_| {
        gets.load(SeqCst) == base + 2
    });
    assert_eq!(call(&mut rt, root, "title"), "B", "stale Get applied");

    *title.lock().unwrap() = "C".into();
    zbus::block_on(iface.get().metadata_changed(iface.signal_emitter())).unwrap();
    lp.until(&mut rt, "the next title", |rt| {
        call(rt, root, "title") == "C"
    });
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// A player's `org.mpris.MediaPlayer2` whose name changes.
struct Named(std::sync::Arc<Mutex<String>>);

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Named {
    #[zbus(property)]
    fn identity(&self) -> String {
        self.0.lock().unwrap().clone()
    }
}

/// A player whose volume and shuffle change.
struct Knobs(std::sync::Arc<Mutex<(f64, bool)>>);

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Knobs {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        "Paused".into()
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.0.lock().unwrap().0
    }
    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.0.lock().unwrap().1
    }
}

/// A player that lists a property as invalidated instead of sending its value has it read
/// again, whichever of the properties shown it is and on the interface it belongs to, as
/// Quickshell does (`DBusPropertyGroup::onPropertiesChanged` asks for every bound property a
/// PropertiesChanged invalidates).
#[test]
fn mpris_invalidated_properties_are_read_again() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::mpris_invalidated_properties_are_read_again"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Services.Mpris\nItem {\n\
             function shown() { const p = Mpris.players.values;\n\
             return p.length ? p[0].identity + ' ' + p[0].volume + ' ' + p[0].shuffle : '' } }",
        )
        .unwrap();
    rt.settle();
    let lp = Loop::new(&mut rt);
    // Instantiates the Mpris singleton; its service starts on the next poll.
    assert_eq!(call(&mut rt, root, "shown"), "");
    turn(&mut rt);
    let _warmup = mpris_up();
    let name = std::sync::Arc::new(Mutex::new("First".to_string()));
    let knobs = std::sync::Arc::new(Mutex::new((0.5, false)));
    let path = "/org/mpris/MediaPlayer2";
    let conn = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at(path, Named(name.clone()))
        .unwrap()
        .serve_at(path, Knobs(knobs.clone()))
        .unwrap()
        .name("org.mpris.MediaPlayer2.knobs")
        .unwrap()
        .build()
        .unwrap();
    lp.until(&mut rt, "the player", |rt| {
        call(rt, root, "shown") == "First 0.5 false"
    });
    let invalidate = |emitter: &zbus::object_server::SignalEmitter<'_>,
                      iface: &'static str,
                      props: &'static [&'static str]| {
        zbus::block_on(zbus::fdo::Properties::properties_changed(
            emitter,
            iface.try_into().unwrap(),
            HashMap::new(),
            std::borrow::Cow::Borrowed(props),
        ))
        .unwrap();
    };

    *knobs.lock().unwrap() = (0.25, true);
    let player = conn.object_server().interface::<_, Knobs>(path).unwrap();
    invalidate(
        player.signal_emitter(),
        "org.mpris.MediaPlayer2.Player",
        &["Volume", "Shuffle"],
    );
    lp.until(&mut rt, "the volume and shuffle read again", |rt| {
        call(rt, root, "shown") == "First 0.25 true"
    });

    *name.lock().unwrap() = "Second".into();
    let app = conn.object_server().interface::<_, Named>(path).unwrap();
    invalidate(
        app.signal_emitter(),
        "org.mpris.MediaPlayer2",
        &["Identity"],
    );
    lp.until(&mut rt, "the name read again", |rt| {
        call(rt, root, "shown") == "Second 0.25 true"
    });
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// A player whose track is `.0` microseconds long.
struct Timed(i64);

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Timed {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        "Paused".into()
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let v = |v: ZV| OwnedValue::try_from(v).unwrap();
        HashMap::from([
            ("xesam:title".to_string(), v(ZV::from("timed"))),
            ("mpris:length".to_string(), v(ZV::from(self.0))),
        ])
    }
}

/// `length` is the track's `mpris:length` in seconds with its milliseconds, as Quickshell's
/// `MprisPlayer::length()` (`bInternalLength / 1000`, then `/ 1000` as a real): the slider's
/// `position / length` reaches 1 at the end of the track, not before it.
#[test]
fn mpris_length_keeps_its_milliseconds() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::mpris_length_keeps_its_milliseconds"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Services.Mpris\nItem {\n\
             function length() { const p = Mpris.players.values; return p.length ? '' + p[0].length : '' } }",
        )
        .unwrap();
    rt.settle();
    // Instantiates the Mpris singleton; its service starts on the next poll.
    assert_eq!(call(&mut rt, root, "length"), "");
    turn(&mut rt);
    let path = "/org/mpris/MediaPlayer2";
    // 3 min 35.7509 s.
    let _player = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at(path, Root)
        .unwrap()
        .serve_at(path, Timed(215_750_900))
        .unwrap()
        .name("org.mpris.MediaPlayer2.timed")
        .unwrap()
        .build()
        .unwrap();
    lp.until(&mut rt, "the timed player", |rt| {
        !call(rt, root, "length").is_empty()
    });
    assert_eq!(call(&mut rt, root, "length"), "215.75");
}

/// A player with a position: `Seeked` and `Rate` are all MPRIS tells about it.
struct Seeker {
    title: &'static str,
    art: &'static str,
    status: String,
    rate: f64,
    position: i64,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Seeker {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.status.clone()
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let v = |v: ZV| OwnedValue::try_from(v).unwrap();
        let mut m = HashMap::from([
            ("xesam:title".to_string(), v(ZV::from(self.title))),
            ("mpris:length".to_string(), v(ZV::from(600_000_000i64))),
        ]);
        if !self.art.is_empty() {
            m.insert("mpris:artUrl".to_string(), v(ZV::from(self.art)));
        }
        m
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        self.rate
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        self.position
    }
    #[zbus(signal)]
    async fn seeked(e: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
}

/// The player `org.mpris.MediaPlayer2.<name>`.
fn seeker_as(name: &str, p: Seeker) -> zbus::blocking::Connection {
    let path = "/org/mpris/MediaPlayer2";
    zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at(path, Root)
        .unwrap()
        .serve_at(path, p)
        .unwrap()
        .name(format!("org.mpris.MediaPlayer2.{name}"))
        .unwrap()
        .build()
        .unwrap()
}

fn seeker(status: &str) -> zbus::blocking::Connection {
    let p = Seeker {
        title: "song",
        art: "",
        status: status.into(),
        rate: 1.0,
        position: 0,
    };
    seeker_as("seeker", p)
}

/// Changes the [`Seeker`] and emits what a player emits for it.
fn seeker_set(conn: &zbus::blocking::Connection, change: &str) {
    let iface = conn
        .object_server()
        .interface::<_, Seeker>("/org/mpris/MediaPlayer2")
        .unwrap();
    let e = iface.signal_emitter();
    match change.split_once('=').unwrap() {
        ("status", v) => {
            iface.get_mut().status = v.into();
            zbus::block_on(iface.get().playback_status_changed(e)).unwrap();
        }
        ("rate", v) => {
            iface.get_mut().rate = v.parse().unwrap();
            zbus::block_on(iface.get().rate_changed(e)).unwrap();
        }
        ("seek", v) => {
            let us: i64 = v.parse().unwrap();
            iface.get_mut().position = us;
            zbus::block_on(Seeker::seeked(e, us)).unwrap();
        }
        _ => unreachable!("{change}"),
    }
}

/// A runtime with the original's singletons (in the fresh home of [`bus_with`]), its loop, and the
/// directory [`media_view`] writes its documents to.
fn media_shell() -> (Runtime, Loop, std::path::PathBuf) {
    for k in ["XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"] {
        std::fs::create_dir_all(std::env::var_os(k).unwrap()).unwrap();
    }
    let ri = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut rt = Runtime::new(Some(ri.clone()));
    let lp = Loop::new(&mut rt);
    rt.load_singletons_lazy(&[
        ri.clone(),
        ri.join("modules/common"),
        ri.join("modules/common/functions"),
        ri.join("services"),
    ]);
    rt.settle();
    let ui = std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join("ui");
    std::fs::create_dir_all(&ui).unwrap();
    (rt, lp, ui)
}

/// Loads an Item with the members `more` that can use the original's media widgets. A file, not
/// `load_str`: `import ri.modules...` component types resolve per document.
fn media_view(rt: &mut Runtime, ui: &std::path::Path, name: &str, more: &str) -> usize {
    let doc = ui.join(name);
    let text = format!(
        "import QtQuick\nimport ri\nimport ri.services\nimport ri.modules.rusty.bar\n\
         import ri.modules.rusty.verticalBar\nimport ri.modules.rusty.mediaControls\n\
         Item {{\n\
         property int notified: 0\n\
         property real shown: MprisController.activePlayer?.position ?? -1\n\
         onShownChanged: notified++\n\
         function status() {{ return MprisController.activePlayer?.playbackState ?? -1 }}\n\
         function rate() {{ return MprisController.activePlayer?.rate ?? -1 }}\n\
         function use() {{ return ResourceUsage.cpuUsage }}\n\
         {more}\n}}"
    );
    std::fs::write(&doc, text).unwrap();
    let root = rt.load_file(&doc).unwrap();
    rt.settle();
    root
}

/// What a pinned wall clock (`Date.now()`, a player's position) reads at loop time 0.
const EPOCH: f64 = 1_700_000_000_000.0;

/// Loop turns at the virtual time `now` until no Timer is due. A pinned wall clock
/// (`qml::builtins::set_clock`) moves with the loop's.
fn run_at(rt: &mut Runtime, now: f64) {
    if qml::builtins::pinned_clock().is_some() {
        qml::builtins::set_clock(Some(EPOCH + now));
    }
    loop {
        rt.tick(now);
        turn(rt);
        if rt.next_deadline(now) > 0.0 {
            return;
        }
    }
}

/// Moves the clock through `ms`, Timer by Timer; the loop wakeups that took.
fn advance(rt: &mut Runtime, now: &mut f64, ms: f64) -> Vec<f64> {
    let end = *now + ms;
    let mut wakeups = Vec::new();
    loop {
        run_at(rt, *now);
        let d = rt.next_deadline(*now);
        if *now + d > end {
            *now = end;
            run_at(rt, *now);
            return wakeups;
        }
        *now += d;
        wakeups.push(*now);
    }
}

/// `running` of the live position timers (the UpdateTimer of the bar's Media, VerticalMedia,
/// PlayerControl), in creation order.
fn position_timers(rt: &mut Runtime) -> Vec<bool> {
    let mut w = rt.world.borrow_mut();
    let mut ts: Vec<usize> = w
        .of_type("UpdateTimer")
        .into_iter()
        .filter(|&t| {
            let p = w.items[t].parent.unwrap_or(0);
            ["Media", "VerticalMedia", "PlayerControl"]
                .iter()
                .any(|n| w.items[p].types.iter().any(|ty| ty.ends_with(n)))
        })
        .collect();
    ts.sort();
    ts.into_iter()
        .map(|t| w.read(t, "running").is_some_and(|v| v.truthy()))
        .collect()
}

fn num(rt: &mut Runtime, i: usize, n: &str) -> f64 {
    rt.world
        .borrow_mut()
        .read(i, n)
        .unwrap_or_default()
        .to_number()
}

/// The bar, the vertical bar and the media controls each re-read the playing player's
/// position every `Config.options.resources.updateInterval`, and ResourceUsage samples at the
/// same period. Their UpdateTimers run only while the player plays and their widget is
/// visible (or the media controls are open), and all of them tick in one loop turn per
/// interval; a `Seeked` reaches the views when it arrives, not at the next tick.
#[test]
fn mpris_position_ticks_once_per_interval_while_shown() {
    let Some(_bus) = bus_with(
        concat!(
            module_path!(),
            "::mpris_position_ticks_once_per_interval_while_shown"
        ),
        true,
    ) else {
        return;
    };
    let wakeups = position_wakeups();
    assert_eq!(
        wakeups.len(),
        10,
        "one wakeup per updateInterval: {wakeups:?}"
    );
    for w in wakeups.windows(2) {
        assert_eq!(w[1] - w[0], 3000.0, "ticks off one phase: {wakeups:?}");
    }
}

/// Timer wakeups of the loop in 30 s with the bar's resources widget and Media and, opened
/// one second later, the vertical bar's Media and a PlayerControl, while a player plays.
/// Checks as well when the position timers run at all.
fn position_wakeups() -> Vec<f64> {
    // A player's position follows the wall clock; pinned, it moves with the loop's. On the
    // real one a Seeked and the view shown again often fall in one millisecond: the same
    // position, rightly no change.
    qml::builtins::set_clock(Some(EPOCH));
    let (mut rt, lp, ui) = media_shell();
    // A resources widget is shown too, so ResourceUsage samples.
    let root = media_view(
        &mut rt,
        &ui,
        "Bar.qml",
        "Media { objectName: 'bar' }\nResources {}",
    );
    call(&mut rt, root, "use");

    let mut now = 0.0;
    let player = seeker("Paused");
    lp.until(&mut rt, "the paused player", |rt| {
        call(rt, root, "status") == "2"
    });
    advance(&mut rt, &mut now, 500.0);
    assert_eq!(
        position_timers(&mut rt),
        [false],
        "position Timer of a paused player"
    );

    seeker_set(&player, "status=Playing");
    lp.until(&mut rt, "the player playing", |rt| {
        call(rt, root, "status") == "1"
    });
    advance(&mut rt, &mut now, 1000.0);
    media_view(
        &mut rt,
        &ui,
        "More.qml",
        "VerticalMedia { objectName: 'vbar' }\n\
         PlayerControl { objectName: 'ctl'; player: MprisController.activePlayer }",
    );
    advance(&mut rt, &mut now, 4500.0);
    assert_eq!(position_timers(&mut rt), [true, true, true]);

    let wakeups = advance(&mut rt, &mut now, 30_000.0);
    {
        // Seeked moves the shown position when it arrives: the clock stands still.
        let n = num(&mut rt, root, "notified");
        seeker_set(&player, "seek=420000000");
        lp.until(&mut rt, "Seeked", |rt| num(rt, root, "shown") >= 420.0);
        assert!(num(&mut rt, root, "notified") > n);
        assert!(num(&mut rt, root, "shown") < 430.0);
        // Rate changes the extrapolation from the moment it arrives.
        seeker_set(&player, "rate=2");
        lp.until(&mut rt, "Rate", |rt| call(rt, root, "rate") == "2");

        // A view hidden with its parent does not tick; shown again, it is up to date at once.
        rt.set(root, "visible", qml::value::Value::Bool(false));
        advance(&mut rt, &mut now, 100.0);
        assert_eq!(
            position_timers(&mut rt),
            [false, true, true],
            "hidden bar Media"
        );
        let n = num(&mut rt, root, "notified");
        rt.set(root, "visible", qml::value::Value::Bool(true));
        advance(&mut rt, &mut now, 10.0);
        assert_eq!(position_timers(&mut rt), [true, true, true]);
        assert!(
            num(&mut rt, root, "notified") > n,
            "shown with a stale position"
        );
        let again = advance(&mut rt, &mut now, 6000.0);
        assert_eq!(again.len(), 2, "back on the one phase: {again:?}");

        seeker_set(&player, "status=Paused");
        lp.until(&mut rt, "the player paused", |rt| {
            call(rt, root, "status") == "2"
        });
        advance(&mut rt, &mut now, 100.0);
        assert_eq!(position_timers(&mut rt), [false, false, false]);
    }
    qml::builtins::set_clock(None);
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
    wakeups
}

/// `MediaControls.qml` shows one card for players it takes for duplicates, and one of its
/// tests is how far apart their positions are. It re-runs that when a position Timer calls
/// `positionChanged()`; for the active player that is the bar widget's Timer. So with the
/// media controls open that Timer ticks even when the bar's widget is hidden (the controls
/// opened by keybind over a hidden bar): the cards split once the playing player has moved
/// away from the paused one; and controls opened after it moved away show the split.
#[test]
fn media_controls_regroup_players_with_the_bar_widget_hidden() {
    let Some(_bus) = bus_with(
        concat!(
            module_path!(),
            "::media_controls_regroup_players_with_the_bar_widget_hidden"
        ),
        true,
    ) else {
        return;
    };
    // A player's position follows the wall clock; pinned, it moves with the loop's.
    qml::builtins::set_clock(Some(EPOCH));
    let (mut rt, lp, ui) = media_shell();
    let bar = media_view(&mut rt, &ui, "Bar.qml", "visible: false\nMedia {}");
    let ctl = media_view(
        &mut rt,
        &ui,
        "Controls.qml",
        "MediaControls { id: mc }\n\
         function order() { return MprisController.players.map(p => p.trackTitle).join() }\n\
         function cards() { return mc.meaningfulPlayers.map(p => p.trackTitle).join() }\n\
         function open(on) { GlobalStates.mediaControlsOpen = on }",
    );
    let mut now = 0.0;
    // The playing player comes first, without art; the paused one has art. Both at the start
    // of their tracks: one card, the paused player's, until the playing one is over 2 s on.
    let playing = seeker_as(
        "a",
        Seeker {
            title: "song",
            art: "",
            status: "Playing".into(),
            rate: 1.0,
            position: 0,
        },
    );
    lp.until(&mut rt, "the playing player", |rt| {
        call(rt, bar, "status") == "1"
    });
    let _paused = seeker_as(
        "b",
        Seeker {
            title: "talk",
            art: "file:///art.png",
            status: "Paused".into(),
            rate: 1.0,
            position: 0,
        },
    );
    lp.until(&mut rt, "both players", |rt| {
        call(rt, ctl, "order") == "song,talk"
    });
    lp.until(&mut rt, "one card", |rt| call(rt, ctl, "cards") == "talk");
    let open = |rt: &mut Runtime, on: bool| {
        rt.call_method(ctl, "open", vec![qml::value::Value::Bool(on)])
            .unwrap()
            .unwrap();
    };

    // Closed controls show no cards, so nothing ticks for the hidden widget while the player
    // plays on.
    advance(&mut rt, &mut now, 4000.0);
    assert_eq!(
        position_timers(&mut rt),
        [false],
        "hidden bar widget, closed controls"
    );
    assert_eq!(call(&mut rt, ctl, "cards"), "talk", "no tick, no regroup");
    // Opened, they show the split: the bar's timer starts with them and ticks at once.
    open(&mut rt, true);
    advance(&mut rt, &mut now, 10.0);
    assert_eq!(call(&mut rt, ctl, "cards"), "song,talk", "opened");

    // Open controls: the playing player seeks back to the start (one card again, the paused
    // player's, whose Timer does not run) and plays on. The bar's Timer is what ticks,
    // hidden as its widget is.
    seeker_set(&playing, "seek=0");
    lp.until(&mut rt, "Seeked", |rt| call(rt, ctl, "cards") == "talk");
    advance(&mut rt, &mut now, 100.0);
    assert_eq!(position_timers(&mut rt), [true, false]);
    assert_eq!(call(&mut rt, ctl, "cards"), "talk", "under 2 s on");
    advance(&mut rt, &mut now, 6000.0);
    assert_eq!(call(&mut rt, ctl, "cards"), "song,talk", "open");

    open(&mut rt, false);
    advance(&mut rt, &mut now, 100.0);
    assert_eq!(position_timers(&mut rt), [false], "closed controls");
    qml::builtins::set_clock(None);
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
