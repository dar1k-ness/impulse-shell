//! A press on a TextField takes active focus and is accepted by it
//! (QQuickTextInput::mousePressEvent), so a MouseArea filling the Button below never sees it.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, keys::Keyboard, scene};

const BTN_LEFT: u32 = 0x110;

const QML: &str = r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    property int presses: 0
    property int clicks: 0
    property var field: f
    Button {
        anchors.fill: parent
        MouseArea {
            anchors.fill: parent
            onPressed: root.presses++
            onClicked: root.clicks++
        }
        contentItem: Item {
            TextField {
                id: f; x: 10; y: 10; width: 200; height: 40
                // MaterialTextField's cursor-shape MouseArea, above the field.
                MouseArea { anchors.fill: parent; acceptedButtons: Qt.NoButton; hoverEnabled: true }
            }
        }
    }
}
"#;

fn num(rt: &mut Runtime, i: usize, p: &str) -> f64 {
    rt.world
        .borrow_mut()
        .read(i, p)
        .map_or(-1.0, |v| v.to_number())
}

#[test]
fn text_field_press_focuses_and_stops_delivery() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [300.0, 200.0]);
    let Some(Value::Item(field)) = rt.world.borrow_mut().read(root, "field") else {
        panic!("no field");
    };
    let mut ptr = Pointer::default();
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    ptr.enter(&mut rt, root, 50.0, 30.0);
    ptr.motion(&mut rt, root, 50.0, 30.0);
    ptr.press(&mut rt, BTN_LEFT);
    ptr.release(&mut rt, BTN_LEFT);
    rt.settle();
    kb.sync(&mut rt);
    assert_eq!(num(&mut rt, root, "presses"), 0.0);
    assert_eq!(num(&mut rt, root, "clicks"), 0.0);
    assert_eq!(kb.active_editor(&rt), Some(field));
    kb.key(&mut rt, 0x61, Some("a"), true, false);
    kb.key(&mut rt, 0x61, None, false, false);
    rt.settle();
    let text = rt
        .world
        .borrow_mut()
        .read(field, "text")
        .map(|v| v.to_js_string());
    assert_eq!(text.as_deref(), Some("a"));
}
