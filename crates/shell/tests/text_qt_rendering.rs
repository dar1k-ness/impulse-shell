//! `Text.QtRendering` as the shell's QML uses it and around: sizes, a pen between pixels,
//! colours with alpha, styles, rotation (OsdValueIndicator turns its icon), scale, quality
//! and texts that share glyphs, pixel by pixel against Qt Quick through OpenGL
//! (support/qt_gl.rs), ours on the CPU and on the GPU at screen scales 1, 1.25 and 2. All the
//! texts of a family are one scene: they share Qt's glyph cache, so the order they enter it
//! in is compared too.
//!
//! `tools/testenv/run.sh cargo nextest run -p shell --features interp --test text_qt_rendering`

#[path = "support/qt_gl.rs"]
mod qt_gl;

use qsg::raster::Canvas;
use shell::scene::{self, Target};

/// What a cell holds; `%` is the family and the render type, `$` those and the text "ABWijm",
/// `@` the family alone: a text of the default render type, which is QtRendering.
const CASES: [&str; 64] = [
    "Text { x: 8; y: 8; $; font.pixelSize: 8 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 10 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 12 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 14 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 20 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 24 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 32 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 48 }",
    "Text { x: 8.3; y: 8.6; $; font.pixelSize: 16 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; color: \"#c00000\" }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; color: \"#80c00000\" }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; opacity: 0.5 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; color: \"#800000c0\"; opacity: 0.7 }",
    "Rectangle { anchors.fill: parent; color: \"#204060\" } \
     Text { x: 8; y: 8; $; font.pixelSize: 16; color: \"#c0ffffff\" }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; style: Text.Outline; styleColor: \"#c00000\" }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; style: Text.Raised; styleColor: \"#c00000\" }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; style: Text.Sunken; styleColor: \"#c00000\" }",
    "Text { x: 100; y: 30; $; font.pixelSize: 16; rotation: 90 }",
    "Text { x: 100; y: 30; $; font.pixelSize: 16; rotation: 180 }",
    "Text { x: 100; y: 30; $; font.pixelSize: 16; rotation: 45 }",
    "Text { x: 100; y: 30; $; font.pixelSize: 16; rotation: 17.3 }",
    "Text { x: 100; y: 30; $; font.pixelSize: 16; rotation: 66.6 }",
    // OsdValueIndicator's icon at value 0.37 and at 1.
    "Item { x: 20; y: 20; width: 30; height: 30; \
     Text { anchors.centerIn: parent; anchors.alignWhenCentered: false; %; text: \"W\"; \
     font.pixelSize: 24; rotation: 180 * 0.37 } }",
    "Item { x: 20; y: 20; width: 30; height: 30; \
     Text { anchors.centerIn: parent; anchors.alignWhenCentered: false; %; text: \"W\"; \
     font.pixelSize: 30; rotation: 180 } }",
    "Text { x: 60; y: 25; $; font.pixelSize: 16; scale: 0.92 }",
    "Text { x: 60; y: 25; $; font.pixelSize: 16; scale: 1.25 }",
    "Text { x: 60; y: 25; $; font.pixelSize: 16; scale: 1.75 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; renderTypeQuality: Text.LowRenderTypeQuality }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; renderTypeQuality: Text.HighRenderTypeQuality }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; renderTypeQuality: Text.VeryHighRenderTypeQuality }",
    // Glyphs the texts before it put in the cache, and new ones.
    "Text { x: 8; y: 8; %; text: \"mjiWBA xyz\"; font.pixelSize: 16 } \
     Text { x: 8; y: 44; %; text: \"xyz ABWijm Q\"; font.pixelSize: 22 }",
    "Text { x: 8; y: 8; $; font.pixelSize: 16; font.bold: true } \
     Text { x: 8; y: 44; $; font.pixelSize: 16; font.italic: true }",
    // Between pixels by quarters, small.
    "Text { x: 8.25; y: 8.75; $; font.pixelSize: 11 }",
    "Text { x: 8.5; y: 8.5; $; font.pixelSize: 11 } \
     Text { x: 8.75; y: 40.25; $; font.pixelSize: 11 }",
    "Text { x: 8.25; y: 8.5; @; text: \"scale 1\"; color: \"#c00000\"; font.pixelSize: 11 }",
    "Item { x: 90.5; y: 20.5; width: 40; height: 31; rotation: 90; \
     Text { x: 3.4; y: 16.3; @; text: \"ab\"; font.pixelSize: 11 } }",
    "Text { x: 8.75; y: 8.25; @; text: \"ABWijm\"; font.pixelSize: 11 } \
     Text { x: 8.5; y: 40.75; @; text: \"ABWijm\"; font.pixelSize: 14 }",
    "Rectangle { anchors.fill: parent; color: \"#202020\" } \
     Item { x: 10.3; y: 8.6; width: 61; height: 33; \
     Text { x: 3.4; y: 18.3; @; text: \"scale 1\"; color: \"#ffffff\"; font.pixelSize: 11 } } \
     Item { x: 90.5; y: 20.5; width: 40; height: 31; rotation: 90; \
     Text { x: 3.4; y: 16.3; @; text: \"ab\"; color: \"#ffffff\"; font.pixelSize: 11 } }",
    "Rectangle { anchors.fill: parent; color: \"#204060\" } \
     Text { x: 8.5; y: 8.25; @; text: \"ABWijm\"; color: \"#ffffff\"; font.pixelSize: 11 }",
    "Text { x: 8.25; y: 8.75; $; font.pixelSize: 11; rotation: 90 }",
    // Under a turned or scaled item.
    "Item { x: 90.5; y: 20.5; width: 40; height: 31; rotation: 270; \
     Text { x: 2.75; y: 3.25; @; text: \"ab\"; color: \"#c00000\"; font.pixelSize: 11 } \
     Text { x: 3.4; y: 16.3; @; text: \"scale 1\"; font.pixelSize: 9 } }",
    "Item { x: 20.5; y: 10.25; width: 80; height: 40; scale: 1.25; \
     Text { x: 3.4; y: 6.3; $; font.pixelSize: 11 } }",
    "Item { x: 60; y: 20; width: 60; height: 40; rotation: 30; \
     Text { x: 5.5; y: 4.25; $; font.pixelSize: 14; rotation: -12 } }",
    "Item { x: 100; y: 10; width: 100; height: 60; rotation: 90; \
     Item { x: 10.5; y: 5.5; width: 50; height: 30; scale: 0.92; \
     Text { x: 2.25; y: 3.75; $; font.pixelSize: 13 } } }",
    "Item { x: 60.5; y: 20.5; width: 80; height: 40; rotation: 17.3; opacity: 0.5; \
     Text { x: 3.4; y: 6.3; $; font.pixelSize: 12 } }",
    "Item { x: 60; y: 10; width: 120; height: 60; rotation: 66.6; \
     Text { x: 3.5; y: 6.25; $; font.pixelSize: 12 } \
     Text { x: 3.75; y: 30.5; @; text: \"xyz Q\"; font.pixelSize: 16 } }",
    "Item { x: 60; y: 10; width: 120; height: 60; rotation: 180; \
     Text { x: 3.5; y: 6.25; $; font.pixelSize: 16; color: \"#80c00000\" } }",
    "Item { x: 60; y: 20; width: 100; height: 40; rotation: 45; \
     Text { x: 3.5; y: 6.25; $; font.pixelSize: 16; style: Text.Outline; \
     styleColor: \"#c00000\" } }",
    // The `transform` list, on the text and above it.
    "Text { x: 40; y: 10; $; font.pixelSize: 16; \
     transform: Rotation { angle: 30; origin.x: 10; origin.y: 10 } }",
    "Text { x: 20.5; y: 20.25; $; font.pixelSize: 16; \
     transform: Scale { xScale: 1.4; yScale: 0.8 } }",
    "Text { x: 20; y: 20; $; font.pixelSize: 16; transform: Translate { x: 10.3; y: 5.6 } }",
    "Text { x: 40; y: 10; $; font.pixelSize: 16; transform: [ Translate { x: 12.5 }, \
     Rotation { angle: 17.3 }, Scale { xScale: 1.2; yScale: 1.2; origin.x: 5 } ] }",
    "Text { x: 60; y: 25; $; font.pixelSize: 16; rotation: 20; scale: 1.1; \
     transform: Rotation { angle: -35; origin.x: 30; origin.y: 10 } }",
    "Text { x: 20; y: 20; $; font.pixelSize: 16; transform: Matrix4x4 { \
     matrix: Qt.matrix4x4(1, 0.3, 0, 10, -0.2, 1, 0, 5, 0, 0, 1, 0, 0, 0, 0, 1) } }",
    "Item { x: 40; y: 10; width: 100; height: 50; \
     transform: Rotation { angle: 25; origin.x: 50; origin.y: 25 } \
     Text { x: 3.5; y: 6.25; $; font.pixelSize: 14 } }",
    "Item { x: 20.5; y: 10.25; width: 80; height: 40; \
     transform: [ Scale { xScale: 1.25; yScale: 1.25 }, Translate { x: 3.3; y: 2.7 } ] \
     Text { x: 3.4; y: 6.3; $; font.pixelSize: 11 } }",
    // What a turned or scaled item draws after its text lies over the text. The rectangles
    // stand on whole pixels of the screen and of the item's layer at every scale, and the one
    // in the scaled item is white on white: their edges are not what this is about.
    "Item { x: 62; y: 26; width: 64; height: 40; rotation: 90; \
     Text { x: 3.5; y: 6.25; $; font.pixelSize: 16 } \
     Rectangle { x: 22; y: 6; width: 32; height: 12; color: \"#3366ff\" } }",
    "Item { x: 62; y: 26; width: 64; height: 40; rotation: 180; \
     Rectangle { x: 2; y: 6; width: 40; height: 20; color: \"#ffcc00\" } \
     Text { x: 3.5; y: 6.25; $; font.pixelSize: 16 } \
     Rectangle { x: 30; y: 2; width: 20; height: 32; color: \"#803366ff\" } }",
    "Item { x: 28; y: 28; width: 64; height: 32; scale: 1.25; \
     Text { x: 3.4; y: 6.3; $; font.pixelSize: 11 } \
     Rectangle { width: 64; height: 32; color: \"#b0ffffff\" } }",
    "Item { x: 62; y: 26; width: 64; height: 40; rotation: 90; \
     Text { x: 3.5; y: 6.25; $; font.pixelSize: 16 } \
     Item { x: 8; y: 4; Rectangle { x: 14; y: 2; width: 24; height: 12; color: \"#c00000\" } \
     Text { x: 4.5; y: 14.25; $; font.pixelSize: 11 } } }",
    "Item { x: 62; y: 24; width: 64; height: 40; rotation: 180; \
     Text { x: 3.5; y: 6.25; $; font.pixelSize: 16; z: 1 } \
     Rectangle { x: 22; y: 6; width: 32; height: 12; color: \"#3366ff\" } }",
    // RotatingDate: turned texts in a turned item in a scaled one.
    "Item { x: 100; y: 5; width: 80; height: 80; scale: 0.92; \
     Item { anchors.fill: parent; rotation: 30; \
     Text { x: 34.5; y: 2.25; %; text: \"W\"; font.pixelSize: 12; rotation: 0 } \
     Text { x: 60.25; y: 34.5; %; text: \"j\"; font.pixelSize: 12; rotation: 90 } \
     Text { x: 34.5; y: 62.75; %; text: \"m\"; font.pixelSize: 12; rotation: 180 } } }",
    "Item { x: 62; y: 24; width: 64; height: 40; rotation: 90; opacity: 0.6; \
     Text { x: 3.5; y: 6.25; $; font.pixelSize: 16 } \
     Rectangle { x: 22; y: 6; width: 32; height: 12; color: \"#3366ff\" } }",
    "Item { x: 62; y: 24; width: 64; height: 40; rotation: 90; \
     Rectangle { x: 22; y: 6; width: 32; height: 12; color: \"#3366ff\" } \
     Text { x: 3.5; y: 6.25; $; font.pixelSize: 16 } \
     Text { x: 9.5; y: 12.25; @; text: \"xyz\"; color: \"#c00000\"; font.pixelSize: 16 } }",
];

const CELL: (usize, usize) = (300, 90);
const COLUMNS: usize = 4;
const SIZE: (usize, usize) = (CELL.0 * COLUMNS, CELL.1 * CASES.len().div_ceil(COLUMNS));

/// Every case in its cell of a white rectangle.
fn source(family: &str) -> String {
    let plain = format!("font.family: \"{family}\"");
    let font = format!("{plain}; renderType: Text.QtRendering");
    let text = format!("text: \"ABWijm\"; {font}");
    let mut s = format!(
        "import QtQuick\nRectangle {{\n    width: {}; height: {}; color: \"white\"\n",
        SIZE.0, SIZE.1
    );
    for (i, case) in CASES.into_iter().enumerate() {
        s += &format!(
            "    Item {{ x: {}; y: {}; width: {}; height: {}; {} }}\n",
            CELL.0 * (i % COLUMNS),
            CELL.1 * (i / COLUMNS),
            CELL.0,
            CELL.1,
            case.replace('$', &text)
                .replace('%', &font)
                .replace('@', &plain)
        );
    }
    s + "}\n"
}

/// The scene under our engine: on the CPU, or on the GPU (llvmpipe, as Qt's) in a frame of
/// that devicePixelRatio.
fn ours(source: &str, gpu: Option<f64>) -> Canvas {
    let mut rt = qml::runtime::Runtime::new(None);
    let mut fonts = scene::Fonts::default();
    let root = rt.load_str(source).unwrap();
    for _ in 0..2 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, root, [SIZE.0 as f64, SIZE.1 as f64]);
    }
    let Some(dpr) = gpu else {
        let mut picture = Canvas::new(SIZE.0, SIZE.1, [0, 0, 0, 0]);
        scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut picture));
        return picture;
    };
    let (mut g, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let mut fr = shell::gpu::Frame::scaled(SIZE.0, SIZE.1, dpr);
    scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Gpu(&mut fr));
    let mut s = g.pbuffer(fr.w as i32, fr.h as i32).expect("pbuffer");
    s.reswap();
    g.render(&mut s, &mut fr, || {});
    g.read_surface(&s)
}

/// Every case in `family` has Qt's pixels: ours on the CPU when `gpu` is None, else on the
/// GPU at that devicePixelRatio, against Qt at the same.
fn compare(family: &str, gpu: Option<f64>) {
    if !(testenv::require("qml6") && qt_gl::available()) {
        return;
    }
    let source = source(family);
    let dpr = gpu.unwrap_or(1.0);
    let (qt, ours) = (qt_gl::run_scaled(&source, dpr).1, ours(&source, gpu));
    let size = shell::gpu::device_size(SIZE.0, SIZE.1, dpr);
    assert_eq!((qt.w, qt.h), size);
    assert_eq!((ours.w, ours.h), size);
    let on = gpu.map_or("cpu".into(), |d| format!("gpu{d}"));
    if let Ok(dir) = std::env::var("RI_TEXT_QT_RENDERING_DUMP") {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, p) in [("qt", &qt), ("ours", &ours)] {
            let raw: Vec<u8> = p.px.iter().flatten().copied().collect();
            let file = format!(
                "{dir}/{}.{on}.{name}.{}x{}.rgba",
                family.replace(' ', "_"),
                p.w,
                p.h
            );
            std::fs::write(file, raw).unwrap();
        }
    }
    let mut wrong = Vec::new();
    let (mut all, mut worst) = (0, 0);
    for (i, case) in CASES.into_iter().enumerate() {
        // The cell's device pixels; at 1.25 a row of pixels can be of two cells.
        let edge = |v: usize| (v as f64 * dpr).floor() as usize;
        let (x0, y0) = (CELL.0 * (i % COLUMNS), CELL.1 * (i / COLUMNS));
        let (mut count, mut most) = (0, 0);
        for y in edge(y0)..edge(y0 + CELL.1) {
            for x in edge(x0)..edge(x0 + CELL.0) {
                let (a, b) = (qt.px[y * size.0 + x], ours.px[y * size.0 + x]);
                let d = (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap_or(0);
                count += (d > 0) as usize;
                most = most.max(d);
            }
        }
        all += count;
        worst = worst.max(most);
        if most > 0 {
            wrong.push(format!(
                "case {i}: {count} pixels differ, by up to {most} of 255: {case}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{family} on {on}: {} of {} cases differ from Qt, {all} pixels by up to {worst}\n{}",
        wrong.len(),
        CASES.len(),
        wrong.join("\n")
    );
}

macro_rules! families {
    ($($name:ident: $gpu:expr;)*) => {
        $(mod $name {
            #[test]
            fn noto_sans_matches_qt() {
                super::compare("Noto Sans", $gpu);
            }

            #[test]
            fn noto_serif_matches_qt() {
                super::compare("Noto Serif", $gpu);
            }

            #[test]
            fn dejavu_sans_mono_matches_qt() {
                super::compare("DejaVu Sans Mono", $gpu);
            }
        })*
    };
}

families! {
    cpu: None;
    gpu: Some(1.0);
    gpu_at_1_25: Some(1.25);
    gpu_at_2: Some(2.0);
}
