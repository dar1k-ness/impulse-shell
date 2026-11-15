//! The wallpaper of `Background.qml` while its output's scale changes, the real Quickshell
//! (Qt 6) vs the native shell on the same config, each in a sway of its own: an asynchronous
//! Image a little larger than the screen whose `sourceSize` follows the window's
//! `devicePixelRatio`, shown once it is Ready. The scale goes 1, 1.25, 1.5, 2 with the shell
//! running; at 2 the image is 4109x2311, 36 MiB decoded, more than the image cache keeps of
//! images no frame draws. QQuickImage holds the pixmap it loaded: the wallpaper is on screen at
//! every scale. A shell whose cache evicts the decode before the Image is laid out decodes it
//! again, for ever, and shows no wallpaper.
use std::path::Path;
use std::process::{Child, Command, Stdio};

use qsg::raster::Canvas;
use testenv::Log;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const NATIVE: &str = env!("CARGO_BIN_EXE_impulse-shell");
const WALLPAPER: [u8; 3] = [255, 0, 0];

const SHELL: &str = r##"//@ pragma Env QS_NO_RELOAD_POPUP=1
import QtQuick
import Quickshell
ShellRoot {
    PanelWindow {
        anchors { top: true; bottom: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        color: "#0000ff"
        Image {
            id: wallpaper
            asynchronous: true
            retainWhileLoading: true
            cache: false
            smooth: false
            visible: opacity > 0
            opacity: status === Image.Ready ? 1 : 0
            source: "wall.png"
            fillMode: Image.PreserveAspectCrop
            anchors.centerIn: parent
            // The screen's size when the shell started, times the workspace zoom.
            width: 1920 * 1.07
            height: 1080 * 1.07
            readonly property real dpr: (QsWindow.window as QsWindow)?.devicePixelRatio ?? 1
            sourceSize: Qt.size(width * dpr, height * dpr)
            onStatusChanged: console.log("WALL status " + status + " dpr [" + dpr + "]")
        }
    }
}
"##;

fn swaymsg(sway: &testenv::Sway, cmd: &str) {
    let st = Command::new("swaymsg")
        .env("SWAYSOCK", sway.ipc_socket())
        .arg(cmd)
        .stdout(Stdio::null())
        .status()
        .expect("swaymsg");
    assert!(st.success(), "swaymsg {cmd}");
}

/// The color in the middle of HEADLESS-1 and near two of its corners.
fn shot(sway: &testenv::Sway, dir: &Path) -> [[u8; 3]; 3] {
    let f = dir.join("shot.ppm");
    let st = sway
        .env(&mut Command::new("grim"))
        .args(["-t", "ppm", "-o", "HEADLESS-1"])
        .arg(&f)
        .status()
        .expect("grim");
    assert!(st.success(), "grim");
    let b = std::fs::read(&f).unwrap();
    // P6\n<w> <h>\n255\n<rgb>
    let head: Vec<&[u8]> = b.splitn(4, |c| c.is_ascii_whitespace()).collect();
    let num = |f: &[u8]| -> usize { String::from_utf8_lossy(f).parse().unwrap() };
    let (w, h) = (num(head[1]), num(head[2]));
    let px = &head[3][head[3].iter().position(|c| *c == b'\n').unwrap() + 1..];
    let at = |x: usize, y: usize| {
        let o = (y * w + x) * 3;
        [px[o], px[o + 1], px[o + 2]]
    };
    [at(w / 2, h / 2), at(10, 10), at(w - 10, h - 10)]
}

/// A frame the shell committed in `log` is on screen: its frame callback was answered
/// (Quickshell) or its presentation feedback `presented` (impulse-shell under `RI_LOG=ri::present`).
fn frame_shown(log: &str) -> bool {
    let lines: Vec<&str> = log.lines().collect();
    let id = |line: &str, request: &str| {
        let (_, r) = line.split_once(request)?;
        r.split(')').next().map(str::to_string)
    };
    (0..lines.len()).any(|i| {
        let later = &lines[i + 1..];
        if let Some(id) = id(lines[i], ".frame(new id wl_callback#") {
            let done = format!("wl_callback#{id}.done(");
            later.iter().any(|l| l.contains(&done))
        } else if let Some(id) = id(lines[i], "new id wp_presentation_feedback#") {
            let presented = format!("wp_presentation_feedback#{id}.presented(");
            later.iter().any(|l| l.contains(&presented))
        } else {
            false
        }
    })
}

/// What `bin` shows on the output at each scale of 1, 1.25, 1.5 and 2, set in that order while
/// it runs.
fn shown(bin: &str) -> Vec<String> {
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
    let [r, g, b] = WALLPAPER;
    Canvas::new(64, 36, [r, g, b, 255])
        .save_png(dir.path().join("wall.png").to_str().unwrap())
        .unwrap();
    // Not in the configuration's directory: a file written there reloads it.
    let shots = tempfile::tempdir().unwrap();
    swaymsg(&sway, "output HEADLESS-1 scale 1");
    let (rd, wr) = std::io::pipe().unwrap();
    let _shell = Kill(
        sway.env(&mut Command::new(bin))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("RI_LOG", "ri::present=debug,ri::imgdbg=debug")
            .stdin(Stdio::null())
            .stdout(wr.try_clone().unwrap())
            .stderr(wr)
            .spawn()
            .unwrap(),
    );
    let log = Log::capture(rd);
    let mut all = Vec::new();
    for scale in ["1", "1.25", "1.5", "2"] {
        swaymsg(&sway, &format!("output HEADLESS-1 scale {scale}"));
        let ready = format!("WALL status 1 dpr [{scale}]");
        // impulse-shell names each decode it starts (`ri::imgdbg`): one per size.
        let again = |l: &str| {
            let loads: Vec<&str> = l.lines().filter(|l| l.contains("imgload ")).collect();
            (loads.iter().enumerate()).any(|(i, a)| loads[..i].contains(a))
        };
        let wait = |what: &str, f: &dyn Fn(&str) -> bool| {
            if let Err(l) = log.wait(|l| f(l) || again(l)) {
                let keep: Vec<&str> = l.lines().filter(|l| l.contains("WALL")).collect();
                panic!("{bin} exited before {what}:\n{}", keep.join("\n"));
            }
            let l = log.text();
            let keep: Vec<&str> = (l.lines())
                .filter(|l| l.contains("WALL") || l.contains("imgload "))
                .collect();
            assert!(
                !again(&l),
                "{bin} at scale {scale} decodes the wallpaper again:\n{}",
                keep.join("\n")
            );
        };
        wait("the wallpaper was Ready", &|l| l.contains(&ready));
        // A frame begun before the Image was Ready may be the first one answered.
        let mut read = log.text().rfind(&ready).unwrap();
        let mut colors = shot(&sway, shots.path());
        for _ in 0..5 {
            wait("a frame was on screen", &|l| frame_shown(&l[read..]));
            read = log.text().len();
            colors = shot(&sway, shots.path());
            if colors == [WALLPAPER; 3] {
                break;
            }
        }
        all.push(format!("scale {scale}: {colors:?}"));
    }
    all
}

#[test]
fn the_wallpaper_stays_while_the_scale_changes() {
    if !testenv::require_all(&["qs", "sway", "swaymsg", "grim"]) {
        return;
    }
    let qt = shown("qs");
    let want: Vec<String> = ["1", "1.25", "1.5", "2"]
        .iter()
        .map(|s| format!("scale {s}: {:?}", [WALLPAPER; 3]))
        .collect();
    assert_eq!(qt, want, "Quickshell shows the wallpaper at every scale");
    assert_eq!(shown(NATIVE), qt);
}
