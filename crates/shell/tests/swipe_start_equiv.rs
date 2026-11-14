//! SwipeView moves to its `currentIndex` with the QSmoothedAnimation of its ListView's
//! highlight, a job of QQmlAnimationTimer as any animation: it starts when `currentIndex` is
//! set, and QQmlAnimationTimer::startAnimations steps a clock that is behind to the current
//! time first (QUnifiedTimer::maybeUpdateAnimationsToCurrentTime). The move begins at that
//! step, however long ago the step before it was. The real quickshell running the fixture vs
//! the native shell (`rusty-impulse -p`).
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

/// The line `SW ...` of `qml` run by `bin`, split at the spaces.
fn line(bin: &str, dir: &Path, qml: &str) -> Vec<String> {
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
        |s: &str| (s.lines()).find_map(|l| l.find("SW ").map(|i| l[i + 3..].trim().to_string()));
    if let Err(s) = log.wait(|s| line(s).is_some()) {
        panic!("{bin}: exited before its line; log:\n{s}");
    }
    (line(&log.text()).unwrap().split(' '))
        .map(str::to_string)
        .collect()
}

/// The fixture's line under Quickshell and under the native shell.
fn both(qml: &str) -> Option<[Vec<String>; 2]> {
    if !testenv::require("qs") {
        return None;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return None;
    }
    let base = tempfile::tempdir().unwrap();
    let qt = line("qs", &base.path().join("orig"), qml);
    let ours = line(
        env!("CARGO_BIN_EXE_rusty-impulse"),
        &base.path().join("native"),
        qml,
    );
    Some([qt, ours])
}

/// Where the pages of a SwipeView 200 wide are, in px, `ms` after it began to move two pages
/// on: QSmoothedAnimation with the velocity of 400 px/s and the 250 ms of the highlight move,
/// half of the time speeding up and half slowing down.
fn moved(ms: f64) -> f64 {
    let t = ms.clamp(0.0, 250.0) / 1000.0;
    if t < 0.125 {
        12800.0 * t * t
    } else {
        400.0 - 12800.0 * (0.25 - t) * (0.25 - t)
    }
}

/// An animation of 100 s gives the clock's time in `ra.x`. Three triggers of 70 ms put the
/// driver on the wall clock; the sixth is busy for 60 ms and sets `currentIndex`. Each trigger
/// writes `frameTime` in ms, `ra.x` and where the pages are (the FrameAnimation started
/// before the move, so it sees them as the step before left them).
const LATE: &str = r#"import QtQuick
import QtQuick.Controls
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
    NumberAnimation { id: a; target: ra; property: "x"; from: 0; to: 100000; duration: 100000 }
    SwipeView {
        id: view
        width: 200; height: 100
        Item { id: p0 }
        Item {}
        Item {}
    }
    FrameAnimation {
        id: fa
        onTriggered: {
            root.n++
            root.log.push(root.n + ":" + frameTime * 1000 + ":" + ra.x + ":" + -p0.mapToItem(view, 0, 0).x)
            if (root.n <= 3)
                root.busy(70)
            else if (root.n == 6) {
                root.busy(60)
                view.currentIndex = 2
            }
            if (root.n == 14) {
                console.log("SW " + root.log.join(" "))
                Qt.quit()
            }
        }
    }
    Timer { interval: 500; running: true; onTriggered: { a.start(); fa.start() } }
}
"#;

/// More than 50 ms after the last step of a driver on wall time, setting `currentIndex` steps
/// what runs to the current time at once, and the pages move from that step: they are where
/// the time since then puts them, not where the time since the step before would.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_swipe_view_moves_from_the_step_to_the_current_time() {
    let Some(runs) = both(LATE) else {
        return;
    };
    for (who, run) in ["Quickshell", "here"].iter().zip(&runs) {
        let t: Vec<[f64; 3]> = (run.iter())
            .map(|t| {
                let v: Vec<f64> = t.split(':').skip(1).map(|n| n.parse().unwrap()).collect();
                v.try_into().unwrap()
            })
            .collect();
        let [time, began, at] = t[6];
        assert!(
            time >= 50.0 && at == 0.0 && t[7][0] < 50.0,
            "{who}: the trigger after `currentIndex` was set is the step to the current \
             time, and the frame follows it: {t:?}"
        );
        for k in 7..t.len() {
            let (ms, at) = (t[k - 1][1] - began, t[k][2]);
            assert!(
                (moved(ms - 2.0)..=moved(ms + 2.0)).contains(&at),
                "{who}: trigger {} sees the pages at {at}, {ms} ms after that step is {}: {t:?}",
                k + 1,
                moved(ms)
            );
        }
    }
}

/// A NumberAnimation of 300 ms whose second and third step are busy for 100 ms each ends with
/// the clock behind the wall, and a Timer waits on that clock. A process that ends 100 ms
/// later sets `currentIndex`. The line has, for each time the pages moved, the ms since then
/// and where they are.
const BEHIND: &str = r#"import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io

FloatingWindow {
    id: root
    visible: true
    implicitWidth: 200
    implicitHeight: 200
    property var log: []
    property real v: 0
    property int frames: 0
    property real t0: 0
    property real t1: 0
    function busy(ms) { var until = Date.now() + ms; while (Date.now() < until) {} }
    function done() {
        console.log("SW " + root.log.join(" "))
        Qt.quit()
    }
    function moved() {
        if (root.t1 == 0)
            return
        const at = -p0.mapToItem(view, 0, 0).x
        root.log.push((Date.now() - root.t1) + ":" + at)
        if (at == 400)
            root.done()
    }
    NumberAnimation on v {
        id: an
        running: false
        from: 0; to: 1000; duration: 300
        onFinished: { root.log.push("end:" + (Date.now() - root.t0)); proc.running = true }
    }
    onVChanged: {
        frames++
        if (frames == 2 || frames == 3)
            busy(100)
    }
    SwipeView {
        id: view
        width: 200; height: 100
        Item { id: p0 }
        Item {}
        Item {}
    }
    // The pages are in the ListView that is the contentItem; here the SwipeView places them.
    Component.onCompleted: {
        if (view.contentItem)
            view.contentItem.contentXChanged.connect(root.moved)
        else
            p0.xChanged.connect(root.moved)
    }
    Process {
        id: proc
        command: ["sleep", "0.1"]
        onExited: {
            root.t1 = Date.now()
            view.currentIndex = 2
        }
    }
    Timer { id: far; interval: 5000; onTriggered: root.done() }
    Timer {
        interval: 500
        running: true
        onTriggered: { root.t0 = Date.now(); an.start(); far.start() }
    }
}
"#;

/// The driver stopped behind the wall clock and starts again for the move: the time it was
/// behind is not time the pages moved for. They begin with one step of a frame, and take
/// the frames of their 250 ms to arrive.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_swipe_view_does_not_move_for_the_time_the_stopped_clock_was_behind() {
    let Some(runs) = both(BEHIND) else {
        return;
    };
    for (who, run) in ["Quickshell", "here"].iter().zip(&runs) {
        let t: Vec<(&str, f64)> = (run.iter())
            .map(|e| {
                let (name, at) = e.split_once(':').unwrap();
                (name, at.parse().unwrap())
            })
            .collect();
        assert!(
            t[0].0 == "end" && t[0].1 > 400.0,
            "{who}: the animation of 300 ms ended more than 100 ms late: {t:?}"
        );
        let first = t[1].1;
        assert!(
            first > 0.0 && first <= moved(25.0) && t.len() > 10 && t[t.len() - 1].1 == 400.0,
            "{who}: the pages begin with the step of one frame ({} px after 16 ms) and \
             arrive after ten more at least: {t:?}",
            moved(16.0)
        );
    }
}
