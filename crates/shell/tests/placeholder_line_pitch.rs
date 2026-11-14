//! A TextArea's placeholder is laid out as a Text item, not as the editor's document. Basic's
//! TextArea.qml shows it in a `PlaceholderText` (a QQuickText), where a line is ascent + descent
//! high; the editor's own lines are QTextDocumentLayout's, which adds the font's leading. With
//! a font that has leading (the original's Google Sans Flex at 15 px: 18 and 19) the placeholder lines of
//! The original's Notes widget were painted one document line apart, 1 px more per line than under
//! Quickshell. The caret of the empty editor stays a document line high (`cursorRectangle`).
//! Compared with Qt itself on the same QML: the page's pixels with Qt Quick through OpenGL
//! (`support/qt_gl.rs`), what the properties read with `qml6`, offscreen.

#[path = "support/qt_gl.rs"]
mod qt_gl;

use qml::runtime::Runtime;
use qml::value::Value;
use qsg::raster::Canvas;
use shell::scene::{self, Target};
use shell::{input::Pointer, keys::Keyboard};

const BTN_LEFT: u32 = 0x110;
const SIZE: [usize; 2] = [420, 300];
const PAPER: [u8; 3] = [255, 255, 255];
const CARET: [u8; 3] = [255, 0, 0];

/// The Notes widget's editor (`NotesContent.qml`): wrapping, padding 24, no background, and
/// its placeholder, whose second line wraps at this width and whose third is empty. The family
/// is the one of ri-testenv with Latin letters and leading (DejaVu Math TeX Gyre at 15 px:
/// ascent + descent 15, leading 3).
const PAGE: &str = r##"
    Rectangle {
        id: page
        width: 420; height: 300
        color: "white"
        TextArea {
            id: ta
            anchors.fill: parent
            padding: 24
            background: null
            wrapMode: TextEdit.Wrap
            renderType: Text.NativeRendering
            color: "#ff0000"
            placeholderTextColor: "#0000ff"
            font.family: "DejaVu Math TeX Gyre"
            font.pixelSize: 15
            font.hintingPreference: Font.PreferFullHinting
            placeholderText: "Write something here...\nUse '-' to create copyable bullet points, like this:\n\nSheep fricker\n- 4x Slab\n- 1x Boat"
        }
    }
"##;

/// The rows of the page that hold placeholder ink as Qt Quick draws it through OpenGL.
fn qt_ink_rows() -> Vec<usize> {
    let source = format!(
        "import QtQuick\nimport QtQuick.Controls.Basic\nItem {{\n    \
         width: 420; height: 300\n{PAGE}}}\n"
    );
    let (_, shot) = qt_gl::run(&source);
    assert_eq!([shot.w, shot.h], SIZE);
    rows(&shot, |p| p != PAPER)
}

/// What Qt reads: the editor's cursorRectangle (y, height), and the height of two lines as a
/// Text and as a TextEdit.
struct Qt {
    caret: (f64, f64),
    two_lines: (f64, f64),
}

fn qt() -> Qt {
    let dir = tempfile::tempdir().unwrap();
    let source = format!(
        "import QtQuick\nimport QtQuick.Controls\nItem {{\n    id: root\n    \
         width: 420; height: 300\n    property bool done: false\n{PAGE}    \
         Text {{ id: asText; visible: false; font: ta.font; text: \"a\\nb\" }}\n    \
         TextEdit {{ id: asEdit; visible: false; font: ta.font; text: \"a\\nb\" }}\n    \
         Connections {{\n        target: root.Window.window\n        \
         function onFrameSwapped() {{\n            if (root.done)\n                return;\n            \
         root.done = true;\n            \
         console.log(\"CARET\", ta.cursorRectangle.y, ta.cursorRectangle.height);\n            \
         console.log(\"TWO\", asText.contentHeight, asEdit.contentHeight);\n            \
         Qt.quit();\n        }}\n    }}\n}}\n"
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
    let pair = |tag: &str| -> (f64, f64) {
        let line = log
            .lines()
            .find_map(|l| l.split_once(tag))
            .unwrap_or_else(|| panic!("no {tag} line; qml6 printed:\n{log}"))
            .1;
        let mut n = line.split_whitespace().map(|x| x.parse().unwrap());
        (n.next().unwrap(), n.next().unwrap())
    };
    Qt {
        caret: pair("CARET "),
        two_lines: pair("TWO "),
    }
}

struct Ours {
    rt: Runtime,
    fonts: scene::Fonts,
    root: usize,
    area: usize,
}

impl Ours {
    fn new() -> Ours {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(&format!(
                "import QtQuick\nimport QtQuick.Controls\nItem {{\n    width: 420; height: 300\n    \
                 property var area: ta\n{PAGE}}}\n"
            ))
            .unwrap();
        let Some(Value::Item(area)) = rt.world.borrow_mut().read(root, "area") else {
            panic!("no area");
        };
        Ours {
            rt,
            fonts: scene::Fonts::default(),
            root,
            area,
        }
    }

    fn paint(&mut self) -> Canvas {
        let size = SIZE.map(|n| n as f64);
        for _ in 0..3 {
            self.rt.settle();
            scene::layout_window(&mut self.rt, &mut self.fonts, self.root, size);
        }
        let mut c = Canvas::new(SIZE[0], SIZE[1], [0, 0, 0, 0]);
        scene::paint_window(
            &mut self.rt,
            &mut self.fonts,
            self.root,
            &mut Target::Cpu(&mut c),
        );
        c
    }

    /// A press on the editor: it takes active focus and shows its caret.
    fn focus(&mut self) {
        let mut kb = Keyboard::default();
        kb.win = Some(self.root);
        kb.sync(&mut self.rt);
        let mut ptr = Pointer::default();
        ptr.enter(&mut self.rt, self.root, 200.0, 250.0);
        ptr.motion(&mut self.rt, self.root, 200.0, 250.0);
        ptr.press(&mut self.rt, BTN_LEFT);
        ptr.release(&mut self.rt, BTN_LEFT);
        self.rt.settle();
        kb.sync(&mut self.rt);
        assert_eq!(kb.active_editor(&self.rt), Some(self.area));
    }
}

/// The rows of `c` with a pixel `hit` accepts.
fn rows(c: &Canvas, hit: impl Fn([u8; 3]) -> bool) -> Vec<usize> {
    (0..c.h)
        .filter(|y| {
            c.px[y * c.w..(y + 1) * c.w]
                .iter()
                .any(|p| hit([p[0], p[1], p[2]]))
        })
        .collect()
}

#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
)]
fn placeholder_lines_are_as_far_apart_as_in_qt() {
    if !testenv::require("qml6") || !qt_gl::available() {
        return;
    }
    let qt = qt();
    if qt.two_lines.0 == qt.two_lines.1 {
        return testenv::unavailable(
            "DejaVu Math TeX Gyre is not installed: the font Qt picked instead has no leading",
        );
    }
    let mut ours = Ours::new();
    let shot = ours.paint();
    assert_eq!(
        rows(&shot, |p| p != PAPER),
        qt_ink_rows(),
        "rows of the page with placeholder ink (ours, then Qt's)"
    );

    // The caret of the empty editor is the document's: one document line high, leading
    // included, so it reaches below the first placeholder line (whose second line's first
    // letter is drawn over it).
    ours.focus();
    let shot = ours.paint();
    let caret = rows(&shot, |p| p == CARET);
    let span = caret
        .first()
        .zip(caret.last())
        .map(|(a, b)| (*a, b + 1 - a));
    let (y, h) = qt.caret;
    assert_eq!(
        span,
        Some((y as usize, h as usize)),
        "first row and height of the caret (ours, then Qt's cursorRectangle)"
    );
}
