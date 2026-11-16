//! A reload keeps the previous image's wallpaper on the screen until the new image shows its
//! own (the windows above it fade out before the `exec` and come as at a start,
//! `reload_handover.rs`). The first frame of a window whose picture loads on a worker and
//! then fades in (a wallpaper) is empty, so at that frame the old window still has to be
//! there: taken away then, the screen behind shows through for the length of the fade, a
//! dark flash of the wallpaper on every reload.
//!
//! The test runs its own sway: the panels of tests running meanwhile would sit at the same
//! spot of the shared one's screen the screenshots read.

use std::path::Path;
use std::process::{Child, Command, Stdio};

use testenv::Log;

const NATIVE: &str = env!("CARGO_BIN_EXE_impulse-shell");

const SHELL: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        // Where the shipped tree's wallpaper is: under the windows.
        WlrLayershell.layer: WlrLayer.Bottom
        color: "transparent"
        Image {
            anchors.fill: parent
            asynchronous: true
            source: "red.png"
            opacity: status === Image.Ready ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 1500 } }
            onOpacityChanged: if (opacity === 1) console.log("SHOWN")
        }
    }
    IpcHandler {
        target: "t"
        function reload(): void { Rustshell.reload(true) }
    }
}
"#;

fn sway() -> bool {
    testenv::require_all(&["sway", "grim"])
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "the_old_picture_stays_until_the_new_one_has_faded_in",
        &[("PATH", Some("/nonexistent"))],
    );
}

struct Shell(Child);

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The RGB pixel at `x,y` of `sway`'s screen.
fn pixel(sway: &testenv::Sway, dir: &Path, x: usize, y: usize) -> [u8; 3] {
    let f = dir.join("shot.ppm");
    let ok = sway
        .env(&mut Command::new("grim"))
        .args(["-t", "ppm", "-g", &format!("{x},{y} 1x1")])
        .arg(&f)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "grim");
    let b = std::fs::read(&f).unwrap();
    // P6\n1 1\n255\n<rgb>
    assert!(b.starts_with(b"P6") && b.len() >= 3, "grim's 1x1 capture");
    [b[b.len() - 3], b[b.len() - 2], b[b.len() - 1]]
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_old_picture_stays_until_the_new_one_has_faded_in() {
    if !sway() {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
    let red = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/image_status/a.png");
    std::fs::copy(red, dir.path().join("red.png")).unwrap();
    let xdg = dir.path().join("xdg");
    let (r, w) = std::io::pipe().unwrap();
    let child = sway
        .env(&mut Command::new(NATIVE))
        .arg("-p")
        .arg(dir.path())
        .env("RI_INTERP", "1")
        .env("XDG_CONFIG_HOME", xdg.join("config"))
        .env("XDG_STATE_HOME", xdg.join("state"))
        .env("XDG_CACHE_HOME", xdg.join("cache"))
        .stdout(Stdio::null())
        .stderr(w)
        .spawn()
        .unwrap();
    let _shell = Shell(child);
    let log = Log::capture(r);
    let shown = |s: &str, n: usize| s.matches("SHOWN").count() >= n;
    log.wait(|s| shown(s, 1))
        .unwrap_or_else(|l| panic!("the picture never showed:\n{l}"));
    // The frame that has it at full opacity reaches the screen.
    let at = std::time::Instant::now();
    while pixel(&sway, dir.path(), 32, 16) != [255, 0, 0] {
        assert!(at.elapsed().as_secs() < 20, "the picture is not on screen");
    }

    let st = sway
        .env(&mut Command::new(NATIVE))
        .arg("-p")
        .arg(dir.path())
        .args(["ipc", "call", "t", "reload"])
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "ipc call t reload");
    // From the reload until the new image's picture is at full opacity and a second on, the
    // picture is on the screen in every screenshot: the old window's, then the new one's.
    let mut seen = Vec::new();
    let mut end = None;
    loop {
        let p = pixel(&sway, dir.path(), 32, 16);
        if p != [255, 0, 0] {
            seen.push(p);
        }
        if end.is_none() && shown(&log.text(), 2) {
            end = Some(std::time::Instant::now());
        }
        if end.is_some_and(|e| e.elapsed().as_secs() >= 1) {
            break;
        }
        assert!(
            at.elapsed().as_secs() < 60,
            "the reloaded shell never showed its picture:\n{}",
            log.text()
        );
    }
    assert!(
        seen.is_empty(),
        "the picture left the screen during the reload: {seen:?}"
    );
}
