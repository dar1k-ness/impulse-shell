//! A scene shown by Qt itself and by us at one devicePixelRatio, and the device pixels compared:
//! tools/oracle/effects_oracle.py shows the items in a QQuickView (OpenGL, under a headless sway).
#![allow(dead_code)]

use qsg::raster::Canvas;
use shell::scene::{self, Target};
use std::path::Path;

/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";
const IMPORTS: &str = "import QtQuick\nimport QtQuick.Controls\nimport QtQuick.Effects\nimport Qt5Compat.GraphicalEffects\n";

/// (the `import` lines `body` opens with, the items after them).
fn split_imports(body: &str) -> (&str, &str) {
    let at = body
        .split_inclusive('\n')
        .take_while(|l| l.starts_with("import "))
        .map(str::len)
        .sum();
    body.split_at(at)
}

/// Where Qt's devicePixelRatio comes from.
#[derive(Clone, Copy, PartialEq)]
pub enum Scale {
    /// `QT_SCALE_FACTOR`: Qt's own high-DPI scaling, under which a font of the default hinting
    /// preference is laid out unhinted (QFontconfigDatabase, `isDprScaling`).
    Qt,
    /// The compositor's output scale (wp_fractional_scale_v1), as a shell on a scaled monitor
    /// has it: fonts are laid out as at scale 1.
    Output,
}

/// Whether a text caret Qt draws blinks.
#[derive(Clone, Copy, PartialEq)]
enum Caret {
    /// At the platform theme's QStyleHints::cursorFlashTime.
    Blinks,
    /// Never (a cursorFlashTime of 0): it is in the shot whenever the shot is taken.
    Still,
}

/// Qt's device pixels of `body` in a `w` x `h` scene at `dpr`, and what it printed; `None`
/// (reported) without the oracle.
fn qt(
    body: &str,
    (w, h): (usize, usize),
    dpr: f64,
    by: Scale,
    caret: Caret,
) -> Option<(Canvas, String)> {
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return None;
    }
    if !testenv::require("sway") {
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("scene.qml");
    let (imports, body) = split_imports(body);
    let qml = format!(
        "{IMPORTS}{imports}Rectangle {{\n    width: {w}; height: {h}\n    color: \"#202020\"\n{body}\n}}\n"
    );
    std::fs::write(&file, qml).unwrap();
    Some(qt_shows(&file, None, dpr, by, caret))
}

/// Qt's device pixels of the scene `file` at `dpr`, and what it printed, with `imports` one
/// more import path.
fn qt_shows(
    file: &Path,
    imports: Option<&Path>,
    dpr: f64,
    by: Scale,
    caret: Caret,
) -> (Canvas, String) {
    let out = file.with_extension("rgba");
    let mut path = String::from("/usr/lib/qt6/qml");
    if let Some(dir) = imports {
        path = format!("{path}:{}", dir.display());
    }
    let sway = testenv::Sway::start();
    if by == Scale::Output {
        let st = std::process::Command::new("swaymsg")
            .env("SWAYSOCK", sway.ipc_socket())
            .arg(format!("output HEADLESS-1 scale {dpr}"))
            .stdout(std::process::Stdio::null())
            .status()
            .expect("swaymsg");
        assert!(st.success(), "swaymsg output scale {dpr}");
    }
    let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/oracle/effects_oracle.py");
    let cache = tempfile::tempdir().unwrap();
    let mut cmd = std::process::Command::new(PYTHON);
    cmd.arg(oracle)
        .args([file, &out])
        .env("QT_QPA_PLATFORM", "wayland")
        .env("QT_QUICK_CONTROLS_STYLE", "Basic")
        // Qt5Compat's blurs bake their shaders into the cache directory and read them back:
        // two oracles at once would read each other's half-written files and draw no blur.
        .env("XDG_CACHE_HOME", cache.path())
        .env("QML_IMPORT_PATH", path);
    match by {
        Scale::Qt => cmd.env("QT_SCALE_FACTOR", dpr.to_string()),
        Scale::Output => cmd.env("ORACLE_DPR", dpr.to_string()),
    };
    // Where QSGRhiDistanceFieldGlyphCache puts a glyph in its texture follows the order of
    // a QSet, which the process's random hash seed gives: without a fixed seed two runs of
    // one scene differ by a level or two in a pixel or two of `Text.QtRendering`.
    cmd.env("QT_HASH_SEED", "0");
    if caret == Caret::Still {
        cmd.env("ORACLE_CURSOR_FLASH_TIME", "0");
    }
    let o = sway.env(&mut cmd).output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(
        o.status.success(),
        "effects_oracle.py failed: {text}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        text.contains(&format!("DPR {dpr:?}\n")),
        "Qt's scale: {text}"
    );
    let raw = std::fs::read(&out).unwrap();
    let w = u32::from_le_bytes(raw[0..4].try_into().unwrap()) as usize;
    let h = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
    let px = raw[8..].as_chunks::<4>().0.to_vec();
    assert_eq!(px.len(), w * h);
    (Canvas::from_px(w, h, px), text)
}

/// Our device pixels of the same scene on the GPU path.
fn ours(body: &str, (w, h): (usize, usize), dpr: f64) -> Canvas {
    let (imports, body) = split_imports(body);
    let qml = format!(
        "{IMPORTS}{imports}import Quickshell\nFloatingWindow {{\n    width: {w}; height: {h}\n    \
         color: \"#202020\"\n{body}\n}}\n"
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&qml).expect("scene loads");
    drawn(&mut rt, win, (w, h), dpr)
}

/// The window `win` of `rt`, `w` x `h`, as the GPU path draws it at `dpr`.
pub fn drawn(
    rt: &mut qml::runtime::Runtime,
    win: usize,
    (w, h): (usize, usize),
    dpr: f64,
) -> Canvas {
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(rt, &mut fonts, win, [w as f64, h as f64]);
    }
    let (mut g, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let mut fr = shell::gpu::Frame::scaled(w, h, dpr);
    scene::paint_window(rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
    let mut s = g.pbuffer(fr.w as i32, fr.h as i32).expect("pbuffer");
    s.reswap();
    g.render(&mut s, &mut fr, || {});
    g.read_surface(&s)
}

/// The device pixels of `body` at `dpr`: (Qt's, ours); `None` (reported) without the oracle.
pub fn both(body: &str, size: (usize, usize), dpr: f64) -> Option<(Canvas, Canvas)> {
    let (want, _) = qt(body, size, dpr, Scale::Qt, Caret::Blinks)?;
    Some((want, ours(body, size, dpr)))
}

/// `both` of a scene with a text caret, which under Qt does not blink here: Qt's shot is taken
/// at its third frame, which a busy machine reaches in the half of the flash time the caret
/// is off.
pub fn both_with_a_still_caret(
    body: &str,
    size: (usize, usize),
    dpr: f64,
) -> Option<(Canvas, Canvas)> {
    let (want, _) = qt(body, size, dpr, Scale::Qt, Caret::Still)?;
    Some((want, ours(body, size, dpr)))
}

/// With `RI_DUMP_DIR` set, both pictures as PPM files there, to look at a difference.
pub fn dump(got: &Canvas, want: &Canvas, dpr: f64) {
    let Some(dir) = std::env::var_os("RI_DUMP_DIR") else {
        return;
    };
    let name = std::thread::current()
        .name()
        .unwrap_or("scene")
        .replace("::", "_");
    for (side, c) in [("ours", got), ("qt", want)] {
        let mut out = format!("P6 {} {} 255\n", c.w, c.h).into_bytes();
        out.extend(c.px.iter().flat_map(|p| [p[0], p[1], p[2]]));
        let f = std::path::Path::new(&dir).join(format!("{name}-{dpr}-{side}.ppm"));
        std::fs::write(f, out).unwrap();
    }
}

/// (pixels whose largest channel difference exceeds `tol`, the largest difference, where).
pub fn diff(a: &Canvas, b: &Canvas, tol: u8) -> (usize, u8, (usize, usize)) {
    assert_eq!((a.w, a.h), (b.w, b.h), "device size");
    let (mut n, mut max, mut at) = (0, 0, (0, 0));
    for (k, (p, q)) in a.px.iter().zip(&b.px).enumerate() {
        let d = (0..4).map(|c| p[c].abs_diff(q[c])).max().unwrap();
        n += (d > tol) as usize;
        if d > max {
            (max, at) = (d, (k % a.w, k / a.w));
        }
    }
    (n, max, at)
}

/// `body` (items, after any `import` lines of its own) at each of `dprs`: at most `allow`
/// device pixels differ from Qt's by more than `tol` a channel.
pub fn like_qt(body: &str, size: (usize, usize), dprs: &[f64], tol: u8, allow: usize) {
    like_qt_by(Scale::Qt, body, size, dprs, tol, allow);
}

/// `like_qt` at scale 1 with no tolerance, for our CPU path.
pub fn cpu_like_qt(body: &str, size: (usize, usize)) {
    let shown = |dpr| {
        let (want, _) = qt(body, size, dpr, Scale::Qt, Caret::Blinks)?;
        Some((want, ours_cpu(body, size)))
    };
    compared(shown, &[1.0], 0, 0);
}

/// `like_qt` at scale 1 for the pixels `ours` has of the scene: at most `allow` of them differ
/// from Qt's.
pub fn shown_like_qt(body: &str, size: (usize, usize), allow: usize, ours: impl Fn() -> Canvas) {
    let shown = |dpr| Some((qt(body, size, dpr, Scale::Qt, Caret::Blinks)?.0, ours()));
    compared(shown, &[1.0], 0, allow);
}

/// How many pixels of `body` are not Qt's on our GPU path at each of `dprs`, with no
/// tolerance; nothing without the oracle. For a scene whose count is held: more of them fail
/// its test and so do fewer.
pub fn off_qt_by(by: Scale, body: &str, size: (usize, usize), dprs: &[f64]) -> Option<Vec<usize>> {
    let off = |&dpr: &f64| {
        let (want, _) = qt(body, size, dpr, by, Caret::Blinks)?;
        let got = ours(body, size, dpr);
        dump(&got, &want, dpr);
        let (n, max, at) = diff(&got, &want, 0);
        println!("dpr {dpr}: {n} px over 0, max {max} at {at:?}");
        Some(n)
    };
    dprs.iter().map(off).collect()
}

/// `off_qt_by` at scale 1 for our CPU path.
pub fn cpu_off_qt(body: &str, size: (usize, usize)) -> Option<usize> {
    let (want, _) = qt(body, size, 1.0, Scale::Qt, Caret::Blinks)?;
    let got = ours_cpu(body, size);
    dump(&got, &want, 1.0);
    let (n, max, at) = diff(&got, &want, 0);
    println!("dpr 1: {n} px over 0, max {max} at {at:?}");
    Some(n)
}

/// Our pixels of the same scene on the CPU path, at scale 1.
fn ours_cpu(body: &str, (w, h): (usize, usize)) -> Canvas {
    let qml = format!(
        "{IMPORTS}import Quickshell\nFloatingWindow {{\n    width: {w}; height: {h}\n    \
         color: \"#202020\"\n{body}\n}}\n"
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&qml).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [w as f64, h as f64]);
    }
    let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
    c
}

/// `like_qt` with Qt's devicePixelRatio set by `by`.
pub fn like_qt_by(
    by: Scale,
    body: &str,
    size: (usize, usize),
    dprs: &[f64],
    tol: u8,
    allow: usize,
) {
    let shown = |dpr| {
        let (want, _) = qt(body, size, dpr, by, Caret::Blinks)?;
        Some((want, ours(body, size, dpr)))
    };
    compared(shown, dprs, tol, allow);
}

/// `like_qt_by` with no tolerance for items of a tree of files: `tree/qs` holds what
/// `import qs.…` names (the shell's root for us, `tree` an import path for Qt), `singletons`
/// its directories of singletons, and `imports` are the scene's own `import` lines.
pub fn like_qt_in(
    by: Scale,
    tree: &Path,
    singletons: &[&str],
    imports: &str,
    body: &str,
    size: (usize, usize),
    dprs: &[f64],
) {
    let (w, h) = size;
    let root = tree.join("ri");
    let (qt_file, our_file) = (tree.join("scene.qml"), root.join("main.qml"));
    let scene = |window: &str| {
        format!(
            "{IMPORTS}{imports}{window} {{\n    width: {w}; height: {h}\n    \
             color: \"#202020\"\n{body}\n}}\n"
        )
    };
    std::fs::write(&qt_file, scene("Rectangle")).unwrap();
    std::fs::write(&our_file, scene("import Quickshell\nFloatingWindow")).unwrap();
    let shown = |dpr| {
        if !Path::new(PYTHON).exists() {
            testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
            return None;
        }
        if !testenv::require("sway") {
            return None;
        }
        let (want, _) = qt_shows(&qt_file, Some(tree), dpr, by, Caret::Blinks);
        let mut rt = qml::runtime::Runtime::new(Some(root.clone()));
        let dirs: Vec<_> = singletons.iter().map(|d| root.join(d)).collect();
        rt.load_singletons(&dirs);
        rt.settle();
        let win = rt.load_file(&our_file).expect("scene loads");
        Some((want, drawn(&mut rt, win, size, dpr)))
    };
    compared(shown, dprs, 0, 0);
}

/// What a scene is known to differ from Qt by: the scale, how many pixels, the largest
/// difference of a channel, and a digest of where they are and of both colours there.
#[allow(dead_code)]
pub type Off = (f64, usize, u8, u64);

/// `like_qt_in` with no singletons for a scene that differs from Qt in exactly the pixels
/// `off` holds: other pixels, more of them or fewer fail it.
#[allow(dead_code)]
pub fn off_qt_in(
    by: Scale,
    tree: &Path,
    imports: &str,
    body: &str,
    size: (usize, usize),
    off: &[Off],
) {
    let (w, h) = size;
    let root = tree.join("ri");
    let (qt_file, our_file) = (tree.join("scene.qml"), root.join("main.qml"));
    let scene = |window: &str| {
        format!(
            "{IMPORTS}{imports}{window} {{\n    width: {w}; height: {h}\n    \
             color: \"#202020\"\n{body}\n}}\n"
        )
    };
    std::fs::write(&qt_file, scene("Rectangle")).unwrap();
    std::fs::write(&our_file, scene("import Quickshell\nFloatingWindow")).unwrap();
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return;
    }
    if !testenv::require("sway") {
        return;
    }
    let mut bad = Vec::new();
    for &(dpr, pixels, largest, held) in off {
        let (want, _) = qt_shows(&qt_file, Some(tree), dpr, by, Caret::Blinks);
        let mut rt = qml::runtime::Runtime::new(Some(root.clone()));
        let win = rt.load_file(&our_file).expect("scene loads");
        let got = drawn(&mut rt, win, size, dpr);
        dump(&got, &want, dpr);
        let (n, max, _) = diff(&got, &want, 0);
        let mut places = 0xcbf2_9ce4_8422_2325u64;
        for (k, (p, q)) in got.px.iter().zip(&want.px).enumerate() {
            if p != q {
                for b in (k as u32).to_le_bytes().iter().chain(p).chain(q) {
                    places = (places ^ *b as u64).wrapping_mul(0x100_0000_01b3);
                }
            }
        }
        let places = if n == 0 { 0 } else { places };
        println!("dpr {dpr}: {n} px, max {max}, places {places:#x}");
        if (n, max, places) != (pixels, largest, held) {
            bad.push(format!(
                "dpr {dpr}: {n} pixels differ from Qt, the largest by {max}, places {places:#x}; \
                 held {pixels} pixels, {largest}, {held:#x}"
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// `shown(dpr)` is (Qt's pixels, ours), or nothing without the oracle.
fn compared(shown: impl Fn(f64) -> Option<(Canvas, Canvas)>, dprs: &[f64], tol: u8, allow: usize) {
    let mut bad = Vec::new();
    for &dpr in dprs {
        let Some((want, got)) = shown(dpr) else {
            return;
        };
        dump(&got, &want, dpr);
        let (n, max, at) = diff(&got, &want, tol);
        println!("dpr {dpr}: {n} px over {tol}, max {max} at {at:?}");
        if n > allow {
            bad.push(format!(
                "dpr {dpr}: {n} pixels differ from Qt by more than {tol} (largest {max} at \
                 {at:?}: ours {:?}, Qt {:?})",
                got.px[at.1 * got.w + at.0],
                want.px[at.1 * want.w + at.0]
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
