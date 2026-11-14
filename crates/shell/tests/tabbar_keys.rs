//! Right sidebar Timer tab (PomodoroWidget): SecondaryTabButtons whose MouseArea consumes the press
//! and calls `click()`, inside a TabBar whose currentIndex PageDown moves.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, keys::Keyboard, scene};

const BTN_LEFT: u32 = 0x110;
const PAGE_DOWN: u32 = 0xff56;

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
            if (event.key === Qt.Key_PageDown) {
                bar.incrementCurrentIndex();
                event.accepted = true;
            } else if (event.key === Qt.Key_Space) {
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

/// QQuickItemView::updateCurrent (TabBar's ListView contentItem): the new current tab takes focus
/// in the view's scope, so it holds active focus after PageDown.
#[test]
fn tab_bar_current_tab_takes_active_focus() {
    let (mut rt, root, mut kb) = setup();
    kb.key(&mut rt, PAGE_DOWN, None, true, false);
    kb.key(&mut rt, PAGE_DOWN, None, false, false);
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [400.0, 300.0]);
    rt.settle();
    kb.sync(&mut rt);
    let (t0, t1) = (item(&mut rt, root, "tab0"), item(&mut rt, root, "tab1"));
    assert_eq!(prop(&mut rt, t0, "activeFocus"), "false");
    assert_eq!(prop(&mut rt, t1, "activeFocus"), "true");
}
