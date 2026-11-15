//! The wallpaper of `Background.qml` while it is changed, the real Quickshell (Qt 6) vs the
//! native shell on the same config, each in a sway of its own: an asynchronous Image with
//! `retainWhileLoading` that is dimmed while it is not Ready. Its next picture is read from a
//! named pipe, so the Image is Loading until the test writes the file. QQuickImageBase keeps
//! the pixmap it shows (`currentPix`) until the next one is loaded: the old picture, dimmed,
//! is on screen all through the load, then the new one. A shell whose Image has no pixmap
//! while it loads shows frames of the bare background at every change of wallpaper.
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
// Half of each is a whole number: a picture at opacity 0.5 over the window has one value.
const WINDOW: [u8; 3] = [0, 0, 254];
const FIRST: [u8; 3] = [254, 0, 0];
const SECOND: [u8; 3] = [0, 254, 0];

const SHELL: &str = r##"//@ pragma Env QS_NO_RELOAD_POPUP=1
import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
    PanelWindow {
        anchors { top: true; bottom: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        color: "#0000fe"
        Image {
            id: wallpaper
            asynchronous: true
            retainWhileLoading: true
            cache: false
            smooth: false
            visible: opacity > 0
            opacity: status === Image.Ready ? 1 : 0.5
            source: "first.png"
            fillMode: Image.PreserveAspectCrop
            anchors.fill: parent
            property real zoom: 1
            sourceSize: Qt.size(width * zoom, height * zoom)
            onStatusChanged: console.log("WALL status " + status + " of [" + source + "]")
        }
        // Frames go on while nothing else changes: the test looks at the ones after a status.
        Rectangle {
            anchors.right: parent.right
            width: 4
            height: 4
            color: "#0000fe"
            NumberAnimation on opacity { from: 0.5; to: 1; duration: 500; loops: Animation.Infinite }
        }
    }
    IpcHandler {
        target: "wall"
        function apply(file: string): void { wallpaper.source = file; }
        function zoom(): void { wallpaper.zoom = 0.5; }
    }
}
"##;

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

fn half(of: [u8; 3]) -> [u8; 3] {
    std::array::from_fn(|k| of[k] / 2 + WINDOW[k] / 2)
}

/// What `bin` shows on the output: with the first wallpaper, while the second is read, with
/// the second, while the second is read again at another size, and after that.
fn shown(bin: &str) -> Vec<String> {
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
    let png = |color: [u8; 3], to: &Path| {
        let [r, g, b] = color;
        Canvas::new(64, 36, [r, g, b, 255])
            .save_png(to.to_str().unwrap())
            .unwrap();
    };
    png(FIRST, &dir.path().join("first.png"));
    // Not in the configuration's directory: a file written there reloads it.
    let out = tempfile::tempdir().unwrap();
    let second = out.path().join("second.png");
    png(SECOND, &second);
    let second = std::fs::read(second).unwrap();
    // A reader of it waits for the test to write the picture.
    let pipe = out.path().join("pipe.png");
    let st = Command::new("mkfifo").arg(&pipe).status().expect("mkfifo");
    assert!(st.success(), "mkfifo");
    let (rd, wr) = std::io::pipe().unwrap();
    let _shell = Kill(
        sway.env(&mut Command::new(bin))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("RI_LOG", "ri::present=debug")
            .stdin(Stdio::null())
            .stdout(wr.try_clone().unwrap())
            .stderr(wr)
            .spawn()
            .unwrap(),
    );
    let log = Log::capture(rd);
    let wait = |what: &str, f: &dyn Fn(&str) -> bool| {
        if let Err(l) = log.wait(f) {
            let keep: Vec<&str> = l.lines().filter(|l| l.contains("WALL")).collect();
            panic!("{bin} exited before {what}:\n{}", keep.join("\n"));
        }
    };
    let ipc = |args: &[&str]| {
        let st = sway
            .env(&mut Command::new(bin))
            .arg("-p")
            .arg(dir.path())
            .args(["ipc", "call", "wall"])
            .args(args)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "{bin} ipc call wall {args:?}");
    };
    let mut all = Vec::new();
    // Where the log was read up to: the status told last.
    let mut from = 0;
    // The picture on screen once the Image has told `status` after that: a frame from before
    // it may be the first one answered, so up to five are looked at.
    let mut look = |what: &str, status: u8, want: [u8; 3]| {
        let told = format!("WALL status {status} ");
        wait(what, &|l| l[from..].contains(&told));
        from += log.text()[from..].find(&told).unwrap() + told.len();
        let mut read = from;
        let mut colors = shot(&sway, out.path());
        for _ in 0..5 {
            wait("a frame was on screen", &|l| frame_shown(&l[read..]));
            read = log.text().len();
            colors = shot(&sway, out.path());
            if colors == [want; 3] {
                break;
            }
        }
        all.push(format!("{what}: {colors:?}"));
    };
    look("the first wallpaper", 1, FIRST);

    ipc(&["apply", pipe.to_str().unwrap()]);
    look("the second is read", 2, half(FIRST));
    std::fs::write(&pipe, &second).unwrap();
    look("the second wallpaper", 1, SECOND);

    // Background.qml sizes its Image by the new picture once it knows its size: another load.
    ipc(&["zoom"]);
    look("the second is read again", 2, half(SECOND));
    std::fs::write(&pipe, &second).unwrap();
    look("the second wallpaper, sized", 1, SECOND);
    all
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_old_wallpaper_stays_while_the_next_loads() {
    if !testenv::require_all(&["qs", "sway", "swaymsg", "grim", "mkfifo"]) {
        return;
    }
    let qt = shown("qs");
    let want = [
        format!("the first wallpaper: {:?}", [FIRST; 3]),
        format!("the second is read: {:?}", [half(FIRST); 3]),
        format!("the second wallpaper: {:?}", [SECOND; 3]),
        format!("the second is read again: {:?}", [half(SECOND); 3]),
        format!("the second wallpaper, sized: {:?}", [SECOND; 3]),
    ];
    assert_eq!(
        qt, want,
        "Quickshell shows a wallpaper all through its change"
    );
    assert_eq!(shown(NATIVE), qt);
}
