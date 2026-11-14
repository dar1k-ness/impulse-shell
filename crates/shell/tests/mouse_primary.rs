//! A left-button mouse selection in a TextInput/TextField/TextArea goes to the primary
//! selection on release, like QQuickTextInput::mouseReleaseEvent (copy(QClipboard::Selection),
//! only for echoMode Normal) and QQuickTextControl (setClipboardSelection). A TextInput keyboard
//! selection does not export (a TextEdit one does: keys.rs tests).

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, keys, scene};

const BTN_LEFT: u32 = 0x110;

const QML: &str = r#"
import QtQuick
import QtQuick.Controls
Item {
    width: 400; height: 400
    property var input: ti
    property var field: tf
    property var area: ta
    property var pass: pw
    TextInput { id: ti; x: 0; y: 0; width: 380; height: 40; text: "hello world" }
    TextField { id: tf; x: 0; y: 60; width: 380; height: 40; text: "hello world" }
    TextArea { id: ta; x: 0; y: 120; width: 380; height: 80; text: "hello world" }
    TextField { id: pw; x: 0; y: 220; width: 380; height: 40; text: "secret"; echoMode: TextInput.Password }
}
"#;

fn item(rt: &mut Runtime, root: usize, p: &str) -> usize {
    match rt.world.borrow_mut().read(root, p) {
        Some(Value::Item(i)) => i,
        _ => panic!("no {p}"),
    }
}

fn setup() -> (Runtime, usize, Pointer) {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [400.0, 400.0]);
    let mut ptr = Pointer::default();
    ptr.fonts = fonts;
    ptr.enter(&mut rt, root, 1.0, 1.0);
    let _ = keys::take_primary_out();
    (rt, root, ptr)
}

/// Item-local y of the editor's middle row, scene x from the left edge to past the end.
fn drag(rt: &mut Runtime, root: usize, ptr: &mut Pointer, i: usize) {
    let (x, y) = rt.world.borrow_mut().scene_pos(i, Some(root));
    let y = y + 20.0;
    ptr.motion(rt, root, x + 1.0, y);
    ptr.press(rt, BTN_LEFT);
    for k in 1..=10 {
        ptr.motion(rt, root, x + 1.0 + k as f64 * 37.0, y);
    }
    assert_eq!(
        keys::take_primary_out(),
        None,
        "exported before the release"
    );
    ptr.release(rt, BTN_LEFT);
}

fn selected(rt: &mut Runtime, i: usize) -> String {
    match rt.world.borrow_mut().read(i, "selectedText") {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

#[test]
fn drag_selection_exports_primary_on_release() {
    let (mut rt, root, mut ptr) = setup();
    for p in ["input", "field", "area"] {
        let i = item(&mut rt, root, p);
        drag(&mut rt, root, &mut ptr, i);
        assert_eq!(selected(&mut rt, i), "hello world", "{p}");
        assert_eq!(
            keys::take_primary_out().as_deref(),
            Some("hello world"),
            "{p}"
        );
    }
}

#[test]
fn password_selection_is_not_exported() {
    let (mut rt, root, mut ptr) = setup();
    let i = item(&mut rt, root, "pass");
    drag(&mut rt, root, &mut ptr, i);
    assert_eq!(selected(&mut rt, i), "secret");
    assert_eq!(keys::take_primary_out(), None);
}

#[test]
fn double_click_selects_word_and_exports() {
    let (mut rt, root, mut ptr) = setup();
    let i = item(&mut rt, root, "field");
    let (x, y) = rt.world.borrow_mut().scene_pos(i, Some(root));
    ptr.motion(&mut rt, root, x + 20.0, y + 20.0);
    for _ in 0..2 {
        ptr.press(&mut rt, BTN_LEFT);
        // A motion event that keeps the position (ydotool sends one) must not reselect.
        ptr.motion(&mut rt, root, x + 20.0, y + 20.0);
        ptr.release(&mut rt, BTN_LEFT);
    }
    assert_eq!(selected(&mut rt, i), "hello");
    assert_eq!(keys::take_primary_out().as_deref(), Some("hello"));
}

#[test]
fn plain_click_exports_nothing() {
    let (mut rt, root, mut ptr) = setup();
    let i = item(&mut rt, root, "input");
    let (x, y) = rt.world.borrow_mut().scene_pos(i, Some(root));
    ptr.motion(&mut rt, root, x + 30.0, y + 20.0);
    ptr.press(&mut rt, BTN_LEFT);
    ptr.release(&mut rt, BTN_LEFT);
    assert_eq!(selected(&mut rt, i), "");
    assert_eq!(keys::take_primary_out(), None);
}

/// The original's To Do dialog: a TextField stacked over a sibling SwipeView. The SwipeView only filters
/// its own descendants, so a selecting drag past startDragDistance is not stolen.
#[test]
fn sibling_swipeview_does_not_steal_selection() {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            r#"
import QtQuick
import QtQuick.Controls
Item {
    width: 400; height: 400
    property var field: tf
    SwipeView { x: 0; y: 0; width: 400; height: 400
        Item { width: 400; height: 400 }
        Item { width: 400; height: 400 }
    }
    TextField { id: tf; x: 0; y: 60; width: 380; height: 40; text: "hello world" }
}
"#,
        )
        .unwrap();
    rt.settle();
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, [400.0, 400.0]);
    let mut ptr = Pointer::default();
    ptr.fonts = fonts;
    ptr.enter(&mut rt, root, 1.0, 1.0);
    let _ = keys::take_primary_out();
    let i = item(&mut rt, root, "field");
    let (x, y) = rt.world.borrow_mut().scene_pos(i, Some(root));
    let y = y + 20.0;
    ptr.motion(&mut rt, root, x + 1.0, y);
    ptr.press(&mut rt, BTN_LEFT);
    for dx in [9.0, 17.0, 60.0, 200.0, 370.0] {
        ptr.motion(&mut rt, root, x + 1.0 + dx, y);
    }
    ptr.release(&mut rt, BTN_LEFT);
    assert_eq!(selected(&mut rt, i), "hello world");
    assert_eq!(keys::take_primary_out().as_deref(), Some("hello world"));
}
