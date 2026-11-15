//! The `ri::present` log says when a window's frame is on screen, also when the commit that
//! carried the frame was superseded before the compositor repainted. sway answers a layer
//! surface's first buffer with a second configure; the shell acks it with a commit that has no
//! new frame, and when that commit reaches sway before its next repaint, the frame's own
//! wp_presentation feedback is `discarded`. A second client with an endless animation keeps
//! sway's output in its repaint cycle (a repaint waits for the output's next frame), which is
//! when the ack gets there first.
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;
use testenv::wayland::presented_after;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const BUSY: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { bottom: true; right: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        Component.onCompleted: console.log("PRESENT busy")
        Rectangle { id: dot; width: 4; height: 4; color: "#0000ff" }
        NumberAnimation {
            target: dot; property: "x"; from: 0; to: 60; duration: 1000
            loops: Animation.Infinite; running: true
        }
    }
}
"##;

const STILL: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: "#ff0000"
        Component.onCompleted: console.log("PRESENT still")
    }
}
"##;

/// impulse-shell running `qml` under `sway`, with its Wayland traffic and presentation feedback logged.
fn shell(sway: &testenv::Sway, qml: &str) -> (Kill, tempfile::TempDir, Arc<Log>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), qml).unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let child = sway
        .env(&mut Command::new(env!("CARGO_BIN_EXE_impulse-shell")))
        .arg("-p")
        .arg(dir.path())
        .env("RI_INTERP", "1")
        .env("WAYLAND_DEBUG", "1")
        .env("RI_LOG", "ri::present=debug")
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w)
        .spawn()
        .unwrap();
    (Kill(child), dir, Log::capture(r))
}

fn wait_presented(log: &Log, mark: &str) {
    if let Err(s) = log.wait(|s| presented_after(s, |l| l.contains(mark))) {
        panic!("impulse-shell exited before a frame after `{mark}` was presented:\n{s}");
    }
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "first_frame_is_reported_presented_while_the_output_repaints",
        &[("PATH", Some("/nonexistent"))],
    );
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn first_frame_is_reported_presented_while_the_output_repaints() {
    if !testenv::require("sway") {
        return;
    }
    let sway = testenv::Sway::start();
    let (_busy, _busy_dir, busy) = shell(&sway, BUSY);
    wait_presented(&busy, "PRESENT busy");
    let (_still, _still_dir, still) = shell(&sway, STILL);
    wait_presented(&still, "PRESENT still");
}
