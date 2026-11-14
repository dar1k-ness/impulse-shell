//! What a layout arranges its children by, bound to a size the layout pass itself sets (the original's
//! Quick settings page: `GridLayout { columns: a.implicitWidth + columnSpacing +
//! b.implicitWidth <= parent.width ? 2 : 1 }`), takes effect in the frame that changed the size.
//! Qt's setters invalidate the layout (QQuickGridLayout::setColumns) and it is polished again
//! before rendering. Ours ended the pass with the old arrangement and the shell kept it: the
//! page showed the arrangement of the previous window width.
//! Expected values are Qt 6.12's for the same QML (qmltestrunner, offscreen, in ri-testenv),
//! read after `waitForRendering` of each width.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r#"
import QtQuick
import QtQuick.Layouts
Item {
    id: root_
    width: 400
    height: 300
    property var o_direct: direct
    property var o_b: b
    property var o_nested: nested
    property var o_d: d
    property var o_row: row
    property var o_f: f
    // The card takes the window's width through its anchors.
    Item {
        anchors.left: parent.left
        anchors.right: parent.right
        height: 100
        GridLayout {
            id: direct
            columns: a.implicitWidth + columnSpacing + b.implicitWidth <= parent.width ? 2 : 1
            columnSpacing: 10
            rowSpacing: 6
            Rectangle { id: a; implicitWidth: 120; implicitHeight: 40 }
            Rectangle { id: b; implicitWidth: 150; implicitHeight: 40 }
        }
    }
    // The card's width is a binding on an item a layout places, and a layout inside the card
    // hands it on to the grid's parent.
    RowLayout {
        anchors.left: parent.left
        anchors.right: parent.right
        Item { id: filler; Layout.fillWidth: true; implicitHeight: 1 }
    }
    Item {
        y: 120
        width: filler.width
        height: 100
        ColumnLayout {
            anchors.left: parent.left
            anchors.right: parent.right
            Item {
                Layout.fillWidth: true
                implicitHeight: 90
                GridLayout {
                    id: nested
                    columns: c.implicitWidth + columnSpacing + d.implicitWidth <= parent.width ? 2 : 1
                    columnSpacing: 10
                    rowSpacing: 6
                    Rectangle { id: c; implicitWidth: 120; implicitHeight: 40 }
                    Rectangle { id: d; implicitWidth: 150; implicitHeight: 40 }
                }
            }
        }
    }
    // A RowLayout's spacing, bound the same way.
    Item {
        y: 220
        anchors.left: parent.left
        anchors.right: parent.right
        height: 100
        RowLayout {
            id: row
            y: 60
            spacing: parent.width >= 280 ? 10 : 30
            Rectangle { implicitWidth: 120; implicitHeight: 20 }
            Rectangle { id: f; implicitWidth: 150; implicitHeight: 20 }
        }
    }
}
"#;

fn num(rt: &mut Runtime, root: usize, item: &str, prop: &str) -> f64 {
    let mut w = rt.world.borrow_mut();
    let Some(Value::Item(i)) = w.read(root, &format!("o_{item}")) else {
        panic!("o_{item} is not an item");
    };
    w.read(i, prop).map_or(f64::NAN, |v| v.to_number())
}

/// Per window width, Qt's (bound property, container width, second child x, second child y)
/// for both GridLayouts (`columns`) and for the RowLayout (`spacing`).
type Row = (f64, [f64; 4], [f64; 4]);
const QT: [Row; 6] = [
    (400.0, [2.0, 280.0, 130.0, 0.0], [10.0, 280.0, 130.0, 0.0]),
    (250.0, [1.0, 150.0, 0.0, 46.0], [30.0, 300.0, 150.0, 0.0]),
    (290.0, [2.0, 280.0, 130.0, 0.0], [10.0, 280.0, 130.0, 0.0]),
    (280.0, [2.0, 280.0, 130.0, 0.0], [10.0, 280.0, 130.0, 0.0]),
    (279.0, [1.0, 150.0, 0.0, 46.0], [30.0, 300.0, 150.0, 0.0]),
    (400.0, [2.0, 280.0, 130.0, 0.0], [10.0, 280.0, 130.0, 0.0]),
];

/// One layout per window width, as the shell's frame after a resize.
#[test]
fn arrangement_bound_to_a_placed_width_applies_in_the_same_layout() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut wrong = vec![];
    for (width, grid, row) in QT {
        scene::layout_window(&mut rt, &mut fonts, root, [width, 300.0]);
        rt.settle();
        assert!(
            !scene::take_unsettled(),
            "{width} px: the layout did not settle"
        );
        for (name, child, prop, qt) in [
            ("direct", "b", "columns", grid),
            ("nested", "d", "columns", grid),
            ("row", "f", "spacing", row),
        ] {
            let got = [
                num(&mut rt, root, name, prop),
                num(&mut rt, root, name, "width"),
                num(&mut rt, root, child, "x"),
                num(&mut rt, root, child, "y"),
            ];
            if got != qt {
                wrong.push(format!(
                    "{width} px, {name} ({prop}, width, second child x, y): {got:?}, Qt 6.12 has {qt:?}"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
