//! Posted events that post themselves again without end (a `Qt.callLater` function calling
//! `Qt.callLater` with itself, a Timer of interval 0 started again from its `triggered`) do not
//! hold the frames: Qt's loop delivers the events standing in the queue (one turn of
//! QCoreApplication::sendPostedEvents) and then goes back to its other sources, so an animation
//! next to the chains still steps at every frame and a Timer still fires on time. The real
//! quickshell running the fixture vs the native shell (`impulse-shell -p`): the frames an animation
//! got in the second a Timer counts, and how long that second took.
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

/// Half a second for the window to appear, then both chains start and the next firing of the
/// Timer, one second later, reports what the animation got meanwhile.
const SHELL: &str = r#"import QtQuick
import Quickshell

FloatingWindow {
    id: root
    visible: true
    implicitWidth: 200
    implicitHeight: 200
    property int moves: 0
    property int laters: 0
    property int timers: 0
    property real t0: 0
    function work() { var s = 0; for (var i = 0; i < 2000; i++) s += i; return s }
    function again() { root.laters++; root.work(); Qt.callLater(root.again) }
    Rectangle {
        width: 10; height: 10; color: "red"
        NumberAnimation on x { from: 0; to: 190; duration: 1000; loops: Animation.Infinite; running: true }
        onXChanged: root.moves++
    }
    Timer { id: self0; interval: 0; onTriggered: { root.timers++; start() } }
    Timer {
        interval: 500; running: true; repeat: true
        onTriggered: {
            if (root.t0 == 0) {
                interval = 1000
                root.moves = 0
                root.t0 = Date.now()
                Qt.callLater(root.again)
                self0.start()
            } else {
                console.log("FRAMES " + root.moves + " " + (Date.now() - root.t0) + " " + root.laters + " " + root.timers)
                Qt.quit()
            }
        }
    }
}
"#;

/// What the fixture reported under `bin`: the steps of the animation in the counted second,
/// the milliseconds that second took, and the turns of the two chains.
fn frames(bin: &str, dir: &Path) -> [u64; 4] {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("shell.qml"), SHELL).unwrap();
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
    let line = |s: &str| {
        (s.lines()).find_map(|l| l.find("FRAMES ").map(|i| l[i + 7..].trim().to_string()))
    };
    if let Err(s) = log.wait(|s| line(s).is_some()) {
        panic!("{bin}: exited before the second was counted; log:\n{s}");
    }
    let line = line(&log.text()).unwrap();
    let n: Vec<u64> = line.split(' ').map(|n| n.parse().unwrap()).collect();
    n.try_into().unwrap()
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn chains_of_posted_events_leave_the_frames_as_in_quickshell() {
    if !testenv::require("qs") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let [qt_moves, qt_ms, qt_laters, qt_timers] = frames("qs", &base.path().join("orig"));
    let [moves, ms, laters, timers] = frames(
        env!("CARGO_BIN_EXE_impulse-shell"),
        &base.path().join("native"),
    );
    assert!(
        qt_laters > 0 && qt_timers > 0 && laters > 0 && timers > 0,
        "both chains run: Qt {qt_laters} and {qt_timers} turns, here {laters} and {timers}"
    );
    // A loaded machine drops frames under both; a held frame costs far more than half of them.
    assert!(
        2 * moves >= qt_moves && ms <= 2 * qt_ms,
        "the animation stepped {moves} times in {ms} ms next to the chains, \
         under Quickshell {qt_moves} times in {qt_ms} ms"
    );
}
