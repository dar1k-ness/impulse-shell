//! Settings MaterialTextArea: a press on a TextArea inside a Loader'd Flickable takes active focus
//! (QQuickTextEdit activeFocusOnPress) and then receives typed keys.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, keys::Keyboard, scene};

const BTN_LEFT: u32 = 0x110;

const QML: &str = r#"
import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
import QtQuick.Controls.Material
Item {
    id: root
    width: 600; height: 400
    property var area: null
    property string saved: ""
    ColumnLayout {
        anchors.fill: parent
        Keys.onPressed: (event) => {
            if (event.modifiers === Qt.ControlModifier) event.accepted = true;
        }
        Button { focus: true; text: "rail" }
        Loader {
            Layout.fillWidth: true
            Layout.fillHeight: true
            sourceComponent: Flickable {
                clip: true
                contentHeight: col.implicitHeight + 100
                ColumnLayout {
                    id: col
                    width: 500
                    anchors.top: parent.top
                    anchors.horizontalCenter: parent.horizontalCenter
                    TextArea {
                        id: ta
                        Layout.fillWidth: true
                        Material.containerStyle: Material.Filled
                        placeholderText: "Crosshair code"
                        text: "abc"
                        wrapMode: TextEdit.Wrap
                        background: Rectangle { implicitHeight: 56 }
                        onTextChanged: root.saved = text
                        Component.onCompleted: root.area = ta
                    }
                }
            }
        }
    }
}
"#;

#[test]
fn text_area_in_flickable_takes_focus_and_keys() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [600.0, 400.0]);
    rt.settle();
    let Some(Value::Item(area)) = rt.world.borrow_mut().read(root, "area") else {
        panic!("no area");
    };
    let (ax, ay) = rt.world.borrow_mut().scene_pos(area, Some(root));
    let mut ptr = Pointer::default();
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    kb.sync(&mut rt);
    let (px, py) = (ax + 20.0, ay + 20.0);
    ptr.enter(&mut rt, root, px, py);
    ptr.motion(&mut rt, root, px, py);
    ptr.press(&mut rt, BTN_LEFT);
    ptr.release(&mut rt, BTN_LEFT);
    rt.settle();
    kb.sync(&mut rt);
    assert_eq!(kb.active_editor(&rt), Some(area));
    kb.key(&mut rt, 0xff57, None, true, false);
    kb.key(&mut rt, 0xff57, None, false, false);
    kb.key(&mut rt, 0x5a, Some("Z"), true, false);
    kb.key(&mut rt, 0x5a, None, false, false);
    rt.settle();
    let saved = rt
        .world
        .borrow_mut()
        .read(root, "saved")
        .map(|v| v.to_js_string());
    assert_eq!(saved.as_deref(), Some("abcZ"));
}

const WRAPPED: &str = r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 200; height: 200
    property var area: ta
    TextArea {
        id: ta
        width: 120
        wrapMode: TextEdit.Wrap
        text: "aaaa bbbb cccc dddd eeee ffff gggg"
    }
}
"#;

fn text(rt: &mut Runtime, i: usize) -> String {
    rt.world
        .borrow_mut()
        .read(i, "text")
        .map(|v| v.to_js_string())
        .unwrap_or_default()
}

/// QQuickTextEdit: a press puts the caret at the pressed glyph boundary, End moves it to the end
/// of that wrapped (visual) line, before the space the soft break consumed; Home to its start.
#[test]
fn text_area_press_places_caret_and_end_stays_on_visual_line() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(WRAPPED).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [200.0, 200.0]);
    rt.settle();
    let Some(Value::Item(area)) = rt.world.borrow_mut().read(root, "area") else {
        panic!("no area");
    };
    let mut ptr = Pointer::default();
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    let (lx, ly) = {
        let mut w = rt.world.borrow_mut();
        let p = scene::edit_position_at(&mut w, &mut fonts, area, 0.0, 1e9).unwrap();
        let (a, b) = scene::edit_visual_line(&mut w, &mut fonts, area, p).unwrap();
        assert!(a > 0 && b == text_len(&w, area), "last line {a}..{b}");
        let (a1, b1) = scene::edit_visual_line(&mut w, &mut fonts, area, 0).unwrap();
        assert!(a1 == 0 && b1 < a, "first line {a1}..{b1}");
        (2.0, 2.0)
    };
    ptr.enter(&mut rt, root, lx, ly);
    ptr.motion(&mut rt, root, lx, ly);
    ptr.press(&mut rt, BTN_LEFT);
    ptr.release(&mut rt, BTN_LEFT);
    rt.settle();
    kb.sync(&mut rt);
    assert_eq!(kb.active_editor(&rt), Some(area));
    let pos = rt
        .world
        .borrow_mut()
        .read(area, "cursorPosition")
        .map(|v| v.to_number());
    assert_eq!(pos, Some(0.0));
    let first_end = {
        let mut w = rt.world.borrow_mut();
        scene::edit_visual_line(&mut w, &mut fonts, area, 0)
            .unwrap()
            .1
    };
    let orig = text(&mut rt, area);
    assert_eq!(orig.chars().nth(first_end), Some(' '));
    kb.key(&mut rt, 0xff57, None, true, false);
    kb.key(&mut rt, 0x5a, Some("Z"), true, false);
    rt.settle();
    let mut want: Vec<char> = orig.chars().collect();
    want.insert(first_end, 'Z');
    assert_eq!(text(&mut rt, area), want.iter().collect::<String>());
    kb.key(&mut rt, 0xff50, None, true, false);
    kb.key(&mut rt, 0x59, Some("Y"), true, false);
    rt.settle();
    assert!(text(&mut rt, area).starts_with("Yaaaa"));
    // Inside the area, below the last line and right of its end: the caret goes to the end.
    let h = rt
        .world
        .borrow_mut()
        .read(area, "height")
        .map_or(0.0, |v| v.to_number());
    ptr.motion(&mut rt, root, 119.0, h - 1.0);
    ptr.press(&mut rt, BTN_LEFT);
    ptr.release(&mut rt, BTN_LEFT);
    rt.settle();
    let n = text_len(&rt.world.borrow(), area) as f64;
    let pos = rt
        .world
        .borrow_mut()
        .read(area, "cursorPosition")
        .map(|v| v.to_number());
    assert_eq!(pos, Some(n));
}

fn text_len(w: &qml::runtime::World, i: usize) -> usize {
    match w.items[i].props.get("text").map(|p| &p.value) {
        Some(Value::Str(s)) => s.chars().count(),
        _ => 0,
    }
}
