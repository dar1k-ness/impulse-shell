//! Methods of TextEdit and TextInput that answer from the laid-out text give what Qt 6.12
//! gives: every fixture of `tests/text_methods` prints the same `console.*` lines in Qt (the
//! PySide6 of ri-testenv, offscreen) and in this engine with the shell's fonts.

use shell::scene;
use std::{cell::RefCell, path::Path, rc::Rc};

#[path = "../../qml/tests/support/qt_log.rs"]
mod qt_log;

thread_local!(static CONSOLE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) });

/// The `console.*` lines of `file` in this engine, with the text hooks the shell installs and
/// the root item laid out at its own size before the first `Qt.callLater` call runs.
fn our_log(file: &Path) -> Vec<String> {
    qml::builtins::CONSOLE_SINK
        .get_or_init(|| |_, line| CONSOLE.with_borrow_mut(|c| c.push(line.to_owned())));
    CONSOLE.with_borrow_mut(Vec::clear);
    let mut rt = qml::runtime::Runtime::new(file.parent().map(Path::to_path_buf));
    let fonts = Rc::new(RefCell::new(scene::Fonts::default()));
    scene::text_hooks(&mut rt.world.borrow_mut(), &fonts);
    let root = rt.load_file(file).unwrap();
    rt.settle();
    let size = {
        let mut w = rt.world.borrow_mut();
        ["width", "height"].map(|n| w.read(root, n).map_or(0.0, |v| v.to_number()))
    };
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, size);
    let mut now = 0.0;
    while rt.world.borrow().quit.is_none() {
        assert!(now < 60_000.0, "{} never called Qt.quit()", file.display());
        now += 16.0;
        rt.tick(now);
        rt.run_later();
        rt.settle();
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    CONSOLE.take()
}

/// Qt's lines and ours for `fixture` of `tests/text_methods`.
fn both(fixture: &str) -> (Vec<String>, Vec<String>) {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/text_methods")
        .join(fixture);
    (qt_log::qt_log(&file), our_log(&file))
}

/// TextEdit.positionToRectangle(pos) as qml/modules/rusty/overlay/notes/NotesContent.qml calls
/// it: the cursor's rectangle at the start and the end of every line of a wrapped plain text
/// with padding, and at positions outside the text.
#[test]
fn text_edit_position_to_rectangle_matches_qt() {
    if !qt_log::qt_available() {
        return;
    }
    let (qt, ours) = both("position_to_rectangle.qml");
    assert_eq!(qt.len(), 10, "Qt printed {qt:#?}");
    assert_eq!(qt[0], "typeof function");
    let differ: Vec<String> = (0..qt.len().max(ours.len()))
        .filter(|&i| qt.get(i) != ours.get(i))
        .map(|i| format!("ours {:?}\n  Qt {:?}", ours.get(i), qt.get(i)))
        .collect();
    assert!(
        differ.is_empty(),
        "{} of {} lines differ from Qt:\n{}",
        differ.len(),
        qt.len(),
        differ.join("\n")
    );
}

/// NotesContent.qml calls positionToRectangle from `onWidthChanged`, which the window's layout
/// pass runs while it holds the fonts: the call answers there as it does afterwards.
#[test]
fn position_to_rectangle_answers_inside_the_layout_pass() {
    let mut rt = qml::runtime::Runtime::new(None);
    let fonts = Rc::new(RefCell::new(scene::Fonts::default()));
    scene::text_hooks(&mut rt.world.borrow_mut(), &fonts);
    let root = rt
        .load_str(
            r#"import QtQuick
Item {
    width: 320; height: 400
    property string during
    property string after
    function keep() { after = ask() }
    function ask() { var r = edit.positionToRectangle(edit.length); return [r.x, r.y, r.width, r.height].join(" ") }
    TextEdit {
        id: edit
        anchors.left: parent.left
        anchors.right: parent.right
        wrapMode: TextEdit.Wrap
        padding: 24
        font.pixelSize: 16
        text: "a line long enough to wrap twice at this width for sure, and then some more words"
        onWidthChanged: parent.during += "," + parent.ask()
    }
}"#,
        )
        .unwrap();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [320.0, 400.0]);
    let during = rt.world.borrow_mut().read(root, "during").unwrap();
    let during = during.host_str().unwrap().to_string();
    rt.call_method(root, "keep", Vec::new()).unwrap().unwrap();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let mut w = rt.world.borrow_mut();
    let after = w.read(root, "after").unwrap();
    let after = after.host_str().unwrap();
    assert!(!during.is_empty(), "the layout pass ran no onWidthChanged");
    assert_eq!(during.rsplit(',').next(), Some(&*after), "{during}");
    let y: f64 = after.split(' ').nth(1).unwrap().parse().unwrap();
    assert!(y > 24.0, "the text did not wrap: {after}");
}
