//! ext-session-lock-v1 `finished`, checked on the requests impulse-shell puts on the wire (recorded by
//! `proxy::EndLock`; `WAYLAND_DEBUG` writes a line's timestamp, `[discarded]` marker and request
//! separately, so other threads' stderr can split them and a discarded request reads as sent): the
//! client must answer it with `destroy` when `locked` never came and with `unlock_and_destroy`
//! once it did ("It is a protocol error to make this request if the locked event was sent").
//! Sway ends only a lock it never confirmed (a second lock while one is held), so the
//! after-`locked` order comes from `proxy::EndLock`, a compositor that ends a confirmed lock.

use std::io::Write;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use testenv::Log;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

/// Tests here lock sway's session: one at a time.
static LOCK: Mutex<()> = Mutex::new(());

const SHELL: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
ShellRoot {
    id: root
    property bool locked: true
    WlSessionLock {
        id: lock
        locked: root.locked
        onLockedChanged: console.log("LOCKED " + locked)
        onSecureChanged: console.log("SECURE " + secure)
        surface: Component {
            WlSessionLockSurface { color: "black" }
        }
    }
    IpcHandler {
        target: "lock"
        function unlock(): void { root.locked = false }
        function relock(): void { root.locked = true }
        function probe(): void { console.log("PROBE " + lock.locked) }
    }
}
"#;

/// A panel on every screen that has changed in the turn the lock is asked for: the lock is
/// asked for by the end of the panel's animation, which turns the panel red. A frame callback
/// of the panel brings that turn, so the panel may draw at once; a callback that comes late
/// (sway under load) has the panel draw the step it missed first, and its red frame then
/// waits for that frame's callback.
const PANEL: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
ShellRoot {
    id: root
    property bool locked: false
    Variants {
        model: Rustshell.screens
        PanelWindow {
            required property var modelData
            screen: modelData
            implicitWidth: 40
            implicitHeight: 40
            Rectangle {
                id: box
                anchors.fill: parent
                color: root.locked ? "red" : "blue"
                NumberAnimation {
                    target: box
                    property: "opacity"
                    from: 0.5
                    to: 1
                    duration: 300
                    running: true
                    onFinished: root.locked = true
                }
            }
        }
    }
    WlSessionLock {
        id: lock
        locked: root.locked
        surface: Component {
            WlSessionLockSurface { color: "black" }
        }
    }
    IpcHandler {
        target: "lock"
        function unlock(): void { root.locked = false }
    }
}
"#;

/// A headless sway to run against; without a Wayland display the test fails (skipped only with
/// RI_EQUIV_SKIP=1).
fn sway() -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("a headless sway (WAYLAND_DISPLAY)");
        return false;
    }
    true
}

#[test]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "finished_after_locked_unlocks_and_destroys",
        &[("WAYLAND_DISPLAY", None)],
    );
}

/// impulse-shell running [`SHELL`] with its Wayland traffic logged.
struct Shell {
    child: Child,
    dir: tempfile::TempDir,
    log: Arc<Log>,
    display: Option<PathBuf>,
}

impl Shell {
    /// Connected to the Wayland socket `display`, or to sway's.
    fn start(display: Option<&Path>) -> Self {
        Self::start_qml(SHELL, display)
    }

    /// As [`Shell::start`], running the shell `qml`.
    fn start_qml(qml: &str, display: Option<&Path>) -> Self {
        Self::start_logging(qml, display, "")
    }

    /// As [`Shell::start_qml`], with the `RI_LOG` categories `ri_log` in the log.
    fn start_logging(qml: &str, display: Option<&Path>, ri_log: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("shell.qml"), qml).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        let xdg = dir.path().join("xdg");
        let child = Command::new(env!("CARGO_BIN_EXE_impulse-shell"))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .envs((!ri_log.is_empty()).then_some(("RI_LOG", ri_log)))
            .env("WAYLAND_DEBUG", "1")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .envs(display.map(|d| ("WAYLAND_DISPLAY", d)))
            .stdout(Stdio::null())
            .stderr(w)
            .spawn()
            .unwrap();
        Shell {
            child,
            dir,
            log: Log::capture(r),
            display: display.map(Path::to_path_buf),
        }
    }

    fn text(&self) -> String {
        wl(&self.log.text())
    }

    /// Waits for `f(log)`: `Err(log)` once impulse-shell exits (closes its log) without it.
    fn wait(&self, f: impl Fn(&str) -> bool) -> Result<String, String> {
        self.log.wait(|s| f(&wl(s))).map_err(|s| wl(&s))?;
        Ok(self.text())
    }

    /// `lock.probe()`: the log once QML printed what it reads, `None` if impulse-shell is gone (the
    /// call fails, or impulse-shell exits before printing it).
    fn probe(&self) -> Option<String> {
        let n = |s: &str| s.matches("PROBE ").count();
        let before = n(&self.text());
        self.call("probe").then_some(())?;
        self.wait(|s| n(s) > before).ok()
    }

    fn call(&self, func: &str) -> bool {
        Command::new(env!("CARGO_BIN_EXE_impulse-shell"))
            .arg("-p")
            .arg(self.dir.path())
            .args(["ipc", "call", "lock", func])
            .envs(self.display.as_deref().map(|d| ("WAYLAND_DISPLAY", d)))
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    fn ipc(&self, func: &str) {
        assert!(self.call(func), "ipc call lock {func}");
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// wayland-backend logs `iface#id`, its discarded-message lines `iface@id`: all as `iface#id`.
fn wl(log: &str) -> String {
    log.replace('@', "#")
}

/// The log lines that matter for these tests (no per-frame traffic).
fn tail(s: &str) -> String {
    let keep = [
        "session_lock",
        "session lock",
        "qml:",
        "LOCKED",
        "SECURE",
        "PROBE",
        "error",
    ];
    let lines: Vec<&str> = s
        .lines()
        .filter(|l| keep.iter().any(|k| l.contains(k)))
        .collect();
    lines[lines.len().saturating_sub(60)..].join("\n")
}

/// A compositor that confirms the lock (sway's `locked`) and then ends it: `finished` is sent
/// right after `locked`, which the spec leaves to compositor policy. A client `destroy` of the
/// confirmed lock gets the spec's `invalid_destroy` protocol error, as from a real compositor.
#[test]
fn finished_after_locked_unlocks_and_destroys() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !sway() {
        return;
    }
    let px = proxy::EndLock::start(proxy::upstream());
    let sh = Shell::start(Some(&px.path));
    let reqs = px.wait_requests(2);
    let s = sh.text();
    assert_eq!(
        reqs,
        ["lock", "unlock_and_destroy"],
        "finished after locked was not answered with unlock_and_destroy:\n{}",
        tail(&s)
    );
    assert!(px.ended(), "the proxy never sent finished after locked");
    // QML reads `locked: false` (Quickshell's WlSessionLock::unlock), so no new lock follows.
    let s = sh
        .wait(|s| s.contains("LOCKED false"))
        .unwrap_or_else(|s| panic!("impulse-shell exited:\n{}", tail(&s)));
    assert!(
        s.contains("The compositor ended the session lock.")
            && !s.contains("Failed to acquire session lock."),
        "wrong log for a lock the compositor ended:\n{}",
        tail(&s)
    );
    let s = sh
        .probe()
        .unwrap_or_else(|| panic!("impulse-shell died:\n{}", tail(&sh.text())));
    assert!(
        s.contains("PROBE false"),
        "QML still reads locked:\n{}",
        tail(&s)
    );
    assert_eq!(
        px.requests(),
        ["lock", "unlock_and_destroy"],
        "the ended lock was retaken:\n{}",
        tail(&s)
    );
}

/// Sway refuses a second lock while one is held: `finished` comes before any `locked`, so the
/// lock goes by `destroy` (`unlock_and_destroy` would be `invalid_unlock`) and is not retried.
#[test]
fn finished_before_locked_destroys() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !sway() {
        return;
    }
    let holder = Shell::start(None);
    holder
        .wait(|s| s.contains("session lock: locked"))
        .unwrap_or_else(|s| panic!("the holder exited:\n{}", tail(&s)));
    // Through the proxy only for its record: sway never confirms this lock, so it never ends one.
    let px = proxy::EndLock::start(proxy::upstream());
    let sh = Shell::start(Some(&px.path));
    let reqs = px.wait_requests(2);
    let s = sh.text();
    let probe = sh.probe();
    // `ri-native.locked` names the shell that holds the lock.
    let marker = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("ri-native.locked");
    let marker = std::fs::read_to_string(marker);
    // Never leave the compositor locked for the tests that run after this one.
    holder.ipc("unlock");
    let released = holder
        .wait(|s| s.contains("session lock: released"))
        .unwrap_or_else(|s| s);
    assert_eq!(
        reqs,
        ["lock", "destroy"],
        "finished before locked was not answered with destroy:\n{}",
        tail(&s)
    );
    let s = probe.unwrap_or_else(|| panic!("impulse-shell died:\n{}", tail(&s)));
    assert!(
        s.contains("Failed to acquire session lock."),
        "no lock failure logged:\n{}",
        tail(&s)
    );
    assert_eq!(
        px.requests(),
        ["lock", "destroy"],
        "the refused lock was retried:\n{}",
        tail(&s)
    );
    // The refused shell never held the lock: it has none to release, and the marker is still
    // the holder's.
    assert!(
        !s.contains("session lock: released"),
        "a lock that was never held was logged as released:\n{}",
        tail(&s)
    );
    assert_eq!(
        marker.ok(),
        Some(holder.child.id().to_string()),
        "the refused shell removed the holder's lock marker:\n{}",
        tail(&s)
    );
    assert!(
        released.contains("session lock: released"),
        "the holder did not unlock:\n{}",
        tail(&released)
    );
}

/// Hyprland 0.57.1 refuses a lock while it handles the request (`CSessionLock::sendDenied`) and
/// creates no object for a `get_lock_surface` on a refused lock, so a request on such a surface
/// is `invalid object` and ends the connection ([`proxy::EndLock::refusing`]). The lock goes by
/// `destroy` with no surface asked for, QML reads what Quickshell's `finished` leaves
/// (`locked` false, `secureChanged` emitted with `secure` still false), and impulse-shell stays up.
#[test]
fn a_lock_refused_as_hyprland_does_asks_for_no_surface() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !sway() {
        return;
    }
    let px = proxy::EndLock::refusing(proxy::upstream(), 1);
    let sh = Shell::start(Some(&px.path));
    let reqs = px.wait_requests(2);
    let s = sh.text();
    assert_eq!(
        reqs,
        ["lock", "destroy"],
        "a refused lock was not answered with destroy alone:\n{}",
        tail(&s)
    );
    let s = sh
        .probe()
        .unwrap_or_else(|| panic!("impulse-shell died:\n{}", tail(&sh.text())));
    assert!(
        s.contains("Failed to acquire session lock.") && !s.contains("session lock: locked"),
        "wrong log for a refused lock:\n{}",
        tail(&s)
    );
    assert!(
        s.contains("PROBE false") && s.matches("SECURE false").count() == 1,
        "QML does not read what Quickshell leaves after a refused lock:\n{}",
        tail(&s)
    );
    assert_eq!(
        px.requests(),
        ["lock", "destroy"],
        "the refused lock was retried:\n{}",
        tail(&s)
    );
}

/// After a refused lock the next one is asked for and taken: `locked` false, then true again.
#[test]
fn a_lock_after_a_refused_one_is_taken() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !sway() {
        return;
    }
    let px = proxy::EndLock::refusing(proxy::upstream(), 1);
    let sh = Shell::start(Some(&px.path));
    let refused = px.wait_requests(2);
    let alive = sh.call("unlock") && sh.call("relock");
    let locked = sh.wait(|s| s.contains("session lock: locked"));
    // Never leave the compositor locked for the tests that run after this one.
    let released = locked.is_ok() && sh.call("unlock");
    let end = sh
        .wait(|s| s.contains("session lock: released"))
        .unwrap_or_else(|s| s);
    // The shell logs the release before the request reaches the proxy.
    let all = if released && end.contains("session lock: released") {
        px.wait_requests(4)
    } else {
        px.requests()
    };
    assert_eq!(
        refused,
        ["lock", "destroy"],
        "a refused lock was not answered with destroy alone:\n{}",
        tail(&end)
    );
    assert!(
        alive,
        "impulse-shell died after the refused lock:\n{}",
        tail(&end)
    );
    assert!(
        locked.is_ok() && end.contains("SECURE true"),
        "the lock after the refused one was not taken:\n{}",
        tail(&end)
    );
    assert!(
        released && end.contains("session lock: released"),
        "the lock after the refused one was not released:\n{}",
        tail(&end)
    );
    assert_eq!(
        all,
        ["lock", "destroy", "lock", "unlock_and_destroy"],
        "{}",
        tail(&end)
    );
}

/// The frames in a shell's log: the line of each commit that shows a new buffer, with its
/// surface (`wl_surface#N`). A commit without one (a layer surface's first) draws nothing.
fn frames<'a>(lines: &[&'a str]) -> Vec<(usize, &'a str)> {
    let mut attached = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(at) = l.find("-> wl_surface#") else {
            continue;
        };
        let request = &l[at + 3..];
        let Some((surface, call)) = request.split_once('.') else {
            continue;
        };
        if call.starts_with("attach(wl_buffer#") {
            attached.insert(surface);
        } else if call.starts_with("commit(") && attached.remove(surface) {
            out.push((i, surface));
        }
    }
    out
}

/// The lock surfaces asked for in a shell's log, in order: the line of each `get_lock_surface`
/// with its surface (`wl_surface#N`).
fn lock_surfaces<'a>(lines: &[&'a str]) -> Vec<(usize, &'a str)> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            // `get_lock_surface(new id ext_session_lock_surface_v1#N, wl_surface#M, wl_output#K)`
            let args = &l[l.find(".get_lock_surface(")?..];
            let surface = &args[args.find(", wl_surface#")? + 2..];
            Some((i, &surface[..surface.find(',')?]))
        })
        .collect()
}

/// What was drawn around the `n`th lock surface a shell asked for: the frames of the windows
/// that are no lock surface between the request for it and its own first frame, and whether
/// such a window has drawn a frame after that one. `None` until the lock surface has a frame.
fn drawn_around_lock_surface(log: &str, n: usize) -> Option<(Vec<String>, bool)> {
    let lines: Vec<&str> = log.lines().collect();
    let locks = lock_surfaces(&lines);
    let &(asked, own) = locks.get(n)?;
    let is_lock = |s: &str| locks.iter().any(|(_, l)| *l == s);
    let frames = frames(&lines);
    let &(first, _) = frames.iter().find(|(i, s)| *i > asked && *s == own)?;
    let other = |from: usize, to: usize| {
        frames
            .iter()
            .filter(move |(i, s)| *i > from && *i < to && !is_lock(s))
    };
    Some((
        other(asked, first).map(|(i, _)| lines[*i].into()).collect(),
        other(first, lines.len()).next().is_some(),
    ))
}

/// The lock screen draws before any other window does: the wait for the compositor's answer
/// costs the lock screen no frame of another window. Against a compositor that sends `locked`
/// once the lock surface has a frame, as Hyprland does ([`proxy::EndLock::locking_late`]); the
/// panel has changed in the turn that asks for the lock.
#[test]
fn the_lock_screen_draws_before_any_other_window() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !sway() {
        return;
    }
    let px = proxy::EndLock::locking_late(proxy::upstream());
    let sh = Shell::start_qml(PANEL, Some(&px.path));
    let locked = sh.wait(|s| s.contains("session lock: locked"));
    // Never leave the compositor locked for the tests that run after this one.
    let released = locked.is_ok() && sh.call("unlock");
    let end = sh
        .wait(|s| !released || s.contains("session lock: released"))
        .unwrap_or_else(|s| s);
    assert!(
        locked.is_ok() && released,
        "the lock was not taken and released:\n{}",
        tail(&end)
    );
    assert_lock_screen_drawn_first(&end);
}

/// A shell's log of a lock taken and released: the panel has a frame before the `lock` request,
/// no window draws one between that request and the one for the lock surface, none but the
/// lock surface until the lock surface's first, and the panel draws one after it.
fn assert_lock_screen_drawn_first(end: &str) {
    let lines: Vec<&str> = end.lines().collect();
    let asked = lines
        .iter()
        .position(|l| l.contains(".lock(new id ext_session_lock_v1#"))
        .expect("locked without a lock request");
    assert!(
        !frames(&lines[..asked]).is_empty(),
        "the panel drew no frame before the lock:\n{}",
        tail(end)
    );
    let between = frames(&lines[asked..lock_surfaces(&lines)[0].0]);
    assert!(
        between.is_empty(),
        "a window drew a frame before the lock surface was asked for: {between:?}"
    );
    let (before, after) =
        drawn_around_lock_surface(end, 0).expect("locked without a frame of the lock surface");
    assert!(
        before.is_empty(),
        "another window drew a frame before the lock screen:\n{}",
        before.join("\n")
    );
    assert!(
        after,
        "the panel drew no frame after the lock screen's:\n{}",
        tail(end)
    );
}

/// As [`PANEL`], but its animation never ends, so each of its frames asks for a frame callback,
/// and the session is locked when `lock` is called, which also changes the panel's colour.
const CALLED_PANEL: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
ShellRoot {
    id: root
    property bool locked: false
    Variants {
        model: Rustshell.screens
        PanelWindow {
            required property var modelData
            screen: modelData
            implicitWidth: 40
            implicitHeight: 40
            Rectangle {
                id: box
                anchors.fill: parent
                color: root.locked ? "red" : "blue"
                NumberAnimation {
                    target: box
                    property: "opacity"
                    from: 0.5
                    to: 1
                    duration: 300
                    loops: Animation.Infinite
                    running: true
                }
            }
        }
    }
    WlSessionLock {
        id: lock
        locked: root.locked
        surface: Component {
            WlSessionLockSurface { color: "black" }
        }
    }
    IpcHandler {
        target: "lock"
        function lock(): void { root.locked = true }
        function unlock(): void { root.locked = false }
    }
}
"#;

/// The frame callbacks (`wl_surface.frame`) answered in a shell's log: the line of each answer
/// with the surface (`wl_surface#N`) it was asked for.
fn frame_callbacks_answered<'a>(lines: &[&'a str]) -> Vec<(usize, &'a str)> {
    let mut asked = std::collections::HashMap::new();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some(at) = l.find(".frame(new id wl_callback#") {
            let id = &l[at + ".frame(new id ".len()..];
            let surface = &l[l.find("wl_surface#").unwrap_or(at)..at];
            asked.insert(id.trim_end_matches(')'), surface);
        } else if !l.contains("->")
            && let Some(at) = l.find("wl_callback#")
            && let Some((id, "done")) = l[at..].split_once('(').and_then(|(c, _)| c.split_once('.'))
            && let Some(surface) = asked.remove(id)
        {
            out.push((i, surface));
        }
    }
    out
}

/// A compositor answers a frame callback when it likes: sway under load sent a panel's 290 ms
/// late, in one batch with its answer to the `lock` request and ahead of it. The panel, changed
/// since its last frame, does not draw at that callback: the lock surface is asked for first
/// and draws first. (`proxy::EndLock::answering_frames_with_lock` gives every run that order.)
#[test]
fn a_late_frame_callback_draws_no_window_before_the_lock_surface_is_asked_for() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !sway() {
        return;
    }
    let px = proxy::EndLock::answering_frames_with_lock(proxy::upstream());
    let sh = Shell::start_qml(CALLED_PANEL, Some(&px.path));
    // The panel has a frame on screen, and its frame callback is answered only with the lock:
    // until then it draws nothing more.
    assert!(
        px.wait_frame_callback_kept(),
        "the panel drew no frame:\n{}",
        tail(&sh.text())
    );
    // The panel turns red, a change it draws at its frame callback.
    let asked = sh.call("lock");
    let locked = sh.wait(|s| !asked || s.contains("session lock: locked"));
    let released = asked && locked.is_ok() && sh.call("unlock");
    let end = sh
        .wait(|s| !released || s.contains("session lock: released"))
        .unwrap_or_else(|s| s);
    assert!(
        released,
        "the lock was not taken and released:\n{}",
        tail(&end)
    );
    let lines: Vec<&str> = end.lines().collect();
    let request = lines
        .iter()
        .position(|l| l.contains(".lock(new id ext_session_lock_v1#"))
        .expect("locked without a lock request");
    let surface = lock_surfaces(&lines)[0].0;
    assert!(
        frame_callbacks_answered(&lines)
            .iter()
            .any(|(i, _)| (request..surface).contains(i)),
        "no frame callback was answered between the lock request and the one for the lock \
         surface:\n{}",
        tail(&end)
    );
    assert_lock_screen_drawn_first(&end);
}

/// The other windows do not wait for `locked`: a compositor may send it late or never, and
/// shows them until then. Against a compositor that keeps `locked` back
/// ([`proxy::EndLock::holding_locked`]): the panel, changed in the turn that asked for the
/// lock, draws once the lock surface has its first frame. Sway has answered the panel's frame
/// callbacks by the time it locks (the proxy sees to it), so the panel waits for none.
#[test]
fn other_windows_draw_once_the_lock_screen_has_its_frame() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !sway() {
        return;
    }
    let px = proxy::EndLock::holding_locked(proxy::upstream());
    let sh = Shell::start_qml(PANEL, Some(&px.path));
    let drawn = sh.wait(|s| drawn_around_lock_surface(s, 0).is_some_and(|(_, after)| after));
    let unconfirmed = sh.text();
    // Never leave the compositor locked for the tests that run after this one.
    px.send_locked();
    let locked = sh.wait(|s| s.contains("session lock: locked"));
    let released = locked.is_ok() && sh.call("unlock");
    let end = sh
        .wait(|s| !released || s.contains("session lock: released"))
        .unwrap_or_else(|s| s);
    assert!(
        drawn.is_ok() && released,
        "the panel drew no frame, or the lock was not taken and released:\n{}",
        tail(&end)
    );
    assert!(
        !unconfirmed.contains("session lock: locked"),
        "`locked` was not kept back:\n{}",
        tail(&unconfirmed)
    );
    let (before, _) = drawn_around_lock_surface(&unconfirmed, 0).unwrap();
    assert!(
        before.is_empty(),
        "the panel drew a frame before the lock screen:\n{}",
        before.join("\n")
    );
}

#[derive(Default)]
struct Globals {
    seat: Option<wl_seat::WlSeat>,
    vkbd: Option<ZwpVirtualKeyboardManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
    fn event(
        g: &mut Self,
        reg: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = ev
        {
            match interface.as_str() {
                "wl_seat" if g.seat.is_none() => g.seat = Some(reg.bind(name, 1, qh, ())),
                "zwp_virtual_keyboard_manager_v1" => g.vkbd = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

delegate_noop!(Globals: ignore wl_seat::WlSeat);
delegate_noop!(Globals: ZwpVirtualKeyboardManagerV1);
delegate_noop!(Globals: ZwpVirtualKeyboardV1);

/// A keyboard on the sway's seat, which has none of its own (headless): with it sway gives a
/// lock surface the keyboard focus. It is gone when this is dropped.
struct Keyboard {
    _conn: Connection,
    _kbd: ZwpVirtualKeyboardV1,
    _keymap: std::fs::File,
}

impl Keyboard {
    fn join() -> Option<Self> {
        let socket = std::os::unix::net::UnixStream::connect(proxy::upstream()).unwrap();
        let conn = Connection::from_socket(socket).unwrap();
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut globals = Globals::default();
        queue.roundtrip(&mut globals).unwrap();
        let (Some(seat), Some(vkbd)) = (globals.seat.clone(), globals.vkbd.clone()) else {
            testenv::unavailable("sway lacks zwp_virtual_keyboard_v1");
            return None;
        };
        let kbd = vkbd.create_virtual_keyboard(&seat, &qh, ());
        let ctx = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
        let km = xkbcommon::xkb::Keymap::new_from_names(
            &ctx,
            "",
            "",
            "us",
            "",
            None,
            xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .unwrap();
        let text = km.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
        let mut keymap = tempfile::tempfile().unwrap();
        keymap.write_all(text.as_bytes()).unwrap();
        keymap.write_all(&[0]).unwrap();
        kbd.keymap(1, keymap.as_fd(), text.len() as u32 + 1);
        queue.roundtrip(&mut globals).unwrap();
        Some(Keyboard {
            _conn: conn,
            _kbd: kbd,
            _keymap: keymap,
        })
    }
}

/// The line of a shell's log (`RI_LOG=ri::keys=debug`) at which a window became the one keys
/// go to: the answer to the sync sent at its `wl_keyboard.enter`.
fn keyboard_active(lines: &[&str]) -> Option<usize> {
    lines.iter().position(|l| l.contains("kbd active Some("))
}

/// The lock screen gets the keyboard before any other window draws again. The compositor
/// gives a lock surface the keyboard focus when its first frame maps it, and the surface gets
/// keys from the answer to the sync sent at that `wl_keyboard.enter` on; a key handled before
/// that answer reaches no item, as in Quickshell (`key_before_activation_equiv.rs`). The loop
/// reads no event while it draws, so a frame of another window drawn in between keeps the lock
/// screen without the keyboard for as long as that frame takes: in a Hyprland guest without a
/// GPU the blurred wallpaper took 1.1 to 1.7 s, a password typed 1.5 s after the lock was lost
/// in 5 of 90 rounds, and its Return sent an empty one (docs/quickshell-differences.md).
///
/// Against a compositor that keeps `locked` back ([`proxy::EndLock::holding_locked`]), as
/// Hyprland does until the lock surface has a frame: the panel, changed in the turn that asked
/// for the lock, is held until that frame and waits for no frame callback, so it draws as soon
/// as it may. That is after the lock surface is active, and it does draw then.
#[test]
fn no_window_draws_between_the_lock_screens_first_frame_and_its_keyboard() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !sway() {
        return;
    }
    let Some(_keyboard) = Keyboard::join() else {
        return;
    };
    let px = proxy::EndLock::holding_locked(proxy::upstream());
    let sh = Shell::start_logging(PANEL, Some(&px.path), "ri::keys=debug");
    let drawn = sh.wait(|s| {
        keyboard_active(&s.lines().collect::<Vec<_>>()).is_some()
            && drawn_around_lock_surface(s, 0).is_some_and(|(_, after)| after)
    });
    // Never leave the compositor locked for the tests that run after this one.
    px.send_locked();
    let locked = sh.wait(|s| s.contains("session lock: locked"));
    let released = locked.is_ok() && sh.call("unlock");
    let end = sh
        .wait(|s| !released || s.contains("session lock: released"))
        .unwrap_or_else(|s| s);
    assert!(
        drawn.is_ok() && released,
        "the lock screen got no keyboard, the panel drew no frame, or the lock was not taken \
         and released:\n{}",
        tail(&end)
    );
    let log = drawn.unwrap();
    let lines: Vec<&str> = log.lines().collect();
    let locks = lock_surfaces(&lines);
    let (asked, own) = locks[0];
    let frames = frames(&lines);
    let &(first, _) = frames
        .iter()
        .find(|(i, s)| *i > asked && *s == own)
        .unwrap();
    let active = keyboard_active(&lines).unwrap();
    assert!(
        first < active,
        "the lock screen had the keyboard before its first frame:\n{}",
        tail(&log)
    );
    let between: Vec<&str> = frames
        .iter()
        .filter(|(i, s)| *i > first && *i < active && !locks.iter().any(|(_, l)| l == s))
        .map(|(i, _)| lines[*i])
        .collect();
    assert!(
        between.is_empty(),
        "another window drew between the lock screen's first frame (line {first}) and its \
         keyboard (line {active}):\n{}",
        between.join("\n")
    );
}

/// The frames of the windows that are no lock surface in a shell's log, from line `from` on.
fn window_frames(lines: &[&str], from: usize) -> usize {
    let locks = lock_surfaces(lines);
    frames(lines)
        .iter()
        .filter(|(i, s)| *i >= from && !locks.iter().any(|(_, l)| l == s))
        .count()
}

/// A panel whose last frame still waits for its frame callback when the session is locked: a
/// locked sway does not show the panel and answers the callback only once the lock has ended
/// ([`proxy::EndLock::holding_locked_and_frames`] gives every run that order). The lock screen
/// gets its frame all the same, ahead of any frame of the panel, the lock is taken and
/// released, and with the callback answered after `unlock_and_destroy` the panel draws frame
/// after frame again.
#[test]
fn a_window_awaiting_its_frame_callback_when_locked_draws_again_once_unlocked() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !sway() {
        return;
    }
    let px = proxy::EndLock::holding_locked_and_frames(proxy::upstream());
    let sh = Shell::start_qml(CALLED_PANEL, Some(&px.path));
    // The panel has a frame on screen and gets no answer to its frame callback.
    assert!(
        px.wait_frame_callback_kept(),
        "the panel drew no frame:\n{}",
        tail(&sh.text())
    );
    let asked = sh.call("lock");
    let framed = sh.wait(|s| !asked || drawn_around_lock_surface(s, 0).is_some());
    let unconfirmed = sh.text();
    // Never leave the compositor locked for the tests that run after this one.
    px.send_locked();
    let locked = sh.wait(|s| !asked || s.contains("session lock: locked"));
    let released = asked && locked.is_ok() && sh.call("unlock");
    // The panel's frames since the lock ended.
    let again = |s: &str| {
        let lines: Vec<&str> = s.lines().collect();
        let ended = lines
            .iter()
            .position(|l| l.contains(".unlock_and_destroy()"));
        ended.map_or(0, |at| window_frames(&lines, at))
    };
    // Each frame of the animating panel asks for a frame callback: a third frame was drawn at
    // the answer to the second's.
    let end = sh
        .wait(|s| !released || (s.contains("session lock: released") && again(s) >= 3))
        .unwrap_or_else(|s| s);
    assert!(
        asked && framed.is_ok(),
        "the lock screen drew no frame:\n{}",
        tail(&end)
    );
    assert!(
        !unconfirmed.contains("session lock: locked"),
        "`locked` was not kept back:\n{}",
        tail(&unconfirmed)
    );
    let (before, _) = drawn_around_lock_surface(&unconfirmed, 0).unwrap();
    assert!(
        before.is_empty(),
        "the panel drew a frame before the lock screen:\n{}",
        before.join("\n")
    );
    assert!(
        released && end.contains("session lock: released"),
        "the lock was not taken and released:\n{}",
        tail(&end)
    );
    // The panel's frame callback was outstanding from before the `lock` request until the
    // lock ended.
    let lines: Vec<&str> = end.lines().collect();
    let request = lines
        .iter()
        .position(|l| l.contains(".lock(new id ext_session_lock_v1#"))
        .expect("locked without a lock request");
    let ended = lines
        .iter()
        .position(|l| l.contains(".unlock_and_destroy()"))
        .expect("released without unlock_and_destroy");
    let before_lock = frames(&lines[..request]);
    let panel = before_lock.first().expect("no frame before the lock").1;
    let answered: Vec<usize> = frame_callbacks_answered(&lines)
        .iter()
        .filter_map(|(i, s)| (*s == panel).then_some(*i))
        .collect();
    assert!(
        !answered.iter().any(|i| *i < ended),
        "a frame callback of the panel was answered before the lock ended:\n{}",
        tail(&end)
    );
    let again = again(&end);
    assert!(
        answered.iter().any(|i| *i > ended) && again >= 3,
        "the panel drew {again} frames after the lock ended:\n{}",
        tail(&end)
    );
}

/// An output connected while the lock waits for `locked`: its lock surface draws before the
/// other windows too (the output's own panel among them), and they draw once it has its frame.
#[test]
fn a_lock_surface_for_an_output_connected_before_locked_draws_first() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !sway() {
        return;
    }
    let px = proxy::EndLock::holding_locked(proxy::upstream());
    let sh = Shell::start_qml(PANEL, Some(&px.path));
    let first = sh.wait(|s| drawn_around_lock_surface(s, 0).is_some_and(|(_, after)| after));
    let out = first.is_ok().then(Output::create);
    let second = sh
        .wait(|s| out.is_none() || drawn_around_lock_surface(s, 1).is_some_and(|(_, after)| after));
    let unconfirmed = sh.text();
    // Never leave the compositor locked for the tests that run after this one.
    px.send_locked();
    let locked = sh.wait(|s| s.contains("session lock: locked"));
    let released = locked.is_ok() && sh.call("unlock");
    let end = sh
        .wait(|s| !released || s.contains("session lock: released"))
        .unwrap_or_else(|s| s);
    drop(out);
    assert!(
        first.is_ok() && second.is_ok() && released,
        "no frame of a panel after a lock surface's, or the lock was not taken and released:\n{}",
        tail(&end)
    );
    assert!(
        !unconfirmed.contains("session lock: locked"),
        "`locked` was not kept back:\n{}",
        tail(&unconfirmed)
    );
    let (before, _) = drawn_around_lock_surface(&unconfirmed, 1).unwrap();
    assert!(
        before.is_empty(),
        "another window drew a frame before the new output's lock screen:\n{}",
        before.join("\n")
    );
}

/// A second headless output of sway; unplugged on drop.
struct Output {
    sock: PathBuf,
    name: String,
}

impl Output {
    fn create() -> Self {
        let sock = std::env::var_os("SWAYSOCK").map(PathBuf::from).or_else(|| {
            std::fs::read_dir(std::env::var_os("XDG_RUNTIME_DIR")?)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .find(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("sway-ipc.") && n.ends_with(".sock"))
                })
        });
        let sock = sock.expect("WAYLAND_DISPLAY is set but no sway IPC socket");
        let names = || {
            let v: serde_json::Value =
                serde_json::from_str(&swaymsg(&sock, &["-r", "-t", "get_outputs"])).unwrap();
            let names = v.as_array().unwrap().iter();
            names
                .filter_map(|o| o["name"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
        };
        let before = names();
        swaymsg(&sock, &["create_output"]);
        let name = names()
            .into_iter()
            .find(|n| !before.contains(n))
            .expect("create_output added no output");
        Output { sock, name }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        swaymsg(&self.sock, &["output", &self.name, "unplug"]);
    }
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

/// A Wayland proxy between impulse-shell and sway that forwards every message and fd unchanged, except
/// on a session lock. [`EndLock::start`]: once sway sends `locked` it adds `finished`, and
/// answers a client `destroy` of that lock with the `invalid_destroy` error (sway gets
/// `unlock_and_destroy`, so its session is never left locked by the test).
/// [`EndLock::refusing`]: the first locks are refused the way Hyprland 0.57.1 does it and never
/// reach sway; the later ones are sway's own.
/// [`EndLock::locking_late`]: sway's `locked` is held back until a lock surface of that lock has
/// its first frame, which is when Hyprland sends it.
/// [`EndLock::holding_locked`]: sway's `locked` is held back until the test lets it through.
/// [`EndLock::answering_frames_with_lock`]: as `locking_late`, and sway's answers to frame
/// callbacks are kept back until it answers the first `lock` request, which they then precede.
/// [`EndLock::holding_locked_and_frames`]: as `holding_locked`, and sway's answers to the frame
/// callbacks of the windows that are no lock surface are kept back until the lock ends.
mod proxy {
    use std::collections::{HashMap, HashSet};
    use std::os::fd::{AsRawFd, RawFd};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    const WL_DISPLAY: u32 = 1;
    // ext-session-lock-v1.xml opcodes and error codes.
    const MANAGER_LOCK: u16 = 1;
    const LOCK_DESTROY: u16 = 0;
    const LOCK_GET_LOCK_SURFACE: u16 = 1;
    const LOCK_UNLOCK_AND_DESTROY: u16 = 2;
    const LOCKED: u16 = 0;
    const FINISHED: u16 = 1;
    const INVALID_DESTROY: u32 = 0;
    // wayland.xml opcodes of wl_surface.
    const SURFACE_DESTROY: u16 = 0;
    const SURFACE_ATTACH: u16 = 1;
    const SURFACE_FRAME: u16 = 3;
    const SURFACE_COMMIT: u16 = 6;

    /// What the proxy does to a session lock.
    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        /// `finished` follows sway's `locked`.
        End,
        /// The first locks are refused; the rest are left to sway.
        Refuse,
        /// `locked` waits for the first frame of a lock surface, and a `lock` request for sway's
        /// answers to the frame callbacks asked for before it.
        Late,
        /// `locked` waits for [`EndLock::send_locked`], and a `lock` request for sway's answers
        /// to the frame callbacks asked for before it.
        Held,
        /// `locked` waits for the first frame of a lock surface, and the `done` of every frame
        /// callback for the `done` of the wl_display.sync that follows the first `lock` request:
        /// it is sent right before it.
        LateFrames,
        /// `locked` waits for [`EndLock::send_locked`], and the `done` of every frame callback
        /// of a window that is no lock surface for the client to end its lock.
        HeldFrames,
    }

    pub struct EndLock {
        pub path: PathBuf,
        ended: Arc<AtomicBool>,
        wire: Arc<Wire>,
        /// Closed by [`EndLock::send_locked`]: every connection's thread polls the other end.
        hold: Mutex<Option<std::io::PipeWriter>>,
        _dir: tempfile::TempDir,
    }

    /// The session lock requests the client sent (`lock`, `destroy`, `unlock_and_destroy`, and
    /// `get_lock_surface` on a refused lock), whether its connection ended, and how many
    /// answers to its frame callbacks [`Mode::LateFrames`] or [`Mode::HeldFrames`] has kept
    /// back.
    #[derive(Default)]
    struct Wire {
        state: Mutex<(Vec<&'static str>, bool, usize)>,
        changed: Condvar,
    }

    impl Wire {
        fn update(&self, f: impl FnOnce(&mut (Vec<&'static str>, bool, usize))) {
            f(&mut self.state.lock().unwrap());
            self.changed.notify_all();
        }
    }

    impl EndLock {
        /// Listens on a fresh socket and connects each client to the compositor at `upstream`.
        pub fn start(upstream: PathBuf) -> Self {
            Self::serve(upstream, Mode::End, 0)
        }

        /// As [`EndLock::start`], but no lock is ended, and the client gets `locked` only once
        /// a lock surface of the lock has a buffer committed. Sway gets a `lock` request as
        /// with [`EndLock::holding_locked`].
        pub fn locking_late(upstream: PathBuf) -> Self {
            Self::serve(upstream, Mode::Late, 0)
        }

        /// As [`EndLock::start`], but no lock is ended, and the client gets `locked` only once
        /// [`EndLock::send_locked`] is called. Sway gets a `lock` request, and what the client
        /// sends after it, once it has answered the frame callbacks asked for before the
        /// request: a locked sway answers the frame callback of a window only when the lock
        /// ends, and a window that still waits for one then draws nothing until it does
        /// ([`EndLock::holding_locked_and_frames`]).
        pub fn holding_locked(upstream: PathBuf) -> Self {
            Self::serve(upstream, Mode::Held, 0)
        }

        /// As [`EndLock::locking_late`], but the client gets no answer to a frame callback
        /// until sway has read its first `lock` request: sway's answers so far then come in
        /// one batch with the answer to the wl_display.sync that follows the request, ahead of
        /// it. (Sway under load answered a frame callback 290 ms late, in that batch.)
        pub fn answering_frames_with_lock(upstream: PathBuf) -> Self {
            Self::serve(upstream, Mode::LateFrames, 0)
        }

        /// As [`EndLock::start`], but no lock is ended, the client gets `locked` only once
        /// [`EndLock::send_locked`] is called, and it gets no answer to a frame callback of a
        /// window that is no lock surface until it ends its lock (`unlock_and_destroy` or
        /// `destroy`): the window a locked sway does not show, whose frame callback it answers
        /// only once it shows the window again.
        pub fn holding_locked_and_frames(upstream: PathBuf) -> Self {
            Self::serve(upstream, Mode::HeldFrames, 0)
        }

        /// Waits until sway's answer to a frame callback is kept back
        /// ([`EndLock::answering_frames_with_lock`], [`EndLock::holding_locked_and_frames`]):
        /// `false` if the client's connection ended without one.
        pub fn wait_frame_callback_kept(&self) -> bool {
            let s = self.wire.state.lock().unwrap();
            let s = self
                .wire
                .changed
                .wait_while(s, |(_, closed, kept)| *kept == 0 && !*closed)
                .unwrap();
            s.2 > 0
        }

        /// Lets `locked` through: the ones kept back so far, and those to come.
        pub fn send_locked(&self) {
            self.hold.lock().unwrap().take();
        }

        /// As [`EndLock::start`], but the first `n` locks are refused as Hyprland 0.57.1 does
        /// (`finished` at once, no object for the lock's surfaces) and no confirmed lock is ended.
        pub fn refusing(upstream: PathBuf, n: usize) -> Self {
            Self::serve(upstream, Mode::Refuse, n)
        }

        fn serve(upstream: PathBuf, mode: Mode, refuse: usize) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("wayland-proxy");
            let listener = UnixListener::bind(&path).unwrap();
            let ended = Arc::new(AtomicBool::new(false));
            let wire = Arc::new(Wire::default());
            let (e, w) = (ended.clone(), wire.clone());
            let left = Arc::new(AtomicUsize::new(refuse));
            let (held, hold) = std::io::pipe().unwrap();
            std::thread::spawn(move || {
                for client in listener.incoming().flatten() {
                    let server = UnixStream::connect(&upstream).unwrap();
                    let (e, w, left) = (e.clone(), w.clone(), left.clone());
                    let held = held.try_clone().unwrap();
                    std::thread::spawn(move || {
                        pump(&e, &w, mode, &left, &held, client, server);
                        w.update(|s| s.1 = true);
                    });
                }
            });
            EndLock {
                path,
                ended,
                wire,
                hold: Mutex::new(Some(hold)),
                _dir: dir,
            }
        }

        /// The session lock requests sent so far.
        pub fn requests(&self) -> Vec<&'static str> {
            self.wire.state.lock().unwrap().0.clone()
        }

        /// The session lock requests, once `n` were sent or the client's connection ended.
        pub fn wait_requests(&self, n: usize) -> Vec<&'static str> {
            let s = self.wire.state.lock().unwrap();
            let s = self
                .wire
                .changed
                .wait_while(s, |(r, closed, _)| r.len() < n && !*closed)
                .unwrap();
            s.0.clone()
        }

        /// `finished` was sent after `locked`.
        pub fn ended(&self) -> bool {
            self.ended.load(Ordering::SeqCst)
        }
    }

    fn header(m: &[u8]) -> (u32, u16) {
        let w = |i: usize| u32::from_ne_bytes(m[i..i + 4].try_into().unwrap());
        (w(0), (w(4) & 0xffff) as u16)
    }

    fn arg(m: &[u8], i: usize) -> u32 {
        u32::from_ne_bytes(m[8 + 4 * i..12 + 4 * i].try_into().unwrap())
    }

    /// A message of `obj` with opcode `op` and 32-bit words `args`.
    fn message(obj: u32, op: u16, args: &[u32]) -> Vec<u8> {
        let size = 8 + 4 * args.len() as u32;
        let mut m = Vec::with_capacity(size as usize);
        m.extend_from_slice(&obj.to_ne_bytes());
        m.extend_from_slice(&((size << 16) | u32::from(op)).to_ne_bytes());
        for a in args {
            m.extend_from_slice(&a.to_ne_bytes());
        }
        m
    }

    /// `wl_display.error(object, code, message)`.
    fn error(obj: u32, code: u32, text: &str) -> Vec<u8> {
        let mut s = text.as_bytes().to_vec();
        s.push(0);
        let len = s.len() as u32;
        s.resize(s.len().div_ceil(4) * 4, 0);
        let mut args = vec![obj, code, len];
        args.extend(
            s.chunks(4)
                .map(|c| u32::from_ne_bytes(c.try_into().unwrap())),
        );
        message(WL_DISPLAY, 0, &args)
    }

    /// Whole messages at the front of `buf`.
    fn split(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while buf.len() >= 8 {
            let size = (u32::from_ne_bytes(buf[4..8].try_into().unwrap()) >> 16) as usize;
            if size < 8 || buf.len() < size {
                break;
            }
            out.push(buf.drain(..size).collect());
        }
        out
    }

    fn recv(fd: RawFd, buf: &mut [u8], fds: &mut Vec<RawFd>) -> isize {
        let mut cmsg = [0u64; 32];
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        };
        // SAFETY: msghdr is plain data; every pointer in it outlives the call.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = cmsg.as_mut_ptr().cast();
        msg.msg_controllen = std::mem::size_of_val(&cmsg) as _;
        // SAFETY: as above; the kernel fills at most the lengths given.
        let n = unsafe { libc::recvmsg(fd, &mut msg, libc::MSG_CMSG_CLOEXEC) };
        if n > 0 {
            // SAFETY: walking the control messages the kernel just wrote into `cmsg`.
            let mut c = unsafe { libc::CMSG_FIRSTHDR(&msg) };
            while !c.is_null() {
                // SAFETY: a non-null header CMSG_FIRSTHDR/CMSG_NXTHDR returned lies within `cmsg`.
                let h = unsafe { &*c };
                if h.cmsg_level == libc::SOL_SOCKET && h.cmsg_type == libc::SCM_RIGHTS {
                    // SAFETY: CMSG_LEN only computes a length.
                    let len = h.cmsg_len as usize - unsafe { libc::CMSG_LEN(0) } as usize;
                    // SAFETY: `c` is a valid header; its `len` data bytes follow it in `cmsg`.
                    let p = unsafe { libc::CMSG_DATA(c) }.cast::<RawFd>();
                    for i in 0..len / 4 {
                        // SAFETY: fd `i` of the `len` bytes at `p`, read unaligned.
                        fds.push(unsafe { p.wrapping_add(i).read_unaligned() });
                    }
                }
                // SAFETY: `c` is a header of `msg`'s control buffer.
                c = unsafe { libc::CMSG_NXTHDR(&msg, c) };
            }
        }
        n
    }

    /// Sends `bytes` with `fds` (passed on and closed here) attached to the first chunk.
    fn send(fd: RawFd, mut bytes: &[u8], fds: &mut Vec<RawFd>) {
        while !bytes.is_empty() || !fds.is_empty() {
            let mut cmsg = [0u64; 32];
            let mut iov = libc::iovec {
                iov_base: bytes.as_ptr() as *mut _,
                iov_len: bytes.len(),
            };
            // SAFETY: as in `recv`.
            let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            if !fds.is_empty() {
                let len = std::mem::size_of_val(fds.as_slice()) as u32;
                msg.msg_control = cmsg.as_mut_ptr().cast();
                // SAFETY: CMSG_SPACE of at most 28 fds fits `cmsg`.
                msg.msg_controllen = unsafe { libc::CMSG_SPACE(len) } as _;
                // SAFETY: `msg_controllen` is CMSG_SPACE(len) of `cmsg`, room for one header.
                let c = unsafe { libc::CMSG_FIRSTHDR(&msg) };
                // SAFETY: CMSG_FIRSTHDR of that buffer is non-null and lies within `cmsg`.
                let h = unsafe { &mut *c };
                h.cmsg_level = libc::SOL_SOCKET;
                h.cmsg_type = libc::SCM_RIGHTS;
                // SAFETY: CMSG_LEN only computes a length.
                h.cmsg_len = unsafe { libc::CMSG_LEN(len) } as _;
                // SAFETY: `c` is a valid header; its data follows it within `cmsg`.
                let data = unsafe { libc::CMSG_DATA(c) }.cast::<RawFd>();
                // SAFETY: the `len` bytes of fds fit the data CMSG_SPACE(len) reserved, which is
                // aligned for them (`cmsg` is u64s, headers and data are CMSG_ALIGNed).
                unsafe { std::ptr::copy_nonoverlapping(fds.as_ptr(), data, fds.len()) };
            }
            // SAFETY: as in `recv`.
            let n = unsafe { libc::sendmsg(fd, &msg, libc::MSG_NOSIGNAL) };
            if n < 0 {
                return;
            }
            for f in fds.drain(..) {
                // SAFETY: our received copy of an fd now passed on.
                unsafe { libc::close(f) };
            }
            bytes = &bytes[n as usize..];
        }
    }

    /// Copies both directions of one connection, doing to its session locks what `mode` says;
    /// `refuse` counts the locks still to refuse, and `hold` is readable (at its end) once
    /// [`Mode::Held`] is to let `locked` through.
    fn pump(
        ended: &AtomicBool,
        wire: &Wire,
        mode: Mode,
        refuse: &AtomicUsize,
        hold: &std::io::PipeReader,
        client: UnixStream,
        server: UnixStream,
    ) {
        let (cfd, sfd) = (client.as_raw_fd(), server.as_raw_fd());
        let (mut up, mut down) = (Vec::new(), Vec::new());
        let (mut up_fds, mut down_fds) = (Vec::new(), Vec::new());
        let mut buf = vec![0u8; 1 << 16];
        let mut registries = HashSet::new();
        let mut managers = HashSet::new();
        // Session locks, and whether `locked` (with our `finished`) was sent for them.
        let mut locks: HashMap<u32, bool> = HashMap::new();
        // Refused locks, which sway never saw, and the surface ids asked for on them, which
        // name no object.
        let mut refused = HashSet::new();
        let mut no_object = HashSet::new();
        // Ids of refused locks that sway holds as a wl_callback: its `done` and `delete_id` for
        // them are not the client's to see.
        let (mut hide_done, mut hide_delete) = (HashSet::new(), HashSet::new());
        // [`Mode::Late`]: the wl_surface of each lock surface with its lock, the ones a buffer
        // was attached to, and the locks a surface of which has a frame. `held`: the locks
        // whose `locked` is held back, in [`Mode::Held`] while `holding` is polled.
        let mut lock_surfaces: HashMap<u32, u32> = HashMap::new();
        let (mut attached, mut framed, mut held) = (HashSet::new(), HashSet::new(), HashSet::new());
        let late = matches!(mode, Mode::Late | Mode::LateFrames);
        // [`Mode::LateFrames`] and [`Mode::HeldFrames`], while `keeping`: the wl_compositors
        // and their wl_surfaces (in `HeldFrames` without the lock surfaces), the frame
        // callbacks asked for, the `done` and `delete_id` sway sent for them (`kept`, with the
        // ids whose `delete_id` is still to come), whether `lock` was asked for and the
        // wl_display.sync after it.
        let mut keeping = matches!(mode, Mode::LateFrames | Mode::HeldFrames);
        let (mut compositors, mut surfaces) = (HashSet::new(), HashSet::new());
        let (mut frame_callbacks, mut kept_ids) = (HashSet::new(), HashSet::new());
        let mut kept = Vec::new();
        let (mut locking, mut lock_sync) = (false, None);
        // [`Mode::Late`] and [`Mode::Held`]: the frame callbacks asked for before a `lock`
        // request that sway has yet to answer, and what the client sent from that request on,
        // which sway gets once it has answered them.
        let stalling = matches!(mode, Mode::Late | Mode::Held);
        let mut awaited: HashSet<u32> = HashSet::new();
        let mut stalled: Option<Vec<u8>> = None;
        // poll(2) skips a negative fd.
        let mut holding = if matches!(mode, Mode::Held | Mode::HeldFrames) {
            hold.as_raw_fd()
        } else {
            -1
        };
        loop {
            let mut pfd = [cfd, sfd, holding].map(|fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
            // SAFETY: three valid pollfds.
            if unsafe { libc::poll(pfd.as_mut_ptr(), 3, -1) } < 0 {
                return;
            }
            if pfd[2].revents != 0 {
                holding = -1;
                for lock in held.drain() {
                    send(cfd, &message(lock, LOCKED, &[]), &mut Vec::new());
                }
            }
            if pfd[0].revents != 0 {
                let n = recv(cfd, &mut buf, &mut up_fds);
                if n <= 0 {
                    return;
                }
                up.extend_from_slice(&buf[..n as usize]);
                let mut out = Vec::new();
                let mut fatal = None;
                let mut ending = false;
                for m in split(&mut up) {
                    let (obj, op) = header(&m);
                    if obj == WL_DISPLAY && op == 0 {
                        if mode == Mode::LateFrames && keeping && locking && lock_sync.is_none() {
                            lock_sync = Some(arg(&m, 0));
                        }
                    } else if obj == WL_DISPLAY && op == 1 {
                        registries.insert(arg(&m, 0));
                    } else if registries.contains(&obj) && op == 0 && m.len() >= 16 {
                        // bind(name, interface string, version, new id)
                        let len = arg(&m, 1) as usize;
                        let iface = &m[16..16 + len.saturating_sub(1)];
                        let id = arg(&m, 2 + len.div_ceil(4) + 1);
                        if iface == b"ext_session_lock_manager_v1" {
                            managers.insert(id);
                        } else if iface == b"wl_compositor" {
                            compositors.insert(id);
                        }
                    } else if managers.contains(&obj) && op == MANAGER_LOCK {
                        wire.update(|s| s.0.push("lock"));
                        let left = refuse.load(Ordering::SeqCst);
                        if left > 0 {
                            refuse.store(left - 1, Ordering::SeqCst);
                            let id = arg(&m, 0);
                            refused.insert(id);
                            send(cfd, &message(id, FINISHED, &[]), &mut Vec::new());
                            // Sway takes ids in order, so it gets one of this id: wl_display.sync.
                            hide_done.insert(id);
                            hide_delete.insert(id);
                            out.extend_from_slice(&message(WL_DISPLAY, 0, &[id]));
                            continue;
                        }
                        locks.insert(arg(&m, 0), false);
                        locking = true;
                        if stalling && !frame_callbacks.is_empty() {
                            // Sway reads the `lock` once it has answered these.
                            if !out.is_empty() {
                                send(sfd, &out, &mut up_fds);
                                out.clear();
                            }
                            awaited = frame_callbacks.clone();
                            stalled = Some(Vec::new());
                        }
                    } else if refused.contains(&obj) {
                        match op {
                            LOCK_DESTROY => {
                                wire.update(|s| s.0.push("destroy"));
                                refused.remove(&obj);
                                // wl_display.delete_id: the client may use the id again.
                                send(cfd, &message(WL_DISPLAY, 1, &[obj]), &mut Vec::new());
                            }
                            LOCK_GET_LOCK_SURFACE => {
                                wire.update(|s| s.0.push("get_lock_surface"));
                                no_object.insert(arg(&m, 0));
                            }
                            _ => wire.update(|s| s.0.push("unlock_and_destroy")),
                        }
                        continue;
                    } else if no_object.contains(&obj) {
                        fatal = Some(error(WL_DISPLAY, 0, &format!("invalid object {obj}")));
                        break;
                    } else if let Some(&lock) = lock_surfaces.get(&obj) {
                        if op == SURFACE_ATTACH && arg(&m, 0) != 0 {
                            attached.insert(obj);
                        } else if op == SURFACE_COMMIT && attached.contains(&obj) {
                            // The frame goes to sway first, then the client is told `locked`.
                            out.extend_from_slice(&m);
                            send(sfd, &out, &mut up_fds);
                            out.clear();
                            lock_surfaces.retain(|_, l| *l != lock);
                            attached.remove(&obj);
                            framed.insert(lock);
                            if held.remove(&lock) {
                                send(cfd, &message(lock, LOCKED, &[]), &mut Vec::new());
                            }
                            continue;
                        }
                    } else if let Some(&confirmed) = locks.get(&obj) {
                        if late && op == LOCK_GET_LOCK_SURFACE {
                            lock_surfaces.insert(arg(&m, 1), obj);
                        }
                        if mode == Mode::HeldFrames && op == LOCK_GET_LOCK_SURFACE {
                            surfaces.remove(&arg(&m, 1));
                        }
                        match op {
                            LOCK_DESTROY => wire.update(|s| s.0.push("destroy")),
                            LOCK_UNLOCK_AND_DESTROY => {
                                wire.update(|s| s.0.push("unlock_and_destroy"))
                            }
                            _ => {}
                        }
                        if op == LOCK_DESTROY && confirmed {
                            locks.remove(&obj);
                            out.extend_from_slice(&message(obj, LOCK_UNLOCK_AND_DESTROY, &[]));
                            fatal = Some(error(
                                obj,
                                INVALID_DESTROY,
                                "destroy after the locked event",
                            ));
                            break;
                        }
                        if op == LOCK_DESTROY || op == LOCK_UNLOCK_AND_DESTROY {
                            ending = true;
                            locks.remove(&obj);
                            lock_surfaces.retain(|_, l| *l != obj);
                            framed.remove(&obj);
                            held.remove(&obj);
                        }
                    } else if compositors.contains(&obj) && op == 0 {
                        // create_surface(new id)
                        surfaces.insert(arg(&m, 0));
                    } else if surfaces.contains(&obj) {
                        if op == SURFACE_DESTROY {
                            surfaces.remove(&obj);
                        } else if op == SURFACE_FRAME && (keeping || stalling) {
                            frame_callbacks.insert(arg(&m, 0));
                        }
                    }
                    out.extend_from_slice(&m);
                }
                match &mut stalled {
                    Some(s) => s.append(&mut out),
                    None => send(sfd, &out, &mut up_fds),
                }
                if ending && mode == Mode::HeldFrames && keeping {
                    // The lock ends: the windows are shown again, their frame callbacks answered.
                    send(cfd, &std::mem::take(&mut kept), &mut Vec::new());
                    keeping = false;
                }
                if let Some(e) = fatal {
                    // A protocol error ends the client's connection.
                    send(cfd, &e, &mut Vec::new());
                    return;
                }
            }
            if pfd[1].revents != 0 {
                let n = recv(sfd, &mut buf, &mut down_fds);
                if n <= 0 {
                    return;
                }
                down.extend_from_slice(&buf[..n as usize]);
                let mut out = Vec::new();
                for m in split(&mut down) {
                    let (obj, op) = header(&m);
                    if hide_done.remove(&obj) {
                        continue;
                    }
                    if keeping {
                        if frame_callbacks.remove(&obj) {
                            kept.extend_from_slice(&m);
                            kept_ids.insert(obj);
                            wire.update(|s| s.2 += 1);
                            continue;
                        }
                        if obj == WL_DISPLAY && op == 1 && kept_ids.remove(&arg(&m, 0)) {
                            kept.extend_from_slice(&m);
                            continue;
                        }
                        if lock_sync == Some(obj) {
                            // Sway read `lock`: the frame callbacks are answered, then the sync.
                            out.append(&mut kept);
                            keeping = false;
                        }
                    } else if frame_callbacks.remove(&obj) {
                        awaited.remove(&obj);
                    }
                    if obj == WL_DISPLAY && op == 1 {
                        // delete_id: the id may now name another object.
                        let id = arg(&m, 0);
                        if hide_delete.remove(&id) {
                            continue;
                        }
                        compositors.remove(&id);
                        surfaces.remove(&id);
                        if frame_callbacks.remove(&id) {
                            // Its surface is gone: no answer is coming.
                            awaited.remove(&id);
                        }
                        registries.remove(&id);
                        managers.remove(&id);
                        locks.remove(&id);
                        lock_surfaces.remove(&id);
                        attached.remove(&id);
                    }
                    let hold = match mode {
                        Mode::Late | Mode::LateFrames => !framed.contains(&obj),
                        Mode::Held | Mode::HeldFrames => holding >= 0,
                        _ => false,
                    };
                    if hold && op == LOCKED && locks.contains_key(&obj) {
                        held.insert(obj);
                        continue;
                    }
                    out.extend_from_slice(&m);
                    if mode == Mode::End && op == LOCKED && locks.get(&obj) == Some(&false) {
                        locks.insert(obj, true);
                        out.extend_from_slice(&message(obj, FINISHED, &[]));
                        ended.store(true, Ordering::SeqCst);
                    }
                }
                send(cfd, &out, &mut down_fds);
                if awaited.is_empty()
                    && let Some(s) = stalled.take()
                {
                    send(sfd, &s, &mut up_fds);
                }
            }
        }
    }

    /// The compositor's socket, from WAYLAND_DISPLAY.
    pub fn upstream() -> PathBuf {
        let d = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").unwrap());
        if d.is_absolute() {
            return d;
        }
        Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join(d)
    }
}
