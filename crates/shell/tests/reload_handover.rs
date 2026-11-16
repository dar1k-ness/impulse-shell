//! A reload changes the screen once. The windows the next image does not start with (a
//! sidebar or a menu opened since the start, the one the reload was asked from) leave before
//! the `exec`: nothing takes their place, and kept, they stay frozen on the screen until the
//! next image has drawn. The windows the next image makes stay unseen (wp_alpha_modifier_v1)
//! while the previous image's are there, and show in the frame those leave: drawn over them
//! as they come, the new bar shows for the length of the wallpaper's fade over the old one.
//!
//! The test runs its own sway: the panels of tests running meanwhile would sit at the same
//! spot of the shared one's screen the screenshots read.

use std::path::Path;
use std::process::{Child, Command, Stdio};

use testenv::Log;

const NATIVE: &str = env!("CARGO_BIN_EXE_impulse-shell");

/// A bar (blue at the start, yellow once `open` is called) with a picture that fades in for
/// 1.5 s, and a menu on the overlay layer over the bar's corner, closed at the start.
const SHELL: &str = r##"import QtQuick
import Rustshell
import Rustshell.Io
ShellRoot {
    id: root
    property color fill: "#0000ff"
    property bool menu: false
    property bool fading: false
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.namespace: "handover:bar"
        color: root.fill
        Image {
            x: 32; width: 32; height: 32
            asynchronous: true
            source: "red.png"
            opacity: status === Image.Ready ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 1500 } }
            onOpacityChanged: {
                if (opacity === 1)
                    console.log("SHOWN");
                else if (opacity > 0.2 && !root.fading) {
                    root.fading = true;
                    console.log("FADING");
                }
            }
        }
    }
    PanelWindow {
        visible: root.menu
        anchors { top: true; left: true }
        implicitWidth: 16; implicitHeight: 16
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.namespace: "handover:menu"
        WlrLayershell.layer: WlrLayer.Overlay
        color: "#00ff00"
    }
    IpcHandler {
        target: "t"
        function reload(): void { Rustshell.reload(true) }
        function open(): void { root.menu = true; root.fill = "#ffff00" }
    }
}
"##;

const BLUE: [u8; 3] = [0, 0, 255];
const YELLOW: [u8; 3] = [255, 255, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const RED: [u8; 3] = [255, 0, 0];

fn sway() -> bool {
    testenv::require_all(&["sway", "grim"])
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "the_screen_changes_once_at_a_reload",
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

/// What one screenshot shows: the corner the menu covers, the bar beside it, the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen {
    corner: [u8; 3],
    bar: [u8; 3],
    picture: [u8; 3],
}

fn seen(sway: &testenv::Sway, dir: &Path) -> Seen {
    let f = dir.join("shot.ppm");
    let ok = sway
        .env(&mut Command::new("grim"))
        .args(["-t", "ppm", "-g", "0,0 64x32"])
        .arg(&f)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "grim");
    let b = std::fs::read(&f).unwrap();
    // P6\n64 32\n255\n<rgb rows>
    assert!(
        b.starts_with(b"P6") && b.len() >= 64 * 32 * 3,
        "grim's capture"
    );
    let rgb = &b[b.len() - 64 * 32 * 3..];
    let at = |x: usize, y: usize| {
        let i = (y * 64 + x) * 3;
        [rgb[i], rgb[i + 1], rgb[i + 2]]
    };
    Seen {
        corner: at(8, 8),
        bar: at(24, 16),
        picture: at(48, 16),
    }
}

fn wait_seen(sway: &testenv::Sway, dir: &Path, want: Seen) {
    let at = std::time::Instant::now();
    loop {
        let s = seen(sway, dir);
        if s == want {
            return;
        }
        assert!(
            at.elapsed().as_secs() < 20,
            "the screen shows {s:?}, not {want:?}"
        );
    }
}

/// The shell running `qml` in `dir`, with what it writes.
fn run(sway: &testenv::Sway, dir: &Path, qml: &str) -> (Shell, std::sync::Arc<Log>) {
    std::fs::write(dir.join("shell.qml"), qml).unwrap();
    let red = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/image_status/a.png");
    std::fs::copy(red, dir.join("red.png")).unwrap();
    let xdg = dir.join("xdg");
    let (r, w) = std::io::pipe().unwrap();
    let child = sway
        .env(&mut Command::new(NATIVE))
        .arg("-p")
        .arg(dir)
        .env("RI_INTERP", "1")
        .env("XDG_CONFIG_HOME", xdg.join("config"))
        .env("XDG_STATE_HOME", xdg.join("state"))
        .env("XDG_CACHE_HOME", xdg.join("cache"))
        .stdout(Stdio::null())
        .stderr(w)
        .spawn()
        .unwrap();
    (Shell(child), Log::capture(r))
}

/// Whether the shell in `dir` took the call of `function`.
fn called(sway: &testenv::Sway, dir: &Path, function: &str) -> bool {
    sway.env(&mut Command::new(NATIVE))
        .arg("-p")
        .arg(dir)
        .args(["ipc", "call", "t", function])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn said(log: &Log, what: &str, n: usize) -> bool {
    log.text().matches(what).count() >= n
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_screen_changes_once_at_a_reload() {
    if !sway() {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    let (_shell, log) = run(&sway, dir.path(), SHELL);
    let shown = |s: &str, n: usize| s.matches("SHOWN").count() >= n;
    log.wait(|s| shown(s, 1))
        .unwrap_or_else(|l| panic!("the picture never showed:\n{l}"));
    let call = |function: &str| {
        assert!(called(&sway, dir.path(), function), "ipc call t {function}");
    };
    let start = Seen {
        corner: BLUE,
        bar: BLUE,
        picture: RED,
    };
    wait_seen(&sway, dir.path(), start);
    call("open");
    let open = Seen {
        corner: GREEN,
        bar: YELLOW,
        picture: RED,
    };
    wait_seen(&sway, dir.path(), open);

    call("reload");
    // The old image's bar is yellow with its menu green in the corner; the new image's is
    // blue with no menu, and has drawn all of it 1.5 s (the picture's fade) before it says
    // SHOWN. The screen goes from the first to the old bar without the menu, and from
    // that to the new bar at SHOWN: nothing of the new image before, nothing else between.
    let old = Seen {
        corner: YELLOW,
        ..open
    };
    let at = std::time::Instant::now();
    let mut steps = vec![open];
    let mut end = None;
    loop {
        let s = seen(&sway, dir.path());
        let new_drawn = shown(&log.text(), 2);
        if steps.last() != Some(&s) {
            steps.push(s);
        }
        assert!(
            s != start || new_drawn,
            "the new image's bar is on the screen before the old one left: {steps:?}"
        );
        assert!(
            s != open || !new_drawn,
            "the menu is still on the screen when the new image has drawn: {steps:?}"
        );
        if end.is_none() && new_drawn {
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
    assert_eq!(
        steps,
        [open, old, start],
        "the screen through the reload: the menu leaves, then the new bar takes the old one's place"
    );
}

/// A reload asked while the image before the running one still has its windows on the
/// screen: those end at the `exec`, and the running image's, unseen until then, are what the
/// screen keeps under the next one. Left unseen, the screen would show neither bar until
/// the next image has drawn.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_reload_during_a_reload_keeps_a_bar_on_the_screen() {
    if !sway() {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    // The picture fades for 4 s: the second reload is asked a second into that.
    let (_shell, log) = run(&sway, dir.path(), &SHELL.replace("1500", "4000"));
    log.wait(|s| s.contains("SHOWN"))
        .unwrap_or_else(|l| panic!("the picture never showed:\n{l}"));
    assert!(called(&sway, dir.path(), "open"), "ipc call t open");
    let open = Seen {
        corner: GREEN,
        bar: YELLOW,
        picture: RED,
    };
    wait_seen(&sway, dir.path(), open);
    assert!(called(&sway, dir.path(), "reload"), "ipc call t reload");
    // The second image draws its picture a fifth in: unseen, under the first one's bar.
    log.wait(|s| s.matches("FADING").count() >= 2)
        .unwrap_or_else(|l| panic!("the reloaded shell never drew its picture:\n{l}"));
    let old = Seen {
        corner: YELLOW,
        ..open
    };
    assert_eq!(seen(&sway, dir.path()), old, "the first image's bar stays");

    let at = std::time::Instant::now();
    while !called(&sway, dir.path(), "reload") {
        assert!(at.elapsed().as_secs() < 20, "ipc call t reload, the second");
    }
    let mut steps = vec![old];
    let mut end = None;
    loop {
        let s = seen(&sway, dir.path());
        if steps.last() != Some(&s) {
            steps.push(s);
        }
        assert!(
            [YELLOW, BLUE].contains(&s.bar),
            "no bar on the screen: {steps:?}"
        );
        // The third image is the second one to show all of its picture.
        if end.is_none() && said(&log, "SHOWN", 2) {
            end = Some(std::time::Instant::now());
        }
        if end.is_some_and(|e| e.elapsed().as_secs() >= 1) {
            break;
        }
        assert!(
            at.elapsed().as_secs() < 60,
            "the shell reloaded twice never showed its picture:\n{}",
            log.text()
        );
    }
    let start = Seen {
        corner: BLUE,
        bar: BLUE,
        picture: RED,
    };
    assert_eq!(
        steps.last(),
        Some(&start),
        "the third image's bar: {steps:?}"
    );
}

/// A menu asked for while the previous image's windows are still on the screen shows as it
/// draws: only the windows the next image starts with wait for those to leave.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_menu_opened_during_a_reload_shows_at_once() {
    if !sway() {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    let (_shell, log) = run(&sway, dir.path(), &SHELL.replace("1500", "4000"));
    log.wait(|s| s.contains("SHOWN"))
        .unwrap_or_else(|l| panic!("the picture never showed:\n{l}"));
    let start = Seen {
        corner: BLUE,
        bar: BLUE,
        picture: RED,
    };
    wait_seen(&sway, dir.path(), start);
    assert!(called(&sway, dir.path(), "reload"), "ipc call t reload");
    log.wait(|s| s.matches("FADING").count() >= 2)
        .unwrap_or_else(|l| panic!("the reloaded shell never drew its picture:\n{l}"));
    assert_eq!(
        seen(&sway, dir.path()),
        start,
        "the first image's bar stays"
    );

    let at = std::time::Instant::now();
    while !called(&sway, dir.path(), "open") {
        assert!(at.elapsed().as_secs() < 20, "ipc call t open");
    }
    // The menu, over the first image's bar: the second one's, yellow now, is not seen yet.
    let menu = Seen {
        corner: GREEN,
        ..start
    };
    wait_seen(&sway, dir.path(), menu);
    assert!(
        !said(&log, "SHOWN", 2),
        "the menu showed only once the new image had drawn"
    );
    let open = Seen {
        corner: GREEN,
        bar: YELLOW,
        picture: RED,
    };
    wait_seen(&sway, dir.path(), open);
}
