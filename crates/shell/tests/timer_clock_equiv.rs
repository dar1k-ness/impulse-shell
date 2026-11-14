//! Timers count on the animation clock (QQmlTimer is a QPauseAnimation). While an animation
//! runs, that clock moves one vsync per frame, so frames that come late leave it behind the
//! wall clock, and the Timers with it. When the animation ends and only Timers are left,
//! QUnifiedTimer stops the driver and reads the wall clock again: the Timer due first fires
//! when it was due on the clock that stood behind, and at that moment the others get the time
//! the clock was behind by. A repeating Timer that this takes across several of its intervals
//! is triggered once. The real quickshell running the fixture vs the native shell (`rusty-impulse -p`).
use std::path::Path;
use std::process::{Child, Command, Stdio};

use testenv::Log;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A NumberAnimation of 300 ms whose second and third frame each keep the thread busy for
/// `BUSY` ms, and the Timers of `MODE` started with it: `two` is a Timer of 500 ms (`A`) and
/// one of 800 ms (`B`), `rep` one that repeats every 100 ms (`r`). The line has when the
/// animation ended and when each Timer was triggered, in ms of the wall clock since the start.
const SHELL: &str = r#"import QtQuick
import Quickshell

FloatingWindow {
    id: root
    visible: true
    implicitWidth: 200
    implicitHeight: 200
    property real t0: 0
    property real v: 0
    property int frames: 0
    property int ticks: 0
    property var seen: []
    function busy(ms) {
        const until = Date.now() + ms
        while (Date.now() < until) {}
    }
    function note(name) { seen.push(name + "=" + (Date.now() - t0)) }
    function done() {
        console.log("CLOCK " + seen.join(" "))
        Qt.quit()
    }
    NumberAnimation on v {
        id: an
        running: false
        from: 0; to: 1000; duration: 300
        onFinished: root.note("end")
    }
    onVChanged: {
        frames++
        if (frames == 2 || frames == 3)
            busy(BUSY)
    }
    Timer { id: a; interval: 500; onTriggered: root.note("A") }
    Timer { id: b; interval: 800; onTriggered: { root.note("B"); root.done() } }
    Timer {
        id: r
        interval: 100
        repeat: true
        onTriggered: {
            root.note("r")
            if (++root.ticks == 12)
                root.done()
        }
    }
    Timer {
        interval: 500
        running: true
        onTriggered: {
            root.t0 = Date.now()
            an.start()
            if ("MODE" == "two") {
                a.start()
                b.start()
            } else {
                r.start()
            }
        }
    }
}
"#;

/// The names and times of the fixture's line.
fn times(bin: &str, dir: &Path, mode: &str, busy: u32) -> Vec<(String, f64)> {
    let qml = (SHELL.replace("MODE", mode)).replace("BUSY", &busy.to_string());
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("shell.qml"), qml).unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let mut c = Command::new(bin);
    c.arg("-p")
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
        |s: &str| (s.lines()).find_map(|l| l.find("CLOCK ").map(|i| l[i + 6..].trim().to_string()));
    if let Err(s) = log.wait(|s| line(s).is_some()) {
        panic!("{bin}: {mode} exited before its Timers were through; log:\n{s}");
    }
    (line(&log.text()).unwrap().split(' '))
        .map(|e| {
            let (name, at) = e.split_once('=').unwrap();
            (name.to_string(), at.parse().unwrap())
        })
        .collect()
}

/// The fixture under Quickshell and under the native shell.
fn both(mode: &str, busy: u32) -> Option<[Vec<(String, f64)>; 2]> {
    if !testenv::require("qs") {
        return None;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return None;
    }
    let base = tempfile::tempdir().unwrap();
    let qt = times("qs", &base.path().join("orig"), mode, busy);
    let ours = times(
        env!("CARGO_BIN_EXE_rusty-impulse"),
        &base.path().join("native"),
        mode,
        busy,
    );
    Some([qt, ours])
}

fn at(times: &[(String, f64)], name: &str) -> f64 {
    (times.iter().find(|(n, _)| n == name))
        .unwrap_or_else(|| panic!("no {name} in {times:?}"))
        .1
}

/// Two frames of 100 ms leave the clock some 150 ms behind (Quickshell) when the animation
/// ends. `A` had 184 ms left then and fires that much later. `B` is 300 ms after `A` on the
/// clock, and less by what the clock was behind on the wall: it gets that when `A` fires.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_second_timer_gets_the_time_the_animation_clock_was_behind() {
    let Some(runs) = both("two", 100) else {
        return;
    };
    let shape = |t: &[(String, f64)]| {
        let (end, a, b) = (at(t, "end"), at(t, "A"), at(t, "B"));
        (a - end > 150.0, b - a < 250.0)
    };
    let [qt, ours] = &runs;
    assert_eq!(
        shape(qt),
        (true, true),
        "Quickshell: A fires as late as the animation ended, B less than 300 ms after: {qt:?}"
    );
    assert_eq!(
        shape(ours),
        shape(qt),
        "A more than 150 ms after the end, B less than 250 ms after A: here {ours:?}, \
         under Quickshell {qt:?}"
    );
}

/// Two frames of 130 ms: when the repeating Timer fires first after the animation, the time
/// the clock was behind takes it across more than one interval. It is triggered once for
/// them, so it has fewer triggers than intervals went by, and from then on it fires on the
/// wall clock's hundreds.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_repeating_timer_is_triggered_once_for_the_intervals_the_clock_was_behind() {
    let Some(runs) = both("rep", 130) else {
        return;
    };
    let shape = |t: &[(String, f64)]| {
        let r: Vec<f64> = (t.iter().filter(|(n, _)| n == "r").map(|e| e.1)).collect();
        let last = r[r.len() - 1];
        // From 20 ms before a hundred (a clock read a moment apart) to 40 ms after it.
        let mut off: Vec<f64> = (r[r.len() - 5..].iter())
            .map(|at| (at + 20.0) % 100.0)
            .collect();
        off.sort_by(f64::total_cmp);
        ((r.len() as f64) < (last / 100.0).floor(), off[2] < 60.0)
    };
    let [qt, ours] = &runs;
    assert_eq!(
        shape(qt),
        (true, true),
        "Quickshell: fewer triggers than intervals, the last ones on the hundreds: {qt:?}"
    );
    assert_eq!(
        shape(ours),
        shape(qt),
        "fewer triggers than intervals, the last ones on the hundreds: here {ours:?}, \
         under Quickshell {qt:?}"
    );
}

/// A NumberAnimation of 300 ms that says each of its steps: the wall clock and the animation's
/// own clock, in ms since `start()`. The steps `LATE` each keep the thread busy for `BUSY` ms.
/// With `PAINT` each step changes what the window shows.
const STEPS: &str = r#"import QtQuick
import Quickshell

FloatingWindow {
    id: root
    visible: true
    implicitWidth: 200
    implicitHeight: 200
    property real t0: 0
    property real v: 0
    property int frames: 0
    property var seen: []
    function busy(ms) {
        const until = Date.now() + ms
        while (Date.now() < until) {}
    }
    Rectangle { width: PAINT ? 10 + root.v / 10 : 10; height: 50; color: "red" }
    NumberAnimation on v {
        id: an
        running: false
        from: 0; to: 3000; duration: 300
        onFinished: {
            console.log("STEPS " + seen.join(" ") + " end=" + (Date.now() - t0))
            Qt.quit()
        }
    }
    onVChanged: {
        frames++
        seen.push((Date.now() - t0) + ":" + Math.round(v / 10))
        console.log("STEP " + frames)
        if (LATE) {
            busy(BUSY)
            console.log("late step over " + (Date.now() - t0))
        }
    }
    Timer {
        interval: 500
        running: true
        onTriggered: { root.t0 = Date.now(); an.start() }
    }
}
"#;

/// The steps of [`STEPS`] as (wall, clock), and when the animation ended.
type Run = (Vec<(f64, f64)>, f64);

/// One run of [`STEPS`] under `bin`.
fn steps(bin: &str, dir: &Path, late: &str, busy: u32, paint: bool) -> Run {
    steps_logged(bin, dir, late, busy, paint, false).0
}

/// One run of [`STEPS`] under `bin`, and its log: the lines `STEP n` and `late step over` and,
/// with `trace`, the Wayland requests and events (`WAYLAND_DEBUG`), in the order they happened.
fn steps_logged(
    bin: &str,
    dir: &Path,
    late: &str,
    busy: u32,
    paint: bool,
    trace: bool,
) -> (Run, String) {
    let qml = (STEPS.replace("LATE", late))
        .replace("BUSY", &busy.to_string())
        .replace("PAINT", &paint.to_string());
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("shell.qml"), qml).unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let mut c = Command::new(bin);
    c.arg("-p")
        .arg(dir)
        .env("RI_INTERP", "1")
        .env("QS_NO_RELOAD_POPUP", "1")
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w);
    if trace {
        c.env("WAYLAND_DEBUG", "1");
    }
    let _k = Kill(c.spawn().unwrap());
    drop(c);
    let log = Log::capture(r);
    let line =
        |s: &str| (s.lines()).find_map(|l| l.find("STEPS ").map(|i| l[i + 6..].trim().to_string()));
    if let Err(s) = log.wait(|s| line(s).is_some()) {
        panic!("{bin}: exited before the animation ended; log:\n{s}");
    }
    let text = log.text();
    let line = line(&text).unwrap();
    let (steps, end) = line.rsplit_once(" end=").unwrap();
    let steps = (steps.split(' '))
        .map(|e| {
            let (wall, clock) = e.split_once(':').unwrap();
            (wall.parse().unwrap(), clock.parse().unwrap())
        })
        .collect();
    ((steps, end.parse().unwrap()), text)
}

/// How many steps a run of [`STEPS`] took between the end of its last late step and the
/// `nth` frame callback after it, of the frames the window asked for once the late step was
/// over. Counted by the order of the lines in `log`, no clock.
fn steps_by_callback(log: &str, nth: usize) -> usize {
    let lines: Vec<&str> = log.lines().collect();
    let end = (lines.iter().rposition(|l| l.contains("late step over"))).expect("a late step");
    let id = |l: &str, before: &str| {
        let at = l.find(before)?;
        let from = l[..at].rfind("wl_callback#")? + "wl_callback#".len();
        l[from..at].parse::<u32>().ok()
    };
    let (mut asked, mut answered, mut steps) = (Vec::new(), 0, 0);
    for l in &lines[end + 1..] {
        if l.contains(".frame(new id wl_callback#") {
            asked.extend(id(l, ")"));
        } else if let Some(k) = id(l, ".done(").and_then(|n| asked.iter().position(|&a| a == n)) {
            asked.remove(k);
            answered += 1;
            if answered == nth {
                return steps;
            }
        } else if l.contains("STEP ") {
            steps += 1;
        }
    }
    panic!("no {nth} frame callbacks after the late step; log:\n{log}");
}

/// [`STEPS`] under Quickshell and under the native shell.
fn both_steps(late: &str, busy: u32, paint: bool) -> Option<[Run; 2]> {
    if !testenv::require("qs") {
        return None;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return None;
    }
    let base = tempfile::tempdir().unwrap();
    let qt = steps("qs", &base.path().join("orig"), late, busy, paint);
    let ours = steps(
        env!("CARGO_BIN_EXE_rusty-impulse"),
        &base.path().join("native"),
        late,
        busy,
        paint,
    );
    Some([qt, ours])
}

/// While the driver runs, the animation clock moves one vsync (16.67 ms) per step whatever the
/// wall clock did (QSGDefaultAnimationDriver::advance in vsync mode: `m_time += m_vsync`): a
/// step whose handler kept the thread busy for 100 ms is followed by a step one vsync further
/// on the clock, and the 100 ms are lost to the animation. The same whether the steps change
/// what the window shows or not, under Quickshell and here.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_animation_clock_moves_one_vsync_a_step_whatever_the_late_step_took() {
    for paint in [false, true] {
        let Some(runs) = both_steps("frames == 5", 100, paint) else {
            return;
        };
        for ((steps, _), who) in runs.iter().zip(["Quickshell", "here"]) {
            assert!(
                steps.len() > 6 && steps[5].0 - steps[4].0 >= 100.0,
                "{who}, paint {paint}: the fifth step took 100 ms: {steps:?}"
            );
            for (n, (_, clock)) in steps[..6].iter().enumerate() {
                let vsyncs = (n + 1) as f64 * 50.0 / 3.0;
                assert!(
                    (clock - vsyncs).abs() <= 1.0,
                    "{who}, paint {paint}: step {} is at {vsyncs:.0} ms of the animation clock, \
                     one vsync a step: {steps:?}",
                    n + 1
                );
            }
        }
    }
}

/// [`STEPS`] with steps of 100 ms under Quickshell, with its trace, and under the native shell,
/// with its trace when its frames are sent (`paint`).
fn late_steps(late: &str, paint: bool) -> Option<[(Run, String); 2]> {
    if !testenv::require("qs") {
        return None;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return None;
    }
    let base = tempfile::tempdir().unwrap();
    let qt = steps_logged("qs", &base.path().join("orig"), late, 100, paint, true);
    let ours = env!("CARGO_BIN_EXE_rusty-impulse");
    let ours = steps_logged(ours, &base.path().join("native"), late, 100, paint, paint);
    Some([qt, ours])
}

/// How many steps a run of [`STEPS`] that sends no frame took at once when its last late step
/// was over: less than half a vsync after it, on the fixture's own clock.
fn steps_at_once((steps, _): &Run, log: &str) -> usize {
    let (_, over) = (log.lines().rev())
        .find_map(|l| l.split_once("late step over "))
        .expect("a late step");
    let digits = over.find(|c: char| !c.is_ascii_digit());
    let over: f64 = over[..digits.unwrap_or(over.len())].parse().unwrap();
    (steps.iter())
        .filter(|(wall, _)| (over..over + 8.0).contains(wall))
        .count()
}

/// The steps change nothing the window shows; the handlers of the second and of the third keep
/// the thread busy for 100 ms each. Three steps follow at once. Under Quickshell (it sends a
/// frame each step whatever changed): the step of the frame callback that came meanwhile,
/// the one of the update request that found no callback to wait for, and the one of sway's
/// answer to the frame they sent, all before the callback of the second frame sent after the
/// late steps; counted by the order of the lines of its trace. Here no frame is sent: the
/// three ticks of the vsync timer that were missed are due at once, and the animation ends as
/// late as there (docs/qt-differences.md, "A frame that comes late").
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn late_frames_make_an_animation_as_late_as_under_quickshell() {
    let Some([(_, qt), (ours, our_log)]) = late_steps("frames == 2 || frames == 3", false) else {
        return;
    };
    let (qt_steps, our_steps) = (steps_by_callback(&qt, 2), steps_at_once(&ours, &our_log));
    assert_eq!(
        qt_steps, 3,
        "Quickshell: three steps after the two of 100 ms before the second frame callback; \
         log:\n{qt}"
    );
    assert_eq!(
        our_steps, qt_steps,
        "steps at once after the two of 100 ms: {our_steps} here, {qt_steps} under Quickshell; \
         here {ours:?}"
    );
}

/// The steps change nothing the window shows; the handler of the fifth keeps the thread busy
/// for 100 ms. The sixth step is taken as soon as the fifth is over, and two more at once.
/// Under Quickshell the second of them is its main thread asking for an update before its
/// render thread has asked for the next frame callback (QWaylandWindow::requestUpdate): three
/// steps before the second frame callback in 15 runs of 20, two in 5. Here the ticks of the
/// vsync timer that were missed are due at once, three of them: as many as Quickshell takes
/// at most.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_late_step_makes_an_animation_that_draws_nothing_as_late_as_under_quickshell() {
    let Some([(_, qt), (ours, our_log)]) = late_steps("frames == 5", false) else {
        return;
    };
    let (qt_steps, our_steps) = (steps_by_callback(&qt, 2), steps_at_once(&ours, &our_log));
    assert!(
        qt_steps == 2 || qt_steps == 3,
        "Quickshell: two or three steps after the one of 100 ms before the second frame \
         callback, not {qt_steps}; log:\n{qt}"
    );
    assert_eq!(
        our_steps, 3,
        "steps at once after the one of 100 ms: {our_steps} here, {qt_steps} under Quickshell \
         in this run, three at most; here {ours:?}"
    );
}

/// Each step changes what the window shows; the handlers of the fifth and of the sixth keep
/// the thread busy for 100 ms each. Three steps follow at once, before the compositor has
/// answered the second frame sent after them: under Quickshell an update request and the
/// callback of the frame sent between the two wait in the queue of the main thread, and sway
/// answers the frame they send at once; here the callback's step, the one owed for the frames
/// that were in flight, and sway's answer. Then one step per frame callback: five by the
/// fourth callback, which is what is compared. A thread that got no processor for a frame
/// takes the third of them after the second callback instead of before it (seen under
/// Quickshell with every core busy), and has taken as many by the fourth. Counted by the
/// order of the lines of both traces.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn two_late_steps_in_a_row_are_followed_by_as_many_steps_at_once_as_under_quickshell() {
    let Some([(_, qt), (_, ours)]) = late_steps("frames == 5 || frames == 6", true) else {
        return;
    };
    let [qt_steps, our_steps] = [&qt, &ours].map(|log| steps_by_callback(log, 4));
    assert_eq!(
        qt_steps, 5,
        "Quickshell: five steps after the two of 100 ms before the fourth frame callback; \
         log:\n{qt}"
    );
    assert_eq!(
        our_steps, qt_steps,
        "steps after the two of 100 ms before the fourth frame callback: {our_steps} here, \
         {qt_steps} under Quickshell; log:\n{ours}"
    );
}
