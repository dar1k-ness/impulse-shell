//! Two things the shell shows on every screen, pixel by pixel against Qt Quick through OpenGL
//! (support/qt_gl.rs), with no tolerance: the screen corner of
//! `qml/modules/common/widgets/RoundCorner.qml` (a `Shape` whose `ShapePath` is filled, drawn
//! by the CurveRenderer), in a layer as the shell has it and without one; and the workspace
//! numbers of `qml/modules/rusty/bar/Workspaces.qml` (native text in a `layer.enabled` Row that
//! is shown and also masks a `MultiEffect`), with the same text outside a layer. Ours is the
//! window as the shell draws it: on the CPU, which draws at scale 1 only (the compositor
//! scales that buffer), and on the GPU (llvmpipe, as Qt's) at scale 1 and 1.5.
//!
//! `tools/ci/gate --run as-tester cargo nextest run -p shell --features shell/interp --test shell_scenes_qt`

#[path = "support/qt_gl.rs"]
mod qt_gl;

use qsg::raster::Canvas;
use shell::scene::{self, Target};

const SIZE: (usize, usize) = (320, 240);

/// The scene as the shell draws a window: on the CPU, or on the GPU at `dpr`; device pixels.
fn ours(source: &str, gpu: bool, dpr: f64) -> Canvas {
    let mut rt = qml::runtime::Runtime::new(None);
    let mut fonts = scene::Fonts::default();
    let root = rt.load_str(source).expect("scene loads");
    let mut g = gpu.then(|| {
        shell::gpu::Gpu::headless("surfaceless")
            .expect("surfaceless EGL/GLES2 context (Mesa)")
            .0
    });
    let mut last = None;
    // Twice: the second frame replays what the first recorded.
    for n in 1..=2 {
        rt.tick(16.0 * n as f64);
        for _ in 0..2 {
            rt.settle();
            scene::layout_window(&mut rt, &mut fonts, root, [SIZE.0 as f64, SIZE.1 as f64]);
        }
        last = Some(match &mut g {
            None => {
                assert_eq!(dpr, 1.0, "the CPU draws a window at scale 1");
                let mut picture = Canvas::new(SIZE.0, SIZE.1, [0, 0, 0, 0]);
                scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut picture));
                picture
            }
            Some(g) => {
                let mut fr = shell::gpu::Frame::scaled(SIZE.0, SIZE.1, dpr);
                scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Gpu(&mut fr));
                let mut s = g.pbuffer(fr.w as i32, fr.h as i32).expect("pbuffer");
                s.reswap();
                g.render(&mut s, &mut fr, || {});
                g.read_surface(&s)
            }
        });
    }
    last.unwrap()
}

fn window(body: &str) -> String {
    format!(
        "import QtQuick\nimport QtQuick.Effects\nimport QtQuick.Shapes\nRectangle {{\n    \
         width: {}; height: {}; color: \"#f3edf7\"\n{body}}}\n",
        SIZE.0, SIZE.1
    )
}

/// `body` in the window: every device pixel is Qt's. `parts` name what stands where, as
/// [x, y, w, h] of the scene, for the report.
fn same_as_qt(body: &str, parts: &[(&str, [usize; 4])], gpu: bool, dpr: f64) {
    if !(testenv::require("qml6") && qt_gl::available()) {
        return;
    }
    let source = window(body);
    let q = qt_gl::run_scaled(&source, dpr).1;
    let o = ours(&source, gpu, dpr);
    let on = format!("the {} at scale {dpr}", if gpu { "GPU" } else { "CPU" });
    // RI_SHELL_SCENES_DUMP=<dir> keeps both frames as raw RGBA, to look at a difference.
    if let Some(dir) = std::env::var_os("RI_SHELL_SCENES_DUMP") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).expect("dump directory");
        let name = std::thread::current()
            .name()
            .unwrap_or("scene")
            .replace("::", ".");
        for (who, c) in [("qt", &q), ("ours", &o)] {
            let file = dir.join(format!("{name}.{who}.{}x{}.rgba", c.w, c.h));
            std::fs::write(file, c.px.concat()).expect("dump written");
        }
    }
    assert_eq!((o.w, o.h), (q.w, q.h), "device size on {on}");
    let dev = |v: usize, up: bool| {
        let d = v as f64 * dpr;
        (if up { d.ceil() } else { d.floor() }) as usize
    };
    let differ = |x: usize, y: usize| {
        let (a, b) = (q.px[y * q.w + x], o.px[y * q.w + x]);
        (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap_or(0)
    };
    let mut lines = Vec::new();
    for (name, [x, y, w, h]) in parts {
        let (x0, y0, x1, y1) = (
            dev(*x, false),
            dev(*y, false),
            dev(x + w, true),
            dev(y + h, true),
        );
        let (mut count, mut most, mut where_) = (0, 0, Vec::new());
        for y in y0..y1 {
            for x in x0..x1 {
                let d = differ(x, y);
                count += (d > 0) as usize;
                most = most.max(d);
                if d > 0 && where_.len() < 24 {
                    where_.push(format!(
                        "    ({}, {}): Qt {:?}, ours {:?}",
                        x - x0,
                        y - y0,
                        q.px[y * q.w + x],
                        o.px[y * q.w + x]
                    ));
                }
            }
        }
        lines.push(format!(
            "{name}: {count} pixels differ, by up to {most} of 255"
        ));
        lines.extend(where_);
    }
    let all = (0..q.w * q.h).filter(|&i| q.px[i] != o.px[i]).count();
    let report = format!(
        "on {on}:\n{}\nthe whole window: {all} pixels differ",
        lines.join("\n")
    );
    eprintln!("{report}");
    assert!(all == 0, "{report}");
}

/// RoundCorner.qml's top left corner: the item's square less a quarter disc.
fn corner(x: usize, layer: &str) -> String {
    format!(
        r##"    Shape {{
        x: {x}; y: 20; width: 46; height: 46; {layer}
        preferredRendererType: Shape.CurveRenderer
        ShapePath {{
            strokeWidth: 0; fillColor: "#1d1b20"
            pathHints: ShapePath.PathSolid & ShapePath.PathNonIntersecting
            startX: 0; startY: 0
            PathAngleArc {{ moveToStart: false; centerX: 46; centerY: 46; radiusX: 46; radiusY: 46; startAngle: 180; sweepAngle: 90 }}
            PathLine {{ x: 0; y: 0 }}
        }}
    }}
"##
    )
}

fn corners() -> String {
    corner(6, "layer.enabled: true; layer.smooth: true") + &corner(162, "")
}

const CORNERS: [(&str, [usize; 4]); 2] = [
    ("the corner in a layer", [6, 20, 46, 46]),
    ("the corner", [162, 20, 46, 46]),
];

/// Workspaces.qml's numbers: a Row in a layer, shown, and the mask of the effect that paints
/// the active workspace's number white over its disc.
fn numbers() -> String {
    r##"    Item {
        x: 6; y: 6; width: 150; height: 26
        Rectangle { id: disc; x: 40; y: 1; width: 24; height: 24; radius: 12; color: "#6750a4"; z: 2 }
        Row {
            id: row; z: 4; spacing: 0; layer.enabled: true
            Repeater {
                model: 6
                Item {
                    width: 24; height: 26
                    Text {
                        anchors.centerIn: parent; text: index + 1; color: "#4a4458"
                        font.family: "Noto Sans"; font.pixelSize: 13; renderType: Text.NativeRendering
                    }
                }
            }
        }
        MultiEffect {
            z: 5; anchors.fill: row
            colorization: 1; brightness: 1; colorizationColor: "#ffffff"
            source: disc; maskEnabled: true; maskSource: row
            maskThresholdMin: 0.5; maskSpreadAtMin: 1
        }
    }
"##
    .to_string()
}

const NUMBERS: [(&str, [usize; 4]); 1] =
    [("numbers in a layer that masks an effect", [6, 6, 150, 26])];

/// The same kind of text with no layer and no effect: a line by itself, and one number
/// centred in its cell as the Row has them.
fn native_text() -> String {
    const TEXT: &str = r##"color: "#4a4458"; font.family: "Noto Sans"; font.pixelSize: 13; renderType: Text.NativeRendering"##;
    format!(
        "    Text {{ x: 10; y: 10; text: \"123456\"; {TEXT} }}\n    \
         Item {{\n        x: 6; y: 40; width: 24; height: 26\n        \
         Text {{ anchors.centerIn: parent; text: \"1\"; {TEXT} }}\n    }}\n"
    )
}

const NATIVE_TEXT: [(&str, [usize; 4]); 2] = [
    ("a line of native text", [6, 6, 150, 26]),
    ("a number centred in its cell", [6, 40, 24, 26]),
];

macro_rules! targets {
    ($($name:ident: $gpu:expr, $dpr:expr;)*) => {
        $(mod $name {
            use super::*;

            #[test]
            fn filled_shape_corner_matches_qt() {
                same_as_qt(&corners(), &CORNERS, $gpu, $dpr);
            }

            #[test]
            fn layered_numbers_masking_an_effect_match_qt() {
                same_as_qt(&numbers(), &NUMBERS, $gpu, $dpr);
            }

            #[test]
            fn native_text_matches_qt() {
                same_as_qt(&native_text(), &NATIVE_TEXT, $gpu, $dpr);
            }
        })*
    };
}

targets! {
    cpu: false, 1.0;
    gpu: true, 1.0;
    gpu_scale_1_5: true, 1.5;
}
