//! A scene shown by Qt itself and by us at one devicePixelRatio, and the device pixels compared:
//! tools/oracle/effects_oracle.py shows the items in a QQuickView (OpenGL, under a headless sway).

use qsg::raster::Canvas;
use shell::scene::{self, Target};
use std::path::Path;

/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";
const IMPORTS: &str = "import QtQuick\nimport QtQuick.Shapes\nimport QtQuick.Effects\nimport Qt5Compat.GraphicalEffects\n";

/// Qt's device pixels of `body` in a `w` x `h` scene at `dpr`, and what it printed; `None`
/// (reported) without the oracle.
fn qt(body: &str, (w, h): (usize, usize), dpr: f64) -> Option<(Canvas, String)> {
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return None;
    }
    if !testenv::require("sway") {
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let (file, out) = (dir.path().join("scene.qml"), dir.path().join("qt.rgba"));
    let qml = format!(
        "{IMPORTS}Rectangle {{\n    width: {w}; height: {h}\n    color: \"#202020\"\n{body}\n}}\n"
    );
    std::fs::write(&file, qml).unwrap();
    let sway = testenv::Sway::start();
    let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/oracle/effects_oracle.py");
    let mut cmd = std::process::Command::new(PYTHON);
    cmd.arg(oracle)
        .args([&file, &out])
        .env("QT_QPA_PLATFORM", "wayland")
        .env("QT_SCALE_FACTOR", dpr.to_string())
        .env("QML_IMPORT_PATH", "/usr/lib/qt6/qml");
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
    Some((Canvas::from_px(w, h, px), text))
}

/// Our device pixels of the same scene on the GPU path.
fn ours(body: &str, (w, h): (usize, usize), dpr: f64) -> Canvas {
    let qml = format!(
        "{IMPORTS}import Rustshell\nFloatingWindow {{\n    width: {w}; height: {h}\n    \
         color: \"#202020\"\n{body}\n}}\n"
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&qml).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [w as f64, h as f64]);
    }
    let (mut g, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let mut fr = shell::gpu::Frame::scaled(w, h, dpr);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
    let mut s = g.pbuffer(fr.w as i32, fr.h as i32).expect("pbuffer");
    s.reswap();
    g.render(&mut s, &mut fr, || {});
    g.read_surface(&s)
}

/// (pixels whose largest channel difference exceeds `tol`, the largest difference, where).
fn diff(a: &Canvas, b: &Canvas, tol: u8) -> (usize, u8, (usize, usize)) {
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

/// `body` at each of `dprs`: at most `allow` device pixels differ from Qt's by more than `tol`
/// a channel.
pub fn like_qt(body: &str, size: (usize, usize), dprs: &[f64], tol: u8, allow: usize) {
    let mut bad = Vec::new();
    for &dpr in dprs {
        let Some((want, _)) = qt(body, size, dpr) else {
            return;
        };
        let got = ours(body, size, dpr);
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
