//! The original's quick toggles in edit mode: each row is a `ButtonGroup` (a `RowLayout`) filled by a
//! Repeater, and a `GroupButton` fills the width of its group when
//! `parent.children.indexOf(button)` is 0. Qt enables a delegate's bindings after
//! QQuickRepeater::initItem stacked it before the Repeater, so the only button of a row reads 0
//! and takes the group's whole width, which counts the Repeater as one more spacing. Qt 6.12's
//! numbers are from these files under qmltestrunner in the testenv image, read after
//! `waitForRendering`.

use qml::runtime::Runtime;
use shell::scene;

const GROUP: &str = r#"import QtQuick
import QtQuick.Layouts
Rectangle {
    id: group
    default property alias groupData: rowLayout.data
    property alias childrenCount: rowLayout.childrenCount
    property alias clickIndex: rowLayout.clickIndex
    property real contentWidth: {
        let total = 0;
        for (let i = 0; i < rowLayout.children.length; ++i) {
            const child = rowLayout.children[i];
            if (!child.visible) continue;
            total += child.baseWidth ?? child.implicitWidth ?? child.width;
        }
        return total + rowLayout.spacing * (rowLayout.children.length - 1);
    }
    color: "transparent"
    width: contentWidth
    implicitWidth: contentWidth
    implicitHeight: rowLayout.implicitHeight
    children: [RowLayout {
        id: rowLayout
        anchors.fill: parent
        spacing: 6
        property int clickIndex: -1
        property int childrenCount: children.length
    }]
}"#;

const ROWS: &str = r#"import QtQuick
import QtQuick.Layouts
Column {
    id: rows
    property var counts: [3, 1]
    spacing: 6
    Repeater {
        model: rows.counts
        delegate: Group {
            id: group
            required property int modelData
            Repeater {
                model: group.modelData
                delegate: Rectangle {
                    id: button
                    objectName: "button"
                    property real baseWidth: 50
                    property var parentGroup: button.parent
                    property int indexInParent: parentGroup?.children.indexOf(button) ?? -1
                    property int clickIndex: parentGroup?.clickIndex ?? -1
                    property bool isAtSide: indexInParent === 0 || indexInParent === (parentGroup?.childrenCount - 1)
                    Layout.fillWidth: (clickIndex - 1 <= indexInParent && indexInParent <= clickIndex + 1)
                    implicitWidth: baseWidth
                    implicitHeight: 20
                }
            }
        }
    }
}"#;

const MAIN: &str = r#"import QtQuick
Item {
    width: 600
    height: 400
    Rows {}
}"#;

#[test]
fn the_only_button_of_a_group_fills_it() {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in [("Group.qml", GROUP), ("Rows.qml", ROWS), ("main.qml", MAIN)] {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&dir.path().join("main.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [600.0, 400.0]);
    assert!(!scene::take_unsettled(), "the layout did not settle");
    let mut w = rt.world.borrow_mut();
    let mut got = Vec::new();
    for i in 0..w.items.len() {
        if w.dead.contains(&i)
            || w.read(i, "objectName").unwrap_or_default().to_js_string() != "button"
        {
            continue;
        }
        let mut num = |p: &str| w.read(i, p).unwrap_or_default().to_number();
        got.push((
            num("indexInParent"),
            num("Layout.fillWidth") != 0.0,
            num("x"),
            num("width"),
        ));
    }
    // (index among the row's children, fills the width, x, width): a row of three, a row of one.
    let qt = [
        (0.0, true, 0.0, 56.0),
        (1.0, false, 62.0, 50.0),
        (2.0, false, 118.0, 50.0),
        (0.0, true, 0.0, 56.0),
    ];
    assert_eq!(got, qt);
    assert_eq!(w.errors, Vec::<String>::new());
}
