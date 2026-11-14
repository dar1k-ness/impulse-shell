//! The lock screen's blur over a wallpaper that is a picture, against Qt itself: the items of
//! `Background.qml` (the wallpaper 1.07 times a 1536 x 864 screen, hidden, and a GaussianBlur
//! of it with `radius: 100` in a Loader scaled by 1.1) in a window of the screen's size.
//! tools/oracle/effects_oracle.py shows them in a QQuickView (OpenGL, under a headless sway) and the
//! window's pixels are compared with ours, none of them apart.

#[path = "support/qt_window.rs"]
mod qt_window;

use qsg::raster::Canvas;

const SIZE: (usize, usize) = (1536, 864);

/// A 1920 x 1080 picture in a file, as a `file:` url: blocks of 32 pixels of one colour each,
/// and every pixel half way to a colour of its own, so no two neighbours are alike and the
/// picture still has shapes larger than the blur's radius.
fn wallpaper() -> String {
    let path = std::env::temp_dir().join(format!("lock_blur_qt_{}.png", std::process::id()));
    let (w, h) = (1920, 1080);
    let mut seed = 0x2545_f491u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        [(seed >> 8) as u8, (seed >> 16) as u8, (seed >> 24) as u8]
    };
    let blocks: Vec<[u8; 3]> = (0..(w / 32) * (h / 32 + 1)).map(|_| next()).collect();
    let mut c = Canvas::new(w, h, [0, 0, 0, 255]);
    for (i, p) in c.px.iter_mut().enumerate() {
        let (block, own) = (blocks[i / w / 32 * (w / 32) + i % w / 32], next());
        for k in 0..3 {
            p[k] = ((block[k] as u16 + own[k] as u16) / 2) as u8;
        }
    }
    c.save_png(path.to_str().unwrap()).expect("the picture");
    format!("file://{}", path.display())
}

/// What `Background.qml` sets on its wallpaper: Qt draws such an Image into a layer for the
/// blur, while an Image of the default settings is the blur's source itself.
const SHELL: &str = "smooth: false; fillMode: Image.PreserveAspectCrop";

/// The lock screen's blur, as in the shell.
const SCALED: &str = "Item {
        width: 1643.52; height: 924.48; scale: 1.1
        GaussianBlur { anchors.fill: parent; source: wallpaper; radius: 100; samples: radius * 2 + 1 }
    }";

/// The same blur in no scaled item.
const PLAIN: &str =
    "GaussianBlur { width: 1643.52; height: 924.48; source: wallpaper; radius: 100; samples: 201 }";

/// A blur of no radius: the two passes read one texel each.
const NONE: &str =
    "GaussianBlur { width: 1643.52; height: 924.48; source: wallpaper; radius: 0; samples: 1 }";

fn body() -> String {
    scene("", SCALED)
}

/// The hidden wallpaper with `image`'s properties and `blur` of it.
fn scene(image: &str, blur: &str) -> String {
    format!(
        "    Image {{ id: wallpaper; width: 1643.52; height: 924.48; source: \"{}\"; visible: false; {image} }}
    {blur}",
        wallpaper()
    )
}

#[test]
fn lock_screen_blur_of_a_picture_on_the_cpu_raster() {
    qt_window::cpu_like_qt(&body(), SIZE);
}

#[test]
fn lock_screen_blur_of_a_picture_on_the_gpu() {
    qt_window::like_qt(&body(), SIZE, &[1.0], 0, 0);
}

#[test]
fn lock_screen_blur_of_the_shell_wallpaper_on_the_cpu_raster() {
    qt_window::cpu_like_qt(&scene(SHELL, SCALED), SIZE);
}

#[test]
fn lock_screen_blur_of_the_shell_wallpaper_on_the_gpu() {
    qt_window::like_qt(&scene(SHELL, SCALED), SIZE, &[1.0], 0, 0);
}

#[test]
fn blur_of_the_shell_wallpaper_in_no_scaled_item_on_the_cpu_raster() {
    qt_window::cpu_like_qt(&scene(SHELL, PLAIN), SIZE);
}

#[test]
fn blur_of_the_shell_wallpaper_in_no_scaled_item_on_the_gpu() {
    qt_window::like_qt(&scene(SHELL, PLAIN), SIZE, &[1.0], 0, 0);
}

#[test]
fn blur_of_no_radius_of_the_shell_wallpaper_on_the_cpu_raster() {
    qt_window::cpu_like_qt(&scene(SHELL, NONE), SIZE);
}

#[test]
fn blur_of_no_radius_of_the_shell_wallpaper_on_the_gpu() {
    qt_window::like_qt(&scene(SHELL, NONE), SIZE, &[1.0], 0, 0);
}

struct Kill(std::process::Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Mesa's variable that takes extensions off a context's list.
const NO_CLIP_CONTROL: (&str, &str) = ("MESA_EXTENSION_OVERRIDE", "-GL_EXT_clip_control");

/// What a screen shows of `body` in a window of the shell itself, and the shell's log:
/// rusty-impulse (with `env` set, if any) under a headless sway draws it in a panel at the
/// screen's corner, on the GPU path, and `grim` reads the screen.
fn on_a_screen(body: &str, env: Option<(&str, &str)>) -> (Canvas, String) {
    use std::process::{Command, Stdio};
    let (w, h) = SIZE;
    let dir = tempfile::tempdir().unwrap();
    let qml = format!(
        "import QtQuick\nimport Qt5Compat.GraphicalEffects\nimport Quickshell\n\
         PanelWindow {{\n    anchors {{ top: true; left: true }}\n    \
         implicitWidth: {w}; implicitHeight: {h}\n    exclusionMode: ExclusionMode.Ignore\n    \
         color: \"#202020\"\n{body}\n    \
         property bool ready: wallpaper.status == Image.Ready\n    \
         onReadyChanged: if (ready) console.log(\"WALLPAPER READY\")\n    \
         Component.onCompleted: if (ready) console.log(\"WALLPAPER READY\")\n}}\n"
    );
    std::fs::write(dir.path().join("shell.qml"), qml).unwrap();
    let sway = testenv::Sway::start();
    let (r, wr) = std::io::pipe().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
    cmd.envs(env);
    let child = sway
        .env(&mut cmd)
        .arg("-p")
        .arg(dir.path())
        .env("RI_INTERP", "1")
        .env_remove("RI_RENDER")
        .env("WAYLAND_DEBUG", "1")
        .env("RI_LOG", "ri::present=debug")
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    let _child = Kill(child);
    drop(cmd);
    let log = testenv::Log::capture(r);
    let shown =
        log.wait(|s| testenv::wayland::presented_after(s, |l| l.contains("WALLPAPER READY")));
    if let Err(s) = shown {
        let lines: Vec<&str> = s
            .lines()
            .filter(|l| !l.contains(" -> ") && !l.contains('@'))
            .collect();
        panic!(
            "rusty-impulse showed no frame with the wallpaper:\n{}",
            lines.join("\n")
        );
    }
    let text = log.text();
    assert!(
        text.contains("renderer: GPU (GLES2, "),
        "rusty-impulse draws on the CPU under sway"
    );
    let f = dir.path().join("screen.png");
    let ok = sway
        .env(&mut Command::new("grim"))
        .arg(&f)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "grim");
    let screen = qsg::image::load_png(f.to_str().unwrap()).unwrap();
    let px = (0..h)
        .flat_map(|y| screen.px[y * screen.w..][..w].to_vec())
        .collect();
    (Canvas::from_px(w, h, px), text)
}

/// The log's lines about a driver without GL_EXT_clip_control.
fn warnings(log: &str) -> usize {
    log.lines()
        .filter(|l| l.contains("no GL_EXT_clip_control"))
        .count()
}

/// The same blur as the user has it: in a window of the shell on a screen.
#[test]
fn lock_screen_blur_of_the_shell_wallpaper_in_a_window_on_a_screen() {
    if !testenv::require_all(&["sway", "grim"]) {
        return;
    }
    let body = scene(SHELL, SCALED);
    qt_window::shown_like_qt(&body, SIZE, 0, || {
        let (shown, log) = on_a_screen(&body, None);
        assert_eq!(warnings(&log), 0, "{log}");
        shown
    });
}

/// A driver without GL_EXT_clip_control: the shell says so once and draws the blur upright,
/// its last pass bottom row first as OpenGL has a window. Mesa then gives the pixels whose
/// centre is on an edge of a triangle to the other side than in Qt's window: 54 of the
/// picture's pixels (the count measured) are a level off Qt's.
#[test]
fn lock_screen_blur_in_a_window_of_a_driver_without_clip_control() {
    if !testenv::require_all(&["sway", "grim"]) {
        return;
    }
    let body = scene(SHELL, SCALED);
    qt_window::shown_like_qt(&body, SIZE, 54, || {
        let (shown, log) = on_a_screen(&body, Some(NO_CLIP_CONTROL));
        assert_eq!(warnings(&log), 1, "{log}");
        shown
    });
}

/// The picture itself, smooth, drawn straight into the window: from its corner past its
/// right and bottom edges, and past all four.
fn pictures_past_the_edges() -> [String; 2] {
    ["", "x: -53.76; y: -30.24; "].map(|at| {
        format!(
            "    Image {{ id: wallpaper; {at}width: 1643.52; height: 924.48; source: \"{}\" }}",
            wallpaper()
        )
    })
}

/// Mesa's llvmpipe cuts a triangle at the edges of its target only when the target has a
/// depth buffer, and the pixels along the cut and the diagonal read other texels: a smooth
/// picture larger than the window shows whether the window is drawn as Qt's.
#[test]
fn a_smooth_picture_past_the_edges_of_a_window_on_a_screen() {
    if !testenv::require_all(&["sway", "grim"]) {
        return;
    }
    for body in pictures_past_the_edges() {
        qt_window::shown_like_qt(&body, SIZE, 0, || on_a_screen(&body, None).0);
    }
}

#[test]
fn a_smooth_picture_past_the_edges_of_a_window_on_the_gpu() {
    for body in pictures_past_the_edges() {
        qt_window::like_qt(&body, SIZE, &[1.0], 0, 0);
    }
}
