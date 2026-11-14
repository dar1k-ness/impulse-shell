//! A window draws only once the compositor answered its last frame: on a headless sway whose
//! output is powered off (the compositor withholds wl_surface.frame callbacks, as Hyprland does
//! with DPMS off or under a fullscreen window) an animating panel draws nothing, and when the
//! output comes back the withheld callback arrives and the panel shows its current state.
//! Hiding a window destroys its surface, so showing it again draws a fresh surface at once even
//! while the old one's callback is still withheld, and that callback arriving late (through
//! `proxy::Withhold`, which holds wl_callback.done between rusty-impulse and sway) does not count as the
//! new surface's.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use testenv::Log;

/// The tests power the one sway output off and on: one at a time.
static OUTPUT: Mutex<()> = Mutex::new(());

const SHELL: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    id: root
    property bool shown: true
    property color fill: "#ff0000"
    PanelWindow {
        visible: root.shown
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: root.fill
        Rectangle { id: dot; width: 4; height: 4; y: 28; color: "#0000ff" }
        NumberAnimation {
            target: dot; property: "x"; from: 0; to: 60; duration: 1000
            loops: Animation.Infinite; running: true
        }
    }
    IpcHandler {
        target: "fx"
        function paint(): void { root.fill = "#00ff00"; console.log("PAINTED") }
        function nudge(): void { dot.width = dot.width === 4 ? 5 : 4; console.log("NUDGED") }
        function hide(): void { root.shown = false }
        function show(): void { root.shown = true }
    }
}
"##;

fn sway_sock() -> Option<PathBuf> {
    if let Some(s) = std::env::var_os("SWAYSOCK") {
        return Some(s.into());
    }
    let run = std::env::var_os("XDG_RUNTIME_DIR")?;
    std::fs::read_dir(run)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("sway-ipc.") && n.ends_with(".sock"))
        })
}

fn sway() -> Option<PathBuf> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("a headless sway (WAYLAND_DISPLAY)");
        return None;
    }
    if !testenv::require_all(&["swaymsg", "grim"]) {
        return None;
    }
    let sock = sway_sock();
    if sock.is_none() {
        testenv::unavailable("the sway IPC socket");
    }
    sock
}

#[test]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "hidden_window_draws_only_after_its_frame_callback",
        &[("WAYLAND_DISPLAY", None)],
    );
}

/// A sway of the test's own, for a test that times the frames of an animation. The shared one
/// serves the windows of the tests that run beside this one and then answers a frame late:
/// with a second `-p shell` run beside it the wl_callback.done came 77 to 720 ms after the
/// request, or not while the animation ran, instead of within a refresh interval. The shell
/// sends no frame while one is unanswered, so its animation ended late or sent fewer frames.
fn own_sway() -> Option<testenv::Sway> {
    testenv::require_all(&["sway", "swaymsg"]).then(testenv::Sway::start)
}

fn swaymsg(sock: &Path, cmd: &str) {
    let st = Command::new("swaymsg")
        .env("SWAYSOCK", sock)
        .arg(cmd)
        .stdout(Stdio::null())
        .status()
        .expect("swaymsg");
    assert!(st.success(), "swaymsg {cmd}");
}

/// HEADLESS-1 powered off; on again on drop.
struct PowerOff<'a>(&'a Path);
impl<'a> PowerOff<'a> {
    fn new(sock: &'a Path) -> Self {
        swaymsg(sock, "output HEADLESS-1 power off");
        PowerOff(sock)
    }
}
impl Drop for PowerOff<'_> {
    fn drop(&mut self) {
        swaymsg(self.0, "output HEADLESS-1 power on");
    }
}

/// rusty-impulse running `shell.qml` with its draws, frame callbacks and Wayland traffic logged.
struct Shell {
    child: Child,
    dir: tempfile::TempDir,
    log: Arc<Log>,
    display: Option<PathBuf>,
}

impl Shell {
    fn start() -> Self {
        Self::start_on(None)
    }

    /// Like `start`, connected to the Wayland socket `display` instead of sway's.
    fn start_on(display: Option<&Path>) -> Self {
        Self::start_qml(SHELL, display)
    }

    /// Like `start_on`, running the shell `qml`.
    fn start_qml(qml: &str, display: Option<&Path>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("shell.qml"), qml).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        let xdg = dir.path().join("xdg");
        let child = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env(
                "RI_LOG",
                "ri::frames=debug,ri::sched=debug,ri::present=debug",
            )
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdout(Stdio::null())
            .stderr(w)
            .envs(display.map(|d| ("WAYLAND_DISPLAY", d)))
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
        self.log.text()
    }

    fn draws(&self) -> usize {
        self.text()
            .lines()
            .filter(|l| l.contains("draw win"))
            .count()
    }

    /// Waits for `f(log)`; panics if rusty-impulse exits first.
    fn wait(&mut self, what: &str, f: impl Fn(&str) -> bool) {
        if let Err(s) = self.log.wait(f) {
            let keep: Vec<&str> = s
                .lines()
                .filter(|l| {
                    l.contains("ri::frames") || l.contains("qml:") || l.contains("layer_surface")
                })
                .collect();
            panic!(
                "rusty-impulse exited before {what}; rusty-impulse log tail:\n{}",
                keep[keep.len().saturating_sub(40)..].join("\n")
            );
        }
    }

    /// Waits until the loop blocks with nothing due (ri::sched logs each turn's next deadline as `t=… next …`)
    /// after the last line `mark` matches: whatever that line started has run its course.
    fn wait_blocked_after(&mut self, what: &str, mark: impl Fn(&str) -> bool) {
        self.wait(what, |s| {
            let lines: Vec<&str> = s.lines().collect();
            let turn = lines
                .iter()
                .rposition(|l| l.trim_start().starts_with("t=") && l.contains(" next "));
            let m = lines.iter().rposition(|l| mark(l));
            turn.is_some_and(|t| lines[t].contains(" next None") && m.is_none_or(|m| m < t))
        });
    }

    /// Waits until a commit made after the last line `since` matches (or any commit) is on
    /// screen: the wp_presentation_feedback rusty-impulse requested for it (under ri::present, logged by
    /// WAYLAND_DEBUG) was `presented`. A feedback ends with its `presented` or `discarded` and
    /// only then may its id be reused, so the first of those after the request is its own.
    fn wait_presented(&mut self, what: &str, since: impl Fn(&str) -> bool) {
        self.wait(what, |s| {
            let lines: Vec<&str> = s.lines().collect();
            let from = lines.iter().rposition(|l| since(l)).map_or(0, |m| m + 1);
            (from..lines.len()).any(|i| {
                let Some(id) = lines[i]
                    .split_once("-> wp_presentation#")
                    .and_then(|(_, r)| r.split_once("new id wp_presentation_feedback#"))
                    .and_then(|(_, r)| r.split(')').next())
                else {
                    return false;
                };
                let (presented, discarded) = (
                    format!("wp_presentation_feedback#{id}.presented("),
                    format!("wp_presentation_feedback#{id}.discarded("),
                );
                lines[i + 1..]
                    .iter()
                    .find(|l| l.contains(&presented) || l.contains(&discarded))
                    .is_some_and(|l| l.contains(&presented))
            })
        });
    }

    /// Wakes the loop with the panel changed (the `nudge` ipc call) and waits until it blocks
    /// again: the panel was due a frame (`skip win0 pending`) but drew nothing, as its frame
    /// callback is still outstanding.
    fn assert_waits_for_callback(&mut self, what: &str) {
        let nudges = |s: &str| s.lines().filter(|l| nudged(l)).count();
        let before = nudges(&self.text());
        self.ipc("nudge");
        self.wait("the nudge handled", |s| nudges(s) > before);
        self.wait_blocked_after("the loop blocked after the nudge", |l| {
            nudged(l) || l.contains("draw win")
        });
        let log = self.text();
        let lines: Vec<&str> = log.lines().collect();
        let after = &lines[lines.iter().rposition(|l| nudged(l)).unwrap()..];
        assert!(
            after.iter().any(|l| l.contains("skip win0 pending")),
            "the nudged panel was not due a frame"
        );
        let n = after.iter().filter(|l| l.contains("draw win")).count();
        assert_eq!(n, 0, "the panel drew {n} frames {what}");
    }

    fn wait_draws(&mut self, more_than: usize, what: &str) {
        self.wait(what, |s| {
            s.lines().filter(|l| l.contains("draw win")).count() > more_than
        });
    }

    fn ipc(&self, func: &str) {
        let st = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .arg("-p")
            .arg(self.dir.path())
            .args(["ipc", "call", "fx", func])
            .envs(self.display.as_deref().map(|d| ("WAYLAND_DISPLAY", d)))
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "ipc call fx {func}");
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The RGB pixel at `x,y` of HEADLESS-1.
fn pixel(dir: &Path, x: usize, y: usize) -> Option<[u8; 3]> {
    let f = dir.join("shot.ppm");
    let ok = Command::new("grim")
        .args(["-t", "ppm", "-o", "HEADLESS-1"])
        .arg(&f)
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let b = std::fs::read(&f).ok()?;
    // P6\n<w> <h>\n255\n<rgb>
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while b.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        let s = i;
        while !b.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        fields.push(String::from_utf8_lossy(&b[s..i]).into_owned());
    }
    let w: usize = fields[1].parse().ok()?;
    let o = i + 1 + (y * w + x) * 3;
    Some([*b.get(o)?, *b.get(o + 1)?, *b.get(o + 2)?])
}

fn assert_pixel(dir: &Path, want: [u8; 3]) {
    assert_eq!(pixel(dir, 10, 10), Some(want), "panel pixel");
}

/// The `paint` ipc call's log line: frames drawn after it are green.
fn painted(l: &str) -> bool {
    l.contains("PAINTED")
}

/// The `nudge` ipc call's log line.
fn nudged(l: &str) -> bool {
    l.contains("NUDGED")
}

#[test]
fn hidden_window_draws_only_after_its_frame_callback() {
    let Some(sock) = sway() else { return };
    let _one = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    let mut sh = Shell::start();
    sh.wait("the panel's first frame callback", |s| {
        s.contains("cb win0")
    });
    sh.wait_draws(3, "the animation drawing");
    let off = PowerOff::new(&sock);
    // The ipc call is handled after any callback sway sent before the output went off, so once
    // the loop blocks after it, a frame that callback allowed has been drawn.
    sh.ipc("paint");
    sh.wait_blocked_after("the frame drawn before the output went off", |l| {
        l.contains("draw win")
    });
    sh.assert_waits_for_callback("with no frame callback (output off)");
    let before = sh.text().lines().count();
    drop(off);
    // The withheld callback comes back and the panel draws its current state.
    sh.wait("the withheld callback", |s| {
        s.lines().skip(before).any(|l| l.contains("cb win0"))
    });
    sh.wait_presented("the green panel shown", painted);
    assert_pixel(sh.dir.path(), [0, 255, 0]);
    let n = sh.draws();
    sh.wait_draws(n + 3, "the animation drawing again");
}

/// A panel whose only animation moves an item nobody sees: its frames change no pixel.
const UNSEEN: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: "#ff0000"
        Rectangle { id: dot; width: 4; height: 4; opacity: 0; color: "#0000ff" }
        NumberAnimation {
            target: dot; property: "x"; from: 0; to: 60; duration: 1000
            loops: Animation.Infinite; running: true
        }
    }
}
"##;

/// An endless animation whose frames change no pixel sends the compositor nothing, so no
/// frame callback paces it: its steps go by the vsync timer, one per refresh interval, not
/// one after another as fast as the loop turns.
#[test]
fn animation_frames_without_damage_go_by_the_vsync_timer() {
    let Some(_sock) = sway() else { return };
    let _one = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    let mut sh = Shell::start_qml(UNSEEN, None);
    let drawn = |s: &str| s.lines().filter(|l| l.contains("draw win0")).count();
    // The first frames show the panel; the animation's own change nothing.
    sh.wait("the animation's first frames", |s| drawn(s) > 5);
    let from = sh.text().lines().count();
    // One more than is counted: the loop turn of the last one counted is then logged.
    sh.wait("30 frames of the animation", |s| {
        s.lines()
            .skip(from)
            .filter(|l| l.contains("draw win0"))
            .count()
            > 31
    });
    let text = sh.text();
    let lines: Vec<&str> = text.lines().skip(from).collect();
    let draws: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains("draw win0"))
        .collect();
    let sent = |req: &str| {
        (lines[draws[0]..draws[30]].iter())
            .filter(|l| l.contains("-> wl_surface#") && l.contains(req))
            .count()
    };
    assert_eq!(
        (sent(".commit("), sent(".frame(")),
        (0, 0),
        "commits and frame requests of 30 frames that change no pixel"
    );
    let took = turn(&lines, draws[30]) - turn(&lines, draws[0]);
    // 30 refresh intervals of sway's 60 Hz headless output are 500 ms; a busy machine only
    // makes them late.
    assert!(
        (450.0..1000.0).contains(&took),
        "30 frames that change no pixel took {took:.0} ms"
    );
}

/// A panel that is one list of four rows, two of them in view. The first row fades a mark in
/// and out without end and says the mark's opacity ten times a second; `away` scrolls that row
/// out of the list, `back` scrolls it in again.
const CULLED: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: "#ff0000"
        ListView {
            id: list
            anchors.fill: parent
            clip: true
            model: 4
            delegate: Rectangle {
                id: row
                required property int index
                width: 64; height: 16
                color: "#00ff00"
                Rectangle {
                    id: mark
                    width: 8; height: 8; color: "#0000ff"
                    NumberAnimation on opacity {
                        running: row.index === 0
                        from: 0; to: 1; duration: 700; loops: Animation.Infinite
                    }
                }
                Timer {
                    running: row.index === 0; interval: 100; repeat: true
                    onTriggered: console.log("TICK " + mark.opacity.toFixed(3))
                }
            }
        }
    }
    IpcHandler {
        target: "fx"
        function away(): void { list.contentY = 32; console.log("AWAY") }
        function back(): void { list.contentY = 0; console.log("BACK") }
    }
}
"##;

/// QQuickItemView culls a delegate that lies outside the view (QQuickItemPrivate::setCulled):
/// it is not painted. An animation inside such a row still runs, and changes no pixel of the
/// window, so the window is not drawn for it; it is drawn again once the row is back in view.
#[test]
fn an_animation_in_a_row_scrolled_out_of_its_list_draws_no_frame() {
    let Some(_sock) = sway() else { return };
    let _one = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    let mut sh = Shell::start_qml(CULLED, None);
    let n = sh.draws();
    sh.wait_draws(n + 5, "the mark fading in view");
    sh.ipc("away");
    let ticks = |s: &str, mark: &str| {
        let (_, after) = s.split_once(mark).unwrap_or_default();
        after.lines().filter(|l| l.contains("TICK ")).count()
    };
    sh.wait("eight ticks with the row out of view", |s| {
        ticks(s, "AWAY") >= 8
    });
    let text = sh.text();
    let lines: Vec<&str> = text.lines().collect();
    let away = lines.iter().position(|l| l.contains("AWAY")).unwrap();
    let tick: Vec<usize> = (away..lines.len())
        .filter(|&i| lines[i].contains("TICK "))
        .collect();
    // The frame that scrolls the row out is drawn before the second tick after it.
    let quiet = &lines[tick[1]..tick[7]];
    let said: std::collections::BTreeSet<&str> = (tick[1..8].iter())
        .filter_map(|&i| lines[i].split("TICK ").nth(1))
        .collect();
    assert!(
        said.len() > 3,
        "the animation out of view stood still: {said:?}"
    );
    let drawn = quiet.iter().filter(|l| l.contains("draw win0")).count();
    assert_eq!(
        drawn, 0,
        "frames drawn in 600 ms for an animation in a row out of view"
    );
    sh.ipc("back");
    sh.wait("the row back in view", |s| s.contains("BACK"));
    let n = sh.draws();
    sh.wait_draws(n + 5, "the mark fading in view again");
}

/// A panel that on `go` starts a 500 ms animation and a 50 ms repeating Timer.
const FINITE: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: "#ff0000"
        Rectangle { id: dot; width: 4; height: 4; y: 28; color: "#0000ff" }
        NumberAnimation {
            id: move
            target: dot; property: "x"; from: 0; to: 60; duration: 500
            onStarted: console.log("STARTED")
            onFinished: console.log("FINISHED at " + dot.x)
        }
        Timer { id: beat; interval: 50; repeat: true; onTriggered: console.log("BEAT") }
    }
    IpcHandler {
        target: "fx"
        function go(): void { beat.start(); move.start() }
    }
}
"##;

/// The compositor withholds the frame callback of a window it does not show and does not say
/// so. A transition started then still ends, and Timers still fire: after QWaylandWindow's
/// 100 ms the window counts as not exposed and its animation steps on the vsync timer, without
/// drawing. The transition takes its 500 ms and at most those 100 ms more: no less by the
/// clock, and no more by the waits the shell asked for ([`waited`]), which a busy machine does
/// not stretch.
#[test]
fn a_transition_ends_and_timers_run_with_the_output_off() {
    let Some(own) = own_sway() else { return };
    let sock = own.ipc_socket();
    let mut sh = Shell::start_qml(FINITE, Some(own.display()));
    sh.wait("the panel drawn", |s| s.contains("draw win0"));
    let off = PowerOff::new(&sock);
    sh.ipc("go");
    sh.wait("the animation ended with the output off", |s| {
        // And the loop turn after it is logged: its time is read below.
        let (_, after) = s.split_once("FINISHED").unwrap_or_default();
        after.lines().any(|l| l.trim_start().starts_with("t="))
    });
    let text = sh.text();
    let lines: Vec<&str> = text.lines().collect();
    let at = |what: &str| lines.iter().position(|l| l.contains(what)).unwrap();
    let (start, end) = (at("STARTED"), at("FINISHED"));
    assert!(lines[end].contains("FINISHED at 60"), "{}", lines[end]);
    let took = turn(&lines, end) - turn(&lines, start);
    let during = &lines[start..end];
    let waits = waited(during);
    println!(
        "the 500 ms transition took {took:.0} ms with the output off, its waits {waits:.0} ms"
    );
    assert!(
        took >= 480.0 && waits < 700.0,
        "the 500 ms transition took {took:.0} ms, its waits {waits:.0} ms"
    );
    let beats = during.iter().filter(|l| l.contains("BEAT")).count();
    assert!(
        beats >= 8,
        "the 50 ms Timer fired {beats} times in {took:.0} ms"
    );
    // The frame that shows the start may still be drawn; none follows without its callback.
    let draws = during.iter().filter(|l| l.contains("draw win0")).count();
    assert!(draws <= 1, "{draws} frames drawn with the output off");
    // One loop turn per vsync step and per Timer beat, not a busy loop.
    let turns = during.iter().filter(|l| l.contains(" next ")).count();
    assert!(
        turns < 80,
        "{turns} loop turns in {took:.0} ms:\n{}",
        during
            .iter()
            .filter(|l| !l.contains("wl_") && !l.contains("wp_") && !l.contains("zwlr"))
            .skip(20)
            .take(70)
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    );
    drop(off);
    sh.wait("the panel drawn again with the output on", |s| {
        s.lines().skip(end).any(|l| l.contains("draw win0"))
    });
    // The same transition with the output on, for the time next to it: drawn frame by frame.
    sh.ipc("go");
    sh.wait("the animation ended with the output on", |s| {
        // And the loop turn after it is logged: its time is read below.
        let (before, after) = s.rsplit_once("FINISHED").unwrap_or_default();
        before.contains("FINISHED") && after.lines().any(|l| l.trim_start().starts_with("t="))
    });
    let text = sh.text();
    let lines: Vec<&str> = text.lines().collect();
    let nth = |what: &str| lines.iter().rposition(|l| l.contains(what)).unwrap();
    let (start, end) = (nth("STARTED"), nth("FINISHED"));
    let shown = turn(&lines, end) - turn(&lines, start);
    let waits = waited(&lines[start..end]);
    let draws = (lines[start..end].iter())
        .filter(|l| l.contains("draw win0"))
        .count();
    println!(
        "with the output on it took {shown:.0} ms, its waits {waits:.0} ms, {draws} frames drawn"
    );
    // The animation clock steps one refresh interval per frame callback, and sway's
    // headless output paces those a little off 60 Hz: about 30 frames in about 500 ms.
    assert!(
        shown >= 400.0 && waits < 700.0 && draws >= 20,
        "{shown:.0} ms, waits {waits:.0} ms, {draws} frames"
    );
}

/// A panel whose item on `go` fades out in 100 ms and keeps moving for 500 ms: the last 400 ms
/// of the transition change no pixel (a popup notification sliding out once it is transparent).
const TAIL: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: "#ff0000"
        Rectangle {
            id: dot; width: 4; height: 4; y: 28; color: "#0000ff"
            onXChanged: console.log("STEP " + x)
            onOpacityChanged: if (opacity === 0) console.log("GONE")
        }
        ParallelAnimation {
            id: leave
            NumberAnimation { target: dot; property: "opacity"; from: 1; to: 0; duration: 100 }
            NumberAnimation { target: dot; property: "x"; from: 0; to: 60; duration: 500 }
            onRunningChanged: console.log("RUNNING " + running)
            onStarted: console.log("STARTED")
            onStopped: console.log("STOPPED")
            onFinished: console.log("FINISHED at " + dot.x + " opacity " + dot.opacity)
        }
    }
    IpcHandler {
        target: "fx"
        function go(): void { leave.start() }
    }
}
"##;

/// A step of a running animation that changes no pixel sends the compositor nothing: no
/// wl_surface.commit and no wl_surface.frame, so the compositor neither answers nor
/// recomposites. The steps go on at the refresh interval, each one seen by QML (`onXChanged`),
/// and the transition ends in the time and with the signals Qt 6.12 gives it
/// (scenes/qt_frames/unchanged_tail.sh). Three times over.
#[test]
fn an_unchanged_frame_is_not_committed_and_the_transition_ends_on_time() {
    let Some(own) = own_sway() else { return };
    let mut sh = Shell::start_qml(TAIL, Some(own.display()));
    // Nothing animates yet, so the panel's first frame asks for no frame callback.
    sh.wait("the panel's first frame", |s| s.contains("draw win0"));
    for run in 1..=3 {
        sh.wait_blocked_after("the loop blocked before the transition", |l| {
            l.contains("draw win") || l.contains("FINISHED")
        });
        let from = sh.text().lines().count();
        sh.ipc("go");
        sh.wait("the transition ended", |s| {
            // And the loop turn after it is logged: its time is read below.
            let mut after = s.lines().skip(from).skip_while(|l| !l.contains("FINISHED"));
            after.any(|l| l.trim_start().starts_with("t="))
        });
        let text = sh.text();
        let lines: Vec<&str> = text.lines().skip(from).collect();
        let at = |what: &str| lines.iter().position(|l| l.contains(what)).unwrap();
        let (start, gone, end) = (at("STARTED"), at("GONE"), at("FINISHED"));
        assert!(
            lines[end].contains("FINISHED at 60 opacity 0"),
            "{}",
            lines[end]
        );
        let took = turn(&lines, end) - turn(&lines, start);
        // The frame that shows the item gone is the last one with anything new on it.
        let shown = gone
            + (lines[gone..end].iter())
                .position(|l| l.contains("-> wl_surface#") && l.contains(".commit("))
                .expect("the frame without the item was not committed");
        let tail = &lines[shown + 1..end];
        let sent = |req: &str| {
            (tail.iter())
                .filter(|l| l.contains("-> wl_surface#") && l.contains(req))
                .count()
        };
        let (commits, frames) = (sent(".commit("), sent(".frame("));
        let steps = tail.iter().filter(|l| l.contains("STEP ")).count();
        let turns = tail.iter().filter(|l| l.contains(" next ")).count();
        println!(
            "run {run}: the 500 ms transition took {took:.0} ms; once the item was gone: \
             {commits} commits, {frames} frame requests, {steps} steps, {turns} loop turns"
        );
        assert_eq!(
            (commits, frames),
            (0, 0),
            "run {run}: commits and frame requests of the {steps} steps that change no pixel"
        );
        // The driver's first step comes with the start and one more with the first frame, so
        // 28 refresh intervals are left: 467 ms and up to one more interval for the first
        // frame callback. Qt 6.12 takes 514 to 543 ms over it under the same sway. A step
        // too many ends it early; a busy machine only makes it late (590 and 675 ms seen).
        assert!(
            (460.0..750.0).contains(&took),
            "run {run}: the 500 ms transition took {took:.0} ms"
        );
        // One step per refresh interval for the 400 ms left, each one a change QML hears of,
        // and one loop turn per step: not a busy loop.
        assert!(
            (20..=30).contains(&steps),
            "run {run}: {steps} steps in the last 400 ms"
        );
        assert!(
            turns <= steps + 10,
            "run {run}: {turns} loop turns for {steps} steps"
        );
        // The signals, in Qt's order.
        let order: Vec<&str> = (lines[..=end].iter())
            .filter_map(|l| {
                [
                    "RUNNING true",
                    "RUNNING false",
                    "STARTED",
                    "STOPPED",
                    "FINISHED",
                ]
                .into_iter()
                .find(|s| l.contains(s))
            })
            .collect();
        assert_eq!(
            order,
            [
                "STARTED",
                "RUNNING true",
                "STOPPED",
                "RUNNING false",
                "FINISHED"
            ],
            "run {run}"
        );
    }
}

/// A panel whose item on `go` moves 6 px in a second, a whole pixel at a time: most of the 60
/// frames change no pixel, and those that do come between them.
const SLOW: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        id: panel
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: "#ff0000"
        property real pos
        Rectangle { width: 4; height: 4; y: 28; x: Math.round(panel.pos); color: "#0000ff" }
        NumberAnimation {
            id: creep
            target: panel; property: "pos"; from: 0; to: 6; duration: 1000
            onStarted: console.log("STARTED")
            onFinished: console.log("FINISHED at " + panel.pos)
        }
    }
    IpcHandler {
        target: "fx"
        function go(): void { creep.start() }
    }
}
"##;

/// Frames that are sent wait for their frame callback and those that change nothing go by
/// the vsync timer; an animation that keeps going from one kind to the other still takes its
/// own time, a second, and sends only the frames that moved the item. No less than its time by
/// the clock, and no more by the waits the shell asked for ([`waited`]).
#[test]
fn an_animation_between_sent_and_unchanged_frames_ends_on_time() {
    let Some(own) = own_sway() else { return };
    let mut sh = Shell::start_qml(SLOW, Some(own.display()));
    sh.wait("the panel's first frame", |s| s.contains("draw win0"));
    sh.wait_blocked_after("the loop blocked before the animation", |l| {
        l.contains("draw win")
    });
    let from = sh.text().lines().count();
    sh.ipc("go");
    sh.wait("the animation ended", |s| {
        let mut after = s.lines().skip(from).skip_while(|l| !l.contains("FINISHED"));
        after.any(|l| l.trim_start().starts_with("t="))
    });
    let text = sh.text();
    let lines: Vec<&str> = text.lines().skip(from).collect();
    let at = |what: &str| lines.iter().position(|l| l.contains(what)).unwrap();
    let (start, end) = (at("STARTED"), at("FINISHED"));
    assert!(lines[end].contains("FINISHED at 6"), "{}", lines[end]);
    let took = turn(&lines, end) - turn(&lines, start);
    let commits = (lines[start..end].iter())
        .filter(|l| l.contains("-> wl_surface#") && l.contains(".commit("))
        .count();
    let waits = waited(&lines[start..end]);
    println!("the 1000 ms animation took {took:.0} ms, its waits {waits:.0} ms, {commits} commits");
    // A step too many for each frame sent ends it early: 837 ms with two. A busy machine
    // only makes it late; Qt 6.12 takes 1191 to 1234 ms over it under the same sway.
    assert!(
        took >= 950.0 && waits < 1300.0,
        "the 1000 ms animation took {took:.0} ms, its waits {waits:.0} ms ({commits} commits)"
    );
    // The item moves six times.
    assert!(
        (6..=8).contains(&commits),
        "{commits} commits for an item that moved 6 times"
    );
}

/// The refresh interval of sway's 60 Hz headless output.
const REFRESH_MS: f64 = 1000.0 / 60.0;

/// The time the waits for the frames of an animation in `lines` add up to by the shell's own
/// count, in ms: a refresh interval for each frame callback it got (`cb win`), and the time it
/// asked for where it timed the wait itself, on the vsync timer or until it gave a frame
/// callback up (ri::sched: `t=<loop ms> next Some((<deadline ms>, Frame))`, from the first turn
/// that names the deadline).
///
/// A busy machine draws a frame slowly and wakes the loop late. That adds to the time an
/// animation takes by the clock, so the clock tells an animation that ended early and not one
/// that ended late. It adds nothing here: an animation that waits longer or more often than it
/// should is told by this sum on any machine.
///
/// Upper bounds compare this sum, not the clock: an end late by the clock alone is not caught here.
fn waited(lines: &[&str]) -> f64 {
    let mut sum = 0.0;
    let mut armed: Option<(f64, f64)> = None;
    for l in lines {
        if l.starts_with("cb win") {
            armed = None;
            sum += REFRESH_MS;
            continue;
        }
        let turn = (l.trim_start().strip_prefix("t="))
            .and_then(|l| l.strip_suffix(", Frame))"))
            .and_then(|l| l.split_once(" next Some(("));
        let Some((now, at)) = turn else { continue };
        let (now, at): (f64, f64) = (now.parse().unwrap(), at.parse().unwrap());
        if armed.is_none_or(|(_, last)| last != at) {
            sum += armed.map_or(0.0, |(now, at)| at - now);
            armed = Some((now, at));
        }
    }
    sum + armed.map_or(0.0, |(now, at)| at - now)
}

/// The loop time of the first turn at or after line `from` (ri::sched: `t=<loop ms> next …`).
fn turn(lines: &[&str], from: usize) -> f64 {
    lines[from..]
        .iter()
        .find_map(|l| {
            l.trim_start()
                .strip_prefix("t=")?
                .split(' ')
                .next()?
                .parse()
                .ok()
        })
        .unwrap()
}

#[test]
fn window_shown_again_while_its_callback_is_withheld_draws() {
    let Some(sock) = sway() else { return };
    let _one = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    let mut sh = Shell::start();
    sh.wait("the panel's first frame callback", |s| {
        s.contains("cb win0")
    });
    sh.wait_draws(3, "the animation drawing");
    let off = PowerOff::new(&sock);
    sh.wait_blocked_after("the frame drawn before the output went off", |l| {
        l.contains("draw win")
    });
    let n = sh.draws();
    sh.ipc("hide");
    sh.wait("the hidden panel's surface destroyed", |s| {
        s.lines()
            .any(|l| l.contains("-> zwlr_layer_surface_v1") && l.contains(".destroy()"))
    });
    sh.ipc("paint");
    sh.ipc("show");
    // A new surface: its first frame needs no callback.
    sh.wait_draws(n, "the shown panel's first frame with the output still off");
    drop(off);
    sh.wait_presented("the green panel shown", painted);
    assert_pixel(sh.dir.path(), [0, 255, 0]);
    let n = sh.draws();
    sh.wait_draws(n + 3, "the animation drawing again");
}

/// A Wayland proxy between rusty-impulse and sway that can withhold wl_surface.frame callbacks: it
/// forwards every message and fd unchanged, except the done events of frame callbacks (and the
/// delete_id that frees them) while holding, which it delivers later on request.
mod proxy {
    use std::collections::{HashMap, HashSet};
    use std::os::fd::{AsRawFd, RawFd};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Condvar, LockResult, Mutex, MutexGuard};

    const WL_DISPLAY: u32 = 1;

    /// One client connection's object bookkeeping: just enough to know which wl_callbacks are
    /// frame callbacks and of which surface. Surfaces get serials, as their ids are reused.
    struct Conn {
        client: UnixStream,
        registries: HashSet<u32>,
        compositors: HashSet<u32>,
        surfaces: HashMap<u32, u64>,
        callbacks: HashMap<u32, u64>,
        /// Held callbacks whose delete_id is held with them.
        held_ids: HashMap<u32, u64>,
        /// Held events: the surface serial, the callback id and the message.
        held: Vec<(u64, u32, Vec<u8>)>,
    }

    #[derive(Default)]
    struct Shared {
        holding: bool,
        alive: HashSet<u64>,
        next: u64,
        conns: Vec<Conn>,
    }

    /// [`Shared`], with a condition signalled whenever a callback is held.
    #[derive(Default)]
    struct Locked {
        s: Mutex<Shared>,
        held: Condvar,
    }

    impl Locked {
        fn lock(&self) -> LockResult<MutexGuard<'_, Shared>> {
            self.s.lock()
        }
    }

    pub struct Withhold {
        pub path: PathBuf,
        shared: Arc<Locked>,
        _dir: tempfile::TempDir,
    }

    impl Withhold {
        /// Listens on a fresh socket and connects each client to the compositor at `upstream`.
        pub fn start(upstream: PathBuf) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("wayland-proxy");
            let listener = UnixListener::bind(&path).unwrap();
            let shared = Arc::new(Locked::default());
            let sh = shared.clone();
            std::thread::spawn(move || {
                for client in listener.incoming().flatten() {
                    let server = UnixStream::connect(&upstream).unwrap();
                    let k = {
                        let mut g = sh.lock().unwrap();
                        g.conns.push(Conn {
                            client: client.try_clone().unwrap(),
                            registries: HashSet::new(),
                            compositors: HashSet::new(),
                            surfaces: HashMap::new(),
                            callbacks: HashMap::new(),
                            held_ids: HashMap::new(),
                            held: Vec::new(),
                        });
                        g.conns.len() - 1
                    };
                    let sh = sh.clone();
                    std::thread::spawn(move || pump(sh, k, client, server));
                }
            });
            Withhold {
                path,
                shared,
                _dir: dir,
            }
        }

        /// Frame callbacks answered from now on are held.
        pub fn hold(&self) {
            self.shared.lock().unwrap().holding = true;
        }

        /// The held callbacks of live surfaces and of destroyed ones.
        pub fn held(&self) -> (Vec<u32>, Vec<u32>) {
            held(&self.shared.lock().unwrap())
        }

        /// Blocks until a live surface's frame callback is held.
        pub fn wait_held(&self) {
            let g = self.shared.lock().unwrap();
            drop(
                self.shared
                    .held
                    .wait_while(g, |g| held(g).0.is_empty())
                    .unwrap(),
            );
        }
        /// Delivers the held callbacks of destroyed surfaces; returns their ids.
        pub fn release_destroyed(&self) -> Vec<u32> {
            let mut g = self.shared.lock().unwrap();
            let Shared { alive, conns, .. } = &mut *g;
            let mut ids = Vec::new();
            for c in conns {
                let (gone, keep): (Vec<_>, Vec<_>) =
                    c.held.drain(..).partition(|(s, ..)| !alive.contains(s));
                c.held = keep;
                for (_, id, m) in gone {
                    if header(&m).0 != WL_DISPLAY {
                        ids.push(id);
                    }
                    send(c.client.as_raw_fd(), &m, &mut Vec::new());
                }
            }
            ids
        }

        /// Delivers every held callback and stops holding.
        pub fn release_all(&self) {
            let mut g = self.shared.lock().unwrap();
            g.holding = false;
            for c in &mut g.conns {
                for (_, _, m) in std::mem::take(&mut c.held) {
                    send(c.client.as_raw_fd(), &m, &mut Vec::new());
                }
            }
        }
    }

    /// The held callbacks of live surfaces and of destroyed ones.
    fn held(g: &Shared) -> (Vec<u32>, Vec<u32>) {
        let mut out = (Vec::new(), Vec::new());
        for c in &g.conns {
            for (serial, id, m) in &c.held {
                if header(m).0 != WL_DISPLAY {
                    let v = if g.alive.contains(serial) {
                        &mut out.0
                    } else {
                        &mut out.1
                    };
                    v.push(*id);
                }
            }
        }
        out
    }

    fn header(m: &[u8]) -> (u32, u16) {
        let w = |i: usize| u32::from_ne_bytes(m[i..i + 4].try_into().unwrap());
        (w(0), (w(4) & 0xffff) as u16)
    }

    fn arg(m: &[u8], i: usize) -> u32 {
        u32::from_ne_bytes(m[8 + 4 * i..12 + 4 * i].try_into().unwrap())
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

    /// Copies both directions of one connection, noting frame callbacks on the way up and
    /// holding their answers on the way down.
    fn pump(shared: Arc<Locked>, k: usize, client: UnixStream, server: UnixStream) {
        let (cfd, sfd) = (client.as_raw_fd(), server.as_raw_fd());
        let (mut up, mut down) = (Vec::new(), Vec::new());
        let (mut up_fds, mut down_fds) = (Vec::new(), Vec::new());
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let mut pfd = [
                libc::pollfd {
                    fd: cfd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: sfd,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: two valid pollfds.
            if unsafe { libc::poll(pfd.as_mut_ptr(), 2, -1) } < 0 {
                return;
            }
            if pfd[0].revents != 0 {
                let n = recv(cfd, &mut buf, &mut up_fds);
                if n <= 0 {
                    return;
                }
                up.extend_from_slice(&buf[..n as usize]);
                let mut out = Vec::new();
                let mut g = shared.lock().unwrap();
                let Shared {
                    alive, next, conns, ..
                } = &mut *g;
                let c = &mut conns[k];
                for m in split(&mut up) {
                    let (obj, op) = header(&m);
                    if obj == WL_DISPLAY && op == 1 {
                        c.registries.insert(arg(&m, 0));
                    } else if c.registries.contains(&obj) && op == 0 {
                        // bind(name, interface string, version, new id)
                        let len = arg(&m, 1) as usize;
                        let iface = &m[16..16 + len.saturating_sub(1)];
                        let id = arg(&m, 2 + len.div_ceil(4) + 1);
                        if iface == b"wl_compositor" {
                            c.compositors.insert(id);
                        }
                    } else if c.compositors.contains(&obj) && op == 0 {
                        *next += 1;
                        c.surfaces.insert(arg(&m, 0), *next);
                        alive.insert(*next);
                    } else if let Some(&serial) = c.surfaces.get(&obj) {
                        match op {
                            0 => {
                                alive.remove(&serial);
                            }
                            3 => {
                                c.callbacks.insert(arg(&m, 0), serial);
                            }
                            _ => {}
                        }
                    }
                    out.extend_from_slice(&m);
                }
                drop(g);
                send(sfd, &out, &mut up_fds);
            }
            if pfd[1].revents != 0 {
                let n = recv(sfd, &mut buf, &mut down_fds);
                if n <= 0 {
                    return;
                }
                down.extend_from_slice(&buf[..n as usize]);
                let mut out = Vec::new();
                let mut g = shared.lock().unwrap();
                let holding = g.holding;
                let c = &mut g.conns[k];
                for m in split(&mut down) {
                    let (obj, op) = header(&m);
                    if let Some(serial) = c.callbacks.remove(&obj).filter(|_| op == 0) {
                        if holding {
                            c.held_ids.insert(obj, serial);
                            c.held.push((serial, obj, m));
                            shared.held.notify_all();
                            continue;
                        }
                    } else if obj == WL_DISPLAY && op == 1 {
                        let id = arg(&m, 0);
                        if let Some(serial) = c.held_ids.remove(&id) {
                            c.held.push((serial, id, m));
                            shared.held.notify_all();
                            continue;
                        }
                        c.registries.remove(&id);
                        c.compositors.remove(&id);
                        c.surfaces.remove(&id);
                        c.callbacks.remove(&id);
                    }
                    out.extend_from_slice(&m);
                }
                let client = c.client.as_raw_fd();
                // Written under the lock, so released events never overtake this batch.
                send(client, &out, &mut down_fds);
                drop(g);
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

/// Occurrences of `wl_callback#id.done` in the rusty-impulse Wayland log.
fn dones(log: &str, id: u32) -> usize {
    log.matches(&format!("wl_callback#{id}.done(")).count()
}

#[test]
fn shown_window_draws_while_the_old_callback_is_held_and_ignores_it_late() {
    if sway().is_none() {
        return;
    }
    let _one = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    let px = proxy::Withhold::start(proxy::upstream());
    let mut sh = Shell::start_on(Some(&px.path));
    sh.wait("the panel's first frame callback", |s| {
        s.contains("cb win0")
    });
    sh.wait_draws(3, "the animation drawing");
    sh.wait_presented("the red panel shown", |_| false);
    assert_pixel(sh.dir.path(), [255, 0, 0]);
    // The compositor shows the panel (the output is on), but its next callback is held.
    px.hold();
    px.wait_held();
    // Once the loop blocks after the frame that asked for it, nothing is due but that callback.
    sh.wait_blocked_after("the frame that asked for the held callback", |l| {
        l.contains("draw win")
    });
    sh.assert_waits_for_callback("while its callback was held");
    // Hidden and shown again: a new surface, which draws its current state at once instead
    // of waiting for the old surface's callback.
    sh.ipc("hide");
    sh.wait("the hidden panel's surface destroyed", |s| {
        s.lines()
            .any(|l| l.contains("-> zwlr_layer_surface_v1") && l.contains(".destroy()"))
    });
    sh.ipc("paint");
    let n = sh.draws();
    sh.ipc("show");
    sh.wait_draws(
        n,
        "the shown panel's first frame with the old callback held",
    );
    // The new surface's own callback is held now too (sway sent it once it showed that frame):
    // it waits for that one, and the old surface's callback arriving late does not stand in
    // for it.
    px.wait_held();
    sh.wait_presented("the green panel shown", painted);
    assert_pixel(sh.dir.path(), [0, 255, 0]);
    let (_, old) = px.held();
    assert!(!old.is_empty(), "the old surface's callback is still held");
    // Once the loop blocks after the frame that asked for it, nothing is due but that callback.
    sh.wait_blocked_after("the frame that asked for the held callback", |l| {
        l.contains("draw win")
    });
    let log = sh.text();
    let seen: Vec<usize> = old.iter().map(|&id| dones(&log, id)).collect();
    let n = sh.draws();
    let released = px.release_destroyed();
    assert_eq!(released, old);
    sh.wait("the old callback's late done", |s| {
        old.iter().zip(&seen).all(|(&id, &c)| dones(s, id) > c)
    });
    // The loop has handled the late done and blocked again: any frame it let through is drawn.
    sh.wait_blocked_after("the old callback's late done handled", |l| {
        old.iter()
            .any(|&id| l.contains(&format!("wl_callback#{id}.done(")))
    });
    let late = sh.draws() - n;
    assert_eq!(
        late, 0,
        "the old surface's late callback let the new one draw {late} frames"
    );
    px.release_all();
    sh.wait_draws(n + 3, "the animation drawing on its own callbacks");
    sh.wait_presented("the green panel shown", painted);
    assert_pixel(sh.dir.path(), [0, 255, 0]);
}
