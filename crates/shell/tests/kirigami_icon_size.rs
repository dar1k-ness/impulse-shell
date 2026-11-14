//! Kirigami.Icon draws the pixmap a QIcon hands it, not an Image scaled to `sourceSize`.
//! Kirigami's Icon::iconPixmap asks `QIcon::actualSize` for the item's size (truncated to
//! int) and then `QIcon::pixmap` for the answer. QSvgIconEngine fits the document's default
//! size (width and height truncated to int) into the request with `QSize::scale(size,
//! Qt::KeepAspectRatio)`, whose arithmetic is integer, both times, and renders the document
//! stretched over the result; QPixmapIconEngine does the same to a raster file that is
//! larger than the request. Icon::updatePaintedGeometry fits that pixmap into the item and
//! rounds the size (a square item of a standard icon size is filled whole), and
//! Icon::calculateNodeRect centres it. Kirigami 6.31 does not round the position there: its
//! icon.vert moves each corner to a whole pixel of the framebuffer, `round(pixel + 0.0001)`,
//! and OpenGL counts the rows from the bottom, so half a pixel goes right and up. The node
//! Kirigami has for the software backend rounds half a pixel right and down instead.
//!
//! The original's Waffle Task View icon (`task-view-dark.svg`, 63.999989 x 61.62962 with a 27x26
//! viewBox, in a 26x26 icon) is a 26x26 pixmap that way and fills the item. Ours loaded it
//! like QQuickImage with a 26x26 `sourceSize` and PreserveAspectFit (27x26) and squeezed that
//! into 26x25.04, so its back rectangle ended one pixel higher than under Quickshell.
//! Compared with Kirigami itself, Qt Quick through OpenGL (`support/qt_gl.rs`), pixel by pixel:
//! what our software painter draws and what ours draws through OpenGL.
//!
//! A theme icon name is QIconLoaderEngine under QKdeTheme (the parity run's platform theme):
//! `actualSize` is the request itself for an entry in a Scalable directory and else a square
//! of the directory's size, or of the request's shorter side when that is less; the pixmap of
//! that size is the entry's file fitted into it once, a raster file only downwards. And
//! Icon::updatePaintedGeometry takes the pixmap's own size for a side of the item that was
//! not set (widthValid(), heightValid(): set explicitly, by anchors, or by a layout that gave
//! the item another size than it had).
//!
//! The original's Waffle start menu draws application icons as `WAppIcon { implicitSize: 34 }` in a
//! ColumnLayout or under `anchors.centerIn`: a 32 px icon of a Fixed or Threshold directory
//! is 32x32 in the middle of the 34x34 item. Ours stretched it over the item.
//! Compared with Kirigami drawing with OpenGL on the Wayland platform, as in the parity
//! guest: that platform loads the desktop's theme (the offscreen one has none and finds no
//! theme icon).

#[path = "support/qt_gl.rs"]
mod qt_gl;

use qsg::raster::Canvas;
use shell::scene::{self, Target};

const W: usize = 420;
const H: usize = 64;
const BG: [u8; 4] = [0x20, 0x20, 0x20, 0xff];
/// How far a channel of a theme icon, or of an icon on a scaled output, may be from
/// Kirigami's: what gradient steps and the bilinear filter of a stretched pixmap round
/// differently. A pixmap of another size is 90 and more off. A file icon has no such
/// allowance: it is Kirigami's pixel for pixel.
const NOISE: u8 = 3;

/// (file, item width, item height)
type Case = (String, f64, f64);

fn asset(name: &str) -> String {
    format!(
        "{}/../../qml/assets/icons/fluent/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// One opaque rectangle over the whole document, so its edges are the pixmap's.
fn write_svg(dir: &std::path::Path, name: &str, size: &str, view_box: &str) -> String {
    let file = dir.join(name);
    std::fs::write(
        &file,
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" {size} {view_box}>\
             <rect x=\"-1000\" y=\"-1000\" width=\"3000\" height=\"3000\" fill=\"#ffffff\"/></svg>\n"
        ),
    )
    .unwrap();
    file.to_str().unwrap().to_string()
}

/// A file whose every pixel differs from its neighbours, so a scaled copy shows how it was scaled.
fn write_png(dir: &std::path::Path, name: &str, w: usize, h: usize) -> String {
    let mut c = Canvas::new(w, h, [0, 0, 0, 255]);
    for (n, p) in c.px.iter_mut().enumerate() {
        let (x, y) = (n % w, n / w);
        *p = [
            (x * 255 / w) as u8,
            (y * 255 / h) as u8,
            ((x + y) % 2 * 255) as u8,
            255,
        ];
    }
    let file = dir.join(name);
    c.save_png(file.to_str().unwrap()).unwrap();
    file.to_str().unwrap().to_string()
}

fn cases(dir: &std::path::Path) -> Vec<Case> {
    let wide = write_svg(dir, "wide.svg", "width=\"100\" height=\"30\"", "");
    let tall = write_svg(dir, "tall.svg", "width=\"30\" height=\"100\"", "");
    let frac = write_svg(
        dir,
        "frac.svg",
        "width=\"63.999989\" height=\"61.62962\"",
        "viewBox=\"0 0 26.999999 26\"",
    );
    let big = write_png(dir, "big.png", 100, 30);
    let small = write_png(dir, "small.png", 10, 6);
    vec![
        (asset("task-view-dark.svg"), 26.0, 26.0),
        (asset("task-view-light.svg"), 26.0, 26.0),
        (asset("task-view-pressed-dark.svg"), 26.0, 26.0),
        (frac.clone(), 26.0, 26.0),
        (frac.clone(), 17.0, 31.0),
        // QSize(width(), height()) truncates the size asked of the QIcon.
        (frac, 26.6, 26.6),
        (wide.clone(), 26.0, 26.0),
        (wide.clone(), 31.0, 17.0),
        // A square item of a standard icon size is filled whole.
        (wide.clone(), 32.0, 32.0),
        (wide, 12.0, 12.0),
        (tall.clone(), 26.0, 26.0),
        (tall, 17.0, 31.0),
        // A raster file is only shrunk; the item stretches the smaller one.
        (big, 26.0, 26.0),
        (small.clone(), 26.0, 26.0),
        (small, 10.0, 6.0),
    ]
}

/// Left edge of each icon: whole, 4 apart.
fn lefts(cases: &[Case]) -> Vec<usize> {
    let mut x = 4;
    cases
        .iter()
        .map(|c| {
            let at = x;
            x += c.1.ceil() as usize + 4;
            at
        })
        .collect()
}

/// The icons side by side, 4 below the top.
fn icons(cases: &[Case]) -> String {
    icons_at(cases, 4)
}

/// The icons side by side, their tops at `y`.
fn icons_at(cases: &[Case], y: usize) -> String {
    let mut s = String::new();
    for ((file, w, h), x) in cases.iter().zip(lefts(cases)) {
        assert!(x + *w as usize + 4 <= W);
        s += &format!(
            "    Kirigami.Icon {{ x: {x}; y: {y}; width: {w}; height: {h}; \
             roundToIconSize: false; source: \"{file}\" }}\n"
        );
    }
    s
}

fn kirigami_pixels(cases: &[Case]) -> Canvas {
    let source = format!(
        "import QtQuick\nimport org.kde.kirigami as Kirigami\nRectangle {{\n    \
         width: {W}; height: {H}; color: \"#202020\"\n{}}}\n",
        icons(cases)
    );
    let (_, c) = qt_gl::run(&source);
    assert_eq!((c.w, c.h), (W, H));
    c
}

/// The same icons as our software painter draws them, and as ours draws them through OpenGL.
fn our_pixels(cases: &[Case]) -> [(&'static str, Canvas); 2] {
    our_scene(&icons(cases)).2
}

/// Our window of `body` laid out, the window, and its picture by either painter.
fn our_scene(body: &str) -> (qml::runtime::Runtime, usize, [(&'static str, Canvas); 2]) {
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport Quickshell\n\
         import org.kde.kirigami as Kirigami\nFloatingWindow {{\n    \
         width: {W}; height: {H}; color: \"#202020\"\n{body}}}\n"
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&source).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [W as f64, H as f64]);
    }
    let mut cpu = Canvas::new(W, H, BG);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut cpu));
    let (mut gpu, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let mut surface = gpu.pbuffer(W as i32, H as i32).expect("pbuffer");
    let mut frame = shell::gpu::Frame::new(W, H);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Gpu(&mut frame));
    gpu.render(&mut surface, &mut frame, || {});
    let gpu = gpu.read_surface(&surface);
    (rt, win, [("software", cpu), ("OpenGL", gpu)])
}

/// First and last row and column that are not background within `x0..x1`.
fn painted_box(c: &Canvas, x0: usize, x1: usize) -> Option<[usize; 4]> {
    let mut b: Option<[usize; 4]> = None;
    for y in 0..c.h {
        for x in x0..x1.min(c.w) {
            if c.px[y * c.w + x] != BG {
                let o = b.get_or_insert([x, y, x, y]);
                *o = [o[0].min(x), o[1].min(y), o[2].max(x), o[3].max(y)];
            }
        }
    }
    b
}

#[test]
fn file_icons_are_drawn_as_kirigami_draws_them() {
    if !qt_gl::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cases = cases(dir.path());
    let qt = kirigami_pixels(&cases);
    let mut wrong = Vec::new();
    for (painter, ours) in our_pixels(&cases) {
        for ((file, w, h), x) in cases.iter().zip(lefts(&cases)) {
            // The icon's column of the picture, with the 2 px on either side of it.
            let (x0, x1) = (x - 2, x + w.ceil() as usize + 2);
            let name = file.rsplit('/').next().unwrap();
            // Relative to the item's corner.
            let rel = |v: Option<[usize; 4]>| {
                v.map(|v| {
                    let (l, t) = (v[0] as isize - x as isize, v[1] as isize - 4);
                    let (r, b) = (v[2] as isize - x as isize, v[3] as isize - 4);
                    [l, t, r, b]
                })
            };
            let (a, b) = (painted_box(&ours, x0, x1), painted_box(&qt, x0, x1));
            if a != b {
                wrong.push(format!(
                    "{painter}, {name} in {w}x{h}: painted pixels (left, top, right, bottom) \
                     {:?}, Kirigami has {:?}",
                    rel(a),
                    rel(b)
                ));
            }
            let differ: Vec<(usize, usize)> = (0..H)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .filter(|&(x, y)| ours.px[y * W + x] != qt.px[y * W + x])
                .collect();
            if let Some(&(px, py)) = differ.first() {
                wrong.push(format!(
                    "{painter}, {name} in {w}x{h}: {} pixels differ, the first at ({}, {}): \
                     {:?}, Kirigami has {:?}",
                    differ.len(),
                    px as isize - x as isize,
                    py as isize - 4,
                    ours.px[py * W + px],
                    qt.px[py * W + px]
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Icon::Icon gives the item an implicit size of 32x32 (`setImplicitSize(32, 32)`): an icon
/// that nothing sized is 32x32, a side that was not set is 32, and a layout or a positioner
/// gives it that room. `first` is what the icons have before anything is laid out.
const UNSIZED: &str = "\
    Kirigami.Icon { id: none }
    Kirigami.Icon { id: small; x: 40; source: \"{p24}\" }
    Kirigami.Icon {
        id: sized; x: 80; width: 40; height: 20; roundToIconSize: false; source: \"{p24}\"
    }
    Kirigami.Icon { id: wide; x: 130; implicitWidth: 34; source: \"{p48}\" }
    Kirigami.Icon { id: exact; x: 170; roundToIconSize: false; source: \"{p48}\" }
    RowLayout {
        id: layout; x: 210; spacing: 0
        Kirigami.Icon { id: laid; source: \"{p24}\" }
        Rectangle { id: after; implicitWidth: 10; implicitHeight: 10; color: \"red\" }
    }
    Kirigami.Icon { id: zero; x: 260; implicitWidth: 0; implicitHeight: 0; source: \"{p24}\" }
    Kirigami.Icon { id: narrow; x: 300; width: 16; source: \"{p48}\" }
    Row {
        id: row; x: 330
        Kirigami.Icon { id: placed; source: \"{p48}\" }
        Rectangle { id: next; width: 10; height: 10; color: \"blue\" }
    }
    property string sizes: [none, small, sized, wide, exact, layout, laid, after, zero, narrow,
        row, placed, next].map(i => i.x + \",\" + i.width + \"x\" + i.height + \"/\"
        + i.implicitWidth + \"x\" + i.implicitHeight).join(\" \")
    property string first
    Component.onCompleted: {
        first = [none, small, sized, wide, exact, laid, zero, narrow, placed].map(
            i => i.implicitWidth + \"x\" + i.implicitHeight).join(\" \");
        console.log(\"FIRST\", first);
    }
    onSizesChanged: console.log(\"SIZES\", sizes)
";

#[test]
fn an_icon_that_nothing_sized_is_32_by_32() {
    if !qt_gl::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let body = UNSIZED
        .replace("{p24}", &write_png(dir.path(), "p24.png", 24, 24))
        .replace("{p48}", &write_png(dir.path(), "p48.png", 48, 48));
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport org.kde.kirigami as Kirigami\n\
         Rectangle {{\n    width: {W}; height: {H}; color: \"#202020\"\n{body}}}\n"
    );
    let (log, qt) = qt_gl::run(&source);
    let said = |mark: &str| {
        let mut lines = log.lines().filter_map(|l| l.split_once(mark));
        lines.next_back().map(|l| l.1.trim().to_string())
    };
    let first = said("FIRST ").expect("Qt printed FIRST");
    let last = said("SIZES ").unwrap_or(first.clone());
    let (rt, root, pictures) = our_scene(&body);
    let mut wrong = Vec::new();
    for (name, qt) in [("first", first), ("sizes", last)] {
        let ours = rt.get(root, name).to_js_string();
        if ours != qt {
            wrong.push(format!("{name}:\n  ours {ours}\n  Qt   {qt}"));
        }
    }
    for (painter, ours) in pictures {
        let differ: Vec<usize> = (0..W * H).filter(|&i| ours.px[i] != qt.px[i]).collect();
        if let Some(&i) = differ.first() {
            wrong.push(format!(
                "{painter}: {} pixels differ, the first at ({}, {}): {:?}, Kirigami has {:?}",
                differ.len(),
                i % W,
                i / W,
                ours.px[i],
                qt.px[i]
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

const THEME_TEST: &str = concat!(
    module_path!(),
    "::theme_icons_are_drawn_as_kirigami_draws_them"
);
/// Each theme case is drawn 4 px inside a cell of this size.
const CELL: usize = 72;
const COLUMNS: usize = 17;

/// An icon theme `t` with a directory of each kind.
fn write_theme(share: &std::path::Path) {
    let t = share.join("icons/t");
    for d in ["f16", "f32", "th24", "sc48"] {
        std::fs::create_dir_all(t.join(d)).unwrap();
    }
    std::fs::write(
        t.join("index.theme"),
        "[Icon Theme]\nName=t\nDirectories=f16,f32,th24,sc48\n\n\
         [f16]\nSize=16\nType=Fixed\n\n[f32]\nSize=32\nType=Fixed\n\n\
         [th24]\nSize=24\nType=Threshold\nThreshold=2\n\n\
         [sc48]\nSize=48\nType=Scalable\nMinSize=48\nMaxSize=256\n",
    )
    .unwrap();
    let px = |w: usize, h: usize| format!("width=\"{w}\" height=\"{h}\"");
    // In both Fixed directories: the larger file is shrunk, the smaller never enlarged.
    write_png(&t.join("f16"), "both.png", 16, 16);
    write_png(&t.join("f32"), "both.png", 32, 32);
    write_png(&t.join("f32"), "p32.png", 32, 32);
    write_svg(&t.join("f32"), "s32.svg", &px(32, 32), "");
    write_png(&t.join("th24"), "p24.png", 24, 24);
    write_svg(&t.join("sc48"), "sc.svg", &px(48, 48), "");
    // Not square: fitted into the square the directory answers.
    write_png(&t.join("f32"), "wide.png", 32, 27);
    write_svg(&t.join("f32"), "wides.svg", &px(32, 27), "");
    // Files of another size than their directory says.
    write_png(&t.join("f16"), "big.png", 64, 64);
    write_png(&t.join("f32"), "tiny.png", 10, 10);
}

/// The QML of each case; `{at}` is its place (`x: ..; y: ..`).
fn theme_cases() -> Vec<String> {
    let icon = |name: &str, size: String| {
        format!("Kirigami.Icon {{ {{at}}; {size}; roundToIconSize: false; source: \"{name}\" }}")
    };
    let implicit = |w: f64, h: f64| format!("implicitWidth: {w}; implicitHeight: {h}");
    let set = |w: f64, h: f64| format!("width: {w}; height: {h}");
    // A `p32` of implicit size 34 in an `outer` item with the properties `sized`.
    let inside = |outer: &str, sized: &str, own: &str| {
        format!(
            "{outer} {{ {{at}}; {sized} Kirigami.Icon {{ implicitWidth: 34; implicitHeight: 34; \
             {own}; roundToIconSize: false; source: \"p32\" }} }}"
        )
    };
    let mut cases = Vec::new();
    // Below, at and above each directory's size; 16, 22, 32 and 48 are standard icon sizes.
    for name in ["both", "p32", "s32", "p24", "sc", "wide", "wides", "big"] {
        for size in [
            12.0, 16.0, 17.0, 26.0, 31.0, 32.0, 33.0, 34.0, 35.0, 40.0, 50.0,
        ] {
            cases.push(icon(name, implicit(size, size)));
        }
        // A set size stretches the pixmap over the item.
        cases.push(icon(name, set(34.0, 34.0)));
    }
    cases.extend([
        icon("tiny", implicit(34.0, 34.0)),
        icon("tiny", set(34.0, 34.0)),
        // The shorter side is asked of a Fixed directory, both of a Scalable one.
        icon("p32", implicit(40.0, 34.0)),
        icon("p32", implicit(20.0, 40.0)),
        icon("sc", implicit(34.0, 40.0)),
        icon("p32", implicit(34.6, 34.6)),
        // One side set: the other is the pixmap's.
        icon("p32", "width: 40; implicitHeight: 34".into()),
        icon("p32", "implicitWidth: 40; height: 34".into()),
        // As the original's start menu has them: a layout that leaves the icon its implicit size.
        inside("ColumnLayout", "", "Layout.alignment: Qt.AlignHCenter"),
        inside("GridLayout", "", "Layout.row: 0"),
        inside("Item", "width: 50; height: 50;", "anchors.centerIn: parent"),
        // A layout or anchors that size the icon set its size.
        inside("ColumnLayout", "width: 60;", "Layout.fillWidth: true"),
        inside(
            "RowLayout",
            "",
            "Layout.preferredWidth: 40; Layout.preferredHeight: 40",
        ),
        inside("Item", "width: 50; height: 50;", "anchors.fill: parent"),
        inside(
            "Item",
            "width: 50; height: 50;",
            "anchors.left: parent.left; anchors.right: parent.right",
        ),
    ]);
    // roundToIconSize (Kirigami's default): the icon is asked at the standard icon size the
    // item's shorter side rounds down to, and drawn that large in the middle of the item.
    let round = |name: &str, size: String, round: &str| {
        format!("Kirigami.Icon {{ {{at}}; {size}; {round} source: \"{name}\" }}")
    };
    for name in ["both", "p32", "sc", "wide", "wides"] {
        for size in [12.0, 20.0, 26.0, 32.0, 34.0, 50.0, 60.0, 64.0] {
            cases.push(round(name, implicit(size, size), "roundToIconSize: true;"));
        }
        cases.push(round(name, set(34.0, 34.0), "roundToIconSize: true;"));
        cases.push(round(name, implicit(20.0, 40.0), "roundToIconSize: true;"));
        cases.push(round(name, set(50.0, 26.0), ""));
    }
    cases
}

/// The corner of case `n`'s item.
fn cell(n: usize) -> (usize, usize) {
    (n % COLUMNS * CELL + 4, n / COLUMNS * CELL + 4)
}

fn theme_scene(cases: &[String]) -> String {
    cases
        .iter()
        .enumerate()
        .map(|(n, c)| {
            let (x, y) = cell(n);
            format!("    {}\n", c.replace("{at}", &format!("x: {x}; y: {y}")))
        })
        .collect()
}

fn kirigami_theme_pixels(cases: &[String], dir: &std::path::Path, size: (usize, usize)) -> Canvas {
    let (w, h) = size;
    let shot = dir.join("kirigami.png");
    // The first frame on screen starts the grab: the icons are polished by then.
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport org.kde.kirigami as Kirigami\nWindow {{\n    \
         width: {w}; height: {h}; visible: true\n    \
         Rectangle {{\n    id: scene\n    width: {w}; height: {h}; color: \"#202020\"\n{}    }}\n    \
         property bool asked: false\n    onFrameSwapped: {{\n        if (asked)\n            \
         return;\n        asked = true;\n        \
         console.log(\"API\", GraphicsInfo.api, Screen.devicePixelRatio);\n        \
         scene.grabToImage(r => {{\n            \
         console.log(\"SAVED\", r.saveToFile(\"{}\"));\n            Qt.quit();\n        }});\n    }}\n}}\n",
        theme_scene(cases),
        shot.display()
    );
    let file = dir.join("icons.qml");
    std::fs::write(&file, source).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_SCALE_FACTOR", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    // QSGRendererInterface::OpenGL at scale 1, as the parity guest draws: Kirigami's node for
    // the software backend places a stretched pixmap's rows otherwise.
    assert!(log.contains("API 3 1"), "qml6 printed:\n{log}");
    assert!(log.contains("SAVED true"), "qml6 printed:\n{log}");
    let c = qsg::image::load_png(shot.to_str().unwrap()).unwrap();
    assert_eq!((c.w, c.h), size);
    c
}

fn our_theme_pixels(cases: &[String], size: (usize, usize)) -> Canvas {
    let (w, h) = size;
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport Quickshell\nimport org.kde.kirigami as Kirigami\n\
         FloatingWindow {{\n    width: {w}; height: {h}; color: \"#202020\"\n{}}}\n",
        theme_scene(cases)
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&source).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [w as f64, h as f64]);
    }
    let mut c = Canvas::new(w, h, BG);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
    c
}

/// First and last column and row that are not background within the cell at (`x0`, `y0`).
fn cell_box(c: &Canvas, x0: usize, y0: usize) -> Option<[usize; 4]> {
    let mut b: Option<[usize; 4]> = None;
    for y in y0..y0 + CELL {
        for x in x0..x0 + CELL {
            if c.px[y * c.w + x] != BG {
                let o = b.get_or_insert([x, y, x, y]);
                *o = [o[0].min(x), o[1].min(y), o[2].max(x), o[3].max(y)];
            }
        }
    }
    b
}

/// Whether `test` goes on: it is in a process of its own whose icon theme is `t`
/// ([`write_theme`]) under QKdeTheme, as tools/parity/setup.sh selects it.
/// Where tools/testenv/Containerfile keeps plasma-integration's platform theme plugin.
const PLUGINS: &str = "/opt/plasma-integration/plugins";

/// Runs `test` in a process of its own under each KDE platform theme: Qt's own QKdeTheme,
/// whose theme icons QIconLoaderEngine answers, and plasma-integration's plugin, which a KDE
/// desktop has and whose theme icons KIconEngine answers.
fn under_both_kde_themes(test: &str, vars: Vec<(&'static str, std::ffi::OsString)>) {
    if !std::path::Path::new(PLUGINS)
        .join("platformthemes")
        .is_dir()
    {
        testenv::unavailable(format_args!(
            "plasma-integration's platform theme plugin in {PLUGINS}"
        ));
        return;
    }
    for plugin in [false, true] {
        let mut vars = vars.clone();
        let mut unset = vec!["XDG_CURRENT_DESKTOP", "KDEHOME", "QS_ICON_THEME"];
        if plugin {
            vars.push(("QT_PLUGIN_PATH", PLUGINS.into()));
            // The shell's own style (`pragma Env` of its entry points): with the plugin Qt
            // Quick Controls would otherwise load the desktop's, whose Kirigami theme makes
            // a KIconEngine of a file as well.
            vars.push(("QT_QUICK_CONTROLS_STYLE", "Basic".into()));
        } else {
            unset.push("QT_PLUGIN_PATH");
        }
        testenv::run_in_own_process_env(test, vars, &unset);
    }
}

fn theme_setup(test: &str) -> bool {
    if !testenv::require("qml6") {
        return false;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("a headless sway (WAYLAND_DISPLAY)");
        return false;
    }
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        let (config, share) = (home.path().join(".config"), home.path().join("share"));
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("kdeglobals"), "[Icons]\nTheme=t\n").unwrap();
        write_theme(&share);
        // No theme but the one written here.
        let mut vars = testenv::home_vars(home.path());
        vars.extend([
            ("QT_QPA_PLATFORMTHEME", "kde".into()),
            ("KDE_SESSION_VERSION", "6".into()),
            ("XDG_DATA_DIRS", share.into()),
        ]);
        under_both_kde_themes(test, vars);
        return false;
    }
    true
}

#[test]
fn theme_icons_are_drawn_as_kirigami_draws_them() {
    if !theme_setup(THEME_TEST) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cases = theme_cases();
    let size = (COLUMNS * CELL, cases.len().div_ceil(COLUMNS) * CELL);
    let (qt, ours) = (
        kirigami_theme_pixels(&cases, dir.path(), size),
        our_theme_pixels(&cases, size),
    );
    let wrong = cell_differences(&cases, &ours, &qt);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The cases whose cell ([`cell`]) ours and Kirigami painted differently, each with how.
fn cell_differences(cases: &[String], ours: &Canvas, qt: &Canvas) -> Vec<String> {
    let mut wrong = Vec::new();
    for (n, case) in cases.iter().enumerate() {
        // The cell starts 4 px before the item's corner.
        let (x, y) = cell(n);
        let (x0, y0) = (x - 4, y - 4);
        let rel = |v: Option<[usize; 4]>| {
            let (x, y) = (x as i64, y as i64);
            v.map(|v| v.map(|c| c as i64))
                .map(|v| [v[0] - x, v[1] - y, v[2] - x, v[3] - y])
        };
        let (a, b) = (cell_box(ours, x0, y0), cell_box(qt, x0, y0));
        assert!(b.is_some(), "Kirigami drew nothing for {case}");
        if a != b {
            wrong.push(format!(
                "{case}: painted pixels (left, top, right, bottom) {:?}, Kirigami has {:?}",
                rel(a),
                rel(b)
            ));
            continue;
        }
        let worst = (y0..y0 + CELL)
            .flat_map(|y| (x0..x0 + CELL).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (p, q) = (ours.px[y * ours.w + x], qt.px[y * ours.w + x]);
                ((0..4).map(|k| p[k].abs_diff(q[k])).max().unwrap(), x, y)
            })
            .max()
            .unwrap();
        if worst.0 > NOISE {
            wrong.push(format!(
                "{case}: pixel ({}, {}) differs by {} of 255",
                worst.1 - x,
                worst.2 - y,
                worst.0
            ));
        }
    }
    wrong
}

const KEPT_TEST: &str = concat!(
    module_path!(),
    "::a_change_of_round_to_icon_size_keeps_the_pixmap"
);

/// Icons whose `roundToIconSize` changes once the first frame is drawn: properties, what is
/// done. `both` at 30 is the 16 px file when rounded (to 22) and the 32 px one when not.
const KEPT: &[(&str, &str)] = &[
    (
        "width: 30; height: 30; roundToIconSize: true; source: \"both\"",
        "roundToIconSize = false",
    ),
    (
        "width: 30; height: 30; roundToIconSize: false; source: \"both\"",
        "roundToIconSize = true",
    ),
    // A polish for another reason loads the pixmap of the new value.
    (
        "width: 30; height: 30; roundToIconSize: true; source: \"both\"",
        "roundToIconSize = false; width = 31; height = 31",
    ),
    (
        "width: 30; height: 30; roundToIconSize: false; source: \"both\"",
        "roundToIconSize = true; source = \"p32\"",
    ),
    (
        "width: 30; height: 30; roundToIconSize: true; source: \"both\"",
        "",
    ),
];

/// The icons of [`KEPT`] in their cells and what `act` does to them.
fn kept_members() -> (String, String) {
    let (mut icons, mut acts) = (String::new(), String::new());
    for (k, (props, act)) in KEPT.iter().enumerate() {
        let (x, y) = cell(k);
        icons += &format!("    Kirigami.Icon {{ id: k{k}; x: {x}; y: {y}; {props} }}\n");
        if !act.is_empty() {
            acts += &format!("k{k}.{}; ", act.replace("; ", &format!("; k{k}.")));
        }
    }
    (icons, acts)
}

/// Kirigami Icon::setRoundToIconSize fits the pixmap the icon has into the item anew and
/// asks for no polish: the icon draws the pixmap loaded for the old value, in the rectangle
/// of the new one, until its size or its source asks for another.
#[test]
fn a_change_of_round_to_icon_size_keeps_the_pixmap() {
    if !theme_setup(KEPT_TEST) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (w, h) = (KEPT.len() * CELL, CELL);
    let (icons, acts) = kept_members();
    let shot = dir.path().join("kirigami.png");
    let source = format!(
        "import QtQuick\nimport org.kde.kirigami as Kirigami\nWindow {{\n    \
         width: {w}; height: {h}; visible: true\n    \
         Rectangle {{\n    id: scene\n    width: {w}; height: {h}; color: \"#202020\"\n{icons}    }}\n    \
         property bool asked: false\n    onFrameSwapped: {{\n        if (asked)\n            \
         return;\n        asked = true;\n        {acts}\n        \
         console.log(\"API\", GraphicsInfo.api, Screen.devicePixelRatio);\n        \
         scene.grabToImage(r => {{\n            \
         console.log(\"SAVED\", r.saveToFile(\"{}\"));\n            Qt.quit();\n        }});\n    }}\n}}\n",
        shot.display()
    );
    let file = dir.path().join("kept.qml");
    std::fs::write(&file, source).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_SCALE_FACTOR", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(log.contains("API 3 1"), "qml6 printed:\n{log}");
    assert!(log.contains("SAVED true"), "qml6 printed:\n{log}");
    let qt = qsg::image::load_png(shot.to_str().unwrap()).unwrap();
    assert_eq!((qt.w, qt.h), (w, h));

    let source = format!(
        "import QtQuick\nimport Quickshell\nimport org.kde.kirigami as Kirigami\n\
         FloatingWindow {{\n    width: {w}; height: {h}; color: \"#202020\"\n{icons}    \
         signal act()\n    onAct: {{ {acts}}}\n}}\n"
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&source).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    let mut ours = Canvas::new(w, h, BG);
    for frame in 0..2 {
        for _ in 0..3 {
            rt.settle();
            scene::layout_window(&mut rt, &mut fonts, win, [w as f64, h as f64]);
        }
        ours = Canvas::new(w, h, BG);
        scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut ours));
        if frame == 0 {
            rt.emit(win, "act", &[], vec![]);
        }
    }
    let errors = rt.take_errors().0;
    let cases: Vec<String> = KEPT.iter().map(|(p, a)| format!("{p} then {a}")).collect();
    let wrong = cell_differences(&cases, &ours, &qt);
    assert!(
        wrong.is_empty() && errors.is_empty(),
        "{} of {} differ:\n{}\nour errors: {errors:?}",
        wrong.len(),
        cases.len(),
        wrong.join("\n")
    );
}

const PAINTED_TEST: &str = concat!(
    module_path!(),
    "::theme_icons_tell_the_painted_size_kirigami_tells"
);

/// What an icon nothing draws for, and one whose painted size the window follows, are.
const PAINTED_PROBES: &str = "    Kirigami.Icon { x: 4; y: 4; implicitWidth: 34; implicitHeight: 34; \
     roundToIconSize: false; source: \"no-theme-has-this\" }\n    \
     Kirigami.Icon { x: 4; y: 4; implicitWidth: 34; implicitHeight: 34; roundToIconSize: false }\n    \
     Kirigami.Icon { id: probe; x: 4; y: 4; implicitWidth: 40; implicitHeight: 34; \
     roundToIconSize: false; source: \"wide\"; onPaintedAreaChanged: win.areas++; \
     onPaintedWidthChanged: win.widths++; onPaintedHeightChanged: win.heights++ }\n";
const PAINTED_WINDOW: &str = "id: win\n    property int areas: 0\n    property int widths: 0\n    \
     property int heights: 0\n    \
     property string follows: probe.paintedWidth + \"x\" + probe.paintedHeight\n    \
     property string told: (areas > 0) + \" \" + (widths > 0) + \" \" + (heights > 0) + \" \" \
     + follows + \" \" + typeof probe.paintedWidth\n";

/// The members of the window that holds the icons of `cases` and [`PAINTED_PROBES`] in an
/// item: `tell()` puts `paintedWidth`x`paintedHeight` of every icon, in order, into `list`.
fn painted_members(cases: &[String], size: (usize, usize)) -> String {
    let (w, h) = size;
    format!(
        "{PAINTED_WINDOW}    width: {w}; height: {h}\n    \
         Item {{\n    id: scene\n    width: {w}; height: {h}\n{}{PAINTED_PROBES}    }}\n    \
         function painted(item) {{\n        let out = [];\n        \
         for (let k = 0; k < item.children.length; k++) {{\n            \
         const c = item.children[k];\n            \
         if (c.roundToIconSize !== undefined)\n                \
         out.push(c.paintedWidth + \"x\" + c.paintedHeight);\n            \
         out = out.concat(painted(c));\n        }}\n        return out;\n    }}\n    \
         property string list: \"\"\n    signal tell()\n    \
         onTell: list = painted(scene).join(\" \")\n",
        theme_scene(cases),
    )
}

/// `paintedWidth`x`paintedHeight` of every icon of the cases and of [`PAINTED_PROBES`] in
/// order, then whether the probe's change signals came, what a binding to its painted size
/// holds and the type of `paintedWidth`, as Kirigami 6.31 in `qml6` tells them once a frame
/// is on screen.
fn kirigami_painted(cases: &[String], dir: &std::path::Path, size: (usize, usize)) -> String {
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport org.kde.kirigami as Kirigami\nWindow {{\n    \
         {}    visible: true\n    \
         property bool asked: false\n    onFrameSwapped: {{\n        if (asked)\n            \
         return;\n        asked = true;\n        tell();\n        \
         console.log(\"PAINTED\", list, \"TOLD\", told);\n        \
         Qt.quit();\n    }}\n}}\n",
        painted_members(cases, size),
    );
    let file = dir.join("painted.qml");
    std::fs::write(&file, source).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_SCALE_FACTOR", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let line = log.lines().find_map(|l| l.split_once("PAINTED "));
    let line = line.unwrap_or_else(|| panic!("qml6 printed:\n{log}")).1;
    line.trim_end().to_string()
}

/// [`kirigami_painted`] of our scene once it was laid out and drawn.
fn our_painted(cases: &[String], size: (usize, usize)) -> String {
    let (w, h) = size;
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport Quickshell\nimport org.kde.kirigami as Kirigami\n\
         FloatingWindow {{\n    {}}}\n",
        painted_members(cases, size)
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&source).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    let mut c = Canvas::new(w, h, BG);
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [w as f64, h as f64]);
    }
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
    rt.settle();
    rt.emit(win, "tell", &[], vec![]);
    rt.settle();
    let text = |rt: &mut qml::runtime::Runtime, name: &str| {
        let v = rt.world.borrow_mut().read(win, name);
        v.map_or("undefined".to_string(), |v| v.to_js_string())
    };
    format!("{} TOLD {}", text(&mut rt, "list"), text(&mut rt, "told"))
}

/// Kirigami Icon::paintedWidth and paintedHeight: the rounded size of the rectangle the
/// pixmap is drawn in (Icon::updatePaintedGeometry), 0 when there is no pixmap, with
/// paintedAreaChanged as the change signal of both.
#[test]
fn theme_icons_tell_the_painted_size_kirigami_tells() {
    if !theme_setup(PAINTED_TEST) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cases = theme_cases();
    let size = (COLUMNS * CELL, cases.len().div_ceil(COLUMNS) * CELL);
    let (qt, ours) = (
        kirigami_painted(&cases, dir.path(), size),
        our_painted(&cases, size),
    );
    let (qt, ours): (Vec<&str>, Vec<&str>) = (qt.split(' ').collect(), ours.split(' ').collect());
    assert_eq!(qt.len(), cases.len() + 3 + 6, "Kirigami tells {qt:?}");
    assert_eq!(ours.len(), qt.len(), "ours tells {ours:?}");
    let named = |n: usize| match cases.get(n) {
        Some(c) => c.clone(),
        None if n < cases.len() + 3 => format!("probe {}", n - cases.len()),
        None => "signals (area, width, height), the binding, the type".to_string(),
    };
    let wrong: Vec<String> = (0..qt.len())
        .filter(|&n| qt[n] != ours[n])
        .map(|n| format!("{}: {}, Kirigami has {}", named(n), ours[n], qt[n]))
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} differ:\n{}",
        wrong.len(),
        qt.len(),
        wrong.join("\n")
    );
}

const PAINTED_WHEN_TEST: &str = concat!(
    module_path!(),
    "::the_painted_size_is_told_when_kirigami_tells_it"
);

/// Icons that log their status and painted size in `Component.onCompleted`,
/// `onStatusChanged` and `onPaintedAreaChanged`, and right after the window does something to
/// them once the first frame is drawn: name, properties, what is done (`""` for nothing).
const PAINTED_WHEN: &[(&str, &str, &str)] = &[
    (
        "wide",
        "implicitWidth: 40; implicitHeight: 34; roundToIconSize: false; source: \"wide\"",
        "implicitWidth = 50",
    ),
    (
        "wider",
        "implicitWidth: 40; implicitHeight: 34; roundToIconSize: true; source: \"wide\"",
        "",
    ),
    (
        "unset",
        "implicitWidth: 40; implicitHeight: 34; source: \"wide\"",
        "",
    ),
    (
        "round34",
        "implicitWidth: 34; implicitHeight: 34; roundToIconSize: true; source: \"p32\"",
        "",
    ),
    (
        "round50",
        "implicitWidth: 50; implicitHeight: 50; roundToIconSize: true; source: \"sc\"",
        "",
    ),
    (
        "round20",
        "implicitWidth: 20; implicitHeight: 40; roundToIconSize: true; source: \"p32\"",
        "",
    ),
    (
        "std32",
        "implicitWidth: 32; implicitHeight: 32; roundToIconSize: false; source: \"p32\"",
        "",
    ),
    (
        "missing",
        "implicitWidth: 34; implicitHeight: 34; roundToIconSize: false; \
         source: \"no-theme-has-this\"",
        "",
    ),
    (
        "none",
        "implicitWidth: 34; implicitHeight: 34; roundToIconSize: false",
        "",
    ),
    (
        "set",
        "width: 34; height: 34; roundToIconSize: false; source: \"wide\"",
        "width = 50; height = 50",
    ),
    (
        "setround",
        "width: 34; height: 34; roundToIconSize: true; source: \"wide\"",
        "width = 50; height = 50",
    ),
    (
        "toggle",
        "width: 40; height: 34; roundToIconSize: true; source: \"wide\"",
        "roundToIconSize = false",
    ),
    (
        "resrc",
        "width: 34; height: 34; roundToIconSize: false; source: \"wide\"",
        "source = \"p32\"",
    ),
    (
        "fallback",
        "width: 34; height: 34; roundToIconSize: false; source: \"no-theme-has-this\"; \
         fallback: \"p32\"",
        "",
    ),
];

/// More of [`PAINTED_WHEN`], where what is done changes the icon's size through its implicit
/// size, its anchors or a layout: name, the type of an outer item of 40x34 (`""` for none),
/// the icon's properties, what is done. Qt resizes the item inside the assignment; our engine
/// resizes any item, an icon or not, in its next layout pass, so what the icon tells right
/// after (`acted`) is not compared here, only that it tells the new size once, before the
/// next frame.
const PAINTED_WHEN_LAID: &[(&str, &str, &str, &str)] = &[
    (
        "grown",
        "",
        "implicitWidth: 34; implicitHeight: 34; roundToIconSize: true; source: \"sc\"",
        "implicitWidth = 50; implicitHeight = 50",
    ),
    (
        "filled",
        "Item",
        "anchors.fill: parent; roundToIconSize: false; source: \"wide\"",
        "parent.width = 50; parent.height = 50",
    ),
    (
        "laid",
        "ColumnLayout",
        "Layout.fillWidth: true; Layout.fillHeight: true; roundToIconSize: false; \
         source: \"wide\"",
        "parent.width = 50; parent.height = 50",
    ),
    (
        "laidround",
        "RowLayout",
        "Layout.fillWidth: true; Layout.fillHeight: true; source: \"sc\"",
        "parent.width = 50; parent.height = 50",
    ),
];

/// The cases of [`PAINTED_WHEN`] and [`PAINTED_WHEN_LAID`]: name, outer type (`""` for
/// none), properties, what is done.
fn painted_when() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    let alone = PAINTED_WHEN.iter().map(|&(n, p, a)| (n, "", p, a));
    alone.chain(PAINTED_WHEN_LAID.iter().copied()).collect()
}

/// The window of [`PAINTED_WHEN`]: every event appends ` name:event:frames:status:WxH` to
/// `events`, `frames` being how many frames the window has drawn; `act()` does to each icon
/// what its case says and logs what it tells right after.
fn painted_when_members() -> String {
    let (mut icons, mut acts) = (String::new(), String::new());
    for (k, (name, outer, props, act)) in painted_when().into_iter().enumerate() {
        let at = format!("x: 4; y: {}", 4 + 60 * k);
        let (open, own, close) = match outer {
            "" => (String::new(), at + "; ", ""),
            _ => (
                format!("{outer} {{ {at}; width: 40; height: 34\n    "),
                String::new(),
                " }",
            ),
        };
        icons += &format!(
            "    {open}Kirigami.Icon {{ id: i{k}; {own}{props}\n        \
             function note(e) {{ win.events += \" {name}:\" + e + \":\" + win.frames + \":\" + \
             status + \":\" + paintedWidth + \"x\" + paintedHeight }}\n        \
             function act() {{ {act}; note(\"acted\") }}\n        \
             Component.onCompleted: note(\"completed\")\n        \
             onStatusChanged: note(\"status\")\n        \
             onPaintedAreaChanged: note(\"area\")\n    }}{close}\n"
        );
        if !act.is_empty() {
            acts += &format!("i{k}.act(); ");
        }
    }
    format!(
        "id: win\n    width: 120; height: {}\n    property string events: \"\"\n    \
         property int frames: 0\n{icons}    signal act()\n    onAct: {{ {acts}}}\n    \
         property string text: \"\"\n    signal tell()\n    onTell: text = win.events\n",
        8 + 60 * painted_when().len()
    )
}

/// Each icon's events in the order it had them (icons apart: Qt polishes the last first).
fn painted_events(log: &str) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut by = std::collections::BTreeMap::<String, Vec<String>>::new();
    for e in log.split_whitespace() {
        let (name, rest) = e.split_once(':').unwrap();
        by.entry(name.to_string())
            .or_default()
            .push(rest.to_string());
    }
    by
}

/// Kirigami Icon::updatePaintedGeometry runs from componentComplete, geometryChange,
/// updatePolish and setRoundToIconSize, not where the icon is drawn: `Component.onCompleted`
/// and `onStatusChanged` read 0 (Icon::findIcon is Ready before the icon has its pixmap),
/// `onPaintedAreaChanged` reads the size before the first frame, and a read right after a
/// change of size or of `roundToIconSize` reads the new one; with `roundToIconSize` both ways.
#[test]
fn the_painted_size_is_told_when_kirigami_tells_it() {
    if !theme_setup(PAINTED_WHEN_TEST) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("when.qml");
    // `pass` is the frame the window is in: set once its items are polished (afterAnimating),
    // 0 again when the window is back in its event loop. A frame heard outside its own pass
    // is counted in `late`.
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport org.kde.kirigami as Kirigami\nWindow {{\n    {}    visible: true\n    \
         property int polished: 0\n    property int pass: 0\n    property int late: 0\n    \
         function left() {{ pass = 0 }}\n    \
         onAfterAnimating: {{\n        pass = ++polished;\n        Qt.callLater(left);\n    }}\n    \
         onFrameSwapped: {{\n        if (pass != frames + 1)\n            late++;\n        frames++;\n        \
         if (frames == 1)\n            act();\n        if (frames == 3) {{\n            \
         tell();\n            console.log(\"WHEN\", late, text);\n            Qt.quit();\n        }}\n    }}\n}}\n",
        painted_when_members()
    );
    std::fs::write(&file, source).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_SCALE_FACTOR", "1")
        // Qt's threaded render loop emits frameSwapped on its render thread, and the window
        // hears it once it is back in its event loop: behind an expose that waits there,
        // which polishes the next frame first. The basic loop draws on the window's thread
        // and the window hears each frame as it is swapped, before anything else is done.
        .env("QSG_RENDER_LOOP", "basic")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let qt = log.lines().find_map(|l| l.split_once("WHEN "));
    let qt = qt.unwrap_or_else(|| panic!("qml6 printed:\n{log}")).1;
    let (late, qt) = qt.trim_end().split_once(' ').unwrap_or((qt.trim_end(), ""));
    // `frames` in Kirigami's log is the frame an icon was polished in only when the window
    // hears each frame before it polishes the next. A frame heard after the window went back
    // to its event loop is heard behind whatever waited there: another expose from the
    // compositor polishes the next frame first, and what was done on frame 1 is then told
    // on frame 2.
    assert_eq!(
        late, "0",
        "Kirigami's window heard {late} of its 3 frames after the frame was over"
    );

    let ours = {
        let source = format!(
            "import QtQuick\nimport QtQuick.Layouts\nimport Quickshell\n\
             import org.kde.kirigami as Kirigami\nFloatingWindow {{\n    {}}}\n",
            painted_when_members()
        );
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt.load_str(&source).expect("scene loads");
        let mut fonts = scene::Fonts::default();
        let size = [120.0, (8 + 60 * painted_when().len()) as f64];
        let mut c = Canvas::new(size[0] as usize, size[1] as usize, BG);
        for frame in 0..3 {
            for _ in 0..3 {
                rt.settle();
                scene::layout_window(&mut rt, &mut fonts, win, size);
            }
            scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
            rt.settle();
            let frames = qml::value::Value::Num(frame as f64 + 1.0);
            rt.world.borrow_mut().write(win, "frames", frames);
            if frame == 0 {
                rt.emit(win, "act", &[], vec![]);
            }
            rt.settle();
        }
        rt.emit(win, "tell", &[], vec![]);
        rt.settle();
        let v = rt.world.borrow_mut().read(win, "text");
        let v = v.map_or("undefined".to_string(), |v| v.to_js_string());
        (v, rt.take_errors().0)
    };
    let (ours, errors) = ours;
    let events = |log: &str| {
        let mut by = painted_events(log);
        for (name, ..) in PAINTED_WHEN_LAID {
            if let Some(of) = by.get_mut(*name) {
                of.retain(|e| !e.starts_with("acted:"));
            }
        }
        by
    };
    let (qt, ours) = (events(qt), events(&ours));
    let cases = painted_when();
    let wrong: Vec<String> = cases
        .iter()
        .map(|p| p.0.to_string())
        .filter(|n| qt.get(n) != ours.get(n))
        .map(|n| format!("{n}: ours {:?}, Kirigami {:?}", ours.get(&n), qt.get(&n)))
        .collect();
    assert!(
        wrong.is_empty() && errors.is_empty(),
        "{} of {} differ:\n{}\nour errors: {errors:?}",
        wrong.len(),
        cases.len(),
        wrong.join("\n")
    );
}

const ORDER_TEST: &str = concat!(
    module_path!(),
    "::icons_are_polished_in_the_order_qt_polishes_them"
);

/// Icons that log their name when their status changes (`s`), when the painted size is told
/// (`p`) and in `Component.onCompleted` (`c`): next to each other, inside items, in the
/// delegates of a Repeater, in a Loader and in a layout. When `k0` is told its painted size
/// for the first time it gives `k2`, polished before it, another source. After the first
/// frame the window gives `z2`, then `z0`, then `z1` another source.
fn order_members(file: &str, other: &str) -> String {
    let told = |id: &str, n: &str, then: &str| {
        format!(
            "Kirigami.Icon {{ {id}property string n: {n}; width: 16; height: 16; \
             source: \"{file}\"; onStatusChanged: win.events += \" s:\" + n; \
             onPaintedAreaChanged: {{ win.events += \" p:\" + n; {then} }}\n \
             Component.onCompleted: win.events += \" c:\" + n }}"
        )
    };
    let i = |id: &str, n: &str| told(id, n, "");
    let (ix, iy) = (
        i("", "\"r\" + index + \"x\""),
        i("", "\"r\" + index + \"y\""),
    );
    let ask = format!("if (!win.asked) {{ win.asked = true; k2.source = \"{other}\" }}");
    let lines = [
        "id: win".to_string(),
        "width: 400; height: 400".to_string(),
        "property string events: \"\"".to_string(),
        "property int frames: 0".to_string(),
        "property bool asked: false".to_string(),
        i("", "\"a0\""),
        i("", "\"a1\""),
        format!(
            "Item {{ {}\n Item {{ {}\n {} }}\n {} }}",
            i("", "\"b0\""),
            i("", "\"b1\""),
            i("", "\"b2\""),
            i("", "\"b3\"")
        ),
        format!("Repeater {{ model: 2; Item {{ required property int index; {ix}\n {iy} }} }}"),
        format!(
            "Loader {{ sourceComponent: Item {{ {}\n {} }} }}",
            i("", "\"l0\""),
            i("", "\"l1\"")
        ),
        format!("RowLayout {{ {}\n {} }}", i("", "\"g0\""), i("", "\"g1\"")),
        i("id: k1; ", "\"k1\""),
        told("id: k0; ", "\"k0\"", &ask),
        i("id: k2; ", "\"k2\""),
        i("id: z0; ", "\"z0\""),
        i("id: z1; ", "\"z1\""),
        i("id: z2; ", "\"z2\""),
        "signal act()".to_string(),
        format!(
            "onAct: {{ events += \" |\"; z2.source = \"{other}\"; z0.source = \"{other}\"; \
             z1.source = \"{other}\" }}"
        ),
        "property string text: \"\"".to_string(),
        "signal tell()".to_string(),
        "onTell: text = win.events".to_string(),
    ];
    lines.iter().map(|l| format!("    {l}\n")).collect()
}

/// QQuickWindowPrivate::polishItems takes the item that asked for a polish last
/// (`itemsToPolish.takeLast()`), and Kirigami's Icon::updatePolish looks its icon up and
/// tells its status and its painted size there: the icon declared last is asked for first,
/// each icon is told both before the next one is polished, an icon that asks while the
/// window polishes is the next one, and of the icons written later the last one written
/// comes first.
#[test]
fn icons_are_polished_in_the_order_qt_polishes_them() {
    if !theme_setup(ORDER_TEST) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let side = "width=\"16\" height=\"16\"";
    let svg = write_svg(dir.path(), "order.svg", side, "viewBox=\"0 0 16 16\"");
    let other = write_svg(dir.path(), "other.svg", side, "viewBox=\"0 0 16 16\"");
    let file = dir.path().join("order.qml");
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport org.kde.kirigami as Kirigami\nWindow {{\n    {}    visible: true\n    \
         onFrameSwapped: {{\n        frames++;\n        \
         if (frames == 1)\n            act();\n        if (frames == 3) {{\n            \
         tell();\n            console.log(\"ORDER\", text);\n            Qt.quit();\n        }}\n    }}\n}}\n",
        order_members(&svg, &other)
    );
    std::fs::write(&file, source).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_SCALE_FACTOR", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let qt = log.lines().find_map(|l| l.split_once("ORDER "));
    let qt = qt
        .unwrap_or_else(|| panic!("qml6 printed:\n{log}"))
        .1
        .trim()
        .to_string();

    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport Quickshell\n\
         import org.kde.kirigami as Kirigami\nFloatingWindow {{\n    {}}}\n",
        order_members(&svg, &other)
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&source).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    let mut c = Canvas::new(400, 400, BG);
    for frame in 0..3 {
        for _ in 0..3 {
            rt.settle();
            scene::layout_window(&mut rt, &mut fonts, win, [400.0, 400.0]);
        }
        scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
        rt.settle();
        let frames = qml::value::Value::Num(frame as f64 + 1.0);
        rt.world.borrow_mut().write(win, "frames", frames);
        if frame == 0 {
            rt.emit(win, "act", &[], vec![]);
        }
        rt.settle();
    }
    rt.emit(win, "tell", &[], vec![]);
    rt.settle();
    let ours = rt.world.borrow_mut().read(win, "text");
    let ours = ours.map_or("undefined".to_string(), |v| v.to_js_string());
    let errors = rt.take_errors().0;
    assert!(
        ours.trim() == qt && errors.is_empty(),
        "the icons are not told in Qt's order:\n  ours {}\n  Qt   {qt}\nour errors: {errors:?}",
        ours.trim()
    );
}

const SCALED_TEST: &str = concat!(
    module_path!(),
    "::icons_are_drawn_as_kirigami_draws_them_on_a_scaled_output"
);

fn swaymsg(sway: &testenv::Sway, cmd: &str) {
    let st = std::process::Command::new("swaymsg")
        .env("SWAYSOCK", sway.ipc_socket())
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .status()
        .expect("swaymsg");
    assert!(st.success(), "swaymsg {cmd}");
}

/// The scene of the two tests above in one window: the theme cases, the file cases below them.
fn scaled_scene(theme: &[String], files: &[Case]) -> (String, (usize, usize)) {
    let rows = theme.len().div_ceil(COLUMNS) * CELL;
    (
        theme_scene(theme) + &icons_at(files, rows + 4),
        (COLUMNS * CELL, rows + H),
    )
}

/// What the sway output `on` of `outputs` (the scale of each) shows of the scene under qml6,
/// in device pixels.
fn kirigami_scaled_pixels(
    scene: &str,
    dir: &std::path::Path,
    size: (usize, usize),
    outputs: &[f64],
    on: usize,
) -> Canvas {
    let (w, h) = size;
    let scale = outputs[on];
    let sway = testenv::Sway::start();
    for (n, scale) in outputs.iter().enumerate() {
        if n > 0 {
            swaymsg(&sway, "create_output");
        }
        let name = format!("output HEADLESS-{}", n + 1);
        swaymsg(&sway, &format!("{name} resolution 2560x1600"));
        swaymsg(&sway, &format!("{name} scale {scale}"));
    }
    let output = format!("HEADLESS-{}", on + 1);
    swaymsg(&sway, &format!("focus output {output}"));
    swaymsg(&sway, "default_border none");
    // A frame is swapped once the compositor showed the one before it: by the tenth the
    // output's scale reached the window and the icons were polished and are on screen.
    // A clear window has an 8 bit buffer; Qt picks RGB565 for an opaque one here.
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport org.kde.kirigami as Kirigami\nWindow {{\n    \
         id: win\n    width: {w}; height: {h}; visible: true; color: \"transparent\"\n    \
         Rectangle {{\n    width: {w}; height: {h}; color: \"#202020\"\n{scene}    }}\n    \
         property int frames: 0\n    onFrameSwapped: {{\n        frames++;\n        \
         if (frames == 9)\n            \
         console.log(\"DPR\", Screen.devicePixelRatio, win.devicePixelRatio);\n        \
         console.log(\"FRAME\", frames, \"API\", GraphicsInfo.api);\n        \
         if (frames < 10)\n            win.update();\n    }}\n}}\n"
    );
    let file = dir.join(format!("icons-{}-{on}.qml", outputs.len()));
    std::fs::write(&file, source).unwrap();
    let (r, wr) = std::io::pipe().unwrap();
    let mut qml = sway
        .env(&mut std::process::Command::new("qml6"))
        .arg(&file)
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_WAYLAND_DISABLE_WINDOWDECORATION", "1")
        .env_remove("QT_SCALE_FACTOR")
        .stdin(std::process::Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    let log = testenv::Log::capture(r);
    let shown = log.wait(|s| s.contains("FRAME 10 API 3"));
    let shot = dir.join(format!("kirigami-{}-{on}.png", outputs.len()));
    let ok = shown.is_ok()
        && sway
            .env(&mut std::process::Command::new("grim"))
            .args(["-o", &output])
            .arg(&shot)
            .status()
            .is_ok_and(|s| s.success());
    let _ = qml.kill();
    let _ = qml.wait();
    assert!(ok, "qml6 printed:\n{}", log.text());
    // The window has its output's scale, and its screen that scale rounded up.
    let ratios = format!("DPR {} {scale}", scale.ceil());
    assert!(
        log.text().contains(&ratios),
        "no `{ratios}` in:\n{}",
        log.text()
    );
    let full = qsg::image::load_png(shot.to_str().unwrap()).unwrap();
    assert_eq!(
        (full.w, full.h),
        (2560, 1600),
        "grim's picture of the output"
    );
    // The scene's rectangle ends where the window is clear: the window draws at `scale`.
    let (dw, dh) = shell::gpu::device_size(w, h, scale);
    let at = |x: usize, y: usize| full.px[y * full.w + x];
    assert!(
        at(dw - 1, dh - 1) == BG && at(dw, 0) != BG && at(0, dh) != BG,
        "the scene is not {dw}x{dh} device pixels at scale {scale}: {:?} {:?} {:?}",
        at(dw - 1, dh - 1),
        at(dw, 0),
        at(0, dh)
    );
    let mut c = Canvas::new(dw, dh, BG);
    for y in 0..dh {
        c.px[y * dw..(y + 1) * dw].copy_from_slice(&full.px[y * full.w..y * full.w + dw]);
    }
    c
}

/// The scene as the shell draws a window on an output of scale `scale`, in device pixels, when
/// the highest wl_output.scale of the outputs is `screens`.
fn our_scaled_pixels(scene: &str, size: (usize, usize), scale: f64, screens: i32) -> Canvas {
    scene::set_screens_scale(screens);
    let (w, h) = size;
    let source = format!(
        "import QtQuick\nimport QtQuick.Layouts\nimport Quickshell\nimport org.kde.kirigami as Kirigami\n\
         FloatingWindow {{\n    width: {w}; height: {h}; color: \"#202020\"\n{scene}}}\n"
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&source).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    let (mut gpu, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let (dw, dh) = shell::gpu::device_size(w, h, scale);
    let mut surface = gpu.pbuffer(dw as i32, dh as i32).expect("pbuffer");
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [w as f64, h as f64]);
    }
    let mut fr = shell::gpu::Frame::scaled(w, h, scale);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
    gpu.render(&mut surface, &mut fr, || {});
    gpu.read_surface(&surface)
}

/// First and last column and row that are not background within `r` (x0, y0, x1, y1).
fn region_box(c: &Canvas, r: [usize; 4]) -> Option<[usize; 4]> {
    let mut b: Option<[usize; 4]> = None;
    for y in r[1]..r[3] {
        for x in r[0]..r[2] {
            if c.px[y * c.w + x] != BG {
                let o = b.get_or_insert([x, y, x, y]);
                *o = [o[0].min(x), o[1].min(y), o[2].max(x), o[3].max(y)];
            }
        }
    }
    b
}

/// Whether this process is the one that runs `test`: with the tools, in a process of its own
/// whose icon theme is the test's.
fn scaled_setup(test: &str) -> bool {
    if !testenv::require_all(&["qml6", "sway", "swaymsg", "grim"]) {
        return false;
    }
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        let (config, share) = (home.path().join(".config"), home.path().join("share"));
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("kdeglobals"), "[Icons]\nTheme=t\n").unwrap();
        write_theme(&share);
        let mut vars = testenv::home_vars(home.path());
        vars.extend([
            ("QT_QPA_PLATFORMTHEME", "kde".into()),
            ("KDE_SESSION_VERSION", "6".into()),
            ("XDG_DATA_DIRS", share.into()),
        ]);
        under_both_kde_themes(test, vars);
        return false;
    }
    true
}

/// Each case's name, the scene rectangle compared for it and its item's corner.
type Regions = Vec<(String, [usize; 4], (usize, usize))>;

/// The scene of [`scaled_scene`], its size and its cases.
fn scaled_cases(dir: &std::path::Path) -> (String, (usize, usize), Regions) {
    let (theme, files) = (theme_cases(), cases(dir));
    let (scene, size) = scaled_scene(&theme, &files);
    let rows = size.1 - H;
    let mut regions: Regions = Vec::new();
    for (n, case) in theme.iter().enumerate() {
        let (x, y) = cell(n);
        regions.push((
            case.clone(),
            [x - 4, y - 4, x - 4 + CELL, y - 4 + CELL],
            (x, y),
        ));
    }
    for ((file, w, h), x) in files.iter().zip(lefts(&files)) {
        let name = file.rsplit('/').next().unwrap();
        regions.push((
            format!("{name} in {w}x{h}"),
            [x - 2, rows, x + w.ceil() as usize + 2, rows + H],
            (x, rows + 4),
        ));
    }
    (scene, size, regions)
}

/// The cases a window on output `on` of `outputs` (the scale of each) draws differently from
/// Kirigami's.
fn scaled_differences(
    (scene, size, regions): &(String, (usize, usize), Regions),
    dir: &std::path::Path,
    outputs: &[f64],
    on: usize,
) -> Vec<String> {
    let scale = outputs[on];
    let place = match outputs {
        [_] => format!("scale {scale}"),
        _ => format!("scale {scale} of {outputs:?}"),
    };
    let screens = outputs.iter().map(|s| s.ceil() as i32).max().unwrap();
    let (qt, ours) = (
        kirigami_scaled_pixels(scene, dir, *size, outputs, on),
        our_scaled_pixels(scene, *size, scale, screens),
    );
    assert_eq!((ours.w, ours.h), (qt.w, qt.h));
    if let Some(keep) = std::env::var_os("KIRIGAMI_ICON_KEEP") {
        let keep = std::path::PathBuf::from(keep);
        for (name, c) in [("qt", &qt), ("ours", &ours)] {
            let f = keep.join(format!("{name}-{}-{on}-{scale}.png", outputs.len()));
            c.save_png(f.to_str().unwrap()).unwrap();
        }
    }
    let mut wrong = Vec::new();
    for (case, r, at) in regions {
        // Whole device pixels that cover the region; the corner in device pixels.
        let d = [
            (r[0] as f64 * scale).floor() as usize,
            (r[1] as f64 * scale).floor() as usize,
            (r[2] as f64 * scale).ceil() as usize,
            (r[3] as f64 * scale).ceil() as usize,
        ];
        let (cx, cy) = (at.0 as f64 * scale, at.1 as f64 * scale);
        let rel = |v: Option<[usize; 4]>| {
            v.map(|v| {
                [
                    v[0] as f64 - cx,
                    v[1] as f64 - cy,
                    v[2] as f64 - cx,
                    v[3] as f64 - cy,
                ]
            })
        };
        let (a, b) = (region_box(&ours, d), region_box(&qt, d));
        assert!(b.is_some(), "Kirigami drew nothing for {case} on {place}");
        if a != b {
            wrong.push(format!(
                "{place}: {case}: painted pixels (left, top, right, bottom) {:?}, \
                 Kirigami has {:?}",
                rel(a),
                rel(b)
            ));
            continue;
        }
        let worst = (d[1]..d[3])
            .flat_map(|y| (d[0]..d[2]).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (p, q) = (ours.px[y * ours.w + x], qt.px[y * qt.w + x]);
                ((0..4).map(|k| p[k].abs_diff(q[k])).max().unwrap(), x, y)
            })
            .max()
            .unwrap();
        if worst.0 > NOISE {
            wrong.push(format!(
                "{place}: {case}: pixel ({}, {}) differs by {} of 255",
                worst.1 as f64 - cx,
                worst.2 as f64 - cy,
                worst.0
            ));
        }
    }
    wrong
}

/// At a fractional scale and at 2, on screen: the pixmap is asked for in device pixels and its
/// corner lands on a device pixel as Kirigami's vertex shader rounds it.
#[test]
fn icons_are_drawn_as_kirigami_draws_them_on_a_scaled_output() {
    if !scaled_setup(SCALED_TEST) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cases = scaled_cases(dir.path());
    let wrong: Vec<String> = [1.25, 2.0]
        .iter()
        .flat_map(|scale| scaled_differences(&cases, dir.path(), &[*scale], 0))
        .collect();
    assert!(
        wrong.is_empty(),
        "{} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

const OUTPUTS_TEST: &str = concat!(
    module_path!(),
    "::icons_go_by_the_highest_scale_of_all_outputs"
);

/// Two outputs of different scales, the window on each in turn: QIcon::actualSize goes by
/// QGuiApplication::devicePixelRatio, the highest wl_output.scale of all outputs, so an icon
/// on the output of scale 1 is not the one a lone such output shows.
#[test]
fn icons_go_by_the_highest_scale_of_all_outputs() {
    if !scaled_setup(OUTPUTS_TEST) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cases = scaled_cases(dir.path());
    let mut wrong = Vec::new();
    for outputs in [[1.0, 2.0], [1.25, 2.0]] {
        for on in 0..2 {
            wrong.extend(scaled_differences(&cases, dir.path(), &outputs, on));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} differ:\n{}",
        wrong.len(),
        4 * cases.2.len(),
        wrong.join("\n")
    );
}
