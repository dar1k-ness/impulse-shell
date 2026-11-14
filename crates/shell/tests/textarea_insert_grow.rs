//! Left sidebar AssistantChat input: Shift+Enter calls `insert(cursorPosition, "\n")` on a wrapping
//! TextArea inside a ScrollView whose RowLayout and background follow its height.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r#"
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Item {
    id: root
    width: 400; height: 600
    property bool go: false
    property var field: field
    property var box: box
    onGoChanged: field.insert(field.cursorPosition, "\n")
    Rectangle {
        id: box
        width: 400
        implicitHeight: Math.max(row.implicitHeight + 15, 45)
        RowLayout {
            id: row
            anchors.left: parent.left
            anchors.right: parent.right
            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(400, field.height)
                clip: true
                TextArea {
                    id: field
                    anchors.fill: parent
                    wrapMode: TextArea.Wrap
                    padding: 10
                    font.family: "Noto Sans"
                    font.pixelSize: 15
                    text: "c"
                }
            }
        }
    }
}
"#;

fn item(rt: &mut Runtime, root: usize, name: &str) -> usize {
    match rt.world.borrow_mut().read(root, name) {
        Some(Value::Item(i)) => i,
        v => panic!("{name}: {v:?}"),
    }
}

fn num(rt: &mut Runtime, i: usize, name: &str) -> f64 {
    rt.world
        .borrow_mut()
        .read(i, name)
        .map(|v| v.to_number())
        .unwrap_or(f64::NAN)
}

#[test]
fn inserted_newline_grows_the_input() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [400.0, 600.0]);
    rt.settle();
    let (field, box_) = (item(&mut rt, root, "field"), item(&mut rt, root, "box"));
    let (h1, b1) = (num(&mut rt, field, "height"), num(&mut rt, box_, "height"));
    // Not a `cursorPosition` in the document: a TextArea being created refuses it.
    rt.set(field, "cursorPosition", Value::Num(1.0));
    rt.set(root, "go", Value::Bool(true));
    rt.settle();
    for _ in 0..3 {
        scene::layout_window(&mut rt, &mut fonts, root, [400.0, 600.0]);
        rt.settle();
    }
    let (h2, b2) = (num(&mut rt, field, "height"), num(&mut rt, box_, "height"));
    assert_eq!(rt.get(field, "text").to_js_string(), "c\n");
    let line = h1 - 20.0;
    assert!(line > 5.0, "one line: {h1}");
    assert!(
        (h2 - (h1 + line)).abs() < 1.0,
        "field {h1} -> {h2}, box {b1} -> {b2}"
    );
    assert!(
        (b2 - (b1 + line)).abs() < 1.0,
        "field {h1} -> {h2}, box {b1} -> {b2}"
    );
}

/// The frame's local relayout (main.rs `draw`): the resized items are re-placed through
/// `relayout_targets`, falling back to a full pass only when a resize is not local.
#[test]
fn inserted_newline_grows_the_input_in_a_local_pass() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [400.0, 600.0]);
    rt.settle();
    rt.world.borrow_mut().resized.clear();
    let (field, box_) = (item(&mut rt, root, "field"), item(&mut rt, root, "box"));
    let (h1, b1) = (num(&mut rt, field, "height"), num(&mut rt, box_, "height"));
    // Not a `cursorPosition` in the document: a TextArea being created refuses it.
    rt.set(field, "cursorPosition", Value::Num(1.0));
    rt.set(root, "go", Value::Bool(true));
    rt.settle();
    let mut rs = std::mem::take(&mut rt.world.borrow_mut().resized);
    for pass in 0..4 {
        if rs.is_empty() {
            break;
        }
        let local = {
            let mut w = rt.world.borrow_mut();
            rs.iter().all(|&i| scene::is_local_resize(&mut w, i))
        };
        if !local {
            scene::layout_window(&mut rt, &mut fonts, root, [400.0, 600.0]);
            rt.settle();
            break;
        }
        scene::relayout_targets(&mut rt.world.borrow_mut(), &mut fonts, &rs, pass);
        rt.settle();
        rs = std::mem::take(&mut rt.world.borrow_mut().resized);
    }
    let (h2, b2) = (num(&mut rt, field, "height"), num(&mut rt, box_, "height"));
    let line = h1 - 20.0;
    assert!(
        (h2 - (h1 + line)).abs() < 1.0,
        "field {h1} -> {h2}, box {b1} -> {b2}"
    );
    assert!(
        (b2 - (b1 + line)).abs() < 1.0,
        "field {h1} -> {h2}, box {b1} -> {b2}"
    );
}
