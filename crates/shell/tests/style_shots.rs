//! Scenes of controls as `qml/` writes them, painted by Qt 6.12 (`qml6`, OpenGL on llvmpipe)
//! and by us: writes `<target>/tmp/style_shots/NAME.{qt,ours}.png` and `report.txt`, and
//! prints how many pixels of each cell differ. It fails for nothing: the numbers are what a
//! change of the painting is measured against (`tools/ci/gate --only tests --test style_shots
//! --no-capture`, `SHOTS=material,combo` for some scenes). The first line of a scene is
//! `// cell WxH cols N: names...`.

use qsg::raster::Canvas;
use shell::scene::{self, Target};
use std::path::Path;

const BG: &str = "#808080";

fn qt(dir: &Path, name: &str, w: usize, h: usize, out: &Path) -> Option<Canvas> {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::copy(
        dir.join(format!("{name}.qml")),
        tmp.path().join("Cells.qml"),
    )
    .unwrap();
    let png = tmp.path().join("qt.png");
    let source = format!(
        "import QtQuick\nWindow {{\n    id: win\n    width: {w}; height: {h}; visible: true\n    \
         color: \"{BG}\"\n    Rectangle {{ id: scene; width: {w}; height: {h}; color: \"{BG}\"\n        \
         Cells {{ anchors.fill: parent }}\n    }}\n    \
         property int frames: 0\n    onFrameSwapped: {{\n        frames++;\n        \
         if (frames != 4)\n            return;\n        \
         console.log(\"API\", GraphicsInfo.api, Screen.devicePixelRatio);\n        \
         win.contentItem.grabToImage(function(a) {{\n            a.saveToFile(\"{}\");\n            \
         Qt.quit();\n        }});\n    }}\n    \
         Timer {{ interval: 16; running: true; repeat: true; onTriggered: win.update() }}\n}}\n",
        png.display()
    );
    std::fs::write(tmp.path().join("scene.qml"), source).unwrap();
    let o = std::process::Command::new("timeout")
        .arg("60")
        .arg("qml6")
        .arg(tmp.path().join("scene.qml"))
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_QUICK_CONTROLS_STYLE", "Basic")
        .env("XDG_CACHE_HOME", tmp.path())
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&o.stderr);
    for l in log.lines() {
        println!("  qt[{name}]: {l}");
    }
    let full = qsg::image::load_png(png.to_str().unwrap()).ok()?;
    if full.w < w || full.h < h {
        return Some(full);
    }
    // The compositor may have made the window larger: the scene is its top left corner.
    let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
    for y in 0..h {
        c.px[y * w..(y + 1) * w].copy_from_slice(&full.px[y * full.w..y * full.w + w]);
    }
    c.save_png(out.join(format!("{name}.qt.png")).to_str().unwrap())
        .unwrap();
    Some(c)
}

fn ours(dir: &Path, name: &str, w: usize, h: usize, out: &Path) -> Canvas {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::copy(
        dir.join(format!("{name}.qml")),
        tmp.path().join("Cells.qml"),
    )
    .unwrap();
    let file = tmp.path().join("scene.qml");
    std::fs::write(
        &file,
        format!(
            "import QtQuick\nimport Quickshell\nFloatingWindow {{\n    width: {w}; height: {h}\n    \
             color: \"{BG}\"\n    Cells {{ id: scene; anchors.fill: parent }}\n}}\n"
        ),
    )
    .unwrap();
    let mut rt = qml::runtime::Runtime::new(Some(tmp.path().to_path_buf()));
    let win = rt.load_file(&file).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    let mut now = 0.0;
    for _ in 0..6 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [w as f64, h as f64]);
        now += 16.0;
        rt.tick(now);
    }
    let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
    c.save_png(out.join(format!("{name}.ours.png")).to_str().unwrap())
        .unwrap();
    c
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn shots() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/style_shots");
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("style_shots");
    std::fs::create_dir_all(&out).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let n = e.unwrap().file_name().into_string().unwrap();
            n.strip_suffix(".qml").map(str::to_string)
        })
        .collect();
    names.sort();
    if let Ok(only) = std::env::var("SHOTS") {
        names.retain(|n| only.split(',').any(|o| o == n));
    }
    let mut report = String::new();
    for name in names {
        let src = std::fs::read_to_string(dir.join(format!("{name}.qml"))).unwrap();
        let head = src.lines().next().unwrap();
        let (geo, cells) = head.trim_start_matches("// cell ").split_once(':').unwrap();
        let mut g = geo.split_whitespace();
        let (cw, ch) = g.next().unwrap().split_once('x').unwrap();
        let (cw, ch): (usize, usize) = (cw.parse().unwrap(), ch.parse().unwrap());
        let cols: usize = g.nth(1).unwrap().parse().unwrap();
        let cells: Vec<&str> = cells.split_whitespace().collect();
        let (w, h) = (cols * cw, cells.len().div_ceil(cols) * ch);
        let o = ours(&dir, &name, w, h, &out);
        let Some(q) = qt(&dir, &name, w, h, &out) else {
            report += &format!("{name}: Qt gave no picture\n");
            continue;
        };
        if (q.w, q.h) != (w, h) {
            report += &format!("{name}: Qt's picture is {}x{}, wanted {w}x{h}\n", q.w, q.h);
            continue;
        }
        for (k, cell) in cells.iter().enumerate() {
            let (x0, y0) = (k % cols * cw, k / cols * ch);
            let mut n = 0;
            for y in y0..y0 + ch {
                for x in x0..x0 + cw {
                    let (a, b) = (o.px[y * w + x], q.px[y * w + x]);
                    if (0..4).any(|c| a[c].abs_diff(b[c]) > 2) {
                        n += 1;
                    }
                }
            }
            report += &format!("{name} {cell}: {n} of {} pixels differ\n", cw * ch);
        }
    }
    std::fs::write(out.join("report.txt"), &report).unwrap();
    println!("{report}");
}
