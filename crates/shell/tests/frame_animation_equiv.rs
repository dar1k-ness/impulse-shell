//! A FrameAnimation is triggered once per step of the animation driver (QFrameAnimationJob is
//! an animation of 1 ms that loops, QQuickFrameAnimationPrivate::maybeTick runs when its loop
//! changes): 60 times a second on a 60 Hz output, whatever else wakes the loop. The real
//! quickshell running the fixture vs the native shell (`impulse-shell -p`): the triggers in the second
//! a Timer counts, alone, next to a `Qt.callLater` function that calls `Qt.callLater` with
//! itself, next to it while every trigger moves an item of the window, in an item that is not
//! visible, in a window that is not, while the pointer moves over the window as often as the
//! compositor takes a move, and while it does and frames come late (the driver follows the wall clock
//! then, QSGDefaultAnimationDriver's TimerMode); and the counters of the first 10 triggers.
//! The steps of the driver are counted where QML sees them: a NumberAnimation that runs next
//! to the FrameAnimation gets a new value at each.
//!
//! Each shell runs on a sway of its own. On one that other windows come to and leave, sway
//! lays Quickshell's window out anew and answers its frames at once meanwhile, and Qt steps
//! the animation driver at each answer: 61 triggers in 0.55 to 0.93 s, 15 of 281 runs beside
//! the other tests on the sway they share, none of 150 on a sway of its own.
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use testenv::Log;
use wayland_client::protocol::{wl_output, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

#[derive(Default)]
struct G {
    seat: Option<wl_seat::WlSeat>,
    vptr: Option<ZwlrVirtualPointerManagerV1>,
    output: Option<wl_output::WlOutput>,
    mode: (u32, u32),
}

impl Dispatch<wl_registry::WlRegistry, ()> for G {
    fn event(
        g: &mut Self,
        reg: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = ev
        {
            match interface.as_str() {
                "wl_seat" if g.seat.is_none() => g.seat = Some(reg.bind(name, 1, qh, ())),
                "zwlr_virtual_pointer_manager_v1" => {
                    g.vptr = Some(reg.bind(name, version.min(2), qh, ()))
                }
                "wl_output" if g.output.is_none() => g.output = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for G {
    fn event(
        g: &mut Self,
        _: &wl_output::WlOutput,
        ev: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode {
            flags,
            width,
            height,
            ..
        } = ev
            && flags
                .into_result()
                .is_ok_and(|f| f.contains(wl_output::Mode::Current))
        {
            g.mode = (width as u32, height as u32);
        }
    }
}

delegate_noop!(G: ignore wl_seat::WlSeat);
delegate_noop!(G: ZwlrVirtualPointerManagerV1);
delegate_noop!(G: ZwlrVirtualPointerV1);

/// A virtual pointer that crosses the first output of a sway again and again, a move each
/// time the compositor has answered the one before, until dropped.
struct Moving {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Moving {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Round trips to the compositor between two moves: a move per round trip is tens of
/// thousands a second, more than a window under the pointer keeps its frames with.
const PER_MOVE: u32 = 16;

/// None where the compositor has no zwlr_virtual_pointer_v1.
fn moving(sway: &testenv::Sway) -> Option<Moving> {
    let conn = Connection::from_socket(UnixStream::connect(sway.display()).ok()?).ok()?;
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut g = G::default();
    q.roundtrip(&mut g).unwrap();
    q.roundtrip(&mut g).unwrap();
    let (seat, vptr) = (g.seat.clone()?, g.vptr.clone()?);
    let ptr = match &g.output {
        Some(o) if vptr.version() >= 2 => {
            vptr.create_virtual_pointer_with_output(Some(&seat), Some(o), &qh, ())
        }
        _ => vptr.create_virtual_pointer(Some(&seat), &qh, ()),
    };
    let (w, h) = g.mode;
    assert!(w >= 100 && h >= 100, "first output mode {:?}", g.mode);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = std::thread::spawn(move || {
        let mut k = 0u32;
        while !stopped.load(Ordering::Relaxed) {
            ptr.motion_absolute(k, 20 + k % (w - 40), h / 2, w, h);
            ptr.frame();
            // The compositor's answer paces the moves.
            for _ in 0..PER_MOVE {
                if q.roundtrip(&mut g).is_err() {
                    return;
                }
            }
            k += 1;
        }
    });
    Some(Moving {
        stop,
        thread: Some(thread),
    })
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Half a second for the window to appear, then the FrameAnimation starts (and the chain, and
/// the moves, by MODE) and the next firing of the Timer, one second later, reports.
const SHELL: &str = r#"import QtQuick
import Quickshell

FloatingWindow {
    id: root
    visible: mode != "unseen"
    implicitWidth: 200
    implicitHeight: 200
    property string mode: "MODE"
    property int frames: 0
    property int laters: 0
    property int moves: 0
    property int steps: 0
    property real v: 0
    onVChanged: root.steps++
    property real t0: 0
    property real last: 0
    property real slow: 0
    property var first: []
    function again() { root.laters++; Qt.callLater(root.again) }
    Rectangle { id: box; width: 10; height: 10; color: "red" }
    NumberAnimation { id: stepper; target: root; property: "v"; from: 0; to: 1; duration: 3600000 }
    MouseArea { anchors.fill: parent; hoverEnabled: true; onPositionChanged: root.moves++ }
    Item {
        visible: root.mode != "hidden"
        FrameAnimation {
            id: fa
            onTriggered: {
                root.frames++
                root.last = elapsedTime
                if (root.first.length < 10)
                    root.first.push([currentFrame, frameTime, smoothFrameTime, elapsedTime].join(":"))
                if (root.mode == "moving")
                    box.x = (box.x + 1) % 190
                // Three frames of 70 ms put the driver on wall time, and one of 30 ms now and
                // then keeps it there.
                var now = Date.now()
                if (root.mode == "late" && (root.frames <= 3 || now - root.slow > 80)) {
                    var until = now + (root.frames <= 3 ? 70 : 30)
                    while (Date.now() < until) {}
                    root.slow = until
                }
            }
        }
    }
    Timer {
        interval: 500; running: true; repeat: true
        onTriggered: {
            if (root.t0 == 0) {
                interval = 1000
                root.t0 = Date.now()
                root.moves = 0
                console.log("FA0")
                stepper.start()
                fa.start()
                if (root.mode == "later" || root.mode == "moving")
                    Qt.callLater(root.again)
            } else {
                console.log("FA " + root.frames + " " + (Date.now() - root.t0) + " " + root.laters
                            + " " + root.moves + " " + root.steps + " " + root.last + " "
                            + root.first.join(","))
                Qt.quit()
            }
        }
    }
}
"#;

/// The least a step of the animation driver takes, seconds: QUnifiedTimer's timer is of 16 ms
/// (Quickshell with no window shown: 15.9 to 16.0 ms a step), and the headless sway shows a
/// frame each 16 to 17 ms.
const STEP: f64 = 0.016;

/// What the fixture reported.
#[derive(Debug)]
struct Second {
    /// Triggers in the counted second.
    frames: u64,
    /// How long that second took, ms.
    ms: f64,
    /// Turns of the `Qt.callLater` chain.
    laters: u64,
    /// Moves of the pointer the window was told of in that second.
    moves: u64,
    /// The steps of the animation driver: how many times it set the value of a NumberAnimation
    /// that runs next to the FrameAnimation.
    steps: u64,
    /// `elapsedTime` at the last trigger, seconds.
    elapsed: f64,
    /// wl_surface.commit requests since the start, where they are logged (`moving`).
    commits: Option<u64>,
    /// `currentFrame`, `frameTime`, `smoothFrameTime`, `elapsedTime` at the first 10 triggers.
    first: Vec<[f64; 4]>,
}

fn second(bin: &str, sway: &testenv::Sway, dir: &Path, mode: &str) -> Second {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("shell.qml"), SHELL.replace("MODE", mode)).unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let mut c = Command::new(bin);
    sway.env(&mut c)
        .arg("-p")
        .arg(dir)
        .env("RI_INTERP", "1")
        .env("QS_NO_RELOAD_POPUP", "1")
        .envs((mode == "moving").then_some(("WAYLAND_DEBUG", "1")))
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w);
    let _k = Kill(c.spawn().unwrap());
    drop(c);
    let log = Log::capture(r);
    let line =
        |s: &str| (s.lines()).find_map(|l| l.find("FA ").map(|i| l[i + 3..].trim().to_string()));
    if let Err(s) = log.wait(|s| line(s).is_some()) {
        panic!("{bin}: exited before the second was counted; log:\n{s}");
    }
    let text = log.text();
    let commits = (mode == "moving").then(|| {
        (text[text.find("FA0").unwrap()..].lines())
            .filter(|l| l.contains("-> wl_surface#") && l.contains(".commit("))
            .count() as u64
    });
    let line = line(&text).unwrap();
    let mut p = line.split(' ');
    let mut num = || p.next().unwrap().parse::<f64>().unwrap();
    let (frames, ms, laters) = (num() as u64, num(), num() as u64);
    let (moves, steps, elapsed) = (num() as u64, num() as u64, num());
    let first = (p.next().unwrap_or("").split(','))
        .filter(|f| !f.is_empty())
        .map(|f| {
            let v: Vec<f64> = f.split(':').map(|n| n.parse().unwrap()).collect();
            v.try_into().unwrap()
        })
        .collect();
    Second {
        frames,
        ms,
        laters,
        moves,
        steps,
        elapsed,
        commits,
        first,
    }
}

/// The counters of the first 10 triggers follow QQuickFrameAnimationPrivate::maybeTick:
/// `currentFrame` counts from 1, `smoothFrameTime` is 0.1 of `frameTime` plus 0.9 of what it
/// was (0 at first), `elapsedTime` is the sum of the frame times.
fn counters_follow_qt(who: &str, s: &Second) {
    assert_eq!(s.first.len(), 10, "{who}: {s:?}");
    let (mut smooth, mut elapsed) = (0.0, 0.0);
    for (k, [frame, time, sm, el]) in s.first.iter().enumerate() {
        smooth = 0.1 * time + 0.9 * smooth;
        elapsed += time;
        assert!(
            *frame == (k + 1) as f64
                && *time > 0.0
                && (sm - smooth).abs() < 1e-9
                && (el - elapsed).abs() < 1e-9,
            "{who}: trigger {} has currentFrame {frame}, frameTime {time}, smoothFrameTime {sm} \
             (by Qt's rule {smooth}), elapsedTime {el} (the sum is {elapsed})",
            k + 1
        );
    }
}

fn same_as_quickshell(mode: &str) {
    if !testenv::require("qs") || !testenv::require("sway") {
        return;
    }
    let moved = matches!(mode, "moved" | "late");
    let base = tempfile::tempdir().unwrap();
    // Each shell on a sway of its own, with the pointer of that sway. While the window sees
    // no move, the second is counted again.
    let counted = |bin: &str, dir: &str| {
        let sway = testenv::Sway::start();
        let _moving = match moved.then(|| moving(&sway)) {
            Some(None) => return None,
            m => m.flatten(),
        };
        let mut s = second(bin, &sway, &base.path().join(dir), mode);
        for _ in 0..4 {
            if !moved || s.moves > 0 {
                break;
            }
            s = second(bin, &sway, &base.path().join(dir), mode);
        }
        Some(s)
    };
    let (Some(qt), Some(here)) = (
        counted("qs", "orig"),
        counted(env!("CARGO_BIN_EXE_impulse-shell"), "native"),
    ) else {
        testenv::unavailable("compositor lacks zwlr_virtual_pointer_v1");
        return;
    };
    println!("{mode}: Quickshell {qt:?}\nhere {here:?}");
    if moved {
        assert!(
            qt.moves > 0 && here.moves > 0,
            "the pointer moves: Quickshell saw {}, here {}",
            qt.moves,
            here.moves
        );
    } else {
        assert!(
            matches!(mode, "alone" | "hidden" | "unseen") || (qt.laters > 0 && here.laters > 0),
            "the chain runs: Qt {} turns, here {}",
            qt.laters,
            here.laters
        );
    }
    counters_follow_qt("Quickshell", &qt);
    counters_follow_qt("here", &here);
    // Once per step of the animation driver, under both. Not as many a second of the wall
    // clock as under Quickshell: it commits a frame per step whether or not anything changed,
    // so on a loaded machine the second its Timer counts is 60 frames that took 1.0 to 1.8 s
    // (or 36 to 61 steps, once late frames put its driver on wall time), while impulse-shell commits
    // no frame that changes nothing and steps by its own timer then.
    for (who, s) in [("Quickshell", &qt), ("here", &here)] {
        assert!(
            s.frames.abs_diff(s.steps) <= 1,
            "{who}: triggered {} times in {} steps of the animation driver",
            s.frames,
            s.steps
        );
    }
    // No more steps than the time between the second trigger and the last has room for (the
    // first comes with `start()`, in the middle of a frame): a step per wakeup is hundreds of
    // times more. `elapsedTime` is read as the step begins, so it is late by no more than its
    // frame was.
    for (who, s) in [("Quickshell", &qt), ("here", &here)] {
        let span = s.elapsed - s.first[1][3];
        assert!(
            s.frames as f64 - 2.0 <= span / STEP + 1.0,
            "{who}: triggered {} times in the {span} s after the second trigger",
            s.frames - 1
        );
    }
    // And once per frame where each step changes what the window shows: the first trigger
    // comes with `start()`, each later one with a wl_surface.commit. impulse-shell draws and steps in
    // one thread; Qt's render thread commits on its own time, a commit before or after.
    for (who, s, slack) in [("Quickshell", &qt, 2), ("here", &here, 1)] {
        if let Some(commits) = s.commits {
            assert!(
                s.frames.abs_diff(commits + 1) <= slack,
                "{who}: triggered {} times, {commits} frames committed",
                s.frames
            );
        }
    }
    // A loaded machine drops frames under both.
    let per_s = |s: &Second| s.frames as f64 * 1000.0 / s.ms;
    assert!(
        2.0 * per_s(&here) >= per_s(&qt),
        "triggered {} times in {} ms, under Quickshell {} times in {} ms",
        here.frames,
        here.ms,
        qt.frames,
        qt.ms
    );
    // The frame times are wall time since the start: their sum is the counted second.
    for (who, s) in [("Quickshell", &qt), ("here", &here)] {
        assert!(
            (s.elapsed * 1000.0 - s.ms).abs() <= 0.1 * s.ms,
            "{who}: elapsedTime is {} s after {} ms",
            s.elapsed,
            s.ms
        );
    }
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_frame_animation_is_triggered_once_per_frame() {
    same_as_quickshell("alone");
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_frame_animation_is_triggered_once_per_frame_next_to_posted_events() {
    same_as_quickshell("later");
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_frame_animation_that_moves_an_item_is_triggered_once_per_frame_next_to_posted_events() {
    same_as_quickshell("moving");
}

/// QQuickFrameAnimation does not look at the item it is declared in.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_frame_animation_in_an_item_that_is_not_visible_is_triggered_once_per_frame() {
    same_as_quickshell("hidden");
}

/// With no window shown the animation driver steps by its own timer.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_frame_animation_in_a_window_that_is_not_visible_is_triggered_once_per_frame() {
    same_as_quickshell("unseen");
}

/// Input wakes the loop between frames: it is no step of the animation driver.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_frame_animation_is_triggered_once_per_frame_while_the_pointer_moves() {
    same_as_quickshell("moved");
}

/// On wall time too: the driver's time is read when a frame steps it, not when input comes.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_frame_animation_is_triggered_once_per_frame_while_frames_come_late() {
    same_as_quickshell("late");
}

/// Three frames of 70 ms put the driver on wall time. The handler of the sixth trigger then
/// takes 60 ms and starts animation `b`. A line per trigger: the count, `frameTime` in ms, and
/// where animations `a` (running from the start) and `b` are.
const LATE_START: &str = r#"import QtQuick
import Quickshell

FloatingWindow {
    id: root
    visible: true
    implicitWidth: 200
    implicitHeight: 200
    property var log: []
    property int n: 0
    function busy(ms) { var until = Date.now() + ms; while (Date.now() < until) {} }
    Rectangle { id: ra; width: 1; height: 1 }
    Rectangle { id: rb; width: 1; height: 1 }
    NumberAnimation { id: a; target: ra; property: "x"; from: 0; to: 100000; duration: 100000 }
    NumberAnimation { id: b; target: rb; property: "x"; from: 0; to: 100000; duration: 100000 }
    FrameAnimation {
        id: fa
        onTriggered: {
            root.n++
            root.log.push(root.n + ":" + frameTime * 1000 + ":" + ra.x + ":" + rb.x)
            if (root.n <= 3)
                root.busy(70)
            else if (root.n == 6) {
                root.busy(60)
                b.start()
            }
            if (root.n == 12) {
                console.log("LATE " + root.log.join(" "))
                Qt.quit()
            }
        }
    }
    Timer { interval: 500; running: true; onTriggered: { a.start(); fa.start() } }
}
"#;

/// The triggers of `LATE_START`: `frameTime` in ms, `a`, `b`.
fn late_start(bin: &str, dir: &Path) -> Vec<[f64; 3]> {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("shell.qml"), LATE_START).unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let sway = testenv::Sway::start();
    let mut c = Command::new(bin);
    sway.env(&mut c)
        .arg("-p")
        .arg(dir)
        .env("RI_INTERP", "1")
        .env("QS_NO_RELOAD_POPUP", "1")
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w);
    let _k = Kill(c.spawn().unwrap());
    drop(c);
    let log = Log::capture(r);
    let line =
        |s: &str| (s.lines()).find_map(|l| l.find("LATE ").map(|i| l[i + 5..].trim().to_string()));
    if let Err(s) = log.wait(|s| line(s).is_some()) {
        panic!("{bin}: exited before the twelfth trigger; log:\n{s}");
    }
    (line(&log.text()).unwrap().split(' '))
        .map(|t| {
            let v: Vec<f64> = t.split(':').skip(1).map(|n| n.parse().unwrap()).collect();
            v.try_into().unwrap()
        })
        .collect()
}

/// QQmlAnimationTimer::startAnimations runs QUnifiedTimer::maybeUpdateAnimationsToCurrentTime:
/// when animations start more than 50 ms after the last step of a driver on wall time, the
/// running ones step to the current time first, without waiting for a frame, and the new
/// ones count from there.
///
/// Told by the order of the steps and by where the animations are, not by the wall clock:
/// the trigger after the start is a step of `a` over the handler's 60 ms with `b` still at
/// its start, and `b` then moves by the steps `a` moves by. A `b` that joined the driver in
/// the handler would be 60 ms along at that trigger.
///
/// What is not told: whether that step was QUnifiedTimer's own or a frame's. Under Quickshell
/// startAnimations is a queued call, and a frame that was already due steps the driver before
/// it (one run in ten on a busy machine: `b` then joins a step later). Nor how soon the next
/// frame comes: 50.04 ms under Quickshell, once.
///
/// Under Quickshell a trigger sees `b` one step behind `a`, in impulse-shell at the same step.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn animations_started_long_after_a_late_frame_step_the_running_ones_first() {
    if !testenv::require("qs") || !testenv::require("sway") {
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let qt = late_start("qs", &base.path().join("orig"));
    let here = late_start(
        env!("CARGO_BIN_EXE_impulse-shell"),
        &base.path().join("native"),
    );
    for (who, t) in [("Quickshell", &qt), ("here", &here)] {
        let (a, b) = (|k: usize| t[k][1], |k: usize| t[k][2]);
        // The handler was busy for 60 ms of the clock the driver reads: a busy machine makes
        // the step longer, never shorter.
        assert!(
            b(6) == 0.0 && a(6) - a(5) >= 50.0,
            "{who}: the trigger after the start is a step of `a` over the handler's time, \
             with `b` at 0: {t:?}"
        );
        let Some(first) = (7..12).find(|&k| b(k) > 0.0) else {
            panic!("{who}: `b` does not run: {t:?}");
        };
        // Qt counts in whole ms.
        for k in first..12 {
            assert!(
                b(k) <= a(k) - a(6) + 1.0,
                "{who}: at trigger {}, `b` is further than the steps since its start: {t:?}",
                k + 1
            );
        }
        for k in first + 1..12 {
            let by = b(k) - b(k - 1);
            let same = (by - (a(k) - a(k - 1))).abs() <= 1.0;
            let behind = (by - (a(k - 1) - a(k - 2))).abs() <= 1.0;
            assert!(
                same || behind,
                "{who}: at trigger {}, `b` moved by {by}, not by a step of `a`: {t:?}",
                k + 1
            );
        }
    }
}
