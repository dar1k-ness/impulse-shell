//! Right sidebar Timer tab (PomodoroWidget): SecondaryTabButtons whose MouseArea consumes the press
//! and calls `click()`, under a parent whose Keys handler toggles the timer on Space.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, keys::Keyboard, scene};

const BTN_LEFT: u32 = 0x110;
const SPACE: u32 = 0x20;

const QML: &str = r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 400; height: 300
    property string log: ""
    property var tab0: t0
    property var tab1: t1
    Item {
        anchors.fill: parent
        Keys.onPressed: (event) => {
            if (event.key === Qt.Key_Space) {
                root.log += "toggle;";
                event.accepted = true;
            }
        }
        TabBar {
            id: bar
            width: 400
            TabButton {
                id: t0
                text: "Pomodoro"
                onClicked: root.log += "click0;"
                MouseArea { anchors.fill: parent; onPressed: t0.click() }
            }
            TabButton {
                id: t1
                text: "Stopwatch"
                onClicked: root.log += "click1;"
                MouseArea { anchors.fill: parent; onPressed: t1.click() }
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

fn prop(rt: &mut Runtime, i: usize, name: &str) -> String {
    rt.world
        .borrow_mut()
        .read(i, name)
        .map(|v| v.to_js_string())
        .unwrap_or_default()
}

fn setup() -> (Runtime, usize, Keyboard) {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [400.0, 300.0]);
    rt.settle();
    let t0 = item(&mut rt, root, "tab0");
    let (x, y) = rt.world.borrow_mut().scene_pos(t0, Some(root));
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    kb.sync(&mut rt);
    let mut ptr = Pointer::default();
    ptr.enter(&mut rt, root, x + 10.0, y + 10.0);
    ptr.motion(&mut rt, root, x + 10.0, y + 10.0);
    ptr.press(&mut rt, BTN_LEFT);
    ptr.release(&mut rt, BTN_LEFT);
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts, root, [400.0, 300.0]);
    rt.settle();
    kb.sync(&mut rt);
    assert_eq!(prop(&mut rt, t0, "activeFocus"), "true");
    (rt, root, kb)
}

/// QQuickAbstractButton::keyPressEvent / keyReleaseEvent: Space on the focused button presses it
/// and clicks on release, accepting both, so the parent's Keys handler never sees it.
#[test]
fn space_clicks_focused_button() {
    let (mut rt, root, mut kb) = setup();
    rt.world.borrow_mut().write(root, "log", Value::str(""));
    kb.key(&mut rt, SPACE, Some(" "), true, false);
    let t0 = item(&mut rt, root, "tab0");
    assert_eq!(prop(&mut rt, t0, "pressed"), "true");
    kb.key(&mut rt, SPACE, None, false, false);
    rt.settle();
    assert_eq!(prop(&mut rt, t0, "pressed"), "false");
    assert_eq!(prop(&mut rt, root, "log"), "click0;");
}
