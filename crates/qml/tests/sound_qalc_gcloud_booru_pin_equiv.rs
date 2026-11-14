//! Four members of the tree's QML, each loaded verbatim: Audio.qml `playSystemSound`,
//! LauncherSearch.qml `mathProc.calculateExpression`, GoogleCloud.qml `getToken`
//! (`tokenProc`) and SidebarLeft.qml `togglePin`. What each does is checked against the real
//! `qalc`, `curl`, Hyprland socket and token script (or argv-recording fakes of the sound
//! players); `getToken` also against the `Process` with the command line the original had there, kept
//! in this file.
//! Compared: the program argv, files, HTTP requests, Hyprland socket requests and QML state.
//! Each test runs in a process of its own (`own_process`), whose environment names its
//! private homes, runtime dir and bus from the start.

use qml::runtime::Runtime;
use qml::value::Value;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use testenv::Wakeups;

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn tools(t: &[&str]) -> bool {
    t.iter().all(|t| testenv::require(t))
}

/// Requests to the fake Hyprland `.socket.sock`.
static HYPR: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// This process's temp root, made by [`run_on_fresh_base`]: config home, runtime dir and a
/// fake Hyprland on it, started once.
fn base() -> &'static Path {
    static B: OnceLock<PathBuf> = OnceLock::new();
    B.get_or_init(|| {
        let rt = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        let d = rt.parent().unwrap().to_path_buf();
        let hypr = rt.join("hypr/TEST");
        let l = UnixListener::bind(hypr.join(".socket.sock")).unwrap();
        // Hyprland writes its lock once its sockets listen.
        std::fs::write(hypr.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                let mut buf = [0u8; 8192];
                let n = s.read(&mut buf).unwrap_or(0);
                let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
                HYPR.lock().unwrap().push(cmd.clone());
                let what = cmd.strip_prefix("j/").unwrap_or(&cmd);
                let r = match what.trim_start_matches('/') {
                    "cursorpos" => "960, 540",
                    "monitors" | "workspaces" | "clients" | "devices" | "binds" => "[]",
                    "activeworkspace" | "activewindow" | "layers" => "{}",
                    _ => "ok",
                };
                let _ = s.write_all(r.as_bytes());
            }
        });
        let ev = UnixListener::bind(hypr.join(".socket2.sock")).unwrap();
        std::thread::spawn(move || {
            let mut keep = vec![];
            for c in ev.incoming().flatten() {
                keep.push(c);
            }
        });
        d
    })
}

/// A root for [`base`] in `d`, and the environment that names it: config home (with an empty
/// rusty-impulse config), cache, state and data homes, and a runtime dir for Hyprland instance `TEST`.
fn base_vars(d: &Path) -> Vec<(&'static str, OsString)> {
    let ri_conf = d.join("config/rusty-impulse");
    std::fs::create_dir_all(&ri_conf).unwrap();
    std::fs::write(ri_conf.join("config.json"), "{}").unwrap();
    std::fs::create_dir_all(d.join("rt/hypr/TEST")).unwrap();
    std::fs::set_permissions(d.join("rt"), std::fs::Permissions::from_mode(0o700)).unwrap();
    vec![
        ("XDG_RUNTIME_DIR", d.join("rt").into()),
        ("HYPRLAND_INSTANCE_SIGNATURE", "TEST".into()),
        ("XDG_CONFIG_HOME", d.join("config").into()),
        ("XDG_CACHE_HOME", d.join("cache").into()),
        ("XDG_STATE_HOME", d.join("state").into()),
        ("XDG_DATA_HOME", d.join("data").into()),
    ]
}

/// Runs `test` in a process of its own (`testenv::run_in_own_process`) on a fresh [`base`]
/// root, with `vars` on top: the original services read these homes, the runtime dir and the bus
/// from the environment.
fn run_on_fresh_base(test: &str, vars: Vec<(&'static str, OsString)>) {
    let d = tempfile::tempdir().unwrap();
    testenv::run_in_own_process(test, base_vars(d.path()).into_iter().chain(vars));
}

/// This is `test`'s own process; elsewhere runs it there ([`run_on_fresh_base`]) first.
fn own_process(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    run_on_fresh_base(test, vec![]);
    false
}

/// The test's host loop on a virtual clock (`Runtime::tick` sees `now`): it sleeps only on the
/// runtime's own wakeups (its waker's ping, its epoll set: process output and exits) and on
/// pool work (`workpool::wait_idle`, which also reaps `execDetached` children), and jumps the
/// clock to the next timer's deadline once nothing is in flight, so a test waits for what it
/// waits for, never for a stretch of wall time.
struct Host {
    wake: Wakeups,
    ep: Option<i32>,
    now: f64,
}

impl Host {
    /// Becomes `rt`'s host loop: installs its waker ping and watches its epoll set.
    fn new(rt: &mut Runtime) -> Host {
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Host { wake, ep, now: 0.0 }
    }

    /// Runs loop turns until nothing is ready at `now`: no pool work, no wakeup, no epoll
    /// event, no timer due and nothing a poll changed left to poll.
    fn drain(&mut self, rt: &mut Runtime) {
        loop {
            let rev = rt.revision();
            workpool::wait_idle();
            rt.poll_native();
            rt.tick(self.now);
            rt.poll_native();
            rt.settle();
            let woke = self.wake.wait(self.ep, Some(Duration::ZERO));
            if woke.fd {
                rt.process_fd_ready();
            }
            if !woke.any() && rt.revision() == rev && rt.next_deadline(self.now) > 0.0 {
                return;
            }
        }
    }

    /// A Process (child or in-process job) or a keyring request is running: its end wakes
    /// the loop.
    fn in_flight(rt: &Runtime) -> bool {
        let w = rt.world.borrow();
        !w.procs.is_empty() || w.keyring.as_ref().is_some_and(|k| !k.idle())
    }

    /// Sleeps until the runtime is woken.
    fn block(&mut self, rt: &mut Runtime) {
        if self.wake.wait(self.ep, None).fd {
            rt.process_fd_ready();
        }
    }

    /// Runs the loop until `done`.
    fn until(&mut self, rt: &mut Runtime, what: &str, mut done: impl FnMut(&mut Runtime) -> bool) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        loop {
            self.drain(rt);
            if done(rt) {
                return;
            }
            let d = rt.next_deadline(self.now);
            if Self::in_flight(rt) || !d.is_finite() {
                self.block(rt);
            } else {
                self.now += d;
            }
        }
    }

    /// Runs the loop through the next `ms` of virtual time: every Process started meanwhile
    /// runs to its end, each timer fires at its deadline.
    fn advance(&mut self, rt: &mut Runtime, ms: f64) {
        let end = self.now + ms;
        loop {
            self.drain(rt);
            if Self::in_flight(rt) {
                self.block(rt);
                continue;
            }
            let d = rt.next_deadline(self.now);
            if self.now + d <= end {
                self.now += d;
            } else if self.now < end {
                self.now = end;
            } else {
                return;
            }
        }
    }
}

/// `src` loaded, with its host loop and its `s` item.
fn load(name: &str, src: &str) -> (Runtime, Host, usize, usize) {
    load_in(&ri(), name, src)
}

/// A copy of the shell tree under `d` (made once per `d`) with the original's `services/gCloud`
/// scripts (fixtures/original-unshipped) put back in their place.
fn tree_with_unshipped(d: &Path) -> PathBuf {
    let tree = d.join("tree");
    if tree.is_dir() {
        return tree;
    }
    std::fs::create_dir_all(&tree).unwrap();
    let unshipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/original-unshipped");
    for from in [ri().join("."), unshipped.join("services")] {
        let st = std::process::Command::new("cp")
            .arg("-r")
            .arg(from)
            .arg(&tree)
            .status();
        assert!(st.unwrap().success());
    }
    tree
}

/// [`load`] from the shell tree at `tree`.
fn load_in(tree: &Path, name: &str, src: &str) -> (Runtime, Host, usize, usize) {
    let mut rt = Runtime::new(Some(tree.to_path_buf()));
    rt.set_screens(&[("DP-1".into(), 0, 0, 1920, 1080)]);
    rt.load_singletons_lazy(&[
        tree.to_path_buf(),
        tree.join("services"),
        tree.join("modules/common"),
        tree.join("modules/common/functions"),
    ]);
    // A file, not `load_str`: `import ri.modules...` component types resolve per document.
    // Not the original type's own name: a document beside it would shadow the import.
    let doc = base().join(format!("Case{name}.qml"));
    std::fs::write(&doc, src).unwrap();
    let r = rt
        .load_file(&doc)
        .unwrap_or_else(|e| panic!("load: {e:?} {:?}", rt.errors()));
    let mut host = Host::new(&mut rt);
    host.advance(&mut rt, 300.0);
    let Value::Item(s) = rt.get(r, "s") else {
        panic!("no s: {:?}", rt.errors())
    };
    (rt, host, r, s)
}

fn act(rt: &mut Runtime, r: usize, f: &str, args: Vec<Value>) -> Value {
    rt.call_method(r, f, args)
        .unwrap_or_else(|| panic!("no {f}"))
        .unwrap_or_else(|e| panic!("{f}: {e} {:?}", rt.errors()))
}

fn done(rt: Runtime) {
    drop(rt);
}

fn fresh(name: &str) -> PathBuf {
    let d = base().join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn lines(p: &Path) -> Vec<String> {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

// --- Audio.qml playSystemSound -------------------------------------------------------------

const AUDIO: &str = "import QtQuick\nimport ri.services\nItem { property var s: Audio\n function act(t, n) { Audio.audioTheme = t; Audio.playSystemSound(n) } }";

/// Sound players that log `name argv… ok|missing` and fail on a missing file, as ffplay does.
fn fake_players(d: &Path, names: &[&str], log: &Path) -> PathBuf {
    let bin = d.join(names.join("+"));
    std::fs::create_dir_all(&bin).unwrap();
    for n in names {
        let f = bin.join(n);
        testenv::install_script(
            &f,
            &format!(
                "#!/bin/sh\nf=\nfor a; do f=$a; done\n[ -f \"$f\" ] && r=ok || r=missing\necho \"{n} $* $r\" >> '{}'\n[ $r = ok ]\n",
                log.display()
            ),
        );
    }
    bin
}

/// What `playSystemSound(name)` started, with PATH = `bin` only and `vars` on top of the
/// side's environment.
fn play_in(
    bin: &Path,
    log: &Path,
    vars: &[(&str, &Path)],
    theme: &str,
    name: &str,
    want: usize,
) -> Vec<String> {
    let _ = std::fs::remove_file(log);
    let root = tempfile::tempdir().unwrap();
    let mut c = std::process::Command::new(std::env::current_exe().unwrap());
    c.args([
        "audio_play_side",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ])
    .envs(base_vars(root.path()))
    .envs(vars.iter().copied())
    .env("PATH", bin)
    .env("RI_AUDIO_SIDE_LOG", log)
    .env("RI_AUDIO_SIDE_THEME", theme)
    .env("RI_AUDIO_SIDE_NAME", name)
    .env("RI_AUDIO_SIDE_WANT", want.to_string());
    let o = c.output().unwrap();
    assert!(
        o.status.success(),
        "{theme} {name}: the side failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    lines(log)
}

/// One `play` in a process of its own, with `play`'s PATH, side and [`base`] root from the
/// start: `Audio`
/// starts the pipewire thread, whose libpipewire reads the environment with getenv(3), so the
/// environment must not change once it runs. A no-op when not run by `play`.
#[test]
fn audio_play_side() {
    let Some(log) = std::env::var_os("RI_AUDIO_SIDE_LOG") else {
        return;
    };
    let var = |k: &str| std::env::var(k).unwrap();
    let want: usize = var("RI_AUDIO_SIDE_WANT").parse().unwrap();
    let log = PathBuf::from(log);
    let (mut rt, mut host, r, _) = load("Audio", AUDIO);
    let args = vec![
        Value::str(&var("RI_AUDIO_SIDE_THEME")),
        Value::str(&var("RI_AUDIO_SIDE_NAME")),
    ];
    act(&mut rt, r, "act", args);
    host.until(&mut rt, "the players", |_| lines(&log).len() >= want);
    host.advance(&mut rt, 200.0);
}

/// `Audio.playSystemSound` plays the one of `<name>.oga` and `<name>.ogg` that exists, once
/// (the `.oga` when both do), with `pw-play`, else with `ffplay -nodisp -autoexit`; nothing
/// starts for a sound the theme does not have. (the original started `ffplay` on both names.)
#[test]
fn audio_play_system_sound_plays_the_theme_file_once() {
    if !tools(&["sh"]) {
        return;
    }
    if !Path::new("/usr/share/sounds/freedesktop/stereo/bell.oga").is_file() {
        testenv::unavailable("sound-theme-freedesktop is not installed");
        return;
    }
    if !own_process(concat!(
        module_path!(),
        "::audio_play_system_sound_plays_the_theme_file_once"
    )) {
        return;
    }
    let d = fresh("audio");
    // A theme of the test's in a system data directory before the installed ones.
    let theme = d.join("share/sounds/ri-test");
    let stereo = theme.join("stereo");
    std::fs::create_dir_all(&stereo).unwrap();
    std::fs::write(
        theme.join("index.theme"),
        "[Sound Theme]\nName=test\nDirectories=stereo\n\n[stereo]\nOutputProfile=stereo\n",
    )
    .unwrap();
    for f in ["a.oga", "b.ogg", "c.oga", "c.ogg"] {
        std::fs::write(stereo.join(f), f).unwrap();
    }
    let log = d.join("log");
    let ff = fake_players(&d, &["ffplay"], &log);
    let both = fake_players(&d, &["ffplay", "pw-play"], &log);
    let dirs = std::env::join_paths([d.join("share").as_path(), Path::new("/usr/share")]).unwrap();
    let vars: [(&str, &Path); 1] = [("XDG_DATA_DIRS", Path::new(&dirs))];
    let installed = Path::new("/usr/share/sounds/freedesktop");
    for (theme_name, dir, n, exists) in [
        ("freedesktop", installed, "bell", [true, false]),
        ("ri-test", theme.as_path(), "a", [true, false]),
        ("ri-test", theme.as_path(), "b", [false, true]),
        ("ri-test", theme.as_path(), "c", [true, true]),
        ("ri-test", theme.as_path(), "none", [false, false]),
    ] {
        let theme = theme_name;
        let paths = ["oga", "ogg"].map(|x| format!("{}/stereo/{n}.{x}", dir.display()));
        let ffplay = |p: &str, ok: bool| {
            format!(
                "ffplay -nodisp -autoexit {p} {}",
                if ok { "ok" } else { "missing" }
            )
        };
        let first = (0..2).find(|&i| exists[i]);
        let native = play_in(&both, &log, &vars, theme, n, first.map_or(0, |_| 1));
        let native_ff = play_in(&ff, &log, &vars, theme, n, first.map_or(0, |_| 1));
        match first {
            Some(i) => {
                assert_eq!(native, [format!("pw-play {} ok", paths[i])], "{theme} {n}");
                // Without pw-play: one ffplay.
                assert_eq!(native_ff, [ffplay(&paths[i], true)], "{theme} {n}");
            }
            None => {
                assert!(
                    native.is_empty() && native_ff.is_empty(),
                    "{theme} {n}: {native:?}"
                );
            }
        }
    }
}

/// The freedesktop Sound Theme Specification's lookup: base directories are
/// `$XDG_DATA_HOME/sounds`, then each `$XDG_DATA_DIRS/sounds`; a theme's directories are the
/// ones its `index.theme` lists; a sound the theme lacks is looked for in the themes it
/// inherits, then in `freedesktop`, then unthemed; a name not found is cut at its last `-`;
/// `.disabled` ends the lookup, then `.oga`, `.ogg`, `.wav`.
#[test]
fn audio_play_system_sound_follows_the_sound_theme_spec() {
    if !tools(&["sh"]) {
        return;
    }
    if !own_process(concat!(
        module_path!(),
        "::audio_play_system_sound_follows_the_sound_theme_spec"
    )) {
        return;
    }
    let d = fresh("audio-spec");
    let (home, sys1, sys2) = (d.join("data"), d.join("sys1"), d.join("sys2"));
    let put = |base: &Path, rel: &str, text: &str| {
        let f = base.join("sounds").join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    };
    let index = |inherits: &str, dirs: &[(&str, &str)]| {
        let names: Vec<&str> = dirs.iter().map(|d| d.0).collect();
        let mut s = format!(
            "[Sound Theme]\nName=T\nComment=T\n{inherits}Directories={}\n",
            names.join(",")
        );
        for (dir, profile) in dirs {
            s += &format!("\n[{dir}]\n{profile}");
        }
        s
    };
    let stereo = "OutputProfile=stereo\n";
    // The user's theme: its index is the user's, its files are in both base directories.
    put(
        &home,
        "mine/index.theme",
        &index(
            "Inherits=parent\n",
            &[("stereo", stereo), ("alerts/loud", stereo), ("flat", "")],
        ),
    );
    put(&home, "mine/stereo/own.oga", "");
    put(&home, "mine/stereo/wave.wav", "");
    put(&home, "mine/stereo/off.disabled", "");
    put(&home, "mine/alerts/loud/deep.ogg", "");
    put(&home, "mine/flat/plain.oga", "");
    put(&home, "mine/stereo/both.oga", "");
    put(&home, "mine/flat/both.oga", "");
    put(&home, "mine/unlisted/hidden.oga", "");
    put(&home, "mine/stereo/message-new.oga", "");
    put(&sys1, "mine/stereo/spread.oga", "");
    put(&sys1, "mine/stereo/own.oga", "");
    // Its parent and grandparent, in the system directories.
    put(
        &sys1,
        "parent/index.theme",
        &index("Inherits=grand\n", &[("stereo", stereo)]),
    );
    put(&sys1, "parent/stereo/inherited.oga", "");
    put(&sys1, "parent/stereo/off.oga", "");
    put(
        &sys2,
        "grand/index.theme",
        &index("", &[("stereo", stereo)]),
    );
    put(&sys2, "grand/stereo/old.oga", "");
    // The default theme and an unthemed sound.
    put(
        &sys2,
        "freedesktop/index.theme",
        &index("", &[("stereo", stereo)]),
    );
    put(&sys2, "freedesktop/stereo/bell.oga", "");
    put(&sys2, "freedesktop/stereo/own.oga", "");
    put(&sys1, "loose.oga", "");
    let log = d.join("log");
    let bin = fake_players(&d, &["pw-play"], &log);
    let dirs = std::env::join_paths([&sys1, &sys2]).unwrap();
    let vars: [(&str, &Path); 2] = [
        ("XDG_DATA_HOME", &home),
        ("XDG_DATA_DIRS", Path::new(&dirs)),
    ];
    let at = |base: &Path, rel: &str| Some(base.join("sounds").join(rel));
    for (theme, name, want) in [
        ("mine", "own", at(&home, "mine/stereo/own.oga")),
        ("mine", "wave", at(&home, "mine/stereo/wave.wav")),
        ("mine", "deep", at(&home, "mine/alerts/loud/deep.ogg")),
        ("mine", "plain", at(&home, "mine/flat/plain.oga")),
        // The stereo profile before the directory without one.
        ("mine", "both", at(&home, "mine/stereo/both.oga")),
        // A theme spread over two base directories.
        ("mine", "spread", at(&sys1, "mine/stereo/spread.oga")),
        // Inherited, recursively; then the default theme; then unthemed.
        (
            "mine",
            "inherited",
            at(&sys1, "parent/stereo/inherited.oga"),
        ),
        ("mine", "old", at(&sys2, "grand/stereo/old.oga")),
        ("mine", "bell", at(&sys2, "freedesktop/stereo/bell.oga")),
        ("mine", "loose", at(&sys1, "loose.oga")),
        // The name cut at its last dash until a sound is found.
        (
            "mine",
            "message-new-email-urgent",
            at(&home, "mine/stereo/message-new.oga"),
        ),
        // Disabled in the theme: the parent's file is not played.
        ("mine", "off", None),
        // A directory the index does not list is not part of the theme.
        ("mine", "hidden", None),
        // A theme that is nowhere: the default theme.
        ("absent", "bell", at(&sys2, "freedesktop/stereo/bell.oga")),
        ("absent", "inherited", None),
        ("mine", "none", None),
    ] {
        let got = play_in(&bin, &log, &vars, theme, name, usize::from(want.is_some()));
        let want: Vec<String> = want
            .iter()
            .map(|f| format!("pw-play {} ok", f.display()))
            .collect();
        assert_eq!(got, want, "{theme} {name}");
    }
}

// --- LauncherSearch.qml mathProc.calculateExpression ----------------------------------------

const LAUNCHER: &str = "import QtQuick\nimport ri.services\nItem { property var s: LauncherSearch\n function act(q) { LauncherSearch.query = q; return LauncherSearch.results.length } }";

/// `mathResult` after typing `=expr` into the launcher.
fn math(expr: &str) -> String {
    let (mut rt, mut host, r, s) = load("LauncherSearch", LAUNCHER);
    act(&mut rt, r, "act", vec![Value::str(&format!("={expr}"))]);
    // Through `nonAppResultsTimer` and the qalc it starts, to its end: qalc may print more
    // than one line; the last one wins in both. A blank expression starts none.
    host.advance(&mut rt, 400.0);
    let v = rt.get(s, "mathResult").to_js_string();
    if v.is_empty() {
        eprintln!("{expr}: no mathResult; errors {:?}", rt.errors());
    }
    done(rt);
    v
}

/// `qalc` itself on `args`: its last stdout line.
fn qalc(args: &[&str]) -> String {
    let o = std::process::Command::new("qalc")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .last()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn launcher_search_calculate_expression_runs_qalc() {
    if !tools(&["qalc"]) {
        return;
    }
    if !own_process(concat!(
        module_path!(),
        "::launcher_search_calculate_expression_runs_qalc"
    )) {
        return;
    }
    // The expression goes to qalc after `--`, so one that starts with `-` (`=-v`, `=--help`)
    // is an expression and not a qalc option.
    for e in [
        "1+1",
        "5 m to ft",
        "sqrt(16)*3",
        "-5*3",
        "2^10 / 4",
        "-v",
        "--help",
    ] {
        let want = qalc(&["-t", "--", e]);
        assert!(!want.is_empty(), "{e}: qalc printed nothing");
        assert_eq!(math(e), want, "{e}");
    }
    // A blank expression has no result (qalc answers `   ` with 0).
    assert_eq!(math("   "), "");
}

// --- GoogleCloud.qml getToken (tokenProc) --------------------------------------------------

/// The tree's `GoogleCloud` (its token request is the `Impulse` type `GoogleCloudToken`).
const GCLOUD: &str = r#"import QtQuick
import ri.services
Item {
    property var s: GoogleCloud
    function act(k) { s.keyContent = JSON.parse(k); s.keyError = false; s.tokenReady = false; s.tokenError = false; s.token = ""; s.getToken() }
    function expiry() { return s.tokenExpiry ? s.tokenExpiry.getTime() : NaN }
}"#;

/// GoogleCloud.qml as the original had it (`tests/gcloud_commands`): `tokenProc`, a MultiTurnProcess
/// running `token-from-key-venv.sh`.
const GCLOUD_COMMAND: &str = r#"import QtQuick
import ri.services
Item {
    property var s: g
    GoogleCloudCommand { id: g }
    function act(k) { s.keyContent = JSON.parse(k); s.keyError = false; s.tokenReady = false; s.tokenError = false; s.token = ""; s.getToken() }
    function expiry() { return s.tokenExpiry ? s.tokenExpiry.getTime() : NaN }
}"#;

/// A token endpoint answering every POST with an access token; counts requests.
fn token_server() -> (u16, Arc<Mutex<usize>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let n = Arc::new(Mutex::new(0));
    let c = n.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let c = c.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut len = 0;
                let mut line = String::new();
                r.read_line(&mut line).unwrap_or(0);
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.trim_end().split_once(": ")
                        && k.eq_ignore_ascii_case("content-length")
                    {
                        len = v.parse().unwrap_or(0);
                    }
                }
                let mut b = vec![0; len];
                let _ = r.read_exact(&mut b);
                *c.lock().unwrap() += 1;
                let body =
                    r#"{"access_token":"ya29.test-token","expires_in":3600,"token_type":"Bearer"}"#;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    (port, n)
}

fn rsa_pem() -> String {
    let o = std::process::Command::new("openssl")
        .args([
            "genpkey",
            "-algorithm",
            "RSA",
            "-pkeyopt",
            "rsa_keygen_bits:2048",
        ])
        .output()
        .unwrap();
    assert!(o.status.success());
    String::from_utf8(o.stdout).unwrap()
}

#[derive(Debug, PartialEq)]
struct Token {
    token: String,
    error: bool,
    ready: bool,
    requests: usize,
}

fn get_token(original: bool, key: &str, reqs: &Mutex<usize>) -> (Token, f64) {
    *reqs.lock().unwrap() = 0;
    for (name, src) in [
        (
            "GoogleCloudCommand.qml",
            include_str!("gcloud_commands/GoogleCloudCommand.qml"),
        ),
        (
            "MultiTurnProcess.qml",
            include_str!("gcloud_commands/MultiTurnProcess.qml"),
        ),
    ] {
        std::fs::write(base().join(name), src).unwrap();
    }
    let doc = if original { GCLOUD_COMMAND } else { GCLOUD };
    // The original runs services/gCloud/token-from-key-venv.sh, which the shell tree no
    // longer holds.
    let (mut rt, mut host, r, s) = if original {
        load_in(&tree_with_unshipped(base()), "GoogleCloud", doc)
    } else {
        load("GoogleCloud", doc)
    };
    // KeyringStorage's own first load (no keyring here) settles before the call.
    host.advance(&mut rt, 700.0);
    act(&mut rt, r, "act", vec![Value::str(key)]);
    host.until(&mut rt, "the token", |rt| rt.get(s, "tokenReady").truthy());
    let t = Token {
        token: rt.get(s, "token").to_js_string(),
        error: rt.get(s, "tokenError").truthy(),
        ready: rt.get(s, "tokenReady").truthy(),
        requests: *reqs.lock().unwrap(),
    };
    if t.error {
        eprintln!("token error; errors {:?}", rt.errors());
    }
    let exp = act(&mut rt, r, "expiry", vec![]).to_number();
    done(rt);
    (t, exp)
}

#[test]
fn google_cloud_get_token_token_proc() {
    if !tools(&["bash", "openssl"]) {
        return;
    }
    let venv = std::env::var("RUSTY_IMPULSE_VIRTUAL_ENV").unwrap_or_default();
    let has_auth = !venv.is_empty()
        && std::process::Command::new(format!("{venv}/bin/python"))
            .args([
                "-c",
                "import google.auth, google.oauth2.service_account, requests",
            ])
            .output()
            .is_ok_and(|o| o.status.success());
    if !has_auth {
        testenv::unavailable("RUSTY_IMPULSE_VIRTUAL_ENV has no google-auth venv");
        return;
    }
    if !own_process(concat!(
        module_path!(),
        "::google_cloud_get_token_token_proc"
    )) {
        return;
    }
    let _ = base();
    let (port, reqs) = token_server();
    let key = |pem: &str| {
        serde_json::json!({
            "type": "service_account",
            "project_id": "proj-1",
            "private_key_id": "abc123",
            "private_key": pem,
            "client_email": "sa@proj-1.iam.gserviceaccount.com",
            "token_uri": format!("http://127.0.0.1:{port}/token"),
            "universe_domain": "googleapis.com",
        })
        .to_string()
    };
    let good = key(&rsa_pem());
    let (want, want_exp) = get_token(true, &good, &reqs);
    assert_eq!(
        want,
        Token {
            token: "ya29.test-token".into(),
            error: false,
            ready: true,
            requests: 1
        },
        "the original"
    );
    let (got, got_exp) = get_token(false, &good, &reqs);
    assert_eq!(got, want);
    assert!(
        (got_exp - want_exp).abs() < 60_000.0,
        "{got_exp} {want_exp}"
    );
    let bad = key("not a key");
    let (want, _) = get_token(true, &bad, &reqs);
    assert!(
        want.error && want.ready && want.token.is_empty(),
        "{want:?}"
    );
    assert_eq!(get_token(false, &bad, &reqs).0, want);
}

// --- SidebarLeft.qml togglePin ---------------------------------------------------------------

const SIDEBAR: &str = "import QtQuick\nimport ri.modules.rusty.sidebarLeft\nItem { property var s: x\n function act() { x.togglePin() }\n SidebarLeft { id: x } }";

/// The original pinned through a cursor dance (`hyprctl cursorpos`, warp to 9999,9999, pin, `sleep 0.01`,
/// warp back) for a flash on Hyprland < 0.57. The shell applies the exclusive zone on the
/// layer surface itself, so a pin is a plain state flip: no Hyprland request, no delay, the
/// cursor never moves.
#[test]
fn sidebar_left_toggle_pin_flips_pin_without_moving_the_cursor() {
    if !tools(&[]) {
        return;
    }
    if !own_process(concat!(
        module_path!(),
        "::sidebar_left_toggle_pin_flips_pin_without_moving_the_cursor"
    )) {
        return;
    }
    let _ = base();
    let (mut rt, mut host, r, s) = load("SidebarLeft", SIDEBAR);
    host.advance(&mut rt, 300.0);
    for want in [true, false, true] {
        HYPR.lock().unwrap().clear();
        act(&mut rt, r, "act", vec![]);
        assert_eq!(rt.get(s, "pin").truthy(), want, "applied synchronously");
        host.advance(&mut rt, 300.0);
        let reqs: Vec<String> = HYPR.lock().unwrap().clone();
        assert!(reqs.is_empty(), "{reqs:?}");
        assert_eq!(rt.get(s, "pin").truthy(), want);
    }
    done(rt);
}
