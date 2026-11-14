//! Windows across monitors, checked on the Wayland requests rusty-impulse sends to a headless sway
//! (`WAYLAND_DEBUG`): a monitor connected while the session is locked gets its own lock surface,
//! a destroyed lock holder leaves the session locked for the next `WlSessionLock`, the bar maps
//! through the PanelWindow path (its `screen`, layer and margins), a shown PanelWindow follows a
//! `screen` change, a popup the compositor dismissed stays closed, popups anchor to a
//! FloatingWindow and to another popup, and panel layout changes (edge, thickness, length, a
//! second panel) apply live with their exclusive zones.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use testenv::Log;
use testenv::wayland::blocked_after;

/// Tests here add and remove compositor outputs and lock the session: one at a time.
static LOCK: Mutex<()> = Mutex::new(());

fn sway_sock() -> Option<PathBuf> {
    if let Some(s) = std::env::var_os("SWAYSOCK") {
        return Some(s.into());
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("sway-ipc.") && n.ends_with(".sock"))
        })
}

/// A headless sway to run against; without a Wayland display the test fails (skipped only with
/// RI_EQUIV_SKIP=1).
fn sway() -> Option<PathBuf> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("a headless sway (WAYLAND_DISPLAY)");
        return None;
    }
    Some(
        sway_sock().expect("WAYLAND_DISPLAY is set but no sway IPC socket (these tests need sway)"),
    )
}

#[test]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "dismissed_popup_stays_closed",
        &[("WAYLAND_DISPLAY", None)],
    );
}

fn swaymsg(sock: &Path, args: &[&str]) -> String {
    let out = Command::new("swaymsg")
        .arg("-s")
        .arg(sock)
        .args(args)
        .output()
        .expect("swaymsg");
    assert!(
        out.status.success(),
        "swaymsg {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Where each workspace is, by its output: the output's area less the exclusive zones of the
/// layer surfaces on it.
fn workspace_rects(sock: &Path) -> Vec<(String, [i64; 4])> {
    let v: serde_json::Value =
        serde_json::from_str(&swaymsg(sock, &["-r", "-t", "get_workspaces"])).unwrap();
    let mut rects: Vec<(String, [i64; 4])> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            let r = ["x", "y", "width", "height"].map(|k| w["rect"][k].as_i64().unwrap());
            (w["output"].as_str().unwrap().to_string(), r)
        })
        .collect();
    rects.sort();
    rects
}

fn output_names(sock: &Path) -> Vec<String> {
    let v: serde_json::Value =
        serde_json::from_str(&swaymsg(sock, &["-r", "-t", "get_outputs"])).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .filter_map(|o| o["name"].as_str().map(str::to_string))
        .collect()
}

/// A second headless output; unplugged on drop.
struct Output {
    sock: PathBuf,
    name: String,
    plugged: bool,
}

impl Output {
    fn create(sock: &Path) -> Self {
        let before = output_names(sock);
        swaymsg(sock, &["create_output"]);
        let name = output_names(sock)
            .into_iter()
            .find(|n| !before.contains(n))
            .expect("create_output added no output");
        swaymsg(
            sock,
            &[
                "output",
                &name,
                "resolution",
                "1280x720",
                "position",
                "4000",
                "0",
            ],
        );
        Output {
            sock: sock.to_path_buf(),
            name,
            plugged: true,
        }
    }

    fn unplug(&mut self) {
        if std::mem::take(&mut self.plugged) {
            swaymsg(&self.sock, &["output", &self.name, "unplug"]);
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.unplug();
    }
}

/// rusty-impulse (or Quickshell) running `shell.qml` with its Wayland traffic logged, and for rusty-impulse its
/// loop turns.
struct Shell {
    child: Child,
    dir: tempfile::TempDir,
    log: Arc<Log>,
    /// Its WAYLAND_DISPLAY, when not this process's.
    display: Option<PathBuf>,
    /// What `ipc call` runs: the binary the shell is.
    program: std::ffi::OsString,
    /// Its XDG_RUNTIME_DIR, when not this process's.
    runtime: Option<PathBuf>,
}

impl Shell {
    fn start(qml: &str) -> Self {
        Self::start_on(qml, None)
    }

    /// On the compositor at `display` (None: WAYLAND_DISPLAY's).
    fn start_on(qml: &str, display: Option<&Path>) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
        cmd.env("RI_INTERP", "1").env("RI_LOG", "ri::sched=debug");
        Self::run(cmd, qml, display, false)
    }

    /// With `runtime` as its XDG_RUNTIME_DIR, which no shell of another test starts in.
    fn start_in(qml: &str, runtime: &Path) -> Self {
        let shared = std::env::var_os("XDG_RUNTIME_DIR").unwrap();
        let display = Path::new(&shared).join(std::env::var_os("WAYLAND_DISPLAY").unwrap());
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
        cmd.env("RI_INTERP", "1")
            .env("RI_LOG", "ri::sched=debug")
            .env("XDG_RUNTIME_DIR", runtime);
        let mut sh = Self::run(cmd, qml, Some(&display), false);
        sh.runtime = Some(runtime.to_path_buf());
        sh
    }

    /// Quickshell on the compositor at `display`.
    fn qs_on(qml: &str, display: &Path) -> Self {
        let mut cmd = Command::new("qs");
        cmd.env("QT_QUICK_BACKEND", "software");
        Self::run(cmd, qml, Some(display), true)
    }

    /// As `start_on`, with the tree's QML (`qs.modules…`, `qs.services`) beside `shell.qml`.
    fn start_in_tree(qml: &str, display: &Path) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
        cmd.env("RI_INTERP", "1").env("RI_LOG", "ri::sched=debug");
        Self::run_in(cmd, qml, Some(display), false, true)
    }

    /// `bin -p dir` with `qml` as the directory's `shell.qml`. `stdout`: it logs there
    /// (Quickshell), so that is captured with stderr.
    fn run(bin: Command, qml: &str, display: Option<&Path>, stdout: bool) -> Self {
        Self::run_in(bin, qml, display, stdout, false)
    }

    /// As `run`. `tree`: the directory also has the tree's QML, as links.
    fn run_in(
        mut bin: Command,
        qml: &str,
        display: Option<&Path>,
        stdout: bool,
        tree: bool,
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        if tree {
            let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
            for e in std::fs::read_dir(ri.canonicalize().unwrap()).unwrap() {
                let e = e.unwrap();
                if e.file_name() != "shell.qml" {
                    std::os::unix::fs::symlink(e.path(), dir.path().join(e.file_name())).unwrap();
                }
            }
        }
        std::fs::write(dir.path().join("shell.qml"), qml).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        let xdg = dir.path().join("xdg");
        let program = bin.get_program().to_owned();
        let child = bin
            .arg("-p")
            .arg(dir.path())
            .env("WAYLAND_DEBUG", "1")
            .envs(display.map(|d| ("WAYLAND_DISPLAY", d)))
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdout(if stdout {
                w.try_clone().unwrap().into()
            } else {
                Stdio::null()
            })
            .stderr(w)
            .spawn()
            .unwrap();
        Shell {
            child,
            dir,
            log: Log::capture(r),
            display: display.map(Path::to_path_buf),
            program,
            runtime: None,
        }
    }

    fn text(&self) -> String {
        // wayland-backend logs `iface#id`, its discarded-message lines `iface@id`.
        self.log.text().replace('@', "#")
    }

    /// Waits for `f(log)`; panics with the log if rusty-impulse exits first.
    fn wait(&mut self, what: &str, f: impl Fn(&str) -> bool) -> String {
        if let Err(s) = self.log.wait(|s| f(&s.replace('@', "#"))) {
            panic!(
                "{} exited before {what}; its log tail:\n{}",
                self.program.display(),
                tail(&s.replace('@', "#"))
            );
        }
        self.text()
    }

    /// Runs `target.probe()` (it logs `PROBE`) and waits until the loop blocks with nothing due
    /// after it: whatever the events that reached rusty-impulse before the call started has run its
    /// course (the loop dispatches every ready source before it blocks).
    fn settle(&mut self, target: &str) -> String {
        self.probe(target);
        self.wait("the loop idle after the probe", |s| {
            blocked_after(s, |l| l.contains("PROBE"))
        })
    }

    /// Runs `target.probe()` and waits for its `PROBE`: the shell has run what it read before
    /// the call. For Quickshell, whose loop turns are not logged.
    fn probe(&mut self, target: &str) -> String {
        let probes = |s: &str| s.lines().filter(|l| l.contains("PROBE")).count();
        let n = probes(&self.text());
        self.ipc(target, "probe");
        self.wait("the probe", |s| probes(s) > n)
    }

    fn ipc(&self, target: &str, func: &str) {
        let st = Command::new(&self.program)
            .arg("-p")
            .arg(self.dir.path())
            .args(["ipc", "call", target, func])
            .envs(self.display.as_ref().map(|d| ("WAYLAND_DISPLAY", d)))
            .envs(self.runtime.as_ref().map(|d| ("XDG_RUNTIME_DIR", d)))
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "ipc call {target} {func}");
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The log lines that matter for these tests (no per-frame traffic).
fn tail(s: &str) -> String {
    let keep = [
        "get_layer_surface",
        "get_popup",
        "get_toplevel",
        "popup_done",
        "lock_surface",
        "session_lock",
        ".name(",
        "set_margin",
        "destroy(",
        "closed(",
        "global_remove(",
        ".enter(",
        "set_keyboard_interactivity(",
        "KB",
        "qml:",
        "LOCKSURFACE",
        "POPUPVISIBLE",
        "PANEL",
        "SCREENS",
        "PROBE",
        "FLIP",
        "session lock",
        // What the compositor ended the shell for.
        "wl_display#1.error(",
    ];
    let lines: Vec<&str> = s
        .lines()
        .filter(|l| keep.iter().any(|k| l.contains(k)))
        .collect();
    lines[lines.len().saturating_sub(80)..].join("\n")
}

/// `wl_output#N` of each output name, from the `wl_output.name` events.
fn output_ids(log: &str) -> Vec<(String, String)> {
    log.lines()
        .filter_map(|l| {
            let (obj, rest) = l.split_once(".name(\"")?;
            let obj = obj.rsplit(' ').next()?;
            obj.starts_with("wl_output#")
                .then(|| (rest.split('"').next().unwrap().to_string(), obj.to_string()))
        })
        .collect()
}

fn output_id(log: &str, name: &str) -> String {
    output_ids(log)
        .into_iter()
        .rev()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no wl_output named {name} in the log"))
        .1
}

/// The argument lists of every `-> iface#N.request(...)` the client sent.
fn requests<'a>(log: &'a str, req: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    log.lines().filter_map(move |l| {
        let (_, rest) = l.split_once(" -> ")?;
        let (_, call) = rest.split_once('.')?;
        call.strip_prefix(req)?.strip_prefix('(')
    })
}

/// xdg_popups parented to a layer surface (`zwlr_layer_surface_v1.get_popup`).
fn popups(log: &str) -> usize {
    log.lines()
        .filter(|l| l.contains("-> zwlr_layer_surface_v1#") && l.contains(".get_popup("))
        .count()
}

/// `zwlr_layer_surface_v1#N` of each layer surface created for `namespace`, with its output.
fn layer_surfaces(log: &str, namespace: &str) -> Vec<(String, String)> {
    let ns = format!("\"{namespace}\"");
    requests(log, "get_layer_surface")
        .filter(|a| a.contains(&ns))
        .map(|a| {
            let parts: Vec<&str> = a.split(", ").collect();
            (
                parts[0].trim_start_matches("new id ").to_string(),
                parts[2].to_string(),
            )
        })
        .collect()
}

/// The lines from layer surface `layer`'s `get_layer_surface` up to its wl_surface's first
/// `commit` (zwlr_layer_surface_v1: the state the surface is mapped with), once that commit was
/// sent.
fn initial_state<'a>(log: &'a str, layer: &str) -> Option<Vec<&'a str>> {
    let created = format!("get_layer_surface(new id {layer}, ");
    let mut lines = log
        .lines()
        .skip_while(|l| !(l.contains(" -> ") && l.contains(&created)));
    let first = lines.next()?;
    let surface = first.split(", ").nth(1)?;
    let commit = format!("-> {surface}.commit(");
    let mut state = vec![first];
    for l in lines {
        state.push(l);
        if l.contains(&commit) {
            return Some(state);
        }
    }
    None
}

/// ext-session-lock-v1 asks for a lock surface on every output, including one connected while
/// the session is locked; Quickshell's WlSessionLock instantiates `surface` for each new screen.
#[test]
fn monitor_connected_while_locked_gets_lock_surface() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let mut sh = Shell::start(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {
    id: root
    property bool locked: true
    WlSessionLock {
        locked: root.locked
        surface: Component {
            WlSessionLockSurface {
                color: "black"
                onScreenChanged: console.log("LOCKSURFACE " + (screen ? screen.name : "none"))
            }
        }
    }
    IpcHandler {
        target: "lock"
        function unlock(): void { root.locked = false }
    }
}
"#,
    );
    sh.wait("the session lock", |s| s.contains("session lock: locked"));
    let mut out = Output::create(&sock);
    let name = out.name.clone();
    let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sh.wait("a lock surface on the new output", |s| {
            output_ids(s).iter().any(|(n, id)| {
                *n == name
                    && requests(s, "get_lock_surface").any(|a| a.ends_with(&format!("{id})")))
            })
        })
    }));
    let s = sh.text();
    let ok = got.is_ok() && s.contains(&format!("LOCKSURFACE {name}"));
    // The disconnected monitor's surface goes with it.
    out.unplug();
    let destroyed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sh.wait("the unplugged output's lock surface destroyed", |s| {
            s.lines()
                .any(|l| l.contains("-> ext_session_lock_surface_v1#") && l.contains(".destroy("))
        })
    }));
    // Never leave the compositor locked for the tests that run after this one.
    sh.ipc("lock", "unlock");
    sh.wait("the unlock", |s| s.contains("session lock: released"));
    let s = sh.text();
    assert!(
        ok,
        "no lock surface for {name} after it was connected while locked:\n{}",
        tail(&s)
    );
    assert!(
        destroyed.is_ok(),
        "the unplugged output's lock surface was not destroyed"
    );
}

/// `session lock: released` and the removal of `ri-native.locked` say that stopping the shell
/// leaves the session unlocked. Before the compositor has read `unlock_and_destroy` it does
/// not: sway keeps a session locked whose lock client is gone, and shows nothing but lock
/// surfaces from then on.
#[test]
fn lock_is_released_once_the_compositor_has_unlocked() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let mut sh = Shell::start_on(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {
    id: root
    property bool locked: true
    WlSessionLock {
        locked: root.locked
        surface: Component {
            WlSessionLockSurface { color: "black" }
        }
    }
    IpcHandler {
        target: "lock"
        function unlock(): void { root.locked = false }
    }
    IpcHandler {
        target: "sync"
        function probe(): void { console.log("PROBE") }
    }
}
"#,
        Some(px.path()),
    );
    sh.wait("the session lock", |s| s.contains("session lock: locked"));
    let marker = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("ri-native.locked");
    let held = || std::fs::read_to_string(&marker).ok();
    let pid = Some(sh.child.id().to_string());
    assert_eq!(held(), pid, "no marker of the held lock");
    px.hold();
    sh.ipc("lock", "unlock");
    sh.wait("the unlock request", |s| s.contains(".unlock_and_destroy("));
    let unread = sh.settle("sync");
    let unread_marker = held();
    // Never leave the compositor locked for the tests that run after this one.
    px.release();
    let s = sh.wait("the unlock", |s| s.contains("session lock: released"));
    assert!(
        !unread.contains("session lock: released"),
        "released before the compositor had read the unlock:\n{}",
        tail(&unread)
    );
    assert_eq!(
        unread_marker,
        pid,
        "the marker went before the compositor had read the unlock:\n{}",
        tail(&unread)
    );
    assert_eq!(held(), None, "the marker outlived the lock:\n{}", tail(&s));
}

/// A shell that locks the session at once and unlocks it on `lock.unlock`.
const LOCKS: &str = r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {
    id: root
    property bool locked: true
    WlSessionLock {
        locked: root.locked
        surface: Component {
            WlSessionLockSurface { color: "black" }
        }
    }
    IpcHandler {
        target: "lock"
        function unlock(): void { root.locked = false }
        function unlockAndQuit(): void { root.locked = false; Qt.quit() }
    }
    IpcHandler {
        target: "sync"
        function probe(): void { console.log("PROBE") }
    }
}
"#;

fn lock_marker() -> Option<String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").unwrap();
    std::fs::read_to_string(Path::new(&dir).join("ri-native.locked")).ok()
}

/// [`LOCKS`] behind `px` with its `unlock_and_destroy` written and held back: the compositor
/// has not read it and cannot answer the sync after it.
fn unlocking(px: &testenv::HeldRequests) -> Shell {
    let mut sh = Shell::start_on(LOCKS, Some(px.path()));
    sh.wait("the session lock", |s| s.contains("session lock: locked"));
    px.hold();
    sh.ipc("lock", "unlock");
    sh.wait("the unlock request", |s| s.contains(".unlock_and_destroy("));
    sh.settle("sync");
    sh
}

/// Takes over the lock a shell that is gone left on the compositor and unlocks, for the tests
/// that run after this one.
fn unlock_compositor() {
    let mut sh = Shell::start(LOCKS);
    sh.wait("the session lock", |s| s.contains("session lock: locked"));
    sh.ipc("lock", "unlock");
    sh.wait("the unlock", |s| s.contains("session lock: released"));
}

/// The compositor shows a panel (the surface enters an output): its session is not locked.
/// sway shows nothing but lock surfaces while it is.
fn compositor_shows_a_panel() {
    let mut sh = Shell::start(
        r#"import QtQuick
import Quickshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true; right: true }
        implicitHeight: 20
        color: "red"
    }
}
"#,
    );
    sh.wait("the panel on an output", |s| {
        s.lines()
            .any(|l| l.contains(" wl_surface#") && l.contains(".enter("))
    });
}

/// How the shell is told to end while the compositor has not read its `unlock_and_destroy`.
enum End {
    /// SIGTERM after the unlock.
    Sigterm,
    /// `Qt.quit()` in the handler that unlocks.
    Quit,
}

/// ext-session-lock-v1, `unlock_and_destroy`: "a correct client that wishes to exit directly
/// after unlocking the session must use the wl_display.sync request to ensure the server
/// receives and processes the unlock_and_destroy request". The shell told to end before the
/// answer to that sync stays until it comes; then the compositor is unlocked, the marker is
/// gone and the shell ends the way it was told to.
fn ends_once_the_compositor_has_unlocked(end: End) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let mut sh = match end {
        End::Sigterm => unlocking(&px),
        End::Quit => {
            let mut sh = Shell::start_on(LOCKS, Some(px.path()));
            sh.wait("the session lock", |s| s.contains("session lock: locked"));
            px.hold();
            sh
        }
    };
    let pid = sh.child.id();
    match end {
        // SAFETY: a signal to the child this test started and has not reaped.
        End::Sigterm => assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) }, 0),
        End::Quit => sh.ipc("lock", "unlockAndQuit"),
    }
    // A shell that ends here closes its log, which fails the wait.
    let waits = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sh.wait("the exit waiting for the unlock", |s| {
            s.contains("the exit waits for the unlock")
        })
    }));
    if let (End::Sigterm, true) = (&end, waits.is_ok()) {
        // A second one does not cut the wait short.
        // SAFETY: as above.
        assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) }, 0);
    }
    let unanswered = waits
        .as_ref()
        .ok()
        .map(|s| (s.contains("session lock: released"), lock_marker()));
    px.release();
    let s = sh.log.until_closed();
    let status = sh.child.wait().unwrap();
    let left = lock_marker();
    let released = s.contains("session lock: released");
    if released {
        compositor_shows_a_panel();
    } else {
        // The unlock did not reach the compositor: the dead shell's lock is still on it.
        unlock_compositor();
    }
    assert!(
        waits.is_ok(),
        "the shell ended before the compositor had read the unlock:\n{}",
        tail(&s)
    );
    assert_eq!(
        unanswered,
        Some((false, Some(pid.to_string()))),
        "released and the marker before the compositor had read the unlock"
    );
    // The answer came after `release`, and the shell was there to get it.
    assert!(
        released,
        "the shell ended without the compositor's answer:\n{}",
        tail(&s)
    );
    assert_eq!(left, None, "the marker outlived the shell:\n{}", tail(&s));
    use std::os::unix::process::ExitStatusExt;
    match end {
        End::Sigterm => assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}"),
        End::Quit => assert_eq!(status.code(), Some(0), "{status:?}"),
    }
}

#[test]
fn sigterm_while_unlocking_waits_for_the_compositor() {
    ends_once_the_compositor_has_unlocked(End::Sigterm);
}

#[test]
fn quit_while_unlocking_waits_for_the_compositor() {
    ends_once_the_compositor_has_unlocked(End::Quit);
}

/// Quickshell 0.3.2 does not wait (docs/quickshell-differences.md, "A shell that ends right
/// after it unlocked"): told to quit by the handler that unlocks, it writes
/// `unlock_and_destroy` and ends while the compositor has not read it.
#[test]
fn quickshell_ends_before_the_compositor_has_read_its_unlock() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_sock) = sway() else { return };
    if !testenv::require("qs") {
        return;
    }
    let px = testenv::HeldRequests::start();
    let mut sh = Shell::qs_on(LOCKS, px.path());
    sh.wait("the session lock", |s| {
        s.lines()
            .any(|l| l.contains("ext_session_lock_v1#") && l.contains(".locked()"))
    });
    px.hold();
    sh.ipc("lock", "unlockAndQuit");
    // Nothing is handed to the compositor meanwhile: Quickshell ends without its answer.
    let s = sh.log.until_closed().replace('@', "#");
    let status = sh.child.wait().unwrap();
    px.release();
    // Whether sway read the unlock of a client that was gone or not.
    unlock_compositor();
    assert_eq!(
        requests(&s, "unlock_and_destroy").count(),
        1,
        "Quickshell did not unlock:\n{}",
        tail(&s)
    );
    assert_eq!(status.code(), Some(0), "{status:?}:\n{}", tail(&s));
}

/// Quickshell 0.3.2 does not wait after SIGTERM either: with `unlock_and_destroy` written and
/// not read by the compositor it ends at once.
#[test]
fn quickshell_ends_on_sigterm_before_the_compositor_has_read_its_unlock() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_sock) = sway() else { return };
    if !testenv::require("qs") {
        return;
    }
    let px = testenv::HeldRequests::start();
    let mut sh = Shell::qs_on(LOCKS, px.path());
    sh.wait("the session lock", |s| {
        s.lines()
            .any(|l| l.contains("ext_session_lock_v1#") && l.contains(".locked()"))
    });
    px.hold();
    sh.ipc("lock", "unlock");
    sh.wait("the unlock request", |s| s.contains(".unlock_and_destroy("));
    sh.probe("sync");
    let pid = sh.child.id();
    // SAFETY: a signal to the child this test started and has not reaped.
    assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) }, 0);
    // Nothing is handed to the compositor meanwhile: Quickshell ends without its answer.
    let s = sh.log.until_closed().replace('@', "#");
    let status = sh.child.wait().unwrap();
    px.release();
    // Whether sway read the unlock of a client that was gone or not.
    unlock_compositor();
    assert_eq!(
        requests(&s, "unlock_and_destroy").count(),
        1,
        "Quickshell did not unlock:\n{}",
        tail(&s)
    );
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        status.signal(),
        Some(libc::SIGTERM),
        "{status:?}:\n{}",
        tail(&s)
    );
}

/// The wait for the compositor's answer ends with the connection: a shell told to end is not
/// kept by a compositor that died.
#[test]
fn connection_lost_while_the_exit_waits_ends_the_shell() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let mut sh = unlocking(&px);
    // SAFETY: a signal to the child this test started and has not reaped.
    unsafe { libc::kill(sh.child.id() as libc::pid_t, libc::SIGTERM) };
    let waits = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sh.wait("the exit waiting for the unlock", |s| {
            s.contains("the exit waits for the unlock")
        })
    }));
    px.close();
    let s = sh.log.until_closed();
    let status = sh.child.wait().unwrap();
    let left = lock_marker();
    // sway itself lives, and the unlock never reached it.
    unlock_compositor();
    assert!(
        waits.is_ok(),
        "the shell ended before the compositor had read the unlock:\n{}",
        tail(&s)
    );
    assert!(!s.contains("session lock: released"), "{}", tail(&s));
    assert_eq!(left, None, "the marker outlived the shell:\n{}", tail(&s));
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
}

/// The connection ends between `unlock_and_destroy` and the compositor's answer (the
/// compositor died): the shell ends, and its marker does not stay behind naming a shell that
/// is gone.
#[test]
fn connection_lost_while_unlocking_leaves_no_marker() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let mut sh = unlocking(&px);
    px.close();
    let s = sh.log.until_closed();
    let status = sh.child.wait().unwrap();
    let left = lock_marker();
    // sway itself lives, and the unlock never reached it.
    unlock_compositor();
    assert_eq!(status.code(), Some(255), "{status:?}:\n{}", tail(&s));
    assert!(!s.contains("session lock: released"), "{}", tail(&s));
    assert_eq!(left, None, "the marker outlived the shell:\n{}", tail(&s));
}

/// A shell that ends while it holds the lock leaves its marker: the session is locked with a
/// dead lockscreen. The next shell to start takes the marker of a pid that is gone away (it
/// holds no lock), and leaves the marker of a process that runs.
#[test]
fn marker_of_a_shell_that_is_gone_goes_at_the_next_start() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_sock) = sway() else { return };
    // A shell another test starts meanwhile takes a dead shell's marker out of the shared
    // directory.
    let run = tempfile::tempdir().unwrap();
    let file = run.path().join("ri-native.locked");
    let marker = || std::fs::read_to_string(&file).ok();
    let mut sh = Shell::start_in(LOCKS, run.path());
    sh.wait("the session lock", |s| s.contains("session lock: locked"));
    let pid = sh.child.id();
    let held = marker();
    // SAFETY: a signal to the child this test started and has not reaped.
    assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) }, 0);
    sh.log.until_closed();
    sh.child.wait().unwrap();
    let after_exit = marker();

    const IDLE: &str = r#"import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    IpcHandler {
        target: "sync"
        function probe(): void { console.log("PROBE") }
    }
}
"#;
    let mut next = Shell::start_in(IDLE, run.path());
    next.wait("the shell started", |s| s.contains("renderer: "));
    next.probe("sync");
    let after_start = marker();
    drop(next);

    // This test's pid: a process that runs.
    let me = std::process::id().to_string();
    std::fs::write(&file, &me).unwrap();
    let mut other = Shell::start_in(IDLE, run.path());
    other.wait("the shell started", |s| s.contains("renderer: "));
    other.probe("sync");
    let of_live = marker();
    drop(other);

    // The compositor is still locked by the first shell.
    unlock_compositor();
    assert_eq!(held, Some(pid.to_string()));
    assert_eq!(
        after_exit,
        Some(pid.to_string()),
        "a shell that ended holding the lock took its marker away"
    );
    assert_eq!(
        after_start, None,
        "the next start kept a dead shell's marker"
    );
    assert_eq!(
        of_live,
        Some(me),
        "a start removed the marker of a process that runs"
    );
}

/// Destroying the `WlSessionLock` that holds the lock (the original's `panelFamily cycle` unloads the
/// family's `Lock {}`) must not unlock the session: Quickshell keeps it locked, and the next
/// `WlSessionLock` with `locked: true` takes the lock over with its own surfaces.
#[test]
fn destroyed_lock_holder_keeps_session_locked() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_sock) = sway() else { return };
    let mut sh = Shell::start(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {
    id: root
    property bool loaded: true
    property bool locked: true
    property int family: 1
    Loader {
        active: root.loaded
        sourceComponent: WlSessionLock {
            id: lock
            readonly property int family: root.family
            locked: root.locked
            surface: Component {
                WlSessionLockSurface {
                    color: "black"
                    onScreenChanged: console.log("LOCKSURFACE family" + lock.family)
                }
            }
        }
    }
    IpcHandler {
        target: "lock"
        function unload(): void { root.loaded = false }
        function reload(): void { root.family = 2; root.loaded = true }
        function unlock(): void { root.locked = false }
        function probe(): void { console.log("PROBE") }
    }
}
"#,
    );
    sh.wait("the session lock", |s| s.contains("session lock: locked"));
    sh.wait("the first family's lock surface", |s| {
        s.contains("LOCKSURFACE family1")
    });
    let unlocks = |s: &str| requests(s, "unlock_and_destroy").count();
    sh.ipc("lock", "unload");
    let s = sh.wait("the destroyed holder's lock surface destroyed", |s| {
        s.lines()
            .any(|l| l.contains("-> ext_session_lock_surface_v1#") && l.contains(".destroy("))
    });
    // A wrong unlock would follow in a later turn: the old surfaces' destruction marks the frame
    // after it.
    let s2 = sh.settle("lock");
    let kept = unlocks(&s) == 0 && unlocks(&s2) == 0 && !s2.contains("session lock: released");
    sh.ipc("lock", "reload");
    let adopted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sh.wait("the second family's lock surface", |s| {
            s.contains("LOCKSURFACE family2")
        })
    }));
    let one_lock = requests(&sh.text(), "lock").count();
    // Never leave the compositor locked for the tests that run after this one.
    sh.ipc("lock", "unlock");
    let released = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sh.wait("the unlock", |s| {
            unlocks(s) == 1 && s.contains("session lock: released")
        })
    }));
    let s = sh.text();
    assert!(
        kept,
        "destroying the locked WlSessionLock unlocked the session:\n{}",
        tail(&s)
    );
    assert!(
        adopted.is_ok(),
        "the next WlSessionLock did not take over the held lock:\n{}",
        tail(&s)
    );
    assert_eq!(one_lock, 1, "the held lock was not reused:\n{}", tail(&s));
    assert!(
        released.is_ok(),
        "locked: false did not unlock:\n{}",
        tail(&s)
    );
}

/// The bar is an ordinary PanelWindow: it maps on its own `screen`, with its
/// `WlrLayershell.layer` and margins (the old dedicated path put bar k on output k, always on
/// the top layer, without margins).
#[test]
fn bar_maps_through_panel_window_path() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let out = Output::create(&sock);
    let mut sh = Shell::start(&format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Wayland
ShellRoot {{
    PanelWindow {{
        screen: Quickshell.screens.find(s => s.name === "{}") ?? null
        WlrLayershell.namespace: "quickshell:bar"
        WlrLayershell.layer: WlrLayer.Overlay
        anchors {{ top: true; left: true; right: true }}
        margins {{ top: 7; left: 3 }}
        exclusiveZone: 40
        implicitHeight: 40
        color: "red"
    }}
}}
"#,
        out.name
    ));
    // Its initial commit closes the state the bar maps with: the log is read on another
    // thread, so the requests after `get_layer_surface` may not be in it yet when that is.
    let s = sh.wait("the bar's initial commit", |s| {
        layer_surfaces(s, "quickshell:bar")
            .first()
            .is_some_and(|(bar, _)| initial_state(s, bar).is_some())
    });
    let id = output_id(&s, &out.name);
    let bars = layer_surfaces(&s, "quickshell:bar");
    assert_eq!(bars.len(), 1, "{bars:?}");
    let (bar, on) = &bars[0];
    assert_eq!(
        on, &id,
        "bar mapped on {on}, its screen {} is {id}",
        out.name
    );
    let created = requests(&s, "get_layer_surface")
        .find(|a| a.contains("quickshell:bar"))
        .unwrap()
        .to_string();
    assert!(
        created.contains(", 3, \"quickshell:bar\""),
        "not on the overlay layer: {created}"
    );
    let margin = initial_state(&s, bar)
        .unwrap()
        .into_iter()
        .find(|l| l.contains(&format!("-> {bar}.set_margin(")))
        .unwrap_or_else(|| panic!("bar margins not in its initial commit:\n{}", tail(&s)));
    assert!(margin.contains("set_margin(7, 0, 0, 3)"), "{margin}");
}

/// Quickshell moves a shown window whose `screen` changes (hide, change the output, show):
/// notification popups follow the focused monitor.
#[test]
fn shown_panel_follows_screen_change() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let out = Output::create(&sock);
    let first = output_names(&sock)
        .into_iter()
        .find(|n| *n != out.name)
        .unwrap();
    let mut sh = Shell::start(&format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {{
    PanelWindow {{
        id: win
        property string want: "{first}"
        screen: Quickshell.screens.find(s => s.name === want) ?? null
        WlrLayershell.namespace: "quickshell:notificationPopup"
        anchors {{ top: true; right: true }}
        implicitWidth: 200
        implicitHeight: 100
        color: "blue"
    }}
    IpcHandler {{
        target: "win"
        function move(): void {{ win.want = "{second}" }}
    }}
}}
"#,
        second = out.name
    ));
    let s = sh.wait("the panel", |s| {
        !layer_surfaces(s, "quickshell:notificationPopup").is_empty()
    });
    let (old, on) = layer_surfaces(&s, "quickshell:notificationPopup")[0].clone();
    assert_eq!(on, output_id(&s, &first));
    sh.ipc("win", "move");
    let id = output_id(&s, &out.name);
    let s = sh.wait("the panel on the new screen", |s| {
        layer_surfaces(s, "quickshell:notificationPopup")
            .iter()
            .any(|(_, o)| *o == id)
    });
    assert!(
        s.contains(&format!("-> {old}.destroy(")),
        "the surface on the old screen was not destroyed:\n{}",
        tail(&s)
    );
}

/// A popup the compositor dismissed (`xdg_popup.done`, here because its parent's output went
/// away) turns invisible like QWindow::close(), instead of being mapped again right away. Its
/// parent's layer surface gets `closed`, so the parent closes too (Quickshell
/// LayerSurface::zwlr_layer_surface_v1_closed), after the popup was destroyed.
#[test]
fn dismissed_popup_stays_closed() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let mut out = Output::create(&sock);
    let mut sh = Shell::start(&format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {{
    PanelWindow {{
        id: panel
        screen: Quickshell.screens.find(s => s.name === "{}") ?? null
        WlrLayershell.namespace: "quickshell:popupParent"
        anchors {{ top: true; left: true }}
        implicitWidth: 300
        implicitHeight: 60
        color: "green"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
        onVisibleChanged: console.log("PANELVISIBLE " + visible)
        onClosed: console.log("PANELCLOSED")
        PopupWindow {{
            anchor.window: panel
            anchor.rect.y: 60
            visible: true
            implicitWidth: 120
            implicitHeight: 80
            color: "yellow"
            onVisibleChanged: console.log("POPUPVISIBLE " + visible)
        }}
    }}
    IpcHandler {{
        target: "sync"
        function probe(): void {{ console.log("PROBE") }}
    }}
}}
"#,
        out.name
    ));
    // The popup's own `configure`: the compositor has read `get_popup`. An unplug before that
    // closes the parent first, and sway ends the client with "xdg_popup has no parent".
    let s = sh.wait("the popup", |s| {
        popups(s) == 1
            && s.lines()
                .any(|l| l.contains("] xdg_popup#") && l.contains(".configure("))
    });
    let (layer, _) = layer_surfaces(&s, "quickshell:popupParent")[0].clone();
    // zwlr_layer_surface_v1: the keyboard interactivity is part of the initial commit.
    let surface = requests(&s, "get_layer_surface")
        .find(|a| a.contains(&format!("new id {layer},")))
        .and_then(|a| a.split(", ").nth(1))
        .unwrap()
        .to_string();
    let kb = s
        .find(&format!("-> {layer}.set_keyboard_interactivity(2)"))
        .expect("no OnDemand keyboard interactivity");
    let commit = s.find(&format!("-> {surface}.commit(")).unwrap();
    assert!(
        kb < commit,
        "keyboard interactivity set after the initial commit"
    );
    out.unplug();
    sh.wait("xdg_popup.done", |s| {
        s.lines()
            .any(|l| l.contains("xdg_popup#") && l.contains(".popup_done("))
    });
    sh.wait("the parent closed", |s| s.contains("PANELCLOSED"));
    // Whatever the unplug made the shell do (map either again on the remaining output) is done.
    let s = sh.settle("sync");
    assert_eq!(
        layer_surfaces(&s, "quickshell:popupParent").len(),
        1,
        "the closed parent was mapped again:\n{}",
        tail(&s)
    );
    assert!(
        s.contains("PANELVISIBLE false"),
        "the parent stayed visible:\n{}",
        tail(&s)
    );
    let popup_gone = s
        .lines()
        .position(|l| l.contains("-> xdg_popup#") && l.contains(".destroy("));
    let layer_gone = s
        .lines()
        .position(|l| l.contains(&format!("-> {layer}.destroy(")));
    assert!(
        matches!((popup_gone, layer_gone), (Some(p), Some(l)) if p < l),
        "the popup was not destroyed before its parent layer surface:\n{}",
        tail(&s)
    );
    assert_eq!(
        popups(&s),
        1,
        "the dismissed popup was mapped again:\n{}",
        tail(&s)
    );
    assert!(
        s.contains("POPUPVISIBLE false"),
        "visible stayed true:\n{}",
        tail(&s)
    );
}

/// `(popup, its xdg_surface, the parent it names)` of every `xdg_surface.get_popup`.
fn xdg_popups(log: &str) -> Vec<(String, String, String)> {
    log.lines()
        .filter_map(|l| {
            let (obj, args) = l.split_once(" -> ")?.1.split_once(".get_popup(new id ")?;
            let mut args = args.split(", ");
            Some((
                args.next()?.to_string(),
                obj.to_string(),
                args.next()?.to_string(),
            ))
        })
        .collect()
}

/// A PopupWindow anchored to a FloatingWindow is an xdg_popup of that toplevel's xdg_surface,
/// and one anchored to a popup an xdg_popup of that popup's (Quickshell makes the backing
/// window of any `anchor.window` the transient parent). Before the fix only a PanelWindow
/// could be the anchor: these popups were never created, and nothing said so. Hiding the
/// window destroys the popups first, the topmost before the one under it.
#[test]
fn popups_anchor_to_a_floating_window_and_to_a_popup() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if sway().is_none() {
        return;
    }
    let mut sh = Shell::start(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    FloatingWindow {
        id: win
        implicitWidth: 300
        implicitHeight: 200
        color: "green"
        PopupWindow {
            id: menu
            anchor.window: win
            anchor.rect.x: 20
            anchor.rect.y: 20
            visible: true
            implicitWidth: 120
            implicitHeight: 80
            color: "yellow"
            PopupWindow {
                anchor.window: menu
                anchor.rect.x: 10
                anchor.rect.y: 10
                visible: true
                implicitWidth: 60
                implicitHeight: 40
                color: "red"
            }
        }
    }
    IpcHandler {
        target: "sync"
        function probe(): void { console.log("PROBE") }
        function hide(): void { win.visible = false }
    }
}
"#,
    );
    sh.wait("the window configured", |s| {
        s.lines()
            .any(|l| l.contains("xdg_toplevel#") && l.contains(".configure("))
    });
    // The turn that saw the window configured asked for the popup anchored to it.
    let s = sh.settle("sync");
    assert!(
        !xdg_popups(&s).is_empty(),
        "no xdg_popup for the PopupWindow anchored to a FloatingWindow:\n{}",
        tail(&s)
    );
    let configured = |s: &str, popup: &str| s.contains(&format!("{popup}.configure("));
    let s = sh.wait("both popups configured", |s| {
        let p = xdg_popups(s);
        p.len() == 2 && p.iter().all(|(popup, ..)| configured(s, popup))
    });
    let window = s
        .lines()
        .find_map(|l| {
            l.split_once(" -> ")?
                .1
                .split_once(".get_toplevel(new id ")
                .map(|(surface, id)| (surface.to_string(), id.trim_end_matches(')').to_string()))
        })
        .expect("no xdg_toplevel");
    let p = xdg_popups(&s);
    assert_eq!(
        p[0].2,
        window.0,
        "the first popup's parent is not the window:\n{}",
        tail(&s)
    );
    assert_eq!(
        p[1].2,
        p[0].1,
        "the second popup's parent is not the first popup:\n{}",
        tail(&s)
    );
    assert_eq!(popups(&s), 0, "a popup went to a layer surface");
    sh.ipc("sync", "hide");
    let destroyed = |s: &str, id: &str| s.find(&format!(" -> {id}.destroy("));
    let s = sh.wait("the window destroyed", |s| {
        destroyed(s, &window.1).is_some()
    });
    let order = [&p[1].0, &p[0].0, &window.1].map(|id| destroyed(&s, id));
    assert!(
        order.iter().all(Option::is_some) && order.is_sorted(),
        "not destroyed topmost first ({order:?}):\n{}",
        tail(&s)
    );
}

/// A window for a screen that went away waits for it: the compositor closes its layer surface
/// and the shell must not create a new one without an output (which the compositor puts on the
/// focused monitor: a second bar, or an exclusive zone over a live one).
#[test]
fn panel_for_unplugged_screen_is_not_moved_to_another_output() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let mut out = Output::create(&sock);
    let mut sh = Shell::start(&format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {{
    PanelWindow {{
        id: win
        visible: false
        WlrLayershell.namespace: "quickshell:staleScreen"
        anchors {{ top: true; left: true; right: true }}
        exclusiveZone: 30
        implicitHeight: 30
        color: "red"
        onClosed: console.log("PANELCLOSED")
        // Kept after the screen is unplugged: nothing re-evaluates it.
        Component.onCompleted: {{
            win.screen = Quickshell.screens.find(s => s.name === "{}");
            win.visible = true;
        }}
    }}
    IpcHandler {{
        target: "sync"
        function probe(): void {{ console.log("PROBE") }}
    }}
}}
"#,
        out.name
    ));
    // Its first configure: the compositor has the layer surface, on the screen asked for.
    let s = sh.wait("the panel", |s| {
        layer_surfaces(s, "quickshell:staleScreen")
            .iter()
            .any(|(ls, _)| s.contains(&format!("{ls}.configure(")))
    });
    let surfaces = layer_surfaces(&s, "quickshell:staleScreen");
    assert_eq!(surfaces.len(), 1, "{surfaces:?}");
    assert_eq!(surfaces[0].1, output_id(&s, &out.name));
    out.unplug();
    sh.wait("the panel closing", |s| s.contains("PANELCLOSED"));
    // Whatever the closed surface made the shell do (recreate it) is done.
    let s = sh.settle("sync");
    let surfaces = layer_surfaces(&s, "quickshell:staleScreen");
    assert_eq!(
        surfaces.len(),
        1,
        "the panel was recreated without its screen:\n{}",
        tail(&s)
    );
}

const RACED: &str = "quickshell:racedScreen";
const LATER: &str = "quickshell:laterPanel";

/// A panel for the screen `screen`, shown by `panel.raise`, and one for no screen in particular,
/// shown by `panel.showLater`, which logs `Quickshell.screens` on every change. (Not
/// `panel.show`: `qs ipc call panel show` lists the target's functions and calls nothing.)
fn raced_panel(screen: &str) -> String {
    const NS: &str = RACED;
    format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {{
    PanelWindow {{
        id: win
        visible: false
        WlrLayershell.namespace: "{NS}"
        anchors {{ top: true; left: true; right: true }}
        exclusiveZone: 30
        implicitHeight: 30
        color: "red"
        onClosed: console.log("PANELCLOSED")
        onVisibleChanged: console.log("PANELVISIBLE " + visible)
        Component.onCompleted: win.screen = Quickshell.screens.find(s => s.name === "{screen}")
    }}
    PanelWindow {{
        id: later
        visible: false
        WlrLayershell.namespace: "{LATER}"
        anchors {{ bottom: true; left: true; right: true }}
        exclusiveZone: 20
        implicitHeight: 20
        color: "blue"
        property string screens: Quickshell.screens.map(s => s.name).join(" ")
        onScreensChanged: console.log("SCREENS " + screens)
    }}
    IpcHandler {{
        target: "sync"
        function probe(): void {{ console.log("PROBE") }}
    }}
    IpcHandler {{
        target: "panel"
        function raise(): void {{ win.visible = true }}
        function showLater(): void {{ later.visible = true }}
    }}
}}
"#
    )
}

/// A panel created for a screen the compositor has removed already, in the milliseconds before
/// the shell reads the removal: the compositor gets a `get_layer_surface` naming a dead
/// wl_output, picks an output itself and never sends `closed`. The shell closes the panel when
/// it reads the removal, as the compositor would have (Quickshell closes on `closed` only).
#[test]
fn panel_created_while_its_screen_is_unplugged_is_closed() {
    const NS: &str = RACED;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let free = workspace_rects(&sock);
    let mut out = Output::create(&sock);
    let mut sh = Shell::start_on(&raced_panel(&out.name), Some(px.path()));
    sh.wait("the screen", |s| {
        output_ids(s).iter().any(|(n, _)| *n == out.name)
    });
    let s = sh.settle("sync");
    let wl_output = output_id(&s, &out.name);
    // The compositor reads nothing the shell sends from here on: it removes the screen first.
    px.hold();
    sh.ipc("panel", "raise");
    let s = sh.wait("the panel", |s| !layer_surfaces(s, NS).is_empty());
    let (ls, on) = layer_surfaces(&s, NS)[0].clone();
    assert_eq!(on, wl_output);
    out.unplug();
    // Quickshell.screens without it: the shell's loop has read the removal (`global_remove` in
    // the log can be another connection of rusty-impulse reading it first).
    sh.wait("the screen's removal", |s| {
        s.lines()
            .rev()
            .find_map(|l| l.split_once("SCREENS "))
            .is_some_and(|(_, names)| names.split(' ').all(|n| n != out.name))
    });
    let s = sh.settle("sync");
    assert!(
        s.contains(&format!("-> {ls}.destroy(")) && s.contains("PANELCLOSED"),
        "the panel of the removed screen was not closed:\n{}",
        tail(&s)
    );
    assert!(
        s.contains("PANELVISIBLE false"),
        "the panel stayed visible:\n{}",
        tail(&s)
    );
    // The compositor now reads the creation and the destruction. It maps a panel shown after
    // them (requests are read in order; `wl_surface.enter` is the mapped surface on a screen),
    // so by then the closed panel is on no screen that remains: only the later panel's 20 px
    // at the bottom are taken from the workspaces.
    px.release();
    sh.ipc("panel", "showLater");
    let s = sh.wait("the later panel on a screen", |s| {
        requests(s, "get_layer_surface")
            .filter(|a| a.contains(LATER))
            .filter_map(|a| a.split(", ").nth(1))
            .any(|surface| s.contains(&format!("{surface}.enter(")))
    });
    let taken: Vec<[i64; 4]> = free
        .iter()
        .zip(workspace_rects(&sock))
        .map(|((_, a), (_, b))| [b[0] - a[0], b[1] - a[1], a[2] - b[2], a[3] - b[3]])
        .filter(|d| *d != [0; 4])
        .collect();
    assert_eq!(
        taken,
        [[0, 0, 0, 20]],
        "the workspaces lost more than the later panel's zone: the closed panel is on a screen"
    );
    assert_eq!(
        layer_surfaces(&s, NS).len(),
        1,
        "the closed panel was created again:\n{}",
        tail(&s)
    );
}

/// Quickshell in the same race, to hold the deliberate difference
/// (docs/quickshell-differences.md): it closes a panel on `closed` only, so the panel of the
/// removed screen stays on the screen the compositor picked.
#[test]
fn quickshell_keeps_a_panel_created_while_its_screen_is_unplugged() {
    const NS: &str = RACED;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    if !testenv::require("qs") {
        return;
    }
    let px = testenv::HeldRequests::start();
    let free = workspace_rects(&sock);
    let mut out = Output::create(&sock);
    let mut sh = Shell::qs_on(&raced_panel(&out.name), px.path());
    sh.wait("the screen", |s| {
        output_ids(s).iter().any(|(n, _)| *n == out.name)
    });
    let s = sh.probe("sync");
    let wl_output = output_id(&s, &out.name);
    px.hold();
    sh.ipc("panel", "raise");
    let s = sh.wait("the panel", |s| !layer_surfaces(s, NS).is_empty());
    let (ls, on) = layer_surfaces(&s, NS)[0].clone();
    assert_eq!(on, wl_output);
    out.unplug();
    sh.wait("the screen's removal", |s| {
        s.lines()
            .rev()
            .find_map(|l| l.split_once("SCREENS "))
            .is_some_and(|(_, names)| names.split(' ').all(|n| n != out.name))
    });
    let s = sh.probe("sync");
    assert!(
        !s.contains(&format!("-> {ls}.destroy(")) && !s.contains("PANELCLOSED"),
        "Quickshell closed the panel of the removed screen:\n{}",
        tail(&s)
    );
    // The compositor now reads the creation, with the dead wl_output: both panels are mapped
    // on the screen that remains, which loses the 30 px of the first at the top as well.
    px.release();
    sh.ipc("panel", "showLater");
    let s = sh.wait("both panels on a screen", |s| {
        [NS, LATER].iter().all(|ns| {
            requests(s, "get_layer_surface")
                .filter(|a| a.contains(ns))
                .filter_map(|a| a.split(", ").nth(1))
                .any(|surface| s.contains(&format!("{surface}.enter(")))
        })
    });
    let taken: Vec<[i64; 4]> = free
        .iter()
        .zip(workspace_rects(&sock))
        .map(|((_, a), (_, b))| [b[0] - a[0], b[1] - a[1], a[2] - b[2], a[3] - b[3]])
        .filter(|d| *d != [0; 4])
        .collect();
    assert_eq!(
        taken,
        [[0, 30, 0, 50]],
        "the panel of the removed screen is not on the screen that remains"
    );
    assert!(
        !s.contains("PANELCLOSED") && !s.contains("PANELVISIBLE false"),
        "Quickshell closed the panel of the removed screen:\n{}",
        tail(&s)
    );
}

/// "Keep awake" (`qml/services/Idle.qml`) holds its idle inhibitor on a window for no screen in
/// particular, which the compositor closes with the monitor it put it on. Runs the service on a
/// sway of its own, turns "Keep awake" on and unplugs that monitor: `only`, sway's one monitor,
/// and another is plugged in after; otherwise a second monitor, and the first remains. Passes
/// once the window was opened again and the inhibitor made on it, with the toggle still on.
/// With the original's file the window stays closed, the toggle stays on and nothing is inhibited
/// (docs/original-differences.md).
fn keep_awake_after_unplug(only: bool) {
    const NS: &str = "rustshell";
    let sway = testenv::Sway::start();
    let sock = sway.ipc_socket();
    let unplugged = if only {
        "HEADLESS-1".to_string()
    } else {
        let before = output_names(&sock);
        swaymsg(&sock, &["create_output"]);
        let name = output_names(&sock)
            .into_iter()
            .find(|n| !before.contains(n))
            .expect("create_output added no output");
        // The compositor puts a window for no screen on the focused one.
        swaymsg(&sock, &["focus", "output", &name]);
        name
    };
    let mut sh = Shell::start_in_tree(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import qs.services
ShellRoot {
    property bool awake: Idle.inhibit
    IpcHandler {
        target: "sync"
        function probe(): void { console.log("PROBE") }
    }
    IpcHandler {
        target: "awake"
        function enable(): void { Idle.toggleInhibit(true) }
        function state(): void { console.log("KEEPAWAKE " + Idle.inhibit) }
    }
}
"#,
        sway.display(),
    );
    // `wl_surface#N` of layer surface `ls`.
    let surface_of = |s: &str, ls: &str| {
        requests(s, "get_layer_surface")
            .find(|a| a.contains(&format!("new id {ls}, ")))
            .and_then(|a| a.split(", ").nth(1))
            .unwrap()
            .to_string()
    };
    let inhibited = |s: &str, surface: &str| {
        requests(s, "create_inhibitor").any(|a| a.ends_with(&format!(", {surface})")))
    };
    // The last window asked for, once it is on a screen: its layer surface and wl_surface.
    let shown = |s: &str, nth: usize| {
        let (ls, on) = layer_surfaces(s, NS).into_iter().nth(nth)?;
        assert_eq!(on, "nil", "the window names a screen");
        let surface = surface_of(s, &ls);
        (s.contains(&format!(" {ls}.configure(")) && s.contains(&format!(" {surface}.enter(")))
            .then_some((ls, surface))
    };
    let s = sh.wait("the window on a screen", |s| shown(s, 0).is_some());
    let (first, surface) = shown(&s, 0).unwrap();
    assert!(
        s.contains(&format!(" {surface}.enter({})", output_id(&s, &unplugged))),
        "the window is not on the screen to be unplugged:\n{}",
        tail(&s)
    );
    sh.ipc("awake", "enable");
    sh.wait("the inhibitor", |s| inhibited(s, &surface));
    swaymsg(&sock, &["output", &unplugged, "unplug"]);
    sh.wait("the window closing", |s| {
        s.contains(&format!(" {first}.closed("))
    });
    if only {
        swaymsg(&sock, &["create_output"]);
        sh.wait("the new screen", |s| output_ids(s).len() == 2);
    }
    let s = sh.settle("sync");
    assert!(
        layer_surfaces(&s, NS).len() > 1,
        "the closed window was not opened again:\n{}",
        tail(&s)
    );
    // With no screen left sway closes a window as it is asked for: the last one stays.
    let last = layer_surfaces(&s, NS).len() - 1;
    let s = sh.wait("the window on a screen again", |s| shown(s, last).is_some());
    let (again, surface) = shown(&s, last).unwrap();
    sh.wait("the inhibitor on the new window", |s| {
        inhibited(s, &surface)
    });
    sh.ipc("awake", "state");
    let s = sh.wait("the toggle's state", |s| s.contains("KEEPAWAKE "));
    assert!(s.contains("KEEPAWAKE true"), "{}", tail(&s));
    assert!(
        !s.contains(&format!(" {again}.closed(")),
        "the new window was closed:\n{}",
        tail(&s)
    );
}

#[test]
fn keep_awake_holds_after_the_only_monitor_is_unplugged_and_another_plugged_in() {
    keep_awake_after_unplug(true);
}

#[test]
fn keep_awake_holds_after_its_windows_monitor_is_unplugged() {
    keep_awake_after_unplug(false);
}

/// Quickshell's WlrLayershell: `focusable` is `keyboardFocus != None`, and setting it selects
/// OnDemand or None, so the last write of either decides what the layer surface asks for.
/// Before the fix `focusable` kept the `true` an earlier Exclusive left in it, and a panel
/// switched to None asked for OnDemand.
#[test]
fn keyboard_focus_none_after_exclusive_asks_for_no_keyboard() {
    const NS: &str = "quickshell:kbMode";
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if sway().is_none() {
        return;
    }
    let mut sh = Shell::start(&format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {{
    PanelWindow {{
        id: win
        property bool grab: true
        WlrLayershell.namespace: "{NS}"
        WlrLayershell.keyboardFocus: grab ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None
        anchors {{ top: true; left: true }}
        implicitWidth: 100
        implicitHeight: 30
        color: "red"
    }}
    PanelWindow {{
        id: byFocusable
        WlrLayershell.namespace: "{NS}:focusable"
        focusable: true
        anchors {{ top: true; right: true }}
        implicitWidth: 100
        implicitHeight: 30
        color: "green"
    }}
    PanelWindow {{
        id: byMode
        WlrLayershell.namespace: "{NS}:mode"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
        anchors {{ bottom: true; right: true }}
        implicitWidth: 100
        implicitHeight: 30
        color: "blue"
    }}
    IpcHandler {{
        target: "sync"
        function probe(): void {{ console.log("PROBE") }}
    }}
    IpcHandler {{
        target: "kb"
        function constants(): void {{
            console.log("KBCONST " + byFocusable.WlrLayershell.keyboardFocus + " " + byMode.focusable)
        }}
        function state(): void {{ console.log("KB focusable " + win.focusable) }}
        function release(): void {{ win.grab = false; state() }}
        function setFocusable(): void {{ win.focusable = true; state() }}
        function clearFocusable(): void {{ win.focusable = false; state() }}
    }}
}}
"#
    ));
    let all = [
        NS.to_string(),
        format!("{NS}:focusable"),
        format!("{NS}:mode"),
    ];
    sh.wait("the panels", |s| {
        all.iter().all(|ns| !layer_surfaces(s, ns).is_empty())
    });
    let s = sh.settle("sync");
    let of = |ns: &str| layer_surfaces(&s, ns)[0].0.clone();
    let ls = of(NS);
    let set = format!("-> {ls}.set_keyboard_interactivity(");
    // The modes the layer surface asked for, in order, and what `focusable` read after each.
    let asked = |s: &str| -> (Vec<String>, Vec<String>) {
        let modes = s
            .lines()
            .filter_map(|l| l.split_once(&set))
            .map(|(_, m)| m.trim_end_matches(')').to_string());
        let focusable = s
            .lines()
            .filter_map(|l| l.split_once("KB focusable "))
            .map(|(_, f)| f.to_string());
        (modes.collect(), focusable.collect())
    };
    sh.ipc("kb", "state");
    let s = sh.settle("sync");
    assert_eq!(
        asked(&s),
        (vec!["1".into()], vec!["true".into()]),
        "{}",
        tail(&s)
    );
    let mut modes = vec!["1"];
    let mut focusable = vec!["true"];
    for (call, mode, f) in [
        ("release", "0", "false"),
        ("setFocusable", "2", "true"),
        ("clearFocusable", "0", "false"),
    ] {
        sh.ipc("kb", call);
        let s = sh.settle("sync");
        modes.push(mode);
        focusable.push(f);
        let want = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            asked(&s),
            (want(&modes), want(&focusable)),
            "after {call}:\n{}",
            tail(&s)
        );
    }
    // Set once in the QML: `focusable: true` is OnDemand (2), and OnDemand is focusable.
    sh.ipc("kb", "constants");
    let s = sh.settle("sync");
    for ns in &all[1..] {
        let set = format!("-> {}.set_keyboard_interactivity(2)", of(ns));
        assert!(s.contains(&set), "no {set}:\n{}", tail(&s));
    }
    assert!(s.contains("KBCONST 2 true"), "{}", tail(&s));
}

/// `wl_surface.frame` is only requested for a frame that is committed: a redraw that leaves
/// the pixels as they are (nothing swapped) must not leave a callback behind, or a static window
/// piles them up and gets them all at once on its next real commit. Nothing on this window
/// animates, so even its swapped frames need none.
#[test]
fn skipped_frames_request_no_frame_callback() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_sock) = sway() else { return };
    let mut sh = Shell::start(
        r##"import QtQuick
import Quickshell
import Quickshell.Wayland
ShellRoot {
    PanelWindow {
        WlrLayershell.namespace: "quickshell:static"
        anchors { top: true; left: true }
        implicitWidth: 200
        implicitHeight: 40
        color: "blue"
        // A changed property that draws the same pixels: a redraw with nothing to swap.
        Rectangle {
            id: same
            property bool flip: false
            width: 10
            height: 10
            color: flip ? "#ff0000" : "red"
        }
        Timer {
            interval: 20
            repeat: true
            running: true
            onTriggered: {
                same.flip = !same.flip;
                console.log("FLIP");
            }
        }
    }
}
"##,
    );
    let s = sh.wait("the panel", |s| {
        !layer_surfaces(s, "quickshell:static").is_empty()
    });
    let surface = requests(&s, "get_layer_surface")
        .find(|a| a.contains("quickshell:static"))
        .and_then(|a| a.split(", ").nth(1).map(str::to_string))
        .unwrap();
    // A hundred redraws that swap nothing (the Timer's flips, 2 s of them).
    let s = sh.wait("100 flips", |s| {
        s.lines().filter(|l| l.contains("FLIP")).count() >= 100
    });
    let (frame, attach) = (
        format!("-> {surface}.frame("),
        format!("-> {surface}.attach("),
    );
    let swaps = s.lines().filter(|l| l.contains(&attach)).count();
    let frames = s.lines().filter(|l| l.contains(&frame)).count();
    assert!(swaps > 0, "{surface} never drew:\n{}", tail(&s));
    assert_eq!(
        frames, 0,
        "{surface} requested {frames} frame callbacks for {swaps} static frames"
    );
}

/// The last `configure` size the compositor sent to layer surface `id`.
fn configured(log: &str, id: &str) -> Option<(u32, u32)> {
    let pat = format!("{id}.configure(");
    log.lines()
        .filter(|l| !l.contains(" -> "))
        .filter_map(|l| l.split_once(&pat))
        .map(|(_, a)| {
            let v: Vec<u32> = a
                .trim_end_matches(')')
                .split(", ")
                .map(|x| x.parse().unwrap())
                .collect();
            (v[1], v[2])
        })
        .next_back()
}

/// The last argument list of `id.req(...)` sent by the client.
fn last_request(log: &str, id: &str, req: &str) -> Option<String> {
    let pat = format!("-> {id}.{req}(");
    log.lines()
        .filter_map(|l| l.split_once(&pat))
        .map(|(_, a)| a.trim_end_matches(')').to_string())
        .next_back()
}

/// A panel follows live layout changes (edge, thickness, length, a corner-anchored panel's
/// exclusive edge, a second panel) on its one layer surface, and the compositor applies the
/// exclusive zones: a non-reserving probe panel along the left edge is configured to what is
/// left of the output's height.
#[test]
fn panel_layout_changes_apply_live() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let out = Output::create(&sock);
    let mut sh = Shell::start(&format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {{
    PanelWindow {{
        id: bar
        screen: Quickshell.screens.find(s => s.name === "{name}") ?? null
        WlrLayershell.namespace: "test:bar"
        anchors {{ top: true; left: true; right: true }}
        implicitHeight: 40
        exclusiveZone: 40
        color: "red"
    }}
    PanelWindow {{
        id: second
        visible: false
        screen: Quickshell.screens.find(s => s.name === "{name}") ?? null
        WlrLayershell.namespace: "test:second"
        anchors {{ top: true; left: true; right: true }}
        implicitHeight: 30
        exclusiveZone: 30
        color: "green"
    }}
    PanelWindow {{
        screen: Quickshell.screens.find(s => s.name === "{name}") ?? null
        WlrLayershell.namespace: "test:probe"
        anchors {{ top: true; bottom: true; left: true }}
        implicitWidth: 10
        exclusiveZone: 0
        color: "blue"
    }}
    IpcHandler {{
        target: "layout"
        function thick(): void {{ bar.implicitHeight = 60; bar.exclusiveZone = 60 }}
        function bottom(): void {{ bar.anchors.top = false; bar.anchors.bottom = true }}
        function short(): void {{
            bar.WlrLayershell.exclusiveEdge = Edges.Bottom
            bar.anchors.right = false
            bar.implicitWidth = 400
        }}
        function noedge(): void {{ bar.WlrLayershell.exclusiveEdge = Edges.None }}
        function add(): void {{ second.visible = true }}
    }}
}}
"#,
        name = out.name
    ));
    let s = sh.wait("the panels", |s| {
        layer_surfaces(s, "test:probe")
            .first()
            .is_some_and(|(id, _)| configured(s, id) == Some((10, 680)))
    });
    let (bar, _) = layer_surfaces(&s, "test:bar")[0].clone();
    let (probe, _) = layer_surfaces(&s, "test:probe")[0].clone();
    assert_eq!(configured(&s, &bar), Some((1280, 40)));

    // Thickness: the bar is configured taller and reserves the new zone.
    sh.ipc("layout", "thick");
    let s = sh.wait("the thicker bar's zone", |s| {
        configured(s, &probe) == Some((10, 660))
    });
    assert_eq!(configured(&s, &bar), Some((1280, 60)));
    assert_eq!(
        last_request(&s, &bar, "set_exclusive_zone").as_deref(),
        Some("60")
    );

    // Edge: top -> bottom, same size, the zone moves with it.
    sh.ipc("layout", "bottom");
    let s = sh.wait("the bar on the bottom edge", |s| {
        last_request(s, &bar, "set_anchor").as_deref() == Some("14")
    });
    assert_eq!(configured(&s, &probe), Some((10, 660)));

    // Quickshell never sends the exclusive edge; neither do we while none is named.
    assert_eq!(last_request(&s, &bar, "set_exclusive_edge"), None);
    assert_eq!(last_request(&s, &probe, "set_exclusive_edge"), None);

    // Length: a 400 px panel in the bottom-left corner keeps its zone through its exclusive edge.
    sh.ipc("layout", "short");
    let s = sh.wait("the short bar", |s| configured(s, &bar) == Some((400, 60)));
    assert_eq!(last_request(&s, &bar, "set_anchor").as_deref(), Some("6"));
    assert_eq!(
        last_request(&s, &bar, "set_size").as_deref(),
        Some("400, 60")
    );
    assert_eq!(
        last_request(&s, &bar, "set_exclusive_edge").as_deref(),
        Some("2")
    );
    assert_eq!(configured(&s, &probe), Some((10, 660)));

    // Without an exclusive edge a corner-anchored zone is ambiguous: nothing is reserved.
    sh.ipc("layout", "noedge");
    let s = sh.wait("the corner zone dropped", |s| {
        configured(s, &probe) == Some((10, 720))
    });
    assert_eq!(
        last_request(&s, &bar, "set_exclusive_edge").as_deref(),
        Some("0")
    );

    // A second panel maps without restarting and reserves its own zone.
    sh.ipc("layout", "add");
    let s = sh.wait("the second panel's zone", |s| {
        configured(s, &probe) == Some((10, 690))
    });
    assert_eq!(layer_surfaces(&s, "test:second").len(), 1);
    assert_eq!(
        layer_surfaces(&s, "test:bar").len(),
        1,
        "the bar's layer surface was recreated:\n{}",
        tail(&s)
    );
}

/// The `layer` argument of the `get_layer_surface` that created the surface of `namespace`.
fn layer_of(log: &str, namespace: &str) -> Option<String> {
    let ns = format!("\"{namespace}\"");
    requests(log, "get_layer_surface")
        .find(|a| a.contains(&ns))
        .and_then(|a| a.split(", ").nth(3).map(str::to_string))
}

/// Quickshell's `exclusionMode` starts as `ExclusionMode.Auto`: a panel that sets neither it
/// nor `exclusiveZone` reserves its own size on the edge it is anchored to (`qs` 0.3.2 sends
/// `set_exclusive_zone(30)` for this bar). A probe along the left edge gets what is left.
#[test]
fn panel_without_exclusion_mode_reserves_its_size() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let out = Output::create(&sock);
    let mut sh = Shell::start(&format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Wayland
ShellRoot {{
    PanelWindow {{
        screen: Quickshell.screens.find(s => s.name === "{name}") ?? null
        WlrLayershell.namespace: "test:auto"
        anchors {{ top: true; left: true; right: true }}
        implicitHeight: 30
        color: "red"
    }}
    PanelWindow {{
        screen: Quickshell.screens.find(s => s.name === "{name}") ?? null
        WlrLayershell.namespace: "test:probe"
        anchors {{ top: true; bottom: true; left: true }}
        implicitWidth: 10
        exclusiveZone: 0
        color: "blue"
    }}
}}
"#,
        name = out.name
    ));
    let s = sh.wait("the bar's zone", |s| {
        layer_surfaces(s, "test:probe")
            .first()
            .is_some_and(|(id, _)| configured(s, id) == Some((10, 690)))
    });
    let (bar, _) = layer_surfaces(&s, "test:auto")[0].clone();
    assert_eq!(configured(&s, &bar), Some((1280, 30)));
    assert_eq!(
        last_request(&s, &bar, "set_exclusive_zone").as_deref(),
        Some("30")
    );
    // `qs` names no exclusive edge for it: the compositor deduces the edge from the anchors.
    assert_eq!(last_request(&s, &bar, "set_exclusive_edge"), None);
}

/// Quickshell's ProxyWindowBase: a window starts 100x100, and `width` and `height` set its
/// implicit size; `aboveWindows: false` puts a panel on the Bottom layer (1) instead of Top
/// (2). `qs` 0.3.2 sends these sizes and layers for the same windows.
#[test]
fn panel_size_and_layer_follow_width_height_and_above_windows() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let out = Output::create(&sock);
    let mut sh = Shell::start(&format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Wayland
ShellRoot {{
    PanelWindow {{
        screen: Quickshell.screens.find(s => s.name === "{name}") ?? null
        WlrLayershell.namespace: "test:default"
        exclusionMode: ExclusionMode.Ignore
    }}
    PanelWindow {{
        screen: Quickshell.screens.find(s => s.name === "{name}") ?? null
        WlrLayershell.namespace: "test:sized"
        width: 77
        height: 33
        aboveWindows: false
        exclusionMode: ExclusionMode.Ignore
        anchors.right: true
    }}
}}
"#,
        name = out.name
    ));
    let size = |s: &str, ns: &str| {
        layer_surfaces(s, ns)
            .first()
            .and_then(|(id, _)| configured(s, id))
    };
    let s = sh.wait("both panels", |s| {
        size(s, "test:default").is_some() && size(s, "test:sized").is_some()
    });
    assert_eq!(size(&s, "test:default"), Some((100, 100)));
    assert_eq!(size(&s, "test:sized"), Some((77, 33)));
    let (sized, _) = layer_surfaces(&s, "test:sized")[0].clone();
    assert_eq!(
        last_request(&s, &sized, "set_size").as_deref(),
        Some("77, 33")
    );
    assert_eq!(layer_of(&s, "test:default").as_deref(), Some("2"));
    assert_eq!(layer_of(&s, "test:sized").as_deref(), Some("1"));
}

const POPUP_RACE: &str = "quickshell:popupRace";

/// A panel on the screen `screen` with a popup shown by `popup.raise` (and a popup of that
/// popup, by `popup.raiseInner`), and a panel for no screen in particular, shown by
/// `popup.showLater`.
fn panel_with_popup(screen: &str) -> String {
    format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {{
    PanelWindow {{
        id: panel
        screen: Quickshell.screens.find(s => s.name === "{screen}") ?? null
        WlrLayershell.namespace: "{POPUP_RACE}"
        anchors {{ top: true; left: true }}
        implicitWidth: 300
        implicitHeight: 60
        color: "green"
        onClosed: console.log("PANELCLOSED")
        PopupWindow {{
            id: popup
            anchor.window: panel
            anchor.rect.y: 60
            visible: false
            implicitWidth: 120
            implicitHeight: 80
            color: "yellow"
            onVisibleChanged: console.log("POPUPVISIBLE " + visible)
            PopupWindow {{
                id: inner
                anchor.window: popup
                anchor.rect.y: 80
                visible: false
                implicitWidth: 60
                implicitHeight: 40
                color: "red"
            }}
        }}
    }}
    PanelWindow {{
        id: later
        visible: false
        WlrLayershell.namespace: "{LATER}"
        anchors {{ bottom: true; left: true }}
        implicitWidth: 100
        implicitHeight: 20
    }}
    IpcHandler {{
        target: "popup"
        function raise(): void {{ popup.visible = true }}
        function raiseInner(): void {{ inner.visible = true }}
        function showLater(): void {{ later.visible = true }}
        function probe(): void {{ console.log("PROBE") }}
    }}
}}
"#
    )
}

/// The `LATER` panel is on a screen.
fn later_panel_shown(log: &str) -> bool {
    requests(log, "get_layer_surface")
        .filter(|a| a.contains(LATER))
        .filter_map(|a| a.split(", ").nth(1))
        .any(|w| log.contains(&format!("{w}.enter(")))
}

/// The wl_surface of the first xdg_popup in `log`.
fn popup_surface(log: &str) -> String {
    let (_, xdg, _) = xdg_popups(log)[0].clone();
    requests(log, "get_xdg_surface")
        .find(|a| a.contains(&format!("new id {xdg},")))
        .and_then(|a| a.split(", ").nth(1))
        .map(|a| a.trim_end_matches(')').to_string())
        .expect("the popup's get_xdg_surface")
}

/// Shows the popup of `sh`'s panel and unplugs the panel's screen `out` before the compositor
/// has read the popup's requests; returns once the shell has read the panel's `closed`, the
/// requests still held by `px`.
fn show_popup_and_unplug(sh: &mut Shell, px: &testenv::HeldRequests, out: &mut Output) {
    sh.wait("the panel on its screen", |s| {
        requests(s, "get_layer_surface")
            .find(|a| a.contains(POPUP_RACE))
            .and_then(|a| a.split(", ").nth(1))
            .is_some_and(|w| s.contains(&format!("{w}.enter(")))
    });
    sh.probe("popup");
    px.hold();
    sh.ipc("popup", "raise");
    let s = sh.wait("get_popup", |s| popups(s) == 1);
    sh.probe("popup");
    let (layer, _) = layer_surfaces(&s, POPUP_RACE)[0].clone();
    out.unplug();
    sh.wait("the panel's closed", |s| {
        s.contains(&format!(" {layer}.closed("))
    });
}

/// The compositor ends a panel's layer surface when its screen goes, and a layer surface that
/// has ended adopts no popup: sway reads `zwlr_layer_surface_v1.get_popup` on it as nothing
/// and answers the first commit of the popup, which has no parent, with the protocol error
/// "xdg_popup has no parent". The shell had written that commit right after `get_popup`, so a
/// screen unplugged before the compositor read the two ended the shell with status 1. It
/// commits once the compositor has read `get_popup` (wl_display.sync); the panel's `closed`
/// comes before that answer and the popup goes uncommitted.
#[test]
fn popup_shown_while_its_panels_screen_is_unplugged_does_not_end_the_shell() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let mut out = Output::create(&sock);
    let mut sh = Shell::start_on(&panel_with_popup(&out.name), Some(px.path()));
    show_popup_and_unplug(&mut sh, &px, &mut out);
    sh.settle("popup");
    // The compositor now reads `get_popup` on the layer surface it ended.
    px.release();
    sh.ipc("popup", "showLater");
    let s = sh.wait("a panel shown after the race", later_panel_shown);
    let surface = popup_surface(&s);
    assert!(
        !s.contains(&format!("-> {surface}.commit(")),
        "the popup of the closed panel was committed:\n{}",
        tail(&s)
    );
    assert!(
        !s.contains("wl_display#1.error("),
        "a protocol error:\n{}",
        tail(&s)
    );
    assert!(
        s.contains("PANELCLOSED") && s.contains("POPUPVISIBLE false"),
        "the panel and its popup were not closed:\n{}",
        tail(&s)
    );
}

/// Quickshell 0.3.2 in the same race ends: QtWayland commits the popup right after
/// `get_popup`, sway answers with the protocol error and closes the connection, and
/// `QWaylandDisplay::checkWaylandError` calls `_exit(-1)`. Qt names one of two reasons.
#[test]
fn quickshell_ends_when_a_popup_is_shown_while_its_panels_screen_is_unplugged() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    if !testenv::require("qs") {
        return;
    }
    let px = testenv::HeldRequests::start();
    let mut out = Output::create(&sock);
    let mut sh = Shell::qs_on(&panel_with_popup(&out.name), px.path());
    show_popup_and_unplug(&mut sh, &px, &mut out);
    px.release();
    let s = sh.log.until_closed().replace('@', "#");
    let status = testenv::wait_exit(&mut sh.child);
    assert_eq!(
        status.code(),
        Some(255),
        "Quickshell did not end with _exit(-1):\n{}",
        tail(&s)
    );
    assert!(
        px.events().contains("xdg_popup has no parent"),
        "sway did not send the protocol error:\n{}",
        tail(&s)
    );
    // Two QtWayland threads read the socket, and Qt names the reason by libwayland's error.
    // Usually that is EPROTO, set by the `wl_display.error` event. Under load, 15 runs
    // in 600, one thread reads the end of the stream before the other has run that event:
    // the error is EPIPE and stays (`display_protocol_error` leaves an error already set
    // alone; `dispatch_queue` runs no event once one is set, so in 5 of the 15 the event's
    // line is not in the log either).
    let error_events: Vec<&str> = s
        .lines()
        .filter(|l| l.contains("wl_display#1.error("))
        .collect();
    assert!(
        error_events
            .iter()
            .all(|l| l.contains("xdg_popup has no parent")),
        "another protocol error: {error_events:?}"
    );
    let by_error = s.contains("The Wayland connection experienced a fatal error: Protocol error");
    let by_end_of_stream =
        s.contains("The Wayland connection broke. Did the Wayland compositor die?");
    assert!(
        matches!(
            (by_error, by_end_of_stream, error_events.len()),
            (true, false, 1) | (false, true, 0 | 1)
        ),
        "neither of Quickshell's two ends (by the error {by_error}, by the end of the stream \
         {by_end_of_stream}, error events {}):\n{s}",
        error_events.len()
    );
}

/// The other parents a compositor can end while the shell's request on them is on its way
/// have no such race under sway: the three tests below hold the request, end the parent and
/// find the shell alive and no protocol error.
fn assert_alive_after(sh: &mut Shell, target: &str, parent: &str) {
    sh.ipc(target, "showLater");
    let s = sh.wait("a panel shown after the parent ended", later_panel_shown);
    assert!(
        !s.contains("wl_display#1.error("),
        "a protocol error after {parent} ended:\n{}",
        tail(&s)
    );
}

/// A popup of a window: the compositor does not end an xdg_toplevel, it asks the client to
/// (`close`), so the popup's parent is there for as long as the shell has not destroyed it.
#[test]
fn popup_shown_while_its_window_is_asked_to_close_does_not_end_the_shell() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let qml = format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {{
    FloatingWindow {{
        id: win
        title: "rustyPopupParent"
        implicitWidth: 300
        implicitHeight: 200
        color: "green"
        PopupWindow {{
            id: popup
            anchor.window: win
            anchor.rect.y: 20
            visible: false
            implicitWidth: 120
            implicitHeight: 80
            color: "yellow"
        }}
    }}
    PanelWindow {{
        id: later
        visible: false
        WlrLayershell.namespace: "{LATER}"
        anchors {{ bottom: true; left: true }}
        implicitWidth: 100
        implicitHeight: 20
    }}
    IpcHandler {{
        target: "popup"
        function raise(): void {{ popup.visible = true }}
        function showLater(): void {{ later.visible = true }}
        function probe(): void {{ console.log("PROBE") }}
    }}
}}
"#
    );
    let mut sh = Shell::start_on(&qml, Some(px.path()));
    let s = sh.wait("the window on a screen", |s| {
        requests(s, "get_toplevel").next().is_some()
            && requests(s, "get_xdg_surface")
                .next()
                .and_then(|a| a.split(", ").nth(1))
                .is_some_and(|w| s.contains(&format!("{}.enter(", w.trim_end_matches(')'))))
    });
    let toplevel = requests(&s, "get_toplevel")
        .next()
        .map(|a| a.trim_start_matches("new id ").trim_end_matches(')'))
        .unwrap()
        .to_string();
    sh.probe("popup");
    px.hold();
    sh.ipc("popup", "raise");
    sh.wait("get_popup", |s| xdg_popups(s).len() == 1);
    sh.probe("popup");
    swaymsg(&sock, &["[title=\"rustyPopupParent\"]", "kill"]);
    sh.wait("the window destroyed", |s| {
        s.contains(&format!(" {toplevel}.close("))
            && s.contains(&format!(" -> {toplevel}.destroy("))
    });
    // The compositor now reads the popup's requests and its parent's destruction.
    px.release();
    assert_alive_after(&mut sh, "popup", "the window");
}

/// A popup of a popup: the screen's removal ends the panel and with it the first popup
/// (`popup_done`), and wlroots takes the surface of an `xdg_popup` parent whatever has become
/// of it (`wlr_xdg_popup.c`, `create_xdg_popup`), so the second popup's commit has a parent.
#[test]
fn popup_of_a_popup_shown_while_its_panels_screen_is_unplugged_does_not_end_the_shell() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let mut out = Output::create(&sock);
    let mut sh = Shell::start_on(&panel_with_popup(&out.name), Some(px.path()));
    sh.wait("the panel on its screen", |s| {
        requests(s, "get_layer_surface")
            .find(|a| a.contains(POPUP_RACE))
            .and_then(|a| a.split(", ").nth(1))
            .is_some_and(|w| s.contains(&format!("{w}.enter(")))
    });
    sh.ipc("popup", "raise");
    let s = sh.wait("the first popup on the screen", |s| {
        xdg_popups(s).len() == 1 && s.contains(&format!(" {}.enter(", popup_surface(s)))
    });
    let (first, ..) = xdg_popups(&s)[0].clone();
    let (layer, _) = layer_surfaces(&s, POPUP_RACE)[0].clone();
    sh.probe("popup");
    px.hold();
    sh.ipc("popup", "raiseInner");
    sh.wait("the second popup's get_popup", |s| xdg_popups(s).len() == 2);
    sh.probe("popup");
    out.unplug();
    sh.wait("the panel's closed and the first popup destroyed", |s| {
        s.contains(&format!(" {layer}.closed(")) && s.contains(&format!(" -> {first}.destroy("))
    });
    // The compositor now reads the second popup's requests on the first, which it has ended.
    px.release();
    assert_alive_after(&mut sh, "popup", "the popup's parent popup");
}

/// The other order of the same race: the screen goes after the compositor has read
/// `get_popup` and before it reads the popup's first commit, which the shell wrote on the
/// compositor's answer. wlroots ends the popup with its layer surface (`popup_done`,
/// `wlr_layer_shell_v1.c` `layer_surface_destroy`, `wlr_xdg_popup_destroy`) and runs no
/// commit of an `xdg_surface` whose popup is gone (`wlr_xdg_surface.c`).
#[test]
fn popup_committed_while_its_panels_screen_is_unplugged_does_not_end_the_shell() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let mut out = Output::create(&sock);
    let mut sh = Shell::start_on(&panel_with_popup(&out.name), Some(px.path()));
    sh.wait("the panel on its screen", |s| {
        requests(s, "get_layer_surface")
            .find(|a| a.contains(POPUP_RACE))
            .and_then(|a| a.split(", ").nth(1))
            .is_some_and(|w| s.contains(&format!("{w}.enter(")))
    });
    sh.probe("popup");
    px.hold();
    sh.ipc("popup", "raise");
    let s = sh.wait("get_popup", |s| popups(s) == 1);
    sh.probe("popup");
    let surface = popup_surface(&s);
    let (layer, _) = layer_surfaces(&s, POPUP_RACE)[0].clone();
    // The compositor reads `get_popup` and answers the sync; the commit written on that
    // answer is held.
    px.pass();
    sh.wait("the popup's first commit", |s| {
        s.contains(&format!("-> {surface}.commit("))
    });
    out.unplug();
    sh.wait("the panel's closed", |s| {
        s.contains(&format!(" {layer}.closed("))
    });
    sh.settle("popup");
    // The compositor now reads the commit of a popup whose panel it has ended.
    px.release();
    assert_alive_after(&mut sh, "popup", "the panel of a committed popup");
    let s = sh.text();
    assert!(
        s.contains("PANELCLOSED") && s.contains("POPUPVISIBLE false"),
        "the panel and its popup were not closed:\n{}",
        tail(&s)
    );
    // wlroots had adopted the popup and ended it with the layer surface. Hyprland 0.57.1
    // sends the panel's `closed` alone, so there this last check fails after the others held.
    let (popup, ..) = xdg_popups(&s)[0].clone();
    assert!(
        s.contains(&format!(" {popup}.popup_done(")),
        "the compositor did not end the popup with its panel:\n{}",
        tail(&s)
    );
}

/// A panel with the tree's tray menu (`SysTrayMenu.qml`, the `PopupWindow` a tray item opens)
/// on `screen`, no tray item behind it, and the panel `later`.
fn panel_with_tray_menu(screen: &str) -> String {
    format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.modules.rusty.bar
ShellRoot {{
    PanelWindow {{
        id: panel
        screen: Quickshell.screens.find(s => s.name === "{screen}") ?? null
        WlrLayershell.namespace: "{POPUP_RACE}"
        anchors {{ top: true; left: true }}
        implicitWidth: 300
        implicitHeight: 60
        onClosed: console.log("PANELCLOSED")
        SysTrayMenu {{
            id: menu
            trayItemMenuHandle: null
            visible: false
            anchor.window: panel
            anchor.rect.y: 60
            onVisibleChanged: console.log("POPUPVISIBLE " + visible)
        }}
    }}
    PanelWindow {{
        id: later
        visible: false
        WlrLayershell.namespace: "{LATER}"
        anchors {{ bottom: true; left: true }}
        implicitWidth: 100
        implicitHeight: 20
    }}
    IpcHandler {{
        target: "popup"
        function raise(): void {{ menu.open() }}
        function showLater(): void {{ later.visible = true }}
        function probe(): void {{ console.log("PROBE") }}
    }}
}}
"#
    )
}

/// The tray menu is a popup of the bar, a layer surface: open on a screen that is then
/// unplugged, it is closed with the bar's panel and the shell stays.
#[test]
fn tray_menu_open_when_its_panels_screen_is_unplugged_is_closed() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let mut out = Output::create(&sock);
    let mut sh = Shell::start_in_tree(&panel_with_tray_menu(&out.name), px.path());
    sh.wait("the panel on its screen", |s| {
        requests(s, "get_layer_surface")
            .find(|a| a.contains(POPUP_RACE))
            .and_then(|a| a.split(", ").nth(1))
            .is_some_and(|w| s.contains(&format!("{w}.enter(")))
    });
    sh.ipc("popup", "raise");
    let s = sh.wait("the menu on the screen", |s| {
        xdg_popups(s).len() == 1 && s.contains(&format!(" {}.enter(", popup_surface(s)))
    });
    let (menu, ..) = xdg_popups(&s)[0].clone();
    let (layer, _) = layer_surfaces(&s, POPUP_RACE)[0].clone();
    out.unplug();
    sh.wait("the panel's closed and the menu destroyed", |s| {
        s.contains(&format!(" {layer}.closed(")) && s.contains(&format!(" -> {menu}.destroy("))
    });
    assert_alive_after(&mut sh, "popup", "the panel of an open tray menu");
    let s = sh.text();
    assert!(
        s.contains("PANELCLOSED") && s.contains("POPUPVISIBLE false"),
        "the panel and its menu were not closed:\n{}",
        tail(&s)
    );
}

/// The tray menu in the race of `popup_shown_while_its_panels_screen_is_unplugged…`: it goes
/// through the same place in the shell, so its first commit waits for the compositor too.
#[test]
fn tray_menu_opened_while_its_panels_screen_is_unplugged_does_not_end_the_shell() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let mut out = Output::create(&sock);
    let mut sh = Shell::start_in_tree(&panel_with_tray_menu(&out.name), px.path());
    show_popup_and_unplug(&mut sh, &px, &mut out);
    sh.settle("popup");
    px.release();
    assert_alive_after(&mut sh, "popup", "the panel of a tray menu being opened");
    let s = sh.text();
    assert!(
        !s.contains(&format!("-> {}.commit(", popup_surface(&s))),
        "the menu of the closed panel was committed:\n{}",
        tail(&s)
    );
    assert!(
        s.contains("PANELCLOSED") && s.contains("POPUPVISIBLE false"),
        "the panel and its menu were not closed:\n{}",
        tail(&s)
    );
}

/// A lock surface: its parent is the screen. wlroots makes the lock surface of a screen that
/// is gone an inert one (`wlr_session_lock_v1.c`, `lock_handle_get_lock_surface`), no error.
/// The surfaces are asked for by a second `WlSessionLock` taking the held lock over: that
/// needs no answer from the compositor in between.
#[test]
fn lock_surface_asked_for_a_screen_being_unplugged_does_not_end_the_shell() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sock) = sway() else { return };
    let px = testenv::HeldRequests::start();
    let mut out = Output::create(&sock);
    let qml = format!(
        r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {{
    id: root
    property bool loaded: true
    property bool locked: true
    Loader {{
        active: root.loaded
        sourceComponent: WlSessionLock {{
            locked: root.locked
            surface: Component {{
                WlSessionLockSurface {{ color: "black" }}
            }}
        }}
    }}
    PanelWindow {{
        id: later
        visible: false
        WlrLayershell.namespace: "{LATER}"
        anchors {{ bottom: true; left: true }}
        implicitWidth: 100
        implicitHeight: 20
    }}
    IpcHandler {{
        target: "lock"
        function unload(): void {{ root.loaded = false }}
        function reload(): void {{ root.loaded = true }}
        function unlock(): void {{ root.locked = false }}
        function showLater(): void {{ later.visible = true }}
        function probe(): void {{ console.log("PROBE") }}
    }}
}}
"#
    );
    let mut sh = Shell::start_on(&qml, Some(px.path()));
    let name = out.name.clone();
    // Lock surfaces asked for on the screen `name`, and lock surfaces destroyed.
    let asked = |s: &str| {
        let id = output_ids(s).into_iter().rev().find(|(n, _)| *n == name);
        id.map_or(0, |(_, id)| {
            requests(s, "get_lock_surface")
                .filter(|a| a.ends_with(&format!("{id})")))
                .count()
        })
    };
    let destroyed = |s: &str| {
        s.lines()
            .filter(|l| l.contains("-> ext_session_lock_surface_v1#") && l.contains(".destroy("))
            .count()
    };
    sh.wait("the session lock", |s| s.contains("session lock: locked"));
    sh.wait("the first holder's lock surface on the screen", |s| {
        asked(s) == 1
    });
    sh.ipc("lock", "unload");
    let s = sh.wait("the first holder's lock surfaces destroyed", |s| {
        destroyed(s) == requests(s, "get_lock_surface").count()
    });
    let before = destroyed(&s);
    sh.settle("lock");
    px.hold();
    sh.ipc("lock", "reload");
    sh.wait("get_lock_surface for the screen", |s| asked(s) == 2);
    sh.probe("lock");
    out.unplug();
    let unplugged = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sh.wait("the unplugged screen's lock surface destroyed", |s| {
            destroyed(s) == before + 1
        })
    }));
    // The compositor now reads `get_lock_surface` for the screen it has removed.
    px.release();
    // Never leave the compositor locked for the tests that run after this one.
    sh.ipc("lock", "unlock");
    sh.wait("the unlock", |s| s.contains("session lock: released"));
    assert!(
        unplugged.is_ok(),
        "the unplugged screen's lock surface was not destroyed"
    );
    // The panel's `enter` also says the compositor has read the unlock.
    assert_alive_after(&mut sh, "lock", "the lock surface's screen");
}
