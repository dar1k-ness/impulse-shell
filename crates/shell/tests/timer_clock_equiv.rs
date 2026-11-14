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

/// A known difference (docs/qt-differences.md, "A frame that comes late"): two frames of
/// 100 ms make the animation of 300 ms end some 145 ms late under Quickshell and 200 ms late
/// here, one vsync more for each late frame.
#[test]
#[ignore = "known difference, see docs/qt-differences.md: a late frame costs one vsync more \
            here than under Qt"]
fn late_frames_make_an_animation_as_late_as_under_quickshell() {
    let Some(runs) = both("two", 100) else {
        return;
    };
    let [qt, ours] = &runs;
    let (qt_end, our_end) = (at(qt, "end"), at(ours, "end"));
    assert!(
        (our_end - qt_end).abs() < 30.0,
        "the animation ended after {our_end} ms here, after {qt_end} ms under Quickshell"
    );
}
