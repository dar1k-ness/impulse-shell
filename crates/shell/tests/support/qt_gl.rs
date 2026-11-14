//! A scene as Qt Quick draws it for a user: through OpenGL, in a window of a headless sway
//! (tools/oracle/effects_oracle.py, tools/oracle/steps_oracle.py), which in the testenv image is llvmpipe.
//! `qml6` on the `offscreen` platform draws with QPainter instead (the scene graph's
//! `software` backend): good for what a property reads, not for pixels. Where two glyphs
//! overlap, QPainter blends with `qt_div_255`, one less than the video card in 24 of the 65536
//! pairs of bytes; and it does not tint a Kirigami.Icon that is a mask at all.

use qsg::raster::Canvas;
use std::path::Path;

/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// Whether the image's Qt and sway are here; reported as a missing oracle otherwise.
pub fn available() -> bool {
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return false;
    }
    testenv::require("sway")
}

/// `script` of tools/oracle/ run on `source` (a whole QML file) under a headless sway, in a window
/// of devicePixelRatio `scale` (QT_SCALE_FACTOR): what Qt printed (`console.log` included),
/// and the directory whose `qt.rgba` the script was given to write to. `roomy` grows sway's
/// output to 3840x4320 first.
fn oracle(script: &str, source: &str, scale: f64, roomy: bool) -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let (file, out) = (dir.path().join("scene.qml"), dir.path().join("qt.rgba"));
    std::fs::write(&file, source).unwrap();
    let sway = testenv::Sway::start();
    if roomy {
        // Sway keeps a floating window within its output, 1920x1080 as it starts.
        let grown = std::process::Command::new("swaymsg")
            .env("SWAYSOCK", sway.ipc_socket())
            .arg("output HEADLESS-1 resolution 3840x4320")
            .output()
            .unwrap();
        assert!(grown.status.success(), "swaymsg: {grown:?}");
    }
    let oracle = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/oracle")
        .join(script);
    let mut cmd = std::process::Command::new(PYTHON);
    cmd.arg(oracle)
        .args([&file, &out])
        .env("QT_QPA_PLATFORM", "wayland")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_SCALE_FACTOR", scale.to_string())
        // The style our controls are those of, as in support/qt_window.rs.
        .env("QT_QUICK_CONTROLS_STYLE", "Basic")
        // Qt5Compat's blurs bake their shaders into the cache directory and read them back:
        // two oracles at once would read each other's half-written files and draw no blur.
        .env("XDG_CACHE_HOME", dir.path().join("cache"))
        .env("QML_IMPORT_PATH", "/usr/lib/qt6/qml")
        .env("QSG_INFO", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        // Where QSGRhiDistanceFieldGlyphCache puts a glyph in its texture follows the order of
        // a QSet, which the process's random hash seed gives: without a fixed seed two runs of
        // one scene differ by a level or two in a pixel or two of `Text.QtRendering`.
        .env("QT_HASH_SEED", "0");
    let o = sway.env(&mut cmd).output().unwrap();
    let log = String::from_utf8_lossy(&o.stderr).into_owned();
    assert!(o.status.success(), "{script} failed:\n{log}");
    assert!(log.contains("RENDERER: llvmpipe"), "Qt drew with:\n{log}");
    (log, dir)
}

/// A picture as the scripts write it: u32 width, u32 height, premultiplied RGBA rows.
fn picture(raw: &[u8]) -> Canvas {
    let w = u32::from_le_bytes(raw[0..4].try_into().unwrap()) as usize;
    let h = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
    let px = raw[8..].as_chunks::<4>().0.to_vec();
    assert_eq!(px.len(), w * h);
    Canvas::from_px(w, h, px)
}

/// What Qt printed (`console.log` included) and the picture of `source`, a whole QML file
/// whose root is an Item no larger than sway's 1920x1080 output.
// A test file takes one of `run` and `steps`.
#[allow(dead_code)]
pub fn run(source: &str) -> (String, Canvas) {
    shot(source, 1.0, false)
}

/// [`run`] in a window of devicePixelRatio `scale`: the picture is the root's device pixels,
/// which may fill an output grown to 3840x4320.
#[allow(dead_code)]
pub fn run_scaled(source: &str, scale: f64) -> (String, Canvas) {
    shot(source, scale, true)
}

fn shot(source: &str, scale: f64, roomy: bool) -> (String, Canvas) {
    let (log, dir) = oracle("effects_oracle.py", source, scale, roomy);
    let raw = std::fs::read(dir.path().join("qt.rgba")).unwrap();
    (log, picture(&raw))
}

/// What Qt printed and the pictures of `source` at each `shoot()` of its root, which declares
/// `signal shoot()` and `signal finished()` and emits them as it changes itself
/// (tools/oracle/steps_oracle.py).
#[allow(dead_code)]
pub fn steps(source: &str) -> (String, Vec<Canvas>) {
    let (log, dir) = oracle("steps_oracle.py", source, 1.0, false);
    let shots = (0..)
        .map_while(|n| std::fs::read(dir.path().join(format!("qt.rgba{n}"))).ok())
        .map(|raw| picture(&raw))
        .collect();
    (log, shots)
}
