//! A GPU context can be lost for good (a driver reset, a suspend that drops video memory):
//! EGL then answers every eglMakeCurrent with EGL_CONTEXT_LOST. The shell used to drop each
//! such frame without a word, so its windows froze on their last frame with nothing in the
//! log. It has to say what happened and keep drawing: with a new context, or on the CPU when
//! the lost context never showed a frame. `RI_DEBUG=gpuloss=<n>` loses the process's first
//! context at its n-th frame.
use std::path::Path;
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

/// A red panel in the top left corner with a dot that never stops moving along its top edge,
/// so the window keeps drawing frames.
const PANEL: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: "#ff0000"
        Rectangle { id: dot; width: 4; height: 4; color: "#0000ff" }
        NumberAnimation {
            target: dot; property: "x"; from: 0; to: 60; duration: 1000
            loops: Animation.Infinite; running: true
        }
    }
}
"##;

/// A panel that holds what the shell keeps in the GPU context besides the window itself: a
/// capture of its screen, an item drawn through a layer, a blur of it, an image big enough to
/// give up its decoded pixels once its texture is uploaded, and a crossfade (the previous
/// frame on a subsurface of its own, fading out for 2 s) that starts a few frames after the
/// capture arrived and the image was decoded. The dot keeps the window drawing.
const BUSY_PANEL: &str = r##"import QtQuick
import Rustshell
import Rustshell.Wayland
import Qt5Compat.GraphicalEffects
import Impulse
ShellRoot {
    id: root
    property color fill: "#ff0000"
    // Frames since the capture and the image are both there; -1 before, and once the fade
    // began.
    property int frames: -1
    property bool ready: view.hasContent && big.status == Image.Ready
    onReadyChanged: if (ready && fill == "#ff0000") frames = 0
    PanelWindow {
        id: panel
        anchors { top: true; left: true }
        implicitWidth: 200; implicitHeight: 64
        exclusionMode: ExclusionMode.Ignore
        color: root.fill
        Rectangle {
            id: dot; width: 4; height: 4; color: "#0000ff"
            onXChanged: {
                if (root.frames < 0 || ++root.frames < 4)
                    return;
                root.frames = -1;
                Crossfade.request(2000);
                root.fill = "#00ff00";
            }
        }
        NumberAnimation {
            target: dot; property: "x"; from: 0; to: 60; duration: 1000
            loops: Animation.Infinite; running: true
        }
        Rectangle {
            y: 8; width: 64; height: 28; color: "#202020"
            Rectangle {
                id: src; x: 4; y: 2; width: 24; height: 24; radius: 6; color: "#3366ff"
                layer.enabled: true
            }
            GaussianBlur { x: 34; y: 2; width: 24; height: 24; source: src; radius: 4; samples: 9 }
        }
        Image { id: big; x: 68; y: 8; width: 28; height: 28; source: "big.png" }
        ScreencopyView { id: view; x: 100; width: 96; height: 54; captureSource: panel.screen }
    }
}
"##;

/// rusty-impulse running `PANEL` under `sway` with `RI_DEBUG=gpuloss=<lose_at>`, its Wayland traffic
/// and presentation feedback logged.
fn shell(sway: &testenv::Sway, lose_at: u32) -> (Kill, tempfile::TempDir, Arc<Log>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), PANEL).unwrap();
    let (shell, log) = shell_with(sway, dir.path(), Some(lose_at), false);
    (shell, dir, log)
}

const BIG: [u8; 3] = [0xc0, 0x80, 0x40];

/// A directory with `BUSY_PANEL` and its image: 160 x 128 of one colour, which is more than
/// the shell keeps decoded beside an uploaded texture.
fn busy_panel() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    image::RgbImage::from_pixel(160, 128, image::Rgb(BIG))
        .save(dir.path().join("big.png"))
        .unwrap();
    std::fs::write(dir.path().join("shell.qml"), BUSY_PANEL).unwrap();
    dir
}

/// rusty-impulse running the shell in `dir` under `sway`, with `RI_DEBUG=gpuloss=<lose_at>` if given; its Wayland
/// traffic, presentation feedback and crossfades logged. `fixed_step` is Qt's fixed animation
/// step: every frame moves the animations 16 ms, however long the host took to draw it.
fn shell_with(
    sway: &testenv::Sway,
    dir: &Path,
    lose_at: Option<u32>,
    fixed_step: bool,
) -> (Kill, Arc<Log>) {
    let (r, w) = std::io::pipe().unwrap();
    let debug = lose_at.map_or(String::new(), |n| format!("gpuloss={n}"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
    if fixed_step {
        cmd.env("QSG_FIXED_ANIMATION_STEP", "1");
    }
    let child = sway
        .env(&mut cmd)
        .arg("-p")
        .arg(dir)
        .env("RI_INTERP", "1")
        .env_remove("RI_RENDER")
        .env("WAYLAND_DEBUG", "1")
        .env("RI_LOG", "ri::present=debug,ri::fade=debug")
        .env("RI_DEBUG", debug)
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w)
        .spawn()
        .unwrap();
    (Kill(child), Log::capture(r))
}

/// The colour sway's screen shows at (x, y).
fn screen_pixel(sway: &testenv::Sway, dir: &Path, x: usize, y: usize) -> [u8; 3] {
    screen(sway, dir)(x, y)
}

/// Sway's screen now: the colour at (x, y).
fn screen(sway: &testenv::Sway, dir: &Path) -> impl Fn(usize, usize) -> [u8; 3] + use<> {
    let f = dir.join("screen.ppm");
    let ok = sway
        .env(&mut Command::new("grim"))
        .args(["-t", "ppm"])
        .arg(&f)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "grim");
    let b = std::fs::read(&f).unwrap();
    // P6\n<w> <h>\n255\n<rgb>: the pixels follow the fourth whitespace-separated field.
    let mut fields = Vec::new();
    let mut i = 0;
    for _ in 0..4 {
        while b[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while !b[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(std::str::from_utf8(&b[start..i]).unwrap());
    }
    let w: usize = fields[1].parse().unwrap();
    move |x, y| {
        let at = i + 1 + (y * w + x) * 3;
        [b[at], b[at + 1], b[at + 2]]
    }
}

/// Whether the shell draws on the GPU, once its log says which renderer it started with;
/// without one there is no context to lose.
fn on_gpu(log: &Log) -> bool {
    if let Err(s) = log.wait(|s| s.contains("renderer: ")) {
        panic!(
            "rusty-impulse exited before it chose a renderer:\n{}",
            tail(&s)
        );
    }
    let gpu = log.text().contains("renderer: GPU (GLES2, ");
    assert!(
        gpu || std::env::var_os("RI_REQUIRE_GL").is_none(),
        "rusty-impulse has no GPU renderer under sway"
    );
    gpu
}

/// Waits for the log line holding `said` and for a frame on screen that was committed after
/// it, then for the panel's red on sway's screen.
fn draws_on_after(sway: &testenv::Sway, dir: &Path, log: &Log, said: &str) {
    if let Err(s) = log.wait(|s| presented_after(s, |l| l.contains(said))) {
        panic!(
            "rusty-impulse exited before a frame after `{said}` was presented:\n{}",
            tail(&s)
        );
    }
    assert_eq!(screen_pixel(sway, dir, 32, 20), [255, 0, 0], "the panel");
}

fn tail(log: &str) -> String {
    let lines: Vec<&str> = log
        .lines()
        .filter(|l| !l.contains(" -> ") && !l.contains("@"))
        .collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "lost_context_is_logged_and_replaced_by_a_new_one",
        &[("PATH", Some("/nonexistent"))],
    );
}

/// The context is lost at its third frame, after two were shown: the log names the failed
/// call and its EGL error, then the new renderer, and frames keep reaching the screen.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn lost_context_is_logged_and_replaced_by_a_new_one() {
    if !testenv::require_all(&["sway", "grim"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let (_shell, dir, log) = shell(&sway, 3);
    if !on_gpu(&log) {
        return;
    }
    draws_on_after(&sway, dir.path(), &log, "a new context after the lost one");
    let text = log.text();
    let lost = "gpu: eglMakeCurrent failed: 0x300e (ContextLost): the GPU context is lost";
    assert_eq!(text.matches(lost).count(), 1, "logged once");
    let renderers: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("renderer: "))
        .collect();
    assert_eq!(renderers.len(), 2, "{renderers:?}");
    assert!(
        renderers[1].starts_with("renderer: GPU (GLES2, "),
        "{renderers:?}"
    );
}

/// The context is lost at its first frame: a new one would be lost the same way, so the
/// shell draws on the CPU from then on, and says so.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn context_lost_before_its_first_frame_falls_back_to_the_cpu() {
    if !testenv::require_all(&["sway", "grim"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let (_shell, dir, log) = shell(&sway, 1);
    if !on_gpu(&log) {
        return;
    }
    let cpu = "renderer: CPU raster (wl_shm) from now on, the GPU context is lost";
    draws_on_after(&sway, dir.path(), &log, cpu);
    let text = log.text();
    let lost = "gpu: eglMakeCurrent failed: 0x300e (ContextLost): the GPU context is lost";
    assert_eq!(text.matches(lost).count(), 1, "logged once");
}

/// The layer and the blur of `BUSY_PANEL` on their backdrop, as sway's screen shows them.
fn layer_and_blur(shot: &impl Fn(usize, usize) -> [u8; 3]) -> Vec<[u8; 3]> {
    (8..36)
        .flat_map(|y| (0..64).map(move |x| (x, y)))
        .map(|(x, y)| shot(x, y))
        .collect()
}

/// Waits for a frame on screen that was committed after the last log line holding `said`.
fn presented(log: &Log, said: &str) {
    if let Err(s) = log.wait(|s| presented_after(s, |l| l.contains(said))) {
        panic!(
            "rusty-impulse exited before a frame after `{said}` was presented:\n{}",
            tail(&s)
        );
    }
}

/// A sway whose desktop is one colour other than black, on screen before this returns: the
/// compositor answered the commit of swaybg's buffer.
fn sway_with_desktop() -> (testenv::Sway, Kill) {
    let sway = testenv::Sway::start();
    let (r, w) = std::io::pipe().unwrap();
    let bg = sway
        .env(&mut Command::new("swaybg"))
        .args(["-c", "#406080"])
        .env("WAYLAND_DEBUG", "1")
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w)
        .spawn()
        .expect("swaybg");
    let (bg, log) = (Kill(bg), Log::capture(r));
    let shown = |s: &str| {
        s.find(".attach(").is_some_and(|at| {
            s[at..].contains("zwlr_layer_surface_v1") && s[at..].contains(".configure(")
        })
    };
    if let Err(s) = log.wait(shown) {
        panic!("swaybg exited before its colour was shown:\n{s}");
    }
    (sway, bg)
}

const DESKTOP: [u8; 3] = [0x40, 0x60, 0x80];

/// The context is lost while the window shows a screen capture, a layer, a blur and an image
/// kept only as a texture, and a crossfade is under way: the fading previous frame goes with
/// the context, and the new context draws the capture, the layer, the blur and the image as
/// the lost one did.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn lost_context_takes_capture_fade_layer_and_blur_and_all_are_drawn_again() {
    if !testenv::require_all(&["sway", "grim", "swaybg"]) {
        return;
    }
    const FILL: (usize, usize) = (80, 50);
    const IMAGE: (usize, usize) = (82, 22);
    // The middle of the layer's item and of its blur.
    const CACHED: [(usize, usize); 2] = [(16, 22), (46, 22)];
    // In the capture (96 x 54 at x 100), away from where the panel is.
    const IN_CAPTURE: (usize, usize) = (180, 40);
    const VEIL: &str = " veil 200x64 2000ms";

    // What a shell that keeps its context shows once the fade is over.
    let (sway, bg) = sway_with_desktop();
    let dir = busy_panel();
    let (kept, log) = shell_with(&sway, dir.path(), None, true);
    if !on_gpu(&log) {
        return;
    }
    presented(&log, " veil end");
    let shot = screen(&sway, dir.path());
    let want = layer_and_blur(&shot);
    assert_eq!(shot(1600, 800), DESKTOP, "the desktop");
    assert_eq!(shot(IN_CAPTURE.0, IN_CAPTURE.1), DESKTOP, "the capture");
    assert_eq!(want[14 * 64 + 16], [0x33, 0x66, 0xff], "the layer's item");
    assert_eq!(shot(IMAGE.0, IMAGE.1), BIG, "the image");
    drop((kept, bg, sway));

    // The fade takes 125 frames from about the 6th; the context is lost at the 40th.
    let (sway, _bg) = sway_with_desktop();
    let dir = busy_panel();
    let (_shell, log) = shell_with(&sway, dir.path(), Some(40), true);
    assert!(on_gpu(&log));
    presented(&log, "a new context after the lost one");
    let shot = screen(&sway, dir.path());
    let text = log.text();
    let lost = "gpu: eglMakeCurrent failed: 0x300e (ContextLost): the GPU context is lost";
    assert_eq!(text.matches(lost).count(), 1, "logged once");
    let before = &text[..text.find(lost).unwrap()];
    assert!(
        before.contains(VEIL) && !before.contains(" veil end"),
        "the context was not lost during the fade:\n{}",
        tail(&text)
    );
    assert_eq!(
        shot(FILL.0, FILL.1),
        [0, 255, 0],
        "the fading previous frame is gone with its context"
    );
    let got = layer_and_blur(&shot);
    let far = (0..got.len())
        .filter(|&i| (0..3).any(|c| got[i][c].abs_diff(want[i][c]) > 1))
        .map(|i| {
            format!(
                "({}, {}): {:?} for {:?}",
                i % 64,
                8 + i / 64,
                got[i],
                want[i]
            )
        })
        .collect::<Vec<_>>();
    assert!(
        far.is_empty(),
        "the layer and the blur changed:\n{}",
        far.join("\n")
    );
    for (x, y) in CACHED {
        assert_ne!(
            shot(x, y),
            [0x20, 0x20, 0x20],
            "nothing drawn at ({x}, {y})"
        );
    }
    assert_eq!(
        shot(IN_CAPTURE.0, IN_CAPTURE.1),
        DESKTOP,
        "the capture shows the screen"
    );
    assert_eq!(
        shot(IMAGE.0, IMAGE.1),
        BIG,
        "the image, whose pixels were only in the lost context, is decoded again"
    );
}
