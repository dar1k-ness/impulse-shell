//! The original's NavigationRailTabArray takes the widest `collapsedWidth` of its column's children, and
//! each NavigationRailButton a Repeater makes there binds `collapsedWidth` to its label's
//! `implicitWidth`. QQuickRepeater::initItem parents a delegate before its bindings are enabled,
//! so the rail's binding runs on childrenChanged and reads the new button's pending binding:
//! QQmlData::flushPendingBinding enables that binding there and then, and the label's bindings
//! enabled later update it again. Qt 6.12 reports no binding loop and gives these widths for the
//! same files under qmltestrunner in the testenv image, with an Item whose `implicitWidth` is
//! ten times the name's length in place of the Text.

use qml::runtime::Runtime;
use qml::value::Value;

const RAIL: &str = r#"import QtQuick
Item {
    id: root
    readonly property real collapsedWidth: {
        let width = 0;
        for (let i = 0; i < column.children.length; i++)
            width = Math.max(width, column.children[i].collapsedWidth ?? 0);
        return width;
    }
    default property alias tabData: column.data
    Column { id: column }
}"#;

const TAB: &str = r#"import QtQuick
Item {
    id: tab
    property string name
    property real base: 56
    readonly property real collapsedWidth: Math.max(tab.base, label.implicitWidth + 8)
    implicitHeight: base
    Item { id: icon; implicitHeight: 32 }
    Text {
        id: label
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: icon.bottom
        text: tab.name
        font.pixelSize: 14
    }
}"#;

const MAIN: &str = r#"import QtQuick
Item {
    id: root
    property var tabs: [{ name: "A much longer label" }, { name: "bb" }, { name: "a" }]
    property alias collapsedWidth: rail.collapsedWidth
    Rail {
        id: rail
        Repeater {
            model: root.tabs
            Tab {
                required property var modelData
                name: modelData.name
            }
        }
    }
    function step(n) {
        root.tabs = n == 1 ? [{ name: "Another long label here" }, { name: "c" }] : [{ name: "c" }];
    }
}"#;

/// Settles, measuring the texts left dirty the way a frame's layout does.
fn settle(rt: &mut Runtime) {
    loop {
        rt.settle();
        let dirty: Vec<usize> = rt.world.borrow().text_dirty.iter().copied().collect();
        if dirty.is_empty() {
            return;
        }
        for i in dirty {
            rt.world.borrow_mut().read(i, "implicitWidth");
        }
    }
}

#[test]
fn a_rail_takes_the_widest_repeated_tab_without_a_binding_loop() {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in [("Rail.qml", RAIL), ("Tab.qml", TAB), ("main.qml", MAIN)] {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    // The shell's measure reads a Text's anchors next to its text: ten pixels a character here.
    rt.world.borrow_mut().text_measure = Some(Box::new(|w, i| {
        let chars = match w.read(i, "text") {
            Some(Value::Str(s)) => s.chars().count(),
            _ => 0,
        };
        w.read(i, "anchors.horizontalCenter");
        w.read(i, "anchors.top");
        w.write(i, "implicitWidth", Value::Num(chars as f64 * 10.0));
    }));
    let root = rt.load_file(&dir.path().join("main.qml")).unwrap();
    settle(&mut rt);
    let mut got = vec![rt.get(root, "collapsedWidth").to_number()];
    for n in [1.0, 2.0] {
        let _ = rt.call_method(root, "step", vec![Value::Num(n)]).unwrap();
        settle(&mut rt);
        got.push(rt.get(root, "collapsedWidth").to_number());
    }
    assert_eq!(got, [198.0, 238.0, 56.0]);
    assert_eq!(rt.world.borrow().errors, Vec::<String>::new());
}
