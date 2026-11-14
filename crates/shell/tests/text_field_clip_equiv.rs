//! A TextField draws its text only inside its paddings: QQuickTextField::updatePaintNode clips
//! the text to `clipRect().adjusted(leftPadding, topPadding, -rightPadding, -bottomPadding)`,
//! whatever `clip` says. A TextInput or TextEdit draws all of its text unless `clip` is set, and
//! then up to its own edges. `clipRect()` of an editor is wider than the editor on the right, for
//! the cursor: by 1 px in a TextInput, by 4 px in a TextEdit with text; a `cursorDelegate` changes
//! that only once its item exists, which is when the cursor is shown. Compared with Qt itself (`qml6`, offscreen, software renderer): the box of
//! the ink of each field, whose text is full blocks (U+2588), so the ink reaches every edge of
//! what is drawn. The fields set `NativeRendering`: the software renderer draws every text so
//! (QSGSoftwareGlyphNode paints the glyph run), while a text that sets no render type is one of
//! distance fields in Qt's OpenGL scene graph and in ours, and those leave a grey pixel past the
//! last block. That text is compared with Qt's OpenGL scene graph (support/qt_window.rs) below.
//! Ours drew all of the text of a TextField: once the field scrolls (`hscroll`), the text of
//! The original's search field ran out of its left edge over the search icon.

#[path = "support/qt_window.rs"]
mod qt_window;

use qml::runtime::Runtime;
use qsg::raster::Canvas;
use shell::scene::{self, Target};

const SIZE: [usize; 2] = [400, 540];
const BAND: usize = 60;
const PAPER: [u8; 3] = [255, 255, 255];

/// Each field stands at x 150, 20 px under the top of its band, 100 px wide; its text is 60
/// blocks, wider than the page. The kind, the pixel size of the font, the
/// cursor (set once the field is complete: Qt moves a cursor set beside the text) and the rest.
const FIELDS: [(&str, u32, usize, &str); 9] = [
    ("TextField", 15, 30, "leftPadding: 10; rightPadding: 6"),
    ("TextField", 15, 0, "leftPadding: 10; rightPadding: 6"),
    (
        "TextField",
        15,
        60,
        "leftPadding: 3; rightPadding: 17; horizontalAlignment: TextInput.AlignRight",
    ),
    (
        "TextField",
        30,
        30,
        "height: 20; topPadding: 6; bottomPadding: 4",
    ),
    ("TextInput", 15, 30, ""),
    ("TextInput", 15, 30, "clip: true"),
    ("TextEdit", 15, 0, "clip: true"),
    ("TextInput", 15, 30, "clip: true; cursorDelegate: Item {}"),
    ("TextEdit", 15, 0, "clip: true; cursorDelegate: Item {}"),
];

fn page() -> String {
    let mut s = String::from(
        "    Rectangle {\n        id: page\n        width: 400; height: 540\n        \
         color: \"white\"\n",
    );
    for (n, (kind, size, cursor, props)) in FIELDS.iter().enumerate() {
        let background = if *kind == "TextField" {
            "background: null; "
        } else {
            ""
        };
        let native = if *kind == "TextEdit" {
            "TextEdit"
        } else {
            "TextInput"
        };
        s += &format!(
            "        {kind} {{ x: 150; y: {}; width: 100; color: \"black\"; {background}\
             renderType: {native}.NativeRendering; font.family: \"DejaVu Sans\"; font.pixelSize: {size}; text: \"{}\"; \
             Component.onCompleted: cursorPosition = {cursor}; {props} }}\n",
            n * BAND + 20,
            "\u{2588}".repeat(60)
        );
    }
    s + "    }\n"
}

/// Per band: the first and last column and the first and last row with ink.
type Boxes = Vec<Option<[usize; 4]>>;

fn boxes(px: impl Fn(usize, usize) -> [u8; 3]) -> Boxes {
    (0..FIELDS.len())
        .map(|n| {
            let mut b: Option<[usize; 4]> = None;
            for y in n * BAND..(n + 1) * BAND {
                for x in 0..SIZE[0] {
                    if px(x, y) != PAPER {
                        let o = b.unwrap_or([x, x, y, y]);
                        b = Some([o[0].min(x), o[1].max(x), o[2].min(y), o[3].max(y)]);
                    }
                }
            }
            b
        })
        .collect()
}

fn qt() -> Boxes {
    let dir = tempfile::tempdir().unwrap();
    let shot = dir.path().join("page.ppm");
    let source = format!(
        "import QtQuick\nimport QtQuick.Controls\nItem {{\n    id: root\n    \
         width: 400; height: 540\n    property bool done: false\n{}    \
         Connections {{\n        target: root.Window.window\n        \
         function onFrameSwapped() {{\n            if (root.done)\n                return;\n            \
         root.done = true;\n            page.grabToImage(r => {{\n                \
         console.log(\"SAVED\", r.saveToFile(\"{}\"));\n                Qt.quit();\n            \
         }});\n        }}\n    }}\n}}\n",
        page(),
        shot.display()
    );
    let file = dir.path().join("page.qml");
    std::fs::write(&file, source).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_QUICK_BACKEND", "software")
        .env("QT_QUICK_CONTROLS_STYLE", "Basic")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(log.contains("SAVED true"), "qml6 printed:\n{log}");
    // Binary PPM: "P6\n<width> <height>\n255\n", then RGB rows.
    let ppm = std::fs::read(&shot).unwrap();
    let header = format!("P6\n{} {}\n255\n", SIZE[0], SIZE[1]);
    assert!(ppm.starts_with(header.as_bytes()), "not a {header:?} image");
    let px = &ppm[header.len()..];
    assert_eq!(px.len(), SIZE[0] * SIZE[1] * 3);
    boxes(|x, y| px[(y * SIZE[0] + x) * 3..][..3].try_into().unwrap())
}

/// `body` in an Item of `size`, drawn by our CPU path.
fn cpu(body: &str, size: [usize; 2]) -> Canvas {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(&format!(
            "import QtQuick\nimport QtQuick.Controls\nItem {{\n    width: {}; height: {}\n{body}}}\n",
            size[0], size[1]
        ))
        .unwrap();
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, root, size.map(|n| n as f64));
    }
    let mut c = Canvas::new(size[0], size[1], [0, 0, 0, 0]);
    scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut c));
    c
}

fn ours() -> Boxes {
    let c = cpu(&page(), SIZE);
    boxes(|x, y| {
        let p = c.px[y * c.w + x];
        [p[0], p[1], p[2]]
    })
}

#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
)]
fn a_text_field_draws_its_text_inside_its_paddings_as_qt() {
    if !testenv::require("qml6") {
        return;
    }
    let (qt, ours) = (qt(), ours());
    let mut report = String::new();
    let mut differ = 0;
    for (n, (kind, size, cursor, props)) in FIELDS.iter().enumerate() {
        if qt[n] != ours[n] {
            differ += 1;
            report += &format!(
                "{kind} {{ font.pixelSize: {size}; {props} }} at x 150, y {}, 100 wide, cursor {cursor}\n    \
                 ink columns and rows: ours {:?}, Qt {:?}\n",
                n * BAND + 20,
                ours[n],
                qt[n]
            );
        }
    }
    assert!(
        differ == 0,
        "{differ} of {} fields draw their text elsewhere than Qt:\n{report}",
        FIELDS.len()
    );
}

/// The right-aligned field with the render type of a text that sets none, QtRendering: every
/// pixel of Qt's OpenGL scene graph, on both of our paths. The distance field of the last block
/// leaves a grey pixel in column 233, one past where the native glyph ends.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
)]
fn a_right_aligned_field_of_distance_fields_is_qts_opengl_picture() {
    let size = [SIZE[0], BAND];
    let (_, pixel, cursor, props) = FIELDS[2];
    let body = format!(
        "    Rectangle {{ anchors.fill: parent; color: \"white\" }}\n    \
         TextField {{ x: 150; y: 20; width: 100; color: \"black\"; background: null; \
         font.family: \"DejaVu Sans\"; font.pixelSize: {pixel}; text: \"{}\"; \
         Component.onCompleted: cursorPosition = {cursor}; {props} }}\n",
        "\u{2588}".repeat(60)
    );
    let Some((qt, gpu)) = qt_window::both(&body, (size[0], size[1]), 1.0) else {
        return;
    };
    let last = |c: &Canvas| (0..c.w).rev().find(|&x| c.px[35 * c.w + x][..3] != PAPER);
    assert_eq!(last(&qt), Some(233), "the last column of Qt's ink");
    for (path, ours) in [("GPU", gpu), ("CPU", cpu(&body, size))] {
        let (n, max, at) = qt_window::diff(&ours, &qt, 0);
        assert_eq!(
            n, 0,
            "{path}: {n} pixels differ from Qt's, by {max} at {at:?}"
        );
    }
}
