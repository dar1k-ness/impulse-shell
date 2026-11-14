//! The original's QML that talks to Hyprland, driven as the shell drives it, against one fake Hyprland
//! (`.socket.sock`, `.socket2.sock`, `.hyprsunset.sock`) that records every request and serves
//! fixed replies. Checked: the requests (bytes; in order for the driven member's own calls, as
//! a set for the concurrent startup refreshes), the QML state and the files under the config
//! home (path, mode, bytes). No program is run for any of it.

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use testenv::Wakeups;

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn reply(cmd: &str) -> String {
    let what = cmd.strip_prefix("j/").unwrap_or(cmd);
    let what = what.strip_prefix('/').unwrap_or(what);
    match what.split_whitespace().next().unwrap_or("") {
        "clients" => r#"[{"address":"0xa1","class":"foot","title":"foot","workspace":{"id":1,"name":"1"},"at":[10,40],"size":[900,600]},{"address":"0xa2","class":"firefox","title":"Mozilla Firefox","workspace":{"id":2,"name":"2"},"at":[0,0],"size":[1920,1080]}]"#.into(),
        "monitors" => r#"[{"id":0,"name":"DP-1","width":1920,"height":1080,"activeWorkspace":{"id":1,"name":"1"},"focused":true},{"id":1,"name":"HDMI-A-1","width":2560,"height":1440,"activeWorkspace":{"id":2147483646,"name":"lock"},"focused":false}]"#.into(),
        "layers" => r#"{"DP-1":{"levels":{"0":[],"2":[{"address":"0xb1","namespace":"rustshell:bar"}]}}}"#.into(),
        "workspaces" => r#"[{"id":1,"name":"1","monitor":"DP-1"},{"id":2,"name":"2","monitor":"DP-1"},{"id":-98,"name":"special:x"},{"id":2147483646,"name":"lock"}]"#.into(),
        "activeworkspace" => r#"{"id":1,"name":"1","monitor":"DP-1"}"#.into(),
        "activewindow" => "{}".into(),
        "devices" => r#"{"mice":[],"keyboards":[{"name":"kb2","layout":"de","active_keymap":"German","main":false},{"name":"kb","layout":"us,ru","variant":",","active_keymap":"English (US)","main":true}]}"#.into(),
        "binds" => r#"[{"key":"Return","description":"Apps: Terminal","dispatcher":"exec","arg":"foot"},{"key":"Q","description":"Window: Close"},{"key":"Tab","description":""},{"key":"T","description":"Apps: Browser"}]"#.into(),
        "getoption" if what.ends_with("animations:enabled") => r#"{"option":"animations:enabled","int":1,"set":true}"#.into(),
        "getoption" if what.ends_with("decoration:screen_shader") => r#"{"option":"decoration:screen_shader","str":"/tmp/x.frag","set":false}"#.into(),
        "getoption" => "no such option".into(),
        "cursorpos" => "960, 540".into(),
        _ => "ok".into(),
    }
}

struct Fake {
    /// (socket, request bytes).
    reqs: Mutex<Vec<(&'static str, String)>>,
    /// `.socket2.sock` connections, oldest first; `connected` is notified on each.
    events: Mutex<Vec<UnixStream>>,
    connected: Condvar,
    held: Mutex<Vec<UnixStream>>,
    /// hyprsunset's temperature: the last `temperature N` set, 6000 (identity) when it starts.
    sunset: Mutex<String>,
    /// The `devices` reply instead of `reply`'s.
    devices: Mutex<Option<&'static str>>,
    rt: PathBuf,
}

impl Fake {
    fn take(&self) -> Vec<(&'static str, String)> {
        std::mem::take(&mut *self.reqs.lock().unwrap())
    }
    fn connections(&self) -> usize {
        self.events.lock().unwrap().len()
    }
    /// Blocks until `.socket2.sock` has accepted more than `n` connections.
    fn wait_connection(&self, n: usize) {
        let mut e = self.events.lock().unwrap();
        while e.len() <= n {
            e = self.connected.wait(e).unwrap();
        }
    }
    fn conf(&self) -> PathBuf {
        self.rt.join("config")
    }
    fn overrides(&self) -> PathBuf {
        self.conf().join("hypr/hyprland/shellOverrides/main.lua")
    }
}

/// Send `line` on every `.socket2.sock` connection.
fn event(f: &Fake, line: &str) {
    for c in f.events.lock().unwrap().iter_mut() {
        let _ = c.write_all(format!("{line}\n").as_bytes());
    }
}

fn serve(l: UnixListener, f: &'static Fake, name: &'static str, keep_open: bool) {
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
            f.reqs.lock().unwrap().push((name, cmd.clone()));
            // The Reload button: the shell must not ask for its own reload before this answer.
            if cmd == "/reload" && qml::native::take_reload() {
                f.reqs
                    .lock()
                    .unwrap()
                    .push((name, "shell reload before Hyprland's answer".into()));
            }
            let r = match cmd.strip_prefix("temperature") {
                Some(t) if name == ".hyprsunset.sock" => {
                    let mut cur = f.sunset.lock().unwrap();
                    match t.trim() {
                        "" => cur.clone(),
                        v => {
                            *cur = v.to_string();
                            "ok".into()
                        }
                    }
                }
                _ => match *f.devices.lock().unwrap() {
                    Some(d) if cmd == "j/devices" => d.into(),
                    _ => reply(&cmd),
                },
            };
            let _ = s.write_all(r.as_bytes());
            if keep_open {
                f.held.lock().unwrap().push(s);
            }
        }
    });
}

/// Whether this is `test`'s own process (`testenv::run_in_own_process`), whose
/// `XDG_RUNTIME_DIR`, `HYPRLAND_INSTANCE_SIGNATURE`, `XDG_CONFIG_HOME` and
/// `RUSTY_IMPULSE_VIRTUAL_ENV` name a fresh directory for [`fake`]. Elsewhere it runs
/// `test` there and returns false.
fn own_process(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let rt = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(rt.path().join("hypr/TEST")).unwrap();
    // hyprconfigurator.py's shebang sources $RUSTY_IMPULSE_VIRTUAL_ENV/bin/activate
    // and execs `python`: an empty activate keeps the PATH python.
    let venv = rt.path().join("venv");
    std::fs::create_dir_all(venv.join("bin")).unwrap();
    std::fs::write(venv.join("bin/activate"), "").unwrap();
    testenv::run_in_own_process(
        test,
        [
            ("XDG_RUNTIME_DIR", rt.path().as_os_str()),
            ("HYPRLAND_INSTANCE_SIGNATURE", std::ffi::OsStr::new("TEST")),
            ("XDG_CONFIG_HOME", rt.path().join("config").as_os_str()),
            ("RUSTY_IMPULSE_VIRTUAL_ENV", venv.as_os_str()),
        ],
    );
    false
}

/// One fake Hyprland per process, in the directory [`own_process`] made; calls take it in turn.
fn fake() -> MutexGuard<'static, &'static Fake> {
    static F: OnceLock<Mutex<&'static Fake>> = OnceLock::new();
    let m = F.get_or_init(|| {
        let rt = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        let dir = rt.join("hypr").join("TEST");
        let f: &'static Fake = Box::leak(Box::new(Fake {
            reqs: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            connected: Condvar::new(),
            held: Mutex::new(Vec::new()),
            sunset: Mutex::new(SUNSET_OFF.into()),
            devices: Mutex::new(None),
            rt: rt.clone(),
        }));
        serve(
            UnixListener::bind(dir.join(".socket.sock")).unwrap(),
            f,
            ".socket.sock",
            false,
        );
        // hyprsunset 0.4 keeps a connection open after replying.
        serve(
            UnixListener::bind(dir.join(".hyprsunset.sock")).unwrap(),
            f,
            ".hyprsunset.sock",
            true,
        );
        let ev = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
        // Hyprland writes its lock once its sockets listen.
        std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
        std::thread::spawn(move || {
            for c in ev.incoming().flatten() {
                f.events.lock().unwrap().push(c);
                f.connected.notify_all();
            }
        });
        Mutex::new(f)
    });
    m.lock().unwrap_or_else(|e| e.into_inner())
}

const BASE_OVERRIDES: &str = "-- ii shell overrides\nhl.config({general={gaps_in=4}})\nhl.config({decoration={rounding=12}})\n";

/// Fresh config home for one run: an existing config.json (Config.ready from one load in both
/// runs, no write-then-reload) and a shell-overrides file for HyprlandConfig.
fn reset_files(f: &Fake) {
    let _ = std::fs::remove_dir_all(f.conf());
    let ri_conf = f.conf().join("rusty-impulse");
    std::fs::create_dir_all(&ri_conf).unwrap();
    std::fs::write(ri_conf.join("config.json"), "{}").unwrap();
    let o = f.overrides();
    std::fs::create_dir_all(o.parent().unwrap()).unwrap();
    std::fs::write(&o, BASE_OVERRIDES).unwrap();
    std::fs::set_permissions(&o, std::fs::Permissions::from_mode(0o640)).unwrap();
}

/// Every file under `dir`: relative path, mode, bytes.
fn files(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, d: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(m) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if m.is_dir() {
                walk(base, &p, out);
            } else {
                out.push(format!(
                    "{} {:o} {:?}",
                    p.strip_prefix(base).unwrap().display(),
                    m.permissions().mode() & 0o7777,
                    String::from_utf8_lossy(&std::fs::read(&p).unwrap_or_default())
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn canon(v: &Value, out: &mut String, depth: u32) {
    use std::fmt::Write as _;
    match v {
        Value::Obj(o) if depth < 8 => {
            let o = o.borrow();
            if let Some(a) = &o.array {
                out.push('[');
                for x in a {
                    canon(x, out, depth + 1);
                    out.push(',');
                }
                out.push(']');
            } else if o.call.is_some() {
                out.push_str("<fn>");
            } else {
                out.push('{');
                for k in &o.keys {
                    let _ = write!(out, "{k}:");
                    canon(o.props.get(k).unwrap_or(&Value::Undefined), out, depth + 1);
                    out.push(',');
                }
                out.push('}');
            }
        }
        Value::Str(s) => {
            let _ = write!(out, "{s:?}");
        }
        v => out.push_str(&v.to_js_string()),
    }
}

/// The test's host loop for one runtime: its waker and epoll set (Process pipes and pidfds,
/// inotify), and its clock in virtual ms: QML timers fire when [`settle`] moves it, never by
/// wall time.
struct Host {
    wake: Wakeups,
    ep: Option<i32>,
    now: f64,
}

impl Host {
    fn new(rt: &mut Runtime) -> Host {
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Host { wake, ep, now: 0.0 }
    }

    /// One loop turn per wakeup at `now`, until `done` holds and nothing is out: every pool job
    /// and `execDetached` child finished (`workpool::wait_idle`), the hyprconf writes are on
    /// disk, every `Hyprland.dispatch` is answered, no Hyprland call, refresh or `Process` is still running, and no
    /// wakeup or change is left for the next poll. In between it blocks on the waker and the
    /// epoll set, as the shell's loop does; whatever ends a wait (a reply, a child's exit, an
    /// event line) wakes one of them.
    fn drain(&mut self, rt: &mut Runtime, done: &mut dyn FnMut(&mut Runtime) -> bool) {
        loop {
            workpool::wait_idle();
            qml::hyprconf::flush();
            qml::hypr::drain();
            // Writes and objects made during a poll count for the next one.
            loop {
                let rev = rt.revision();
                rt.poll_native();
                rt.tick(self.now);
                rt.settle();
                if rt.revision() == rev {
                    break;
                }
            }
            let woke = self.wake.wait(self.ep, Some(Duration::ZERO));
            if woke.fd {
                rt.process_fd_ready();
            }
            if woke.any() {
                continue;
            }
            let out = {
                let w = rt.world.borrow();
                qml::hyprctl::in_flight(&w) != 0 || !w.procs.is_empty()
            };
            if !out && !rt.hyprland_refreshing() && done(rt) {
                return;
            }
            if self.wake.wait(self.ep, None).fd {
                rt.process_fd_ready();
            }
        }
    }
}

/// Until nothing is out ([`Host::drain`]), running the QML timers due in the next `timers` of
/// virtual time at their deadlines (Lock's 150 ms restore).
fn settle(rt: &mut Runtime, h: &mut Host, timers: Duration) {
    let end = h.now + timers.as_secs_f64() * 1e3;
    loop {
        h.drain(rt, &mut |_| true);
        let d = rt.next_deadline(h.now);
        if h.now + d > end {
            break;
        }
        h.now += d;
    }
    if h.now < end {
        h.now = end;
        h.drain(rt, &mut |_| true);
    }
}

#[derive(Clone, Copy)]
struct Case {
    /// Item whose `s` is the object under test; `function act()` on it drives the member.
    src: &'static str,
    props: &'static [&'static str],
    /// Hyprland events sent on `.socket2.sock` after `act`.
    events: &'static [&'static str],
    /// Built-in handler to fire on the first item whose `buttonIcon` is this, after `act`.
    click_icon: Option<&'static str>,
    /// Virtual time the QML timers run through after `act` (loading runs none).
    min: Duration,
    /// The member's requests race the `Hyprland` singleton's own event refresh (both runs):
    /// compare them as a multiset.
    unordered: bool,
}

impl Default for Case {
    fn default() -> Self {
        Case {
            src: "",
            props: &[],
            events: &[],
            click_icon: None,
            min: Duration::ZERO,
            unordered: false,
        }
    }
}

#[derive(Debug, PartialEq)]
struct Outcome {
    /// Requests while loading, as a set: they go out concurrently.
    load: Vec<(&'static str, String)>,
    /// The temperatures hyprsunset was set to while loading, in order.
    load_temperatures: Vec<String>,
    /// Requests the member sent when driven, in order.
    act: Vec<(&'static str, String)>,
    state: Vec<String>,
    files: Vec<String>,
}

/// A fresh runtime on `f` with the document `src` loaded and its startup settled: the root
/// item and the requests made while loading.
fn boot(f: &Fake, src: &str) -> (Runtime, Host, usize, Vec<(&'static str, String)>) {
    let (rt, h, r, mut load) = boot_in_order(f, src);
    load.sort();
    load.dedup();
    (rt, h, r, load)
}

/// [`boot`] with the requests in the order the sockets took them.
fn boot_in_order(f: &Fake, src: &str) -> (Runtime, Host, usize, Vec<(&'static str, String)>) {
    boot_with(f, src, "{}", SUNSET_OFF)
}

/// [`boot`] with `config` as config.json and hyprsunset at the temperature `sunset` before the
/// shell starts; the requests made while loading are in the order they arrived.
fn boot_with(
    f: &Fake,
    src: &str,
    config: &str,
    sunset: &str,
) -> (Runtime, Host, usize, Vec<(&'static str, String)>) {
    reset_files(f);
    std::fs::write(f.conf().join("rusty-impulse/config.json"), config).unwrap();
    f.take();
    *f.sunset.lock().unwrap() = sunset.into();
    let mut rt = Runtime::new(Some(ri()));
    let mut h = Host::new(&mut rt);
    rt.set_screens(&[
        ("DP-1".into(), 0, 0, 1920, 1080),
        ("HDMI-A-1".into(), 1920, 0, 2560, 1440),
    ]);
    rt.load_singletons_lazy(&[
        ri(),
        ri().join("services"),
        ri().join("modules/common"),
        ri().join("modules/common/functions"),
    ]);
    // A file, not `load_str`: `import ri.modules...` component types resolve per document.
    let doc = f.rt.join("Case.qml");
    std::fs::write(&doc, src).unwrap();
    let r = rt
        .load_file(&doc)
        .unwrap_or_else(|e| panic!("load: {e:?} {:?}", rt.errors()));
    settle(&mut rt, &mut h, Duration::ZERO);
    let load = f.take();
    (rt, h, r, load)
}

fn run(f: &Fake, c: &Case) -> Outcome {
    let conns = f.connections();
    // With events, the document counts the `rawEvent`s the runtime has handled.
    let src = match c.src.rfind('}') {
        Some(end) if !c.events.is_empty() => format!(
            "import Rustshell.Hyprland\n{}\n property int rawEvents: 0\n Connections {{ target: Hyprland; function onRawEvent(event) {{ rawEvents++ }} }}\n}}",
            &c.src[..end]
        ),
        _ => c.src.to_string(),
    };
    let (mut rt, mut h, r, mut load) = boot_in_order(f, &src);
    let load_temperatures = load
        .iter()
        .filter(|r| r.0 == ".hyprsunset.sock")
        .filter_map(|r| r.1.strip_prefix("temperature "))
        .map(str::to_string)
        .collect();
    load.sort();
    load.dedup();
    let Value::Item(s) = rt.get(r, "s") else {
        panic!("no s: {:?}", rt.errors())
    };
    if rt.has_handler(r, "act") || matches!(rt.get(r, "act"), Value::Obj(_)) {
        rt.call_method(r, "act", vec![])
            .unwrap()
            .unwrap_or_else(|e| panic!("act: {e} {:?}", rt.errors()));
    }
    if !c.events.is_empty() {
        // This runtime's event connection; earlier runtimes' ones get the lines too.
        f.wait_connection(conns);
    }
    for (n, e) in c.events.iter().enumerate() {
        settle(&mut rt, &mut h, Duration::ZERO);
        event(f, e);
        let n = n as f64 + 1.0;
        h.drain(&mut rt, &mut |rt| rt.get(r, "rawEvents").to_number() >= n);
    }
    if let Some(icon) = c.click_icon {
        let item = {
            let w = rt.world.borrow();
            (0..w.items.len())
                .find(|&i| {
                    w.items
                        .get(i)
                        .and_then(|it| it.props.get("buttonIcon"))
                        .is_some_and(|p| p.value.to_js_string() == icon)
                })
                .unwrap_or_else(|| panic!("no button {icon}"))
        };
        rt.fire(item, "onClicked", &[], vec![]);
    }
    settle(&mut rt, &mut h, c.min);
    let mut act = f.take();
    if c.unordered {
        act.sort();
    }
    let state = c
        .props
        .iter()
        .map(|p| {
            let mut o = format!("{p}=");
            canon(&rt.get(s, p), &mut o, 0);
            o
        })
        .collect();
    drop(rt);
    Outcome {
        load,
        load_temperatures,
        act,
        state,
        files: files(&f.conf()),
    }
}

/// A local wall-clock time, hours and minutes.
type Hm = (u32, u32);

/// `hm` as the engine's clock takes it (`qml::builtins::set_clock`).
fn clock(hm: Hm) -> f64 {
    testenv::local_time_ms(hm.0, hm.1)
}

/// The times the Hyprsunset cases run at, around the default night window (`light.night`,
/// 19:00 to 06:30, both ends in it): midday, the last minute before it, its first minute,
/// either side of midnight, its last minute and the first one after it.
const CLOCKS: &[Hm] = &[
    (12, 0),
    (18, 59),
    (19, 0),
    (23, 59),
    (0, 0),
    (6, 30),
    (6, 31),
];

/// Whether the default schedule has night light on at `hm`.
fn night(hm: Hm) -> bool {
    hm >= (19, 0) || hm <= (6, 30)
}

/// One run of `c` at midday: the real clock decides nothing in it.
fn drive(c: Case) -> Outcome {
    drive_at((12, 0), c)
}

/// One run of `c` with the engine's clock pinned to `hm`.
fn drive_at(hm: Hm, c: Case) -> Outcome {
    let f = fake();
    qml::builtins::set_clock(Some(clock(hm)));
    let got = run(&f, &c);
    qml::builtins::set_clock(None);
    // The toggle shows what hyprsunset was last set to, not an answer read before that.
    if c.props.contains(&"temperatureActive") {
        let on = *f.sunset.lock().unwrap() != SUNSET_OFF;
        assert!(
            got.state.contains(&format!("temperatureActive={on}")),
            "at {hm:?}: temperatureActive vs hyprsunset's temperature: {:?}",
            got.state
        );
    }
    got
}

fn has(o: &Outcome, req: &str) -> bool {
    o.act.iter().chain(&o.load).any(|r| r.1 == req)
}

const DATA: &str = "import QtQuick\nimport ri.services\nItem { property var s: HyprlandData\n function act() { HyprlandData.MEMBER() } }";
const DATA_PROPS: &[&str] = &[
    "windowList",
    "windowByAddress",
    "addresses",
    "monitors",
    "layers",
    "workspaces",
    "workspaceById",
    "workspaceIds",
    "activeWorkspace",
];

fn data(member: &str, req: &str) {
    let src: &'static str = Box::leak(DATA.replace("MEMBER", member).into_boxed_str());
    let o = drive(Case {
        src,
        props: DATA_PROPS,
        // updateWorkspaces starts its workspaces and activeworkspace Processes together.
        unordered: true,
        ..Default::default()
    });
    assert!(o.act.iter().any(|r| r.1 == req), "{o:?}");
}

#[test]
fn hyprland_data_update_window_list() {
    if !own_process(concat!(
        module_path!(),
        "::hyprland_data_update_window_list"
    )) {
        return;
    }
    data("updateWindowList", "j/clients");
}

#[test]
fn hyprland_data_update_layers() {
    if !own_process(concat!(module_path!(), "::hyprland_data_update_layers")) {
        return;
    }
    data("updateLayers", "j/layers");
}

#[test]
fn hyprland_data_update_monitors() {
    if !own_process(concat!(module_path!(), "::hyprland_data_update_monitors")) {
        return;
    }
    data("updateMonitors", "j/monitors");
}

#[test]
fn hyprland_data_update_workspaces() {
    if !own_process(concat!(module_path!(), "::hyprland_data_update_workspaces")) {
        return;
    }
    data("updateWorkspaces", "j/workspaces");
}

/// `onRawEvent`: a focus change refetches only the queries it can affect (no layers).
#[test]
fn hyprland_data_on_raw_event() {
    if !own_process(concat!(module_path!(), "::hyprland_data_on_raw_event")) {
        return;
    }
    let src = "import QtQuick\nimport ri.services\nItem { property var s: HyprlandData\n function act() {} }";
    let got = drive(Case {
        src,
        props: DATA_PROPS,
        events: &["activewindowv2>>a1"],
        unordered: true,
        ..Default::default()
    });
    let n = |o: &Outcome, r: &str| o.act.iter().filter(|x| x.1 == r).count();
    assert_eq!(n(&got, "j/layers"), 0, "{got:?}");
    assert_eq!(n(&got, "j/monitors"), 0, "{got:?}");
    assert!(n(&got, "j/clients") >= 1, "{got:?}");
}

const XKB: &[&str] = &[
    "layoutCodes",
    "currentLayoutName",
    "currentLayoutCode",
    "cachedLayoutCodes",
];

/// `running` (the devices fetch at startup) and `updateLayoutCode`.
#[test]
fn hyprland_xkb_running_update_layout_code() {
    if !own_process(concat!(
        module_path!(),
        "::hyprland_xkb_running_update_layout_code"
    )) {
        return;
    }
    let o = drive(Case {
        src: "import QtQuick\nimport ri.services\nItem { property var s: HyprlandXkb\n function act() { HyprlandXkb.updateLayoutCode() } }",
        props: XKB,
        ..Default::default()
    });
    assert!(has(&o, "j/devices"), "{o:?}");
}

#[test]
fn hyprland_xkb_on_raw_event() {
    if !own_process(concat!(module_path!(), "::hyprland_xkb_on_raw_event")) {
        return;
    }
    drive(Case {
        src: "import QtQuick\nimport ri.services\nItem { property var s: HyprlandXkb }",
        props: XKB,
        events: &["activelayout>>kb,Russian", "configreloaded>>"],
        unordered: true,
        ..Default::default()
    });
}

/// Hyprland sends `activelayout` for every keyboard, the shell's own virtual keyboard (OSK and
/// paste keys) too, whose layout group is its own: on `switchxkblayout all next` from Russian
/// its event says English last. The indicator and the OSK follow the real keyboards only.
#[test]
fn hyprland_xkb_ignores_virtual_keyboards() {
    if !own_process(concat!(
        module_path!(),
        "::hyprland_xkb_ignores_virtual_keyboards"
    )) {
        return;
    }
    let f = fake();
    let o = run(
        &f,
        &Case {
            src: "import QtQuick\nimport ri.services\nimport ri.modules.common\nItem { property var s: QtObject { property string currentLayoutName: HyprlandXkb.currentLayoutName; property string osk: Config.options.osk.layout } }",
            props: &["currentLayoutName", "osk"],
            events: &[
                "activelayout>>kb,Russian",
                "activelayout>>hl-virtual-keyboard-rusty-impulse,English (US)",
                "activelayout>>hl-virtual-keyboard,English (US)",
            ],
            ..Default::default()
        },
    );
    assert_eq!(
        o.state,
        ["currentLayoutName=\"Russian\"", "osk=\"Russian\""]
    );
}

/// After an OSK key or a paste Hyprland reports the virtual keyboard as `main`; the startup
/// and config-reload fetch takes a real keyboard's layout instead.
#[test]
fn hyprland_xkb_fetch_skips_virtual_main_keyboard() {
    if !own_process(concat!(
        module_path!(),
        "::hyprland_xkb_fetch_skips_virtual_main_keyboard"
    )) {
        return;
    }
    let f = fake();
    *f.devices.lock().unwrap() = Some(
        r#"{"mice":[],"keyboards":[{"name":"kb","layout":"us,ru","active_keymap":"Russian","main":false},{"name":"hl-virtual-keyboard-rusty-impulse","layout":"us,ru","active_keymap":"English (US)","main":true}]}"#,
    );
    let o = run(
        &f,
        &Case {
            src: "import QtQuick\nimport ri.services\nItem { property var s: HyprlandXkb }",
            props: &["layoutCodes", "currentLayoutName"],
            ..Default::default()
        },
    );
    *f.devices.lock().unwrap() = None;
    assert_eq!(
        o.state,
        [
            "layoutCodes=[\"us\",\"ru\",]",
            "currentLayoutName=\"Russian\""
        ]
    );
}

/// `running` (the binds fetch at startup) and `onRawEvent` (re-fetch on config reload).
#[test]
fn hyprland_keybinds_running_on_raw_event() {
    if !own_process(concat!(
        module_path!(),
        "::hyprland_keybinds_running_on_raw_event"
    )) {
        return;
    }
    let o = drive(Case {
        src: "import QtQuick\nimport ri.services\nItem { property var s: HyprlandKeybinds }",
        props: &["keybinds", "keybindCategories"],
        events: &["configreloaded>>"],
        unordered: true,
        ..Default::default()
    });
    assert!(o.act.iter().any(|r| r.1 == "j/binds"), "{o:?}");
}

#[test]
fn hyprland_config_option_fetch() {
    if !own_process(concat!(module_path!(), "::hyprland_config_option_fetch")) {
        return;
    }
    for key in ["animations:enabled", "decoration:screen_shader", "no:such"] {
        let src = format!(
            "import QtQuick\nimport ri.modules.common.models.hyprland\nItem {{ property var s: o\n function act() {{ o.fetch() }}\n HyprlandConfigOption {{ id: o; key: \"{key}\" }} }}"
        );
        let o = drive(Case {
            src: Box::leak(src.into_boxed_str()),
            props: &["set", "value"],
            ..Default::default()
        });
        assert!(has(&o, &format!("j/getoption {key}")), "{o:?}");
    }
}

const SUNSET_OFF: &str = "6000";
const SUNSET: &[&str] = &["temperatureActive", "gamma", "colorTemperature"];

/// What Quickshell 0.3.2 set the temperature to while loading `Hyprsunset.qml` and
/// `Config.qml` with an empty config at `hm` (`HyprsunsetClient` a QML object printing its
/// calls). `onConfigReadyChanged` runs before `automatic` follows `configReady` and reads the
/// config itself, so it leaves the temperature to the schedule. By day the schedule sets it
/// once. At night it sets it once where `onClockMinuteChanged` evaluated it during creation
/// (a minute other than 0), twice otherwise (`onShouldBeOnChanged`, then the first
/// evaluation).
fn quickshell_load_temperatures(hm: Hm) -> &'static [&'static str] {
    match (night(hm), hm.1 == 0) {
        (false, _) => &["6000"],
        (true, true) => &["5000", "5000"],
        (true, false) => &["5000"],
    }
}

fn sunset(act: &str, want: &str) {
    let src = format!(
        "import QtQuick\nimport ri.modules.common\nimport ri.services\nItem {{ property var s: Hyprsunset\n function act() {{ {act} }} }}"
    );
    let c = Case {
        src: Box::leak(src.into_boxed_str()),
        props: SUNSET,
        ..Default::default()
    };
    for &hm in CLOCKS {
        let o = drive_at(hm, c);
        // What the schedule did while loading: night light on inside the window, off outside.
        assert_eq!(
            o.load_temperatures,
            quickshell_load_temperatures(hm),
            "at {hm:?}: {o:?}"
        );
        assert!(
            o.act
                .iter()
                .any(|r| r == &(".hyprsunset.sock", want.to_string())),
            "at {hm:?}: {o:?}"
        );
    }
}

#[test]
fn hyprsunset_enable_temperature() {
    if !own_process(concat!(module_path!(), "::hyprsunset_enable_temperature")) {
        return;
    }
    sunset("Hyprsunset.enableTemperature()", "temperature 5000");
}

#[test]
fn hyprsunset_disable_temperature() {
    if !own_process(concat!(module_path!(), "::hyprsunset_disable_temperature")) {
        return;
    }
    sunset("Hyprsunset.disableTemperature()", "temperature 6000");
}

#[test]
fn hyprsunset_set_gamma() {
    if !own_process(concat!(module_path!(), "::hyprsunset_set_gamma")) {
        return;
    }
    sunset("Hyprsunset.setGamma(70)", "gamma 70");
}

/// end-4/dots-hyprland #3373: with `light.night.automatic` false, `ensureState()` returned early, so
/// leaving the schedule (or reloading) kept hyprsunset's previous temperature and gamma.
/// `load()` resets both.
#[test]
fn hyprsunset_manual_mode_resets_stale_state() {
    if !own_process(concat!(
        module_path!(),
        "::hyprsunset_manual_mode_resets_stale_state"
    )) {
        return;
    }
    // A from == to window wraps around the whole day: the schedule has night light on at any
    // time (it turns it on now, as the next minute tick would, unless the clock already had it
    // on while loading). Then manual mode, and `load()`.
    let c = Case {
        src: "import QtQuick\nimport ri.services\nItem { property var s: Hyprsunset\n function act() { const s = Hyprsunset; s.toHour = s.fromHour; s.toMinute = s.fromMinute; s.reEvaluate(); s.automatic = false; s.load() } }",
        props: &[
            "automatic",
            "configReady",
            "shouldBeOn",
            "gamma",
            "temperatureActive",
        ],
        ..Default::default()
    };
    for &hm in CLOCKS {
        let o = drive_at(hm, c);
        assert_eq!(
            o.state,
            [
                "automatic=false",
                "configReady=true",
                "shouldBeOn=true",
                "gamma=100",
                "temperatureActive=false"
            ],
            "at {hm:?}"
        );
        let sent: Vec<&str> = o
            .act
            .iter()
            .filter(|r| r.0 == ".hyprsunset.sock")
            .map(|r| r.1.as_str())
            .collect();
        let loading = o
            .load
            .contains(&(".hyprsunset.sock", "temperature 5000".to_string()));
        assert_eq!(loading, night(hm), "at {hm:?}: {o:?}");
        // Everything after the schedule turned night light on: all of `act` when that was
        // while loading.
        let on = match sent.iter().rposition(|r| *r == "temperature 5000") {
            Some(on) => on,
            None if loading => 0,
            None => panic!("at {hm:?}: the schedule never turned night light on: {sent:?}"),
        };
        for want in ["temperature 6000", "gamma 100"] {
            assert!(
                sent[on..].contains(&want),
                "at {hm:?}: {want} not after {sent:?}"
            );
        }
    }
}

/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// What qml/services/Hyprsunset.qml imports, for Qt itself: a `Config` that is read after the
/// singleton exists (`ready` turns true from a timer, as the config file's `onLoaded` does), a
/// clock at 19:00 and a `HyprsunsetClient` that keeps what it is asked to send.
const QT_HYPRSUNSET: &[(&str, &str)] = &[
    (
        "ri/modules/common/qmldir",
        "module ri.modules.common\nsingleton Config Config.qml\nsingleton Appearance Appearance.qml\n",
    ),
    (
        "ri/modules/common/Config.qml",
        "pragma Singleton\nimport QtQuick\nQtObject {\n    property bool ready: false\n    property QtObject options: QtObject {\n        property QtObject light: QtObject {\n            property QtObject night: QtObject {\n                property bool automatic: AUTOMATIC\n                property int colorTemperature: 5000\n                property string from: \"19:00\"\n                property string to: \"06:30\"\n            }\n        }\n        property QtObject appearance: QtObject { property bool autoDarkMode: false }\n    }\n}\n",
    ),
    (
        "ri/modules/common/Appearance.qml",
        "pragma Singleton\nimport QtQuick\nQtObject { property QtObject m3colors: QtObject { property bool darkmode: true } }\n",
    ),
    (
        "ri/services/qmldir",
        "module ri.services\nsingleton Hyprsunset Hyprsunset.qml\nsingleton DateTime DateTime.qml\nsingleton Switchwall Switchwall.qml\n",
    ),
    (
        "ri/services/DateTime.qml",
        "pragma Singleton\nimport QtQuick\nQtObject { property QtObject clock: QtObject { property int hours: 19; property int minutes: 0 } }\n",
    ),
    (
        "ri/services/Switchwall.qml",
        "pragma Singleton\nimport QtQuick\nQtObject { function run(o) {} }\n",
    ),
    (
        "Rustshell/qmldir",
        "module Rustshell\nSingleton 1.0 Singleton.qml\n",
    ),
    ("Rustshell/Singleton.qml", "import QtQuick\nItem {}\n"),
    (
        "Rustshell/Io/qmldir",
        "module Rustshell.Io\nNone 1.0 None.qml\n",
    ),
    ("Rustshell/Io/None.qml", "import QtQuick\nQtObject {}\n"),
    (
        "Rustshell/Hyprland/qmldir",
        "module Rustshell.Hyprland\nNone 1.0 None.qml\n",
    ),
    (
        "Rustshell/Hyprland/None.qml",
        "import QtQuick\nQtObject {}\n",
    ),
    (
        "Impulse/qmldir",
        "module Impulse\nHyprsunsetClient 1.0 HyprsunsetClient.qml\nsingleton Sent 1.0 Sent.qml\n",
    ),
    (
        "Impulse/Sent.qml",
        "pragma Singleton\nimport QtQuick\nQtObject { property var lines: [] }\n",
    ),
    (
        "Impulse/HyprsunsetClient.qml",
        "import QtQuick\nQtObject {\n    signal temperatureRead(string text)\n    function start() {}\n    function readTemperature() { Sent.lines.push(\"temperature\") }\n    function setTemperature(t) { Sent.lines.push(\"temperature \" + t) }\n    function setGamma(g) { Sent.lines.push(\"gamma \" + g) }\n}\n",
    ),
    (
        "startup.qml",
        "import QtQuick\nimport ri.modules.common\nimport ri.services\nimport Impulse\nItem {\n    id: root\n    property string result: \"\"\n    property var s: Hyprsunset\n    Timer {\n        interval: 1; running: true\n        onTriggered: { Config.ready = true; root.result = Sent.lines.join(\", \") + \"; active=\" + Hyprsunset.temperatureActive }\n    }\n}\n",
    ),
];

/// What Qt 6.12 makes of qml/services/Hyprsunset.qml as the shell starts at 19:00 with
/// `light.night.automatic` as given: the requests to hyprsunset in order and
/// `temperatureActive` after them. `None` (reported) without PySide6.
fn qt_hyprsunset_startup(automatic: bool) -> Option<(Vec<String>, bool)> {
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in QT_HYPRSUNSET {
        let file = dir.path().join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text.replace("AUTOMATIC", &automatic.to_string())).unwrap();
    }
    std::fs::copy(
        ri().join("services/Hyprsunset.qml"),
        dir.path().join("ri/services/Hyprsunset.qml"),
    )
    .unwrap();
    let o = std::process::Command::new(PYTHON)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../rigen/check/qt.py"))
        .arg(dir.path().join("startup.qml"))
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QML_IMPORT_PATH", dir.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    let got = text
        .lines()
        .find_map(|l| l.strip_prefix("startup: "))
        .and_then(|l| l.rsplit_once("; active="))
        .unwrap_or_else(|| panic!("qt.py: {text}{}", String::from_utf8_lossy(&o.stderr)));
    let sent = got.0.split(", ").filter(|r| !r.is_empty());
    Some((sent.map(str::to_string).collect(), got.1 == "true"))
}

/// The shell starting at 19:00, where the config file is read after `Hyprsunset` exists. On
/// the schedule night light is turned on and nothing turns it off first, as in the original. Off the
/// schedule (end-4/dots-hyprland #3373) a temperature hyprsunset still has from before the
/// shell started is reset: docs/original-differences.md. Qt 6.12 sends the same for the same file.
#[test]
fn hyprsunset_at_startup_like_qt() {
    if !own_process(concat!(module_path!(), "::hyprsunset_at_startup_like_qt")) {
        return;
    }
    // `light.night.automatic`, hyprsunset's temperature before the shell starts, what it is
    // sent, and `temperatureActive` after that.
    let cases: [(bool, &str, &[&str], bool); 2] = [
        (
            true,
            SUNSET_OFF,
            &["temperature", "temperature 5000", "temperature 5000"],
            true,
        ),
        (false, "4000", &["temperature", "temperature 6000"], false),
    ];
    for (automatic, before, want, active) in cases {
        let f = fake();
        qml::builtins::set_clock(Some(clock((19, 0))));
        let (rt, _h, r, load) = boot_with(
            &f,
            "import QtQuick\nimport ri.services\nItem { property var s: Hyprsunset }",
            &format!(r#"{{"light":{{"night":{{"automatic":{automatic}}}}}}}"#),
            before,
        );
        qml::builtins::set_clock(None);
        let Value::Item(s) = rt.get(r, "s") else {
            panic!("no s: {:?}", rt.errors())
        };
        let sent = load.into_iter().filter(|r| r.0 == ".hyprsunset.sock");
        let ours: (Vec<String>, bool) = (
            sent.map(|r| r.1).collect(),
            rt.get(s, "temperatureActive").truthy(),
        );
        let want: Vec<String> = want.iter().map(|w| w.to_string()).collect();
        assert_eq!(ours, (want, active), "automatic: {automatic}");
        assert_eq!(
            *f.sunset.lock().unwrap() != SUNSET_OFF,
            active,
            "automatic: {automatic}"
        );
        drop(rt);
        drop(f);
        if let Some(qt) = qt_hyprsunset_startup(automatic) {
            assert_eq!(ours, qt, "ours and Qt 6.12, automatic: {automatic}");
        }
    }
}

/// The night schedule as the clock moves, a minute tick at a time where it matters: off until
/// `from`, on from that minute, still on over midnight and in the minute of `to`, off from the
/// next one, on again the next evening. hyprsunset is told once at each change and nothing in
/// between.
#[test]
fn hyprsunset_schedule_follows_the_clock() {
    if !own_process(concat!(
        module_path!(),
        "::hyprsunset_schedule_follows_the_clock"
    )) {
        return;
    }
    let f = fake();
    let mut now = clock((18, 59));
    qml::builtins::set_clock(Some(now));
    let (mut rt, mut h, r, load) = boot(
        &f,
        "import QtQuick\nimport ri.services\nItem { property var s: Hyprsunset }",
    );
    let Value::Item(s) = rt.get(r, "s") else {
        panic!("no s: {:?}", rt.errors())
    };
    let sunset = |reqs: Vec<(&'static str, String)>| -> Vec<String> {
        let reqs = reqs.into_iter().filter(|r| r.0 == ".hyprsunset.sock");
        reqs.map(|r| r.1).collect()
    };
    assert_eq!(sunset(load), ["temperature", "temperature 6000"]);
    // Minutes the clock moves by, the time it then reads, whether night light is on, and
    // what hyprsunset is sent for it.
    let steps: [(u32, Hm, bool, &[&str]); 9] = [
        (0, (18, 59), false, &[]),
        (1, (19, 0), true, &["temperature 5000"]),
        (1, (19, 1), true, &[]),
        (298, (23, 59), true, &[]),
        (1, (0, 0), true, &[]),
        (390, (6, 30), true, &[]),
        (1, (6, 31), false, &["temperature 6000"]),
        (748, (18, 59), false, &[]),
        (1, (19, 0), true, &["temperature 5000"]),
    ];
    for (minutes, hm, on, want) in steps {
        let by = f64::from(minutes) * 60e3;
        now += by;
        h.now += by;
        qml::builtins::set_clock(Some(now));
        h.drain(&mut rt, &mut |_| true);
        let got = (
            (
                rt.get(s, "clockHour").to_number(),
                rt.get(s, "clockMinute").to_number(),
            ),
            rt.get(s, "shouldBeOn").truthy(),
            rt.get(s, "temperatureActive").truthy(),
            sunset(f.take()),
        );
        let hm = (f64::from(hm.0), f64::from(hm.1));
        let want: Vec<String> = want.iter().map(|w| w.to_string()).collect();
        assert_eq!(got, (hm, on, on, want), "at {hm:?}");
        assert_eq!(*f.sunset.lock().unwrap() != SUNSET_OFF, on, "at {hm:?}");
    }
    qml::builtins::set_clock(None);
}

/// `fetchState()` and the temperature read it starts.
#[test]
fn hyprsunset_fetch_state_running() {
    if !own_process(concat!(module_path!(), "::hyprsunset_fetch_state_running")) {
        return;
    }
    sunset("Hyprsunset.fetchState()", "temperature");
}

/// qml/services/Hyprsunset.qml `onColorTemperatureChanged` while active.
#[test]
fn hyprsunset_on_color_temperature_changed() {
    if !own_process(concat!(
        module_path!(),
        "::hyprsunset_on_color_temperature_changed"
    )) {
        return;
    }
    sunset(
        "Hyprsunset.temperatureActive = true; Config.options.light.night.colorTemperature = 3500",
        "temperature 3500",
    );
}

/// qml/services/Hyprsunset.qml `onColorTemperatureChanged` while inactive sends nothing; the
/// change made after enabling does.
#[test]
fn hyprsunset_on_color_temperature_changed_while_inactive() {
    if !own_process(concat!(
        module_path!(),
        "::hyprsunset_on_color_temperature_changed_while_inactive"
    )) {
        return;
    }
    let src = "import QtQuick\nimport ri.modules.common\nimport ri.services\nItem { property var s: Hyprsunset\n function act() { Hyprsunset.temperatureActive = false; Config.options.light.night.colorTemperature = 3100; Hyprsunset.temperatureActive = true; Config.options.light.night.colorTemperature = 3600 } }";
    let c = Case {
        src,
        props: SUNSET,
        ..Default::default()
    };
    for &hm in CLOCKS {
        let o = drive_at(hm, c);
        let sent: Vec<&str> = o
            .act
            .iter()
            .filter(|r| r.0 == ".hyprsunset.sock")
            .map(|r| r.1.as_str())
            .collect();
        assert!(!sent.contains(&"temperature 3100"), "at {hm:?}: {o:?}");
        assert!(sent.contains(&"temperature 3600"), "at {hm:?}: {o:?}");
    }
}

const LOCK: &str = "import QtQuick\nimport ri\nimport ri.modules.rusty.lock\nItem { property var s: l\n function act() { ACT }\n Lock { id: l } }";

fn lock(act: &str) -> Outcome {
    let src: &'static str = Box::leak(LOCK.replace("ACT", act).into_boxed_str());
    drive(Case {
        src,
        props: &["savedWorkspaces"],
        min: Duration::from_millis(600),
        ..Default::default()
    })
}

/// qml/modules/rusty/lock/Lock.qml `onScreenLockedChanged` (lock: every monitor moves to its
/// hidden workspace).
#[test]
fn lock_on_screen_locked_changed() {
    if !own_process(concat!(module_path!(), "::lock_on_screen_locked_changed")) {
        return;
    }
    let o = lock("GlobalStates.screenLocked = true");
    // In the order made: each monitor is focused, then switched to its hidden workspace.
    let sent: Vec<&str> = o.act.iter().map(|r| r.1.as_str()).collect();
    assert_eq!(
        sent,
        [
            "eval return hl.dispatch(hl.dsp.focus({monitor=\"DP-1\"}))",
            "eval return hl.dispatch(hl.dsp.focus({workspace=2147483646}))",
            "eval return hl.dispatch(hl.dsp.focus({monitor=\"HDMI-A-1\"}))",
            "eval return hl.dispatch(hl.dsp.focus({workspace=1}))",
        ]
    );
}

/// qml/modules/rusty/lock/Lock.qml `onScreenLockedChanged` with a screen HyprlandData doesn't know:
/// nothing is dispatched and `savedWorkspaces` stays as it was.
#[test]
fn lock_on_screen_locked_changed_unknown_monitor() {
    if !own_process(concat!(
        module_path!(),
        "::lock_on_screen_locked_changed_unknown_monitor"
    )) {
        return;
    }
    let o = lock(
        "HyprlandData.monitors = HyprlandData.monitors.filter(m => m.name === \"DP-1\"); GlobalStates.screenLocked = true",
    );
    assert!(!o.act.iter().any(|r| r.1.contains("dispatch")), "{o:?}");
    assert_eq!(o.state, ["savedWorkspaces={}"], "{o:?}");
}

/// Unlocking starts `restoreTimer`, whose `onTriggered` restores the saved workspaces.
#[test]
fn lock_restore_on_triggered() {
    if !own_process(concat!(module_path!(), "::lock_restore_on_triggered")) {
        return;
    }
    let o = lock("GlobalStates.screenLocked = true; GlobalStates.screenLocked = false");
    // Locking moves HDMI-A-1 from the lock workspace to 1, restoring moves DP-1 back to 1.
    let to_1 = o
        .act
        .iter()
        .filter(|r| r.1 == "eval return hl.dispatch(hl.dsp.focus({workspace=1}))")
        .count();
    assert_eq!(to_1, 2, "{o:?}");
    assert_eq!(o.state, ["savedWorkspaces={}"], "{o:?}");
}

/// While locked, a monitor whose active workspace changed (a new `HyprlandData.monitors`) is
/// moved back to its hidden workspace by `onMonitorsChanged` itself: no timer runs here.
#[test]
fn lock_rehides_on_monitors_changed() {
    if !own_process(concat!(
        module_path!(),
        "::lock_rehides_on_monitors_changed"
    )) {
        return;
    }
    let o = drive(Case {
        src: Box::leak(
            LOCK.replace(
                "ACT",
                "GlobalStates.screenLocked = true; \
                 HyprlandData.monitors = HyprlandData.monitors.map(m => m.name === \"DP-1\" \
                 ? Object.assign({}, m, {activeWorkspace: {id: 7}}) : m)",
            )
            .into_boxed_str(),
        ),
        props: &["savedWorkspaces"],
        // A second drain, which waits for Hyprland's answers to the dispatches; the timer
        // this replaced ran 300 ms.
        min: Duration::from_millis(1),
        ..Default::default()
    });
    let sent: Vec<&str> = o.act.iter().map(|r| r.1.as_str()).collect();
    let hide = [
        "eval return hl.dispatch(hl.dsp.focus({monitor=\"DP-1\"}))",
        "eval return hl.dispatch(hl.dsp.focus({workspace=2147483646}))",
    ];
    assert!(
        sent.windows(2).filter(|w| *w == hide).count() >= 2,
        "locking hides DP-1 and the monitors change hides it again: {o:?}"
    );
}

#[test]
fn lock_rehide_workspaces() {
    if !own_process(concat!(module_path!(), "::lock_rehide_workspaces")) {
        return;
    }
    let o = lock("l.savedWorkspaces = {\"DP-1\": 5}; l.rehideWorkspaces()");
    assert!(!o.act.is_empty(), "{o:?}");
}

#[test]
fn sidebar_left_toggle_pin() {
    if !own_process(concat!(module_path!(), "::sidebar_left_toggle_pin")) {
        return;
    }
    let got = drive(Case {
        src: "import QtQuick\nimport ri.modules.rusty.sidebarLeft\nItem { property var s: x\n function act() { x.togglePin() }\n SidebarLeft { id: x } }",
        props: &["pin"],
        min: Duration::from_millis(300),
        ..Default::default()
    });
    // No cursor dance: the pin is the layer surface's exclusive zone, nothing else.
    assert!(got.act.is_empty(), "{got:?}");
    assert_eq!(got.state, ["pin=true"], "{got:?}");
}

/// The Reload button: `reload` to Hyprland, then `Rustshell.reload()` on its answer. The
/// shell's reload replaces the process, so a request still queued then would be lost.
#[test]
fn sidebar_right_content_reload_on_clicked() {
    if !own_process(concat!(
        module_path!(),
        "::sidebar_right_content_reload_on_clicked"
    )) {
        return;
    }
    let o = drive(Case {
        src: "import QtQuick\nimport ri.modules.rusty.sidebarRight\nItem { property var s: x\n SidebarRightContent { id: x } }",
        props: &[],
        click_icon: Some("restart_alt"),
        ..Default::default()
    });
    assert_eq!(o.act, [(".socket.sock", "/reload".to_string())]);
    // Hyprland's answer is what reloads the shell.
    assert!(qml::native::take_reload(), "no Rustshell.reload()");
}

fn game_mode(act: &str) -> Outcome {
    let src = format!(
        "import QtQuick\nimport ri.modules.rusty.sidebarRight.quickToggles.classicStyle\nItem {{ property var s: g\n function act() {{ {act} }}\n GameMode {{ id: g }} }}"
    );
    drive(Case {
        src: Box::leak(src.into_boxed_str()),
        props: &["toggled"],
        ..Default::default()
    })
}

/// The animations probe at startup and `onClicked` turning game mode on.
#[test]
fn game_mode_running_on_clicked() {
    if !own_process(concat!(module_path!(), "::game_mode_running_on_clicked")) {
        return;
    }
    let got = game_mode("g.clicked()");
    assert!(has(&got, "j/getoption animations:enabled"), "{got:?}");
    // The fake's animations are on, so game mode starts off and the click turns it on.
    assert_eq!(got.state, ["toggled=true"], "{got:?}");
    // Hyprland 0.57 has no `keyword` request: the same settings go through the Lua config.
    assert_eq!(
        got.act,
        [(
            ".socket.sock",
            "/eval hl.config({ animations = { enabled = false }, decoration = { shadow = { enabled = false }, blur = { enabled = false }, rounding = 0 }, general = { gaps_in = 0, gaps_out = 0, border_size = 1, allow_tearing = true } })".to_string()
        )]
    );
}

#[test]
fn game_mode_on_clicked_off() {
    if !own_process(concat!(module_path!(), "::game_mode_on_clicked_off")) {
        return;
    }
    let o = game_mode("g.toggled = true; g.clicked()");
    assert_eq!(o.act, [(".socket.sock", "/reload".to_string())]);
}

/// `act` on HyprlandConfig leaves `want` in the overrides file, mode 640. The expected bytes
/// are what the original's `hyprconfigurator.py` wrote for the same edits.
fn hyprconf(act: &str, want: &str) {
    let src = format!(
        "import QtQuick\nimport ri.services\nItem {{ property var s: HyprlandConfig\n function act() {{ {act} }} }}"
    );
    let f = drive(Case {
        src: Box::leak(src.into_boxed_str()),
        props: &[],
        min: Duration::from_millis(500),
        ..Default::default()
    })
    .files;
    let main = f
        .iter()
        .find(|l| l.starts_with("hypr/hyprland/shellOverrides/main.lua"))
        .unwrap();
    assert_eq!(
        main,
        &format!("hypr/hyprland/shellOverrides/main.lua 640 {want:?}")
    );
}

#[test]
fn hyprland_config_set() {
    if !own_process(concat!(module_path!(), "::hyprland_config_set")) {
        return;
    }
    hyprconf(
        "HyprlandConfig.set(\"general:gaps_out\", 8)",
        &format!("{BASE_OVERRIDES}hl.config({{general={{gaps_out=8}}}})\n"),
    );
}

#[test]
fn hyprland_config_set_many() {
    if !own_process(concat!(module_path!(), "::hyprland_config_set_many")) {
        return;
    }
    hyprconf(
        "HyprlandConfig.setMany({\"general:gaps_in\": 2, \"decoration:blur:enabled\": false, \"misc:col\": \"rgba(0,0,0,0.5)\"})",
        "-- ii shell overrides\nhl.config({general={gaps_in=2}})\nhl.config({decoration={rounding=12}})\nhl.config({decoration={blur={enabled=false}}})\nhl.config({misc={col=\"rgba(0,0,0,0.5)\"}})\n",
    );
}

/// The overrides file `act` leaves.
fn overrides_after(act: &str) -> String {
    let src = format!(
        "import QtQuick\nimport ri.services\nItem {{ property var s: HyprlandConfig\n function act() {{ {act} }} }}"
    );
    let c = Case {
        src: Box::leak(src.into_boxed_str()),
        props: &[],
        min: Duration::from_millis(500),
        ..Default::default()
    };
    let f = fake();
    run(&f, &c)
        .files
        .into_iter()
        .find(|l| l.starts_with("hypr/hyprland/shellOverrides/main.lua"))
        .unwrap()
}

/// A value with spaces is written as given and the other keys are left alone (the original's
/// `hyprconfigurator.py` word-split its arguments and lost such a setting).
#[test]
fn hyprland_config_set_value_with_spaces() {
    if !own_process(concat!(
        module_path!(),
        "::hyprland_config_set_value_with_spaces"
    )) {
        return;
    }
    let main =
        overrides_after("HyprlandConfig.setMany({\"general:gaps_in\": 2, \"misc:x\": \"a b\"})");
    assert!(main.contains("a b") && main.contains("gaps_in=2"), "{main}");
    let main = overrides_after(
        "HyprlandConfig.set(\"general:gaps_in\", \"1 --reset decoration:rounding\")",
    );
    assert!(
        main.contains("1 --reset decoration:rounding") && main.contains("rounding=12"),
        "{main}"
    );
}

#[test]
fn hyprland_config_reset() {
    if !own_process(concat!(module_path!(), "::hyprland_config_reset")) {
        return;
    }
    hyprconf(
        "HyprlandConfig.reset(\"general:gaps_in\")",
        "-- ii shell overrides\nhl.config({decoration={rounding=12}})\n",
    );
}

#[test]
fn hyprland_config_reset_many() {
    if !own_process(concat!(module_path!(), "::hyprland_config_reset_many")) {
        return;
    }
    hyprconf(
        "HyprlandConfig.resetMany([\"general:gaps_in\", \"decoration:rounding\"])",
        "-- ii shell overrides\n",
    );
}

fn cpu_ms() -> f64 {
    // SAFETY: rusage is plain integers, for which all-zero bytes are a valid value.
    let mut r: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `r` is a valid, writable rusage for getrusage(2) to fill.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut r) };
    let ms = |v: libc::timeval| v.tv_sec as f64 * 1e3 + v.tv_usec as f64 / 1e3;
    ms(r.ru_utime) + ms(r.ru_stime)
}

/// Workspace-switch storm on `HyprlandData`: `cargo test --features live --release --test
/// hyprland_qml -- --nocapture storm` prints the time and the requests 200 events cost.
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn storm() {
    if !own_process(concat!(module_path!(), "::storm")) {
        return;
    }
    let f = fake();
    let conns = f.connections();
    let mut rt = Runtime::new(Some(ri()));
    let mut h = Host::new(&mut rt);
    rt.load_singletons_where(&[ri().join("services")], &|n| n == "HyprlandData");
    // Counts the events the runtime has handled.
    let r = rt
        .load_str("import QtQuick\nimport Rustshell.Hyprland\nimport ri.services\nItem { property var s: HyprlandData\n property int rawEvents: 0\n Connections { target: Hyprland; function onRawEvent(event) { rawEvents++ } } }")
        .unwrap();
    settle(&mut rt, &mut h, Duration::ZERO);
    assert!(matches!(rt.get(r, "s"), Value::Item(_)));
    f.wait_connection(conns);
    f.take();
    let c0 = cpu_ms();
    let t = std::time::Instant::now();
    let n = 200;
    // Back to back, as Hyprland writes them; the shell takes them as they come.
    for i in 0..n {
        event(&f, &format!("workspace>>{}", i % 5 + 1));
    }
    h.drain(&mut rt, &mut |rt| {
        rt.get(r, "rawEvents").to_number() >= n as f64
    });
    let reqs = f.take();
    let data = ["j/monitors", "j/workspaces", "j/activeworkspace"];
    let mine = reqs.iter().filter(|r| data.contains(&r.1.as_str())).count();
    println!(
        "storm events={n} wall={:.0}ms shell_cpu={:.0}ms requests={} data_requests={mine}",
        t.elapsed().as_secs_f64() * 1e3,
        cpu_ms() - c0,
        reqs.len()
    );
    // A burst costs far fewer calls than events: a request made while one is out replaces
    // the one waiting.
    assert!(mine < n * data.len(), "{mine} requests for {n} events");
}
