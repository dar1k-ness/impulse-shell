//! The main loop sleeps until something is due: an idle shell blocks without a timeout, a
//! Timer wakes it at its interval, animations step at wl_surface frame callbacks (or on a
//! vsync timer when nothing is drawn) and the loop goes back to sleep when they finish.
//! Wakeups are the loop turns rusty-impulse reports (ri::sched logs one line per turn with its next
//! deadline) and the main thread's voluntary context switches, counted between events: log
//! lines the shell prints, or the frame callbacks sway sends a second, animating rusty-impulse
//! (`Metronome`), one per output refresh. Durations are the runtime's own clock: the
//! deadlines it logs, or animation frames under `QSG_FIXED_ANIMATION_STEP`.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;
use wayland_client::protocol::{wl_output, wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

struct Shell {
    child: Child,
    dir: PathBuf,
    /// Its sway: where `rusty-impulse ipc` finds it.
    runtime: PathBuf,
    display: PathBuf,
    log: Arc<Log>,
}

impl Shell {
    fn spawn_with(sway: &testenv::Sway, dir: &Path, qml: &str, env: &[(&str, &str)]) -> Self {
        std::fs::write(dir.join("shell.qml"), qml).unwrap();
        for d in ["config", "state", "cache"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let (r, w) = std::io::pipe().unwrap();
        let child = sway
            .env(&mut Command::new(env!("CARGO_BIN_EXE_rusty-impulse")))
            .arg("-p")
            .arg(dir)
            .env("RI_INTERP", "1")
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("XDG_STATE_HOME", dir.join("state"))
            .env("XDG_CACHE_HOME", dir.join("cache"))
            .envs(env.iter().copied())
            .stdout(w.try_clone().unwrap())
            .stderr(w)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        Shell {
            child,
            dir: dir.to_path_buf(),
            runtime: sway.runtime_dir().to_path_buf(),
            display: sway.display().to_path_buf(),
            log: Log::capture(r),
        }
    }

    fn wait(&self, what: &str, ok: impl Fn(&str) -> bool) -> String {
        let found = |s: &str| texts(s).into_iter().rev().find(|t| ok(t));
        match self.log.wait(|s| found(s).is_some()) {
            Ok(()) => found(&self.log.text()).unwrap(),
            Err(log) => panic!("{what}; rusty-impulse printed:\n{log}"),
        }
    }

    /// Waits until `f(log)`; panics if rusty-impulse exits first.
    fn wait_log(&self, what: &str, f: impl Fn(&str) -> bool) {
        if let Err(log) = self.log.wait(f) {
            panic!("{what}; rusty-impulse printed:\n{log}");
        }
    }

    /// Waits until the line `mark` last matches is followed by a loop turn (ri::sched: spawn
    /// with `RI_LOG=ri::sched=debug`) as the last one, which blocked with nothing due or with
    /// a deadline containing one of `due`: the work `mark` started has run its course. With
    /// `drawn`, a frame must also have been drawn (ri::frames) between `mark` and that turn.
    fn wait_blocked_on(&self, what: &str, mark: &str, due: &[&str], drawn: bool) {
        let settled = |log: &str| {
            let lines: Vec<&str> = log.lines().collect();
            let (Some(m), Some(t)) = (
                lines.iter().rposition(|l| l.contains(mark)),
                lines.iter().rposition(|l| is_turn(l)),
            ) else {
                return false;
            };
            let blocked =
                lines[t].contains(" next None") || due.iter().any(|d| lines[t].contains(d));
            m < t && blocked && (!drawn || lines[m..t].iter().any(|l| l.contains("draw win")))
        };
        self.wait_log(what, settled);
    }

    fn wait_blocked(&self, what: &str, mark: &str, due: &[&str]) {
        self.wait_blocked_on(what, mark, due, false);
    }

    /// Waits until `n` lines containing all of `pats` were printed after `at` (see [`Self::at`])
    /// and returns the output from `at` to the `n`th of them.
    fn wait_lines(&self, what: &str, at: usize, pats: &[&str], n: usize) -> String {
        let window = |log: &str| -> Option<String> {
            let mut end = 0;
            let mut seen = 0;
            for l in log[at..].split_inclusive('\n') {
                end += l.len();
                if pats.iter().all(|p| l.contains(p)) {
                    seen += 1;
                    if seen == n {
                        return Some(log[at..at + end].to_string());
                    }
                }
            }
            None
        };
        self.wait_log(what, |log| window(log).is_some());
        window(&self.log.text()).unwrap()
    }

    /// The length of the output so far: `since` it is the output printed after this point.
    fn at(&self) -> usize {
        self.log.text().len()
    }

    fn since(&self, at: usize) -> String {
        self.log.text()[at..].to_string()
    }

    /// Lines of the shell's output (with WAYLAND_DEBUG: its protocol messages) containing all
    /// of `pats`.
    fn count(&self, pats: &[&str]) -> usize {
        count(&self.log.text(), pats)
    }

    /// Times the main thread blocked and was woken again.
    fn wakeups(&self) -> u64 {
        let pid = self.child.id();
        let st = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/status")).unwrap();
        st.lines()
            .find_map(|l| l.strip_prefix("voluntary_ctxt_switches:"))
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    /// Calls the IpcHandler `fx`'s `func` through `rusty-impulse ipc call`, which returns once the
    /// shell answered.
    fn ipc(&self, func: &str) {
        let st = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .arg("-p")
            .arg(&self.dir)
            .args(["ipc", "call", "fx", func])
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("WAYLAND_DISPLAY", &self.display)
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

/// A second rusty-impulse whose panel animates without end: sway answers its frame callbacks once per
/// output refresh, so counting them measures a span of refreshes without a clock.
struct Metronome {
    sh: Shell,
    _dir: tempfile::TempDir,
}

impl Metronome {
    /// Started once the measured shell has shown its frames (two shells starting together can
    /// lose a first frame), and returned after its first frame callback.
    fn start(sway: &testenv::Sway) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let sh = Shell::spawn_with(
            sway,
            dir.path(),
            "import QtQuick\nimport Quickshell\nShellRoot {\n PanelWindow {\n  anchors.bottom: true\n  \
             anchors.right: true\n  implicitWidth: 60\n  implicitHeight: 60\n  \
             exclusionMode: ExclusionMode.Ignore\n  color: \"#202020\"\n  \
             Rectangle { x: 10; y: 10; width: 40; height: 40; color: \"#30c070\"\n   \
             RotationAnimation on rotation { running: true; loops: Animation.Infinite; \
             duration: 1000; from: 0; to: 360 } }\n }\n}\n",
            &[("RI_LOG", "ri::frames=debug")],
        );
        let m = Metronome { sh, _dir: dir };
        m.refreshes(1);
        m
    }

    /// Waits for `n` more output refreshes.
    fn refreshes(&self, n: usize) {
        let to = self.sh.count(&["cb win0 "]) + n;
        self.sh.wait_log("the metronome stopped animating", |s| {
            count(s, &["cb win0 "]) >= to
        });
    }
}

/// Main-thread wakeups and the output `sh` printed while the metronome counted `n` refreshes.
fn over_refreshes(sh: &Shell, m: &Metronome, n: usize) -> (u64, String) {
    let (at, w0) = (sh.at(), sh.wakeups());
    m.refreshes(n);
    (sh.wakeups() - w0, sh.since(at))
}

/// The `TXT ` lines of `log`, the text after the marker.
fn texts(log: &str) -> Vec<String> {
    log.lines()
        .filter_map(|l| l.split_once("TXT ").map(|(_, t)| t.to_string()))
        .collect()
}

fn count(log: &str, pats: &[&str]) -> usize {
    log.lines()
        .filter(|l| pats.iter().all(|p| l.contains(p)))
        .count()
}

/// A loop turn's ri::sched line: `t=<now> next <the earliest deadline>`.
fn is_turn(l: &str) -> bool {
    l.trim_start().starts_with("t=") && l.contains(" next ")
}

fn turns(log: &str) -> usize {
    log.lines().filter(|l| is_turn(l)).count()
}

/// A turn line's time and deadline (`None`: it blocks until an event): ms on the loop's clock.
fn turn(l: &str) -> (f64, Option<(f64, String)>) {
    let (t, next) = l.trim_start()["t=".len()..].split_once(" next ").unwrap();
    let next = next.trim().strip_prefix("Some((").map(|r| {
        let (at, why) = r.split_once(", ").unwrap();
        (at.parse().unwrap(), why.trim_end_matches("))").to_string())
    });
    (t.parse().unwrap(), next)
}

/// A sway of the test's own. In the testenv's shared one every shell another test starts or
/// stops maps or unmaps a panel, and sway then configures each layer surface of the output
/// again: the measured shell wakes for it (an ack_configure and a commit, nothing drawn).
fn compositor() -> Option<testenv::Sway> {
    testenv::require("sway").then(testenv::Sway::start)
}

#[test]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "idle_shell_does_not_wake",
        &[("PATH", Some("/nonexistent"))],
    );
}

fn panel(body: &str) -> String {
    format!(
        "import QtQuick\nimport Quickshell\nShellRoot {{\n PanelWindow {{\n  anchors.top: true\n  \
         anchors.left: true\n  implicitWidth: 400\n  implicitHeight: 100\n  \
         exclusionMode: ExclusionMode.Ignore\n  color: \"#202020\"\n{body}\n }}\n}}\n"
    )
}

/// A drawn panel with no timers, clocks or animations: once its first frame is drawn the
/// loop has nothing due, blocks until an event, and nothing wakes it while the output
/// refreshes 180 times (3 s at 60 Hz). The loop also blocks with nothing due before that
/// frame, between the layer surface's first commit and sway's `configure`: the refreshes are
/// counted from the block that follows the frame (its draw takes most of a second on a
/// loaded llvmpipe, and its turns are no wakeups of an idle shell).
#[test]
fn idle_shell_does_not_wake() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        &panel(
            "  Rectangle { x: 10; y: 10; width: 80; height: 40; color: \"#3070c0\" }\n  \
             Text { x: 100; y: 10; text: \"idle\"; color: \"white\" }\n  \
             Component.onCompleted: console.log(\"TXT ready\")",
        ),
        &[("RI_LOG", "ri::sched=debug,ri::frames=debug")],
    );
    sh.wait("the shell never loaded", |t| t == "ready");
    sh.wait_blocked_on(
        "the loaded shell never went idle after its first frame",
        "TXT ready",
        &[],
        true,
    );
    let m = Metronome::start(&sway);
    let (n, log) = over_refreshes(&sh, &m, 180);
    let turned = turns(&log);
    eprintln!("idle: {n} main-thread wakeups, {turned} loop turns in 180 refreshes");
    assert!(
        n <= 2 && turned <= 2,
        "an idle shell woke {n} times ({turned} loop turns) in 180 refreshes:\n{log}"
    );
}

/// A 250 ms repeating Timer: the loop wakes for each trigger (and the turn it causes), not on a
/// fixed tick, and the triggers keep their interval: each turn blocks until the next trigger,
/// whose deadline is 250 ms on the runtime's clock after the previous one, and the trigger runs
/// in the turn that deadline wakes.
#[test]
fn timer_wakes_the_loop_only_when_due() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        &panel(
            "  Timer { interval: 250; running: true; repeat: true\n   \
             property int n\n   onTriggered: console.log(\"TXT tick \" + (++n)) }",
        ),
        &[("RI_LOG", "ri::sched=debug")],
    );
    sh.wait("the timer never fired", |t| t == "tick 2");
    sh.wait_blocked(
        "the loop never blocked after a tick",
        "TXT tick 2",
        &["Timers))"],
    );
    let (at, w0) = (sh.at(), sh.wakeups());
    let fired = 12;
    let window = sh.wait_lines("the timer stopped firing", at, &["TXT tick "], fired);
    let n = sh.wakeups() - w0;
    sh.wait_blocked(
        "the loop never blocked after a tick",
        "TXT tick 14",
        &["Timers))"],
    );
    let log = sh.log.text();
    let lines: Vec<&str> = log.lines().collect();
    let turn_lines: Vec<usize> = (0..lines.len()).filter(|&i| is_turn(lines[i])).collect();
    let mut due = vec![];
    for k in 2..=14 {
        let at = lines
            .iter()
            .position(|l| l.ends_with(&format!("TXT tick {k}")))
            .unwrap();
        let before = *turn_lines.iter().rfind(|&&i| i < at).unwrap();
        let after = *turn_lines.iter().find(|&&i| i > at).unwrap();
        let (_, next) = turn(lines[before]);
        let d = match next {
            Some((d, why)) if why == "Timers" => d,
            _ => panic!(
                "tick {k} was not the loop's next deadline: {}",
                lines[before]
            ),
        };
        let (t, _) = turn(lines[after]);
        assert!(t >= d, "tick {k} due at {d} ran at {t}");
        due.push(d);
    }
    for w in due.windows(2) {
        let dt = w[1] - w[0];
        assert!(
            (dt - 250.0).abs() < 0.5,
            "triggers due {dt} ms apart: {due:?}"
        );
    }
    for l in window.lines().filter(|l| is_turn(l)) {
        let (t, next) = turn(l);
        let ok = next
            .as_ref()
            .is_some_and(|(d, why)| why == "Timers" || *d <= t + 0.5);
        assert!(ok, "the loop waited for something else than the timer: {l}");
    }
    let turned = turns(&window);
    eprintln!("timer: {fired} triggers, {turned} loop turns, {n} main-thread wakeups");
    assert!(
        turned <= 2 * fired + 2,
        "{turned} loop turns for {fired} triggers: the loop wakes without anything due"
    );
    assert!(
        n <= 3 * fired as u64 + 3,
        "{n} wakeups for {fired} triggers: the loop wakes without anything due"
    );
}

/// A visible NumberAnimation steps at frame callbacks, keeps its duration (frames of the
/// fixed 16 ms step `QSG_FIXED_ANIMATION_STEP` sets), and the loop sleeps again once it
/// finished.
#[test]
fn animation_steps_at_frames_then_sleeps() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        &panel(
            "  Rectangle { id: r; y: 10; width: 40; height: 40; color: \"#c07030\"\n   \
             onXChanged: console.log(\"TXT x \" + x)\n   \
             NumberAnimation on x { id: a; running: false; from: 0; to: 300; duration: 1000\n    \
             onFinished: console.log(\"TXT done\") } }\n  \
             Timer { interval: 1000; running: true; onTriggered: { console.log(\"TXT go\"); a.start() } }",
        ),
        &[
            ("RI_LOG", "ri::sched=debug,ri::frames=debug"),
            ("QSG_FIXED_ANIMATION_STEP", "1"),
        ],
    );
    sh.wait("the animation never started", |t| t == "go");
    let at = sh.at();
    sh.wait("the animation never finished", |t| t == "done");
    let log = sh.since(at);
    let log = &log[..log.find("TXT done").unwrap()];
    // Its steps on the runtime's clock: the x it moved to at each.
    let xs: Vec<f64> = texts(log)
        .iter()
        .filter_map(|t| t.strip_prefix("x "))
        .map(|x| x.parse().unwrap())
        .collect();
    let (during, frames) = (turns(log), count(log, &["draw win"]));
    eprintln!(
        "animation: {} steps, {frames} frames, {during} loop turns",
        xs.len()
    );
    // 1000 ms in 16 ms steps (the first one shorter when the animation started between two
    // driver frames), each moving x by 300 px * 16 / 1000.
    assert!(
        (62..=64).contains(&xs.len()),
        "a 1000 ms animation took {} steps: {xs:?}",
        xs.len()
    );
    assert_eq!(
        xs.last(),
        Some(&300.0),
        "the animation stopped short: {xs:?}"
    );
    for w in xs[1..xs.len() - 1].windows(2) {
        assert!(
            (w[1] - w[0] - 4.8).abs() < 1e-9,
            "a step of the animation was not 16 ms: {xs:?}"
        );
    }
    assert!(
        (30..=66).contains(&frames),
        "{frames} frames drawn by a 1 s animation"
    );
    assert!(
        during <= frames + 10,
        "{during} loop turns for {frames} frames of a 1 s animation"
    );
    sh.wait_blocked(
        "the loop never went idle after the animation",
        "TXT done",
        &[],
    );
    let m = Metronome::start(&sway);
    let (n, log) = over_refreshes(&sh, &m, 120);
    let turned = turns(&log);
    eprintln!("after the animation: {n} main-thread wakeups, {turned} loop turns in 120 refreshes");
    assert!(
        n <= 2 && turned <= 2,
        "the loop kept waking ({n}, {turned} loop turns in 120 refreshes) after the animation \
         finished:\n{log}"
    );
}

/// Endless spinners in several windows step together: one loop turn per frame for all of them.
/// The GL driver's buffer releases and swap throttling (Mesa's own event queue) arrive between
/// frames and must not wake the loop.
#[test]
fn concurrent_spinners_take_one_turn_per_frame() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let win = |anchor: &str, spinners: &str| {
        format!(
            "    PanelWindow {{\n        anchors.{anchor}: true\n        anchors.left: true\n        \
             implicitWidth: 200\n        implicitHeight: 80\n        \
             exclusionMode: ExclusionMode.Ignore\n        color: \"#202020\"\n{spinners}    }}\n"
        )
    };
    let spinner = |x: u32, ms: u32| {
        format!(
            "        Rectangle {{ x: {x}; y: 20; width: 40; height: 40; color: \"#c07030\"\n            \
             RotationAnimation on rotation {{ running: true; loops: Animation.Infinite; \
             duration: {ms}; from: 0; to: 360 }} }}\n"
        )
    };
    let qml = format!(
        "import QtQuick\nimport Quickshell\nShellRoot {{\n{}{}{}    \
         Timer {{ interval: 500; running: true; onTriggered: console.log(\"TXT ready\") }}\n}}\n",
        win("top", &(spinner(10, 1200) + &spinner(80, 1000))),
        win("bottom", &spinner(10, 12000)),
        win("right", &spinner(10, 1500)),
    );
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        &qml,
        &[("RI_LOG", "ri::sched=debug,ri::frames=debug")],
    );
    sh.wait("the shell never loaded", |t| t == "ready");
    // Blocked on the frame callbacks: the one deadline is the end of the wait for them.
    sh.wait_blocked("the loaded shell never blocked", "TXT ready", &["Frame"]);
    let frames = 120;
    let log = sh.wait_lines(
        "the spinners stopped stepping",
        sh.at(),
        &["cb win0 "],
        frames,
    );
    let during = turns(&log);
    eprintln!("3 windows, 4 spinners: {frames} frames, {during} loop turns");
    assert!(
        during <= frames + frames / 4 + 4,
        "{during} loop turns for {frames} frames of concurrent spinners"
    );
}

/// An animation of a property nothing draws gets no frame callbacks: the driver falls back to
/// a vsync timer (QUnifiedTimer) and the animation still finishes on time, in the 16 ms steps
/// of `QSG_FIXED_ANIMATION_STEP`, each in a turn that timer woke.
#[test]
fn undrawn_animation_steps_on_a_timer() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        &panel(
            "  id: win\n  property real level\n  property int steps\n  \
             onLevelChanged: steps++\n  \
             NumberAnimation on level { id: a; running: false; from: 0; to: 1; duration: 500\n   \
             onFinished: console.log(\"TXT done \" + win.steps) }\n  \
             Timer { interval: 1000; running: true; onTriggered: { console.log(\"TXT go\"); a.start() } }",
        ),
        &[
            ("RI_LOG", "ri::sched=debug,ri::frames=debug"),
            ("QSG_FIXED_ANIMATION_STEP", "1"),
        ],
    );
    sh.wait("the animation never started", |t| t == "go");
    let at = sh.at();
    let done = sh.wait("the animation never finished", |t| t.starts_with("done "));
    let steps: usize = done["done ".len()..].parse().unwrap();
    let log = sh.since(at);
    let log = &log[..log.find("TXT done").unwrap()];
    let timed = log
        .lines()
        .filter(|l| is_turn(l) && l.contains(", Frame))"))
        .count();
    let (cbs, draws) = (count(log, &["cb win"]), count(log, &["draw win"]));
    eprintln!("undrawn animation: {steps} steps, {timed} turns waiting for the vsync timer");
    // 500 ms in 16 ms steps.
    assert!(
        (31..=33).contains(&steps),
        "a 500 ms animation took {steps} steps"
    );
    assert_eq!(
        (cbs, draws),
        (0, 0),
        "the undrawn animation drew or waited for frame callbacks"
    );
    assert!(
        timed + 2 >= steps,
        "{timed} turns waited for the vsync timer for {steps} steps"
    );
}

/// An endless animation that shows nothing (one in a hidden window, one with nothing to animate)
/// never finishes and changes no pixel: the loop sleeps until the 4 s Timer instead of stepping
/// it on the vsync timer, and the hidden one animates again as soon as its window is shown.
#[test]
fn unseen_endless_animations_do_not_wake() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        r##"import QtQuick
import Rustshell
ShellRoot {
    NumberAnimation { running: true; loops: Animation.Infinite; duration: 1000 }
    PanelWindow {
        anchors.top: true
        anchors.left: true
        implicitWidth: 400
        implicitHeight: 100
        exclusionMode: ExclusionMode.Ignore
        color: "#202020"
        Component.onCompleted: console.log("TXT ready")
    }
    PanelWindow {
        id: hidden
        visible: false
        anchors.bottom: true
        anchors.left: true
        implicitWidth: 100
        implicitHeight: 100
        exclusionMode: ExclusionMode.Ignore
        Rectangle {
            width: 40; height: 40; color: "#c07030"
            RotationAnimation on rotation { running: true; loops: Animation.Infinite; duration: 1000; from: 0; to: 360 }
        }
    }
    Timer { interval: 4000; running: true; onTriggered: { hidden.visible = true; console.log("TXT shown") } }
}
"##,
        &[("RI_LOG", "ri::sched=debug,ri::frames=debug")],
    );
    sh.wait("the shell never loaded", |t| t == "ready");
    // The panel has drawn its first frame and only the 4 s Timer is due.
    sh.wait_blocked_on(
        "the loaded shell never blocked",
        "TXT ready",
        &["Timers))"],
        true,
    );
    let at = sh.at();
    sh.wait("the window was never shown", |t| t == "shown");
    let log = sh.since(at);
    let log = &log[..log.find("TXT shown").unwrap()];
    let turned = turns(log);
    eprintln!("unseen endless animations: {turned} loop turns until the Timer");
    assert!(
        turned <= 4 && !log.lines().any(|l| is_turn(l) && l.contains(", Frame))")),
        "unseen endless animations woke the loop {turned} times before the Timer; rusty-impulse \
         printed meanwhile:\n{log}"
    );
    let log = sh.wait_lines(
        "the shown animation never stepped",
        sh.log.text().find("TXT shown").unwrap(),
        &["cb win"],
        20,
    );
    let draws = count(&log, &["draw win"]);
    eprintln!("shown: {draws} frames drawn at 20 frame callbacks");
    assert!(
        draws >= 20,
        "the shown animation drew {draws} frames at 20 frame callbacks"
    );
}

/// A Process started by another's `onExited`: each start and exit is handled at once, with no
/// tick to pick them up: from an idle loop, every turn of the chain blocks until an event.
#[test]
fn chained_processes_run_without_a_tick() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        &format!(
            "import QtQuick\nimport Quickshell\nimport Rustshell.Io\n{}",
            panel(
                "  Process { id: p; command: [\"true\"]\n   property int runs\n   \
                 onExited: { if (++p.runs < 20) p.running = true; else console.log(\"TXT done\") } }\n  \
                 IpcHandler { target: \"fx\"\n   \
                 function go(): void { console.log(\"TXT go\"); p.running = true } }\n  \
                 Component.onCompleted: console.log(\"TXT ready\")",
            )
            .trim_start_matches("import QtQuick\nimport Quickshell\n")
        ),
        &[("RI_LOG", "ri::sched=debug")],
    );
    sh.wait("the shell never loaded", |t| t == "ready");
    sh.wait_blocked("the loaded shell never went idle", "TXT ready", &[]);
    sh.ipc("go");
    sh.wait("the process chain never finished", |t| t == "done");
    let log = sh.log.text();
    let log = &log[log.find("TXT go").unwrap()..log.find("TXT done").unwrap()];
    let turned = turns(log);
    eprintln!("20 chained processes: {turned} loop turns");
    for l in log.lines().filter(|l| is_turn(l)) {
        let (t, next) = turn(l);
        assert!(
            next.is_none_or(|(d, _)| d <= t + 0.5),
            "a turn of the process chain waited for a deadline: {l}"
        );
    }
    assert!(
        turned <= 20 * 4,
        "{turned} loop turns for 20 chained `true` runs"
    );
}

/// Hidden PanelWindows are laid out ahead of their first show in the loop's idle turns: once
/// the shown panel's first frame is drawn and nothing else is due, not after a delay the loop
/// waits for.
#[test]
fn hidden_windows_are_laid_out_when_idle() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        &format!(
            "{}\n",
            panel(
                "  Text { text: \"shown\"; color: \"white\" }\n \
                 }\n PanelWindow {\n  visible: false\n  anchors.top: true\n  \
                 implicitWidth: 300\n  implicitHeight: 200\n  \
                 Text { text: \"hidden\"; color: \"white\" }"
            )
        ),
        &[("RI_LOG", "ri::sched=debug,ri::stats=debug,ri::frames=debug")],
    );
    sh.wait_log("the hidden window was never laid out", |log| {
        log.lines().any(|l| l.contains("prelayout "))
    });
    let log = sh.log.text();
    let log = &log[..log.find("prelayout ").unwrap()];
    assert!(
        log.contains("draw win"),
        "laid out before the first frame:\n{log}"
    );
    for l in log.lines().filter(|l| is_turn(l)) {
        let (t, next) = turn(l);
        assert!(
            next.is_none_or(|(d, _)| d <= t + 0.5),
            "the prelayout waited for a deadline: {l}"
        );
    }
}

/// Before the first frame the loop runs until a native poll finds nothing to do and nothing
/// changed: nine FileViews, each loaded once the one before it set its path from `onLoaded`
/// (one poll each), all load before the panel is first drawn.
#[test]
fn startup_settles_before_the_first_frame() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    const N: usize = 9;
    let mut views = String::new();
    for k in 0..N {
        let next = if k + 1 == N {
            "end".to_string()
        } else {
            format!("f{}", k + 1)
        };
        std::fs::write(dir.path().join(format!("f{k}")), format!("{next}\n")).unwrap();
        let path = if k == 0 {
            format!("path: \"{}\"; ", dir.path().join("f0").display())
        } else {
            String::new()
        };
        let then = if k + 1 == N {
            "console.log(\"TXT chain \" + text().trim())".to_string()
        } else {
            format!(
                "v{}.path = \"{}/\" + text().trim()",
                k + 1,
                dir.path().display()
            )
        };
        views = format!("  FileView {{ id: v{k}; {path}onLoaded: {then} }}\n{views}");
    }
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        &format!(
            "import QtQuick\nimport Quickshell\nimport Rustshell.Io\n{}",
            panel(views.trim_end()).trim_start_matches("import QtQuick\nimport Quickshell\n")
        ),
        &[("RI_LOG", "ri::frames=debug")],
    );
    sh.wait_log("the panel was never drawn", |log| log.contains("draw win"));
    let log = sh.log.text();
    let first = &log[..log.find("draw win").unwrap()];
    assert!(
        texts(first).iter().any(|t| t == "chain end"),
        "the first frame was drawn before the FileView chain settled:\n{log}"
    );
}

/// A Rectangle recolored by a 250 ms Timer: each change is drawn and committed, but a static
/// frame requests no wl_surface.frame callback, so the loop is not woken again when the
/// compositor shows the frame. (Main-thread wakeups here are mostly llvmpipe's rasterizer
/// handoffs inside each swap, so they are reported, not bounded.)
#[test]
fn static_redraws_request_no_frame_callback() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        &panel(
            "  Rectangle { id: r; width: 40; height: 40; color: \"#c07030\" }\n  \
             Timer { interval: 250; running: true; repeat: true\n   property int n\n   \
             onTriggered: { r.color = ++n % 2 ? \"#3070c0\" : \"#c07030\"; console.log(\"TXT tick \" + n) } }",
        ),
        &[("WAYLAND_DEBUG", "client"), ("RI_LOG", "ri::sched=debug")],
    );
    sh.wait("the timer never fired", |t| t == "tick 2");
    sh.wait_blocked(
        "the loop never blocked after a tick",
        "TXT tick 2",
        &["Timers))"],
    );
    let (at, w0) = (sh.at(), sh.wakeups());
    let fired = 12;
    sh.wait_lines("the timer stopped firing", at, &["TXT tick "], fired);
    sh.wait_blocked(
        "the loop never blocked after the last tick",
        "TXT tick 14",
        &["Timers))"],
    );
    let n = sh.wakeups() - w0;
    let log = sh.since(at);
    let commits = count(&log, &["-> wl_surface#", ".commit()"]);
    let frames = count(&log, &["-> wl_surface#", ".frame("]);
    eprintln!(
        "static redraws: {fired} triggers, {commits} commits, {frames} frame requests, {n} wakeups"
    );
    assert!(
        commits + 1 >= fired,
        "{commits} commits for {fired} recolors"
    );
    assert_eq!(
        frames, 0,
        "static frames requested {frames} frame callbacks"
    );
}

/// When libwayland wrote each `wl_surface.commit` in `log` (WAYLAND_DEBUG stamps its lines
/// `[HH:MM:SS.µs]`), in µs of the day.
fn commit_stamps(log: &str) -> Vec<u64> {
    log.lines()
        .filter(|l| l.contains(".commit()"))
        .filter_map(|l| l.split_once("-> wl_surface#"))
        .map(|(head, _)| {
            let t = head
                .rsplit_once('[')
                .and_then(|(_, t)| t.split_once(']'))
                .map(|(t, _)| t);
            let us = t.and_then(|t| {
                let (hms, us) = t.split_once('.')?;
                let mut day = 0;
                for part in hms.split(':') {
                    day = day * 60 + part.parse::<u64>().ok()?;
                }
                Some(day * 1_000_000 + us.parse::<u64>().ok()?)
            });
            us.unwrap_or_else(|| panic!("no [HH:MM:SS.µs] stamp before the commit in {head:?}"))
        })
        .collect()
}

/// The refresh interval in µs of the output's current mode (the `wl_output.mode` the shell was
/// sent). A mode that names no rate (sway's headless output: 0 mHz) is 60 Hz to the shell.
fn refresh_us(log: &str) -> u64 {
    let mhz = log.lines().find_map(|l| {
        let (object, args) = l.split_once(".mode(")?;
        object.contains("wl_output#").then_some(())?;
        let args: Vec<&str> = args.trim_end_matches(')').split(", ").collect();
        (args.len() == 4 && args[0].parse::<u32>().ok()? & 1 == 1)
            .then(|| args[3].parse::<u64>().ok())?
    });
    let mhz = mhz.expect("the shell was sent no current wl_output.mode");
    1_000_000_000 / if mhz >= 1000 { mhz } else { 60_000 }
}

/// A Rectangle recolored every millisecond is drawn at the output's refresh rate: a static
/// frame without a callback holds the next draw for one refresh interval, and the next draw
/// follows when the hold ends. Read from the shell's own commits and the times it made them
/// at, which a busy machine only moves apart. Counting them against a second client's frame
/// callbacks does not measure the shell: under load sway's frame timer and that client run
/// late, a callback then stands for several intervals, and the shell, still one interval
/// apart, makes more commits than callbacks came.
#[test]
fn static_redraws_stay_at_the_refresh_rate() {
    let Some(sway) = compositor() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        &panel(
            "  Rectangle { id: r; width: 40; height: 40; color: \"#c07030\" }\n  \
             Timer { interval: 1; running: true; repeat: true\n   property int n\n   \
             onTriggered: { r.color = ++n % 2 ? \"#3070c0\" : \"#c07030\"; if (n == 50) console.log(\"TXT go\") } }",
        ),
        &[("WAYLAND_DEBUG", "client")],
    );
    sh.wait("the timer never fired", |t| t == "go");
    let recolored = |log: &str| -> Vec<u64> {
        let from = log.rfind("TXT go").unwrap();
        let whole = log.rfind('\n').unwrap();
        commit_stamps(&log[from..whole])
    };
    sh.wait_log("the recolored panel stopped drawing", |s| {
        recolored(s).len() > 60
    });
    let log = sh.since(0);
    let (stamps, refresh) = (recolored(&log), refresh_us(&log));
    const DAY: u64 = 86_400_000_000;
    let mut gaps: Vec<u64> = stamps
        .windows(2)
        .map(|w| (w[1] + DAY - w[0]) % DAY)
        .collect();
    gaps.sort_unstable();
    let (least, median) = (gaps[0], gaps[gaps.len() / 2]);
    eprintln!(
        "1 ms recolors: {} commits, {least} µs apart at least, {median} µs at the median; \
         a refresh takes {refresh} µs",
        stamps.len()
    );
    assert!(
        least >= refresh,
        "two commits {least} µs apart, a refresh takes {refresh} µs"
    );
    assert!(
        median < 2 * refresh,
        "half of the commits came {median} µs or more after the one before, a refresh takes \
         {refresh} µs"
    );
}

/// The globals a virtual pointer needs, and the output's size its positions are fractions of.
#[derive(Default)]
struct Globals {
    seat: Option<wl_seat::WlSeat>,
    vptr: Option<ZwlrVirtualPointerManagerV1>,
    mode: (u32, u32),
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
                "zwlr_virtual_pointer_manager_v1" => g.vptr = Some(reg.bind(name, 1, qh, ())),
                "wl_output" if g.mode == (0, 0) => {
                    reg.bind::<wl_output::WlOutput, _, _>(name, 1, qh, ());
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for Globals {
    fn event(
        g: &mut Self,
        _: &wl_output::WlOutput,
        ev: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode { width, height, .. } = ev {
            g.mode = (width as u32, height as u32);
        }
    }
}

delegate_noop!(Globals: ignore wl_seat::WlSeat);
delegate_noop!(Globals: ZwlrVirtualPointerManagerV1);
delegate_noop!(Globals: ZwlrVirtualPointerV1);

/// Three views of a 400 px wide panel, one per row of 100 px below an empty one: each logs
/// where its content is (`at <x>`) and the first two when they stop (`moving false`).
const MOVED_CONTENT: &str = r##"import QtQuick
import QtQuick.Controls
import Rustshell
ShellRoot {
    PanelWindow {
        anchors.top: true
        anchors.left: true
        implicitWidth: 400
        implicitHeight: 400
        exclusionMode: ExclusionMode.Ignore
        color: "#202020"
        Component.onCompleted: console.log("TXT ready")
        Flickable {
            y: 100; width: 400; height: 100
            contentWidth: 4000; contentHeight: 100
            onContentXChanged: console.log("TXT at " + contentX)
            onMovingChanged: console.log("TXT moving " + moving)
            Row {
                Repeater {
                    model: 40
                    Rectangle { width: 100; height: 100; color: index % 2 ? "#335" : "#533" }
                }
            }
        }
        ListView {
            y: 200; width: 400; height: 100
            orientation: ListView.Horizontal
            model: 40
            delegate: Rectangle { width: 100; height: 100; color: index % 2 ? "#353" : "#535" }
            onContentXChanged: console.log("TXT at " + contentX)
            onMovingChanged: console.log("TXT moving " + moving)
        }
        SwipeView {
            y: 300; width: 400; height: 100
            Rectangle { color: "#533"; onXChanged: console.log("TXT at " + -x) }
            Rectangle { color: "#355" }
            Rectangle { color: "#553" }
        }
    }
}
"##;

/// Content a released drag left moving (a flicked Flickable and ListView, a SwipeView's pages
/// on their way to the next page) is drawn at each step by its own movement, with nothing
/// else in the window changing and no input, until it rests. Then the loop blocks, and
/// nothing is drawn or wakes it while the output refreshes 120 times.
#[test]
fn released_content_draws_until_it_rests_then_the_loop_sleeps() {
    let Some(sway) = compositor() else {
        return;
    };
    let stream = std::os::unix::net::UnixStream::connect(sway.display()).unwrap();
    let conn = Connection::from_socket(stream).unwrap();
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut g = Globals::default();
    q.roundtrip(&mut g).unwrap();
    q.roundtrip(&mut g).unwrap();
    let (Some(seat), Some(vptr)) = (g.seat.clone(), g.vptr.clone()) else {
        testenv::unavailable("compositor lacks zwlr_virtual_pointer_v1");
        return;
    };
    let ptr = vptr.create_virtual_pointer(Some(&seat), &qh, ());
    let (w, h) = g.mode;
    assert!(w >= 400 && h >= 400, "first output mode {:?}", g.mode);
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn_with(
        &sway,
        dir.path(),
        MOVED_CONTENT,
        &[
            ("RI_LOG", "ri::sched=debug,ri::frames=debug"),
            ("WAYLAND_DEBUG", "1"),
            ("QSG_FIXED_ANIMATION_STEP", "1"),
        ],
    );
    sh.wait("the shell never loaded", |t| t == "ready");
    sh.wait_blocked("the loaded shell never went idle", "TXT ready", &[]);
    // The pointer events the shell has read, as its WAYLAND_DEBUG lines.
    let events = |log: &str| {
        log.lines()
            .filter(|l| l.contains("wl_pointer#") && !l.contains(" -> "))
            .filter(|l| l.contains(".motion(") || l.contains(".button(") || l.contains(".enter("))
            .count()
    };
    // Sends what `send` queues and waits until the shell has read `n` more pointer events
    // and ended the turn that read the last: the next ones come out of another queue, so
    // each move is a step of the drag (a burst would reach the scene as its first and last).
    let step = |n: usize, send: &dyn Fn()| {
        let to = events(&sh.log.text()) + n;
        send();
        conn.flush().unwrap();
        sh.wait_log("the shell never read a pointer event", |log| {
            let lines: Vec<&str> = log.lines().collect();
            let last = lines.iter().rposition(|l| events(l) == 1);
            events(log) == to && last.is_some_and(|m| lines[m + 1..].iter().any(|l| is_turn(l)))
        });
    };
    let m = Metronome::start(&sway);
    const BTN_LEFT: u32 = 0x110;
    let mut t = 10;
    for (row, name) in ["Flickable", "ListView", "SwipeView"]
        .into_iter()
        .enumerate()
    {
        let y = 150 + 100 * row as u32;
        // A press at x 350, a move over the drag threshold, then to x 90 in steps 10 ms apart.
        step(1, &|| {
            ptr.motion_absolute(t, 350, y, w, h);
            ptr.frame();
        });
        step(1, &|| {
            ptr.button(t + 10, BTN_LEFT, wl_pointer::ButtonState::Pressed);
            ptr.frame();
        });
        t += 20;
        for x in [330, 320, 250, 170, 90] {
            step(1, &|| {
                ptr.motion_absolute(t, x, y, w, h);
                ptr.frame();
            });
            t += 10;
        }
        let at = sh.at();
        step(1, &|| {
            ptr.button(t, BTN_LEFT, wl_pointer::ButtonState::Released);
            ptr.frame();
        });
        t += 10;
        // The SwipeView rests on its second page, 400 px on.
        let rest = if row == 2 { "at 400" } else { "moving false" };
        sh.wait_log(&format!("the {name} never came to rest"), |log| {
            texts(&log[at..]).iter().any(|l| l == rest)
        });
        let log = sh.since(at);
        let moving = &log[..log.find(&format!("TXT {rest}")).unwrap()];
        let steps = texts(moving)
            .iter()
            .filter(|l| l.starts_with("at "))
            .count();
        let frames = count(moving, &["draw win"]);
        eprintln!("{name}: {steps} steps after the release, {frames} frames drawn");
        assert!(
            steps >= 5 && frames >= steps / 2,
            "the released {name} moved in {steps} steps and {frames} frames:\n{log}"
        );
        // At rest and shown: the loop blocks with nothing due, and the last frame it
        // committed asked for no frame callback, so no draw is still to come.
        sh.wait_log(
            &format!("the loop never went idle once the {name} rested"),
            |log| {
                let lines: Vec<&str> = log.lines().collect();
                let commit = |l: &&str| l.contains(" -> wl_surface#") && l.contains(".commit()");
                let (Some(m), Some(t), Some(c)) = (
                    lines
                        .iter()
                        .rposition(|l| l.ends_with(&format!("TXT {rest}"))),
                    lines.iter().rposition(|l| is_turn(l)),
                    lines.iter().rposition(&commit),
                ) else {
                    return false;
                };
                let before = lines[..c].iter().rposition(commit).unwrap_or(0);
                let callback = lines[before..c]
                    .iter()
                    .any(|l| l.contains(" -> wl_surface#") && l.contains(".frame("));
                m < t && c < t && lines[t].contains(" next None") && !callback
            },
        );
        let (n, log) = over_refreshes(&sh, &m, 120);
        let (turned, frames) = (turns(&log), count(&log, &["draw win"]));
        eprintln!(
            "{name} at rest: {frames} frames drawn, {n} main-thread wakeups, {turned} loop \
             turns in 120 refreshes"
        );
        assert!(
            frames == 0 && n <= 2 && turned <= 2,
            "the {name} at rest: {frames} frames drawn, {n} wakeups, {turned} loop turns in \
             120 refreshes:\n{log}"
        );
    }
}
