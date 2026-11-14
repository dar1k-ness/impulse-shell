//! A positioner (`Grid`, `Row`, `Column`, `Flow`) whose arrangement is written after it was placed
//! (`columns`, `rows`, `spacing`) has its children in their new places when the frame is
//! drawn, like Qt: QQuickGrid::setColumns positions at once, QQuickBasePositioner::setSpacing
//! on polish. A Grid places both coordinates of an item, so one turned from a column into a
//! row brings its items back up. The original's `Box` is such a `Grid`, turned by `vertical`. Qt 6.12's
//! numbers are from this scene under qmltestrunner in the testenv image, read after
//! `waitForRendering`.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r#"
import QtQuick
Item {
    id: root_
    width: 400
    height: 300
    property var o_direct: direct
    property var o_b: b
    property var o_box: box
    property var o_d: d
    property var o_inrow: inrow
    property var o_f: f
    property var o_after: after
    property var o_row: row
    property var o_h: h
    property var o_col: col
    property var o_k: k
    property var o_rtl: rtl
    property var o_m: m
    property var o_n: n
    property var o_flw: flw
    property var o_q: q
    // `columns` bound to a width the window's layout sets.
    Item {
        anchors.left: parent.left
        anchors.right: parent.right
        height: 60
        Grid {
            id: direct
            columns: a.width + columnSpacing + b.width <= parent.width ? 2 : 1
            columnSpacing: 10
            rowSpacing: 6
            Rectangle { id: a; width: 120; height: 40 }
            Rectangle { id: b; width: 150; height: 40 }
        }
    }
    // `columns` and `rows` both, as the original's Box (modules/common/widgets/Box.qml) turns by `vertical`.
    Item {
        y: 100
        anchors.left: parent.left
        anchors.right: parent.right
        height: 60
        Grid {
            id: box
            property bool vertical: parent.width < 280
            columns: vertical ? 1 : -1
            rows: vertical ? -1 : 1
            rowSpacing: 4
            columnSpacing: rowSpacing
            Rectangle { width: 120; height: 40 }
            Rectangle { id: d; width: 150; height: 40 }
        }
    }
    // The same inside a Row: what stands after it follows its new size.
    Item {
        y: 200
        anchors.left: parent.left
        anchors.right: parent.right
        height: 60
        Row {
            Grid {
                id: inrow
                property bool vertical: parent.parent.width < 280
                columns: vertical ? 1 : -1
                rows: vertical ? -1 : 1
                Rectangle { width: 60; height: 20 }
                Rectangle { id: f; width: 70; height: 20 }
            }
            Rectangle { id: after; width: 10; height: 10 }
        }
    }
    // A Row's and a Column's spacing, bound the same way.
    Item {
        y: 260
        anchors.left: parent.left
        anchors.right: parent.right
        height: 40
        Row {
            id: row
            spacing: parent.width >= 280 ? 10 : 30
            Rectangle { width: 120; height: 20 }
            Rectangle { id: h; width: 150; height: 20 }
        }
        Column {
            id: col
            spacing: parent.width >= 280 ? 1 : 7
            Rectangle { width: 20; height: 5 }
            Rectangle { id: k; width: 20; height: 5 }
        }
    }
    // Right to left: the row runs from the right, the column's items stand at its right.
    Item {
        y: 160
        anchors.left: parent.left
        anchors.right: parent.right
        height: 40
        Grid {
            id: rtl
            property bool vertical: parent.width < 280
            columns: vertical ? 1 : -1
            rows: vertical ? -1 : 1
            layoutDirection: Qt.RightToLeft
            Rectangle { id: m; width: 120; height: 10 }
            Rectangle { id: n; width: 150; height: 10 }
        }
    }
    // A Flow's spacing: the second item goes to the next line when it no longer fits.
    Item {
        y: 230
        anchors.left: parent.left
        anchors.right: parent.right
        height: 20
        Flow {
            id: flw
            width: parent.width
            spacing: parent.width >= 280 ? 10 : 30
            Rectangle { width: 120; height: 20 }
            Rectangle { id: q; width: 150; height: 20 }
        }
    }
}
"#;

/// What is compared: (item, property).
const SHOWN: [(&str, &str); 29] = [
    ("direct", "columns"),
    ("direct", "width"),
    ("b", "x"),
    ("b", "y"),
    ("box", "columns"),
    ("box", "rows"),
    ("box", "width"),
    ("box", "height"),
    ("d", "x"),
    ("d", "y"),
    ("inrow", "width"),
    ("f", "x"),
    ("f", "y"),
    ("after", "x"),
    ("row", "spacing"),
    ("row", "width"),
    ("h", "x"),
    ("col", "spacing"),
    ("col", "height"),
    ("k", "y"),
    ("rtl", "width"),
    ("m", "x"),
    ("m", "y"),
    ("n", "x"),
    ("n", "y"),
    ("flw", "spacing"),
    ("flw", "height"),
    ("q", "x"),
    ("q", "y"),
];

/// Qt's `SHOWN` per window width, in the order the window takes them.
#[rustfmt::skip]
const WIDTHS: [(f64, [f64; 29]); 6] = [
    (400.0, [
        2.0, 280.0, 130.0, 0.0, -1.0, 1.0, 274.0, 40.0, 124.0, 0.0,
        130.0, 60.0, 0.0, 130.0, 10.0, 280.0, 130.0, 1.0, 11.0, 6.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 10.0, 20.0, 130.0, 0.0,
    ]),
    (250.0, [
        1.0, 150.0, 0.0, 46.0, 1.0, -1.0, 150.0, 84.0, 0.0, 44.0,
        70.0, 0.0, 20.0, 70.0, 30.0, 300.0, 150.0, 7.0, 17.0, 12.0,
        150.0, 30.0, 0.0, 0.0, 10.0, 30.0, 70.0, 0.0, 50.0,
    ]),
    (290.0, [
        2.0, 280.0, 130.0, 0.0, -1.0, 1.0, 274.0, 40.0, 124.0, 0.0,
        130.0, 60.0, 0.0, 130.0, 10.0, 280.0, 130.0, 1.0, 11.0, 6.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 10.0, 20.0, 130.0, 0.0,
    ]),
    (280.0, [
        2.0, 280.0, 130.0, 0.0, -1.0, 1.0, 274.0, 40.0, 124.0, 0.0,
        130.0, 60.0, 0.0, 130.0, 10.0, 280.0, 130.0, 1.0, 11.0, 6.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 10.0, 20.0, 130.0, 0.0,
    ]),
    (279.0, [
        1.0, 150.0, 0.0, 46.0, 1.0, -1.0, 150.0, 84.0, 0.0, 44.0,
        70.0, 0.0, 20.0, 70.0, 30.0, 300.0, 150.0, 7.0, 17.0, 12.0,
        150.0, 30.0, 0.0, 0.0, 10.0, 30.0, 70.0, 0.0, 50.0,
    ]),
    (400.0, [
        2.0, 280.0, 130.0, 0.0, -1.0, 1.0, 274.0, 40.0, 124.0, 0.0,
        130.0, 60.0, 0.0, 130.0, 10.0, 280.0, 130.0, 1.0, 11.0, 6.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 10.0, 20.0, 130.0, 0.0,
    ]),
];

/// Qt's `SHOWN` after each write (item, property, value) to the 400 px wide window, in order.
#[rustfmt::skip]
const WRITES: [(&str, &str, Value, [f64; 29]); 11] = [
    ("direct", "columns", Value::Num(1.0), [
        1.0, 150.0, 0.0, 46.0, -1.0, 1.0, 274.0, 40.0, 124.0, 0.0,
        130.0, 60.0, 0.0, 130.0, 10.0, 280.0, 130.0, 1.0, 11.0, 6.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 10.0, 20.0, 130.0, 0.0,
    ]),
    ("direct", "columns", Value::Num(2.0), [
        2.0, 280.0, 130.0, 0.0, -1.0, 1.0, 274.0, 40.0, 124.0, 0.0,
        130.0, 60.0, 0.0, 130.0, 10.0, 280.0, 130.0, 1.0, 11.0, 6.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 10.0, 20.0, 130.0, 0.0,
    ]),
    ("box", "vertical", Value::Bool(true), [
        2.0, 280.0, 130.0, 0.0, 1.0, -1.0, 150.0, 84.0, 0.0, 44.0,
        130.0, 60.0, 0.0, 130.0, 10.0, 280.0, 130.0, 1.0, 11.0, 6.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 10.0, 20.0, 130.0, 0.0,
    ]),
    ("inrow", "vertical", Value::Bool(true), [
        2.0, 280.0, 130.0, 0.0, 1.0, -1.0, 150.0, 84.0, 0.0, 44.0,
        70.0, 0.0, 20.0, 70.0, 10.0, 280.0, 130.0, 1.0, 11.0, 6.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 10.0, 20.0, 130.0, 0.0,
    ]),
    ("row", "spacing", Value::Num(30.0), [
        2.0, 280.0, 130.0, 0.0, 1.0, -1.0, 150.0, 84.0, 0.0, 44.0,
        70.0, 0.0, 20.0, 70.0, 30.0, 300.0, 150.0, 1.0, 11.0, 6.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 10.0, 20.0, 130.0, 0.0,
    ]),
    ("col", "spacing", Value::Num(7.0), [
        2.0, 280.0, 130.0, 0.0, 1.0, -1.0, 150.0, 84.0, 0.0, 44.0,
        70.0, 0.0, 20.0, 70.0, 30.0, 300.0, 150.0, 7.0, 17.0, 12.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 10.0, 20.0, 130.0, 0.0,
    ]),
    ("flw", "spacing", Value::Num(30.0), [
        2.0, 280.0, 130.0, 0.0, 1.0, -1.0, 150.0, 84.0, 0.0, 44.0,
        70.0, 0.0, 20.0, 70.0, 30.0, 300.0, 150.0, 7.0, 17.0, 12.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 30.0, 20.0, 150.0, 0.0,
    ]),
    ("rtl", "vertical", Value::Bool(true), [
        2.0, 280.0, 130.0, 0.0, 1.0, -1.0, 150.0, 84.0, 0.0, 44.0,
        70.0, 0.0, 20.0, 70.0, 30.0, 300.0, 150.0, 7.0, 17.0, 12.0,
        150.0, 30.0, 0.0, 0.0, 10.0, 30.0, 20.0, 150.0, 0.0,
    ]),
    ("rtl", "vertical", Value::Bool(false), [
        2.0, 280.0, 130.0, 0.0, 1.0, -1.0, 150.0, 84.0, 0.0, 44.0,
        70.0, 0.0, 20.0, 70.0, 30.0, 300.0, 150.0, 7.0, 17.0, 12.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 30.0, 20.0, 150.0, 0.0,
    ]),
    ("box", "vertical", Value::Bool(false), [
        2.0, 280.0, 130.0, 0.0, -1.0, 1.0, 274.0, 40.0, 124.0, 0.0,
        70.0, 0.0, 20.0, 70.0, 30.0, 300.0, 150.0, 7.0, 17.0, 12.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 30.0, 20.0, 150.0, 0.0,
    ]),
    ("inrow", "vertical", Value::Bool(false), [
        2.0, 280.0, 130.0, 0.0, -1.0, 1.0, 274.0, 40.0, 124.0, 0.0,
        130.0, 60.0, 0.0, 130.0, 30.0, 300.0, 150.0, 7.0, 17.0, 12.0,
        270.0, 150.0, 0.0, 0.0, 0.0, 30.0, 20.0, 150.0, 0.0,
    ]),
];

fn item(rt: &mut Runtime, root: usize, name: &str) -> usize {
    match rt.world.borrow_mut().read(root, &format!("o_{name}")) {
        Some(Value::Item(i)) => i,
        _ => panic!("o_{name} is not an item"),
    }
}

fn differences(rt: &mut Runtime, root: usize, when: &str, qt: &[f64; 29]) -> Vec<String> {
    let mut wrong = vec![];
    for ((name, prop), qt) in SHOWN.iter().zip(qt) {
        let i = item(rt, root, name);
        let got = rt
            .world
            .borrow_mut()
            .read(i, prop)
            .map_or(f64::NAN, |v| v.to_number());
        if got != *qt {
            wrong.push(format!("{when}: {name}.{prop} is {got}, Qt 6.12 has {qt}"));
        }
    }
    wrong
}

/// One layout per window width, as the shell's frame after a resize.
#[test]
fn arrangement_bound_to_a_placed_width_applies_in_the_same_layout() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut wrong = vec![];
    for (width, qt) in &WIDTHS {
        scene::layout_window(&mut rt, &mut fonts, root, [*width, 300.0]);
        rt.settle();
        assert!(
            !scene::take_unsettled(),
            "{width} px: the layout did not settle"
        );
        wrong.extend(differences(&mut rt, root, &format!("{width} px"), qt));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The shell's frame: a full layout when something outside the queued items changed, else
/// the queued items alone.
fn frame(rt: &mut Runtime, fonts: &mut scene::Fonts, root: usize, laid: &mut u64) {
    for _ in 0..4 {
        let rs = std::mem::take(&mut rt.world.borrow_mut().resized);
        let mut w = rt.world.borrow_mut();
        if w.layout_rev != *laid || !rs.iter().all(|&i| scene::is_local_resize(&mut w, i)) {
            drop(w);
            scene::layout_window(rt, fonts, root, [400.0, 300.0]);
            *laid = rt.world.borrow().layout_rev;
            rt.world.borrow_mut().resized.clear();
            return;
        }
        if rs.is_empty() {
            return;
        }
        let rev = w.layout_rev;
        scene::relayout_targets(&mut w, fonts, &rs, 0);
        w.layout_rev = rev;
        drop(w);
        rt.settle();
    }
}

/// Writes from outside a layout (a handler, a config change), each drawn by the next frame.
#[test]
fn arrangement_written_after_the_first_layout_is_placed_by_the_next_frame() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [400.0, 300.0]);
    rt.settle();
    let mut laid = rt.world.borrow().layout_rev;
    rt.world.borrow_mut().resized.clear();
    let mut wrong = differences(&mut rt, root, "first layout", &WIDTHS[0].1);
    for (name, prop, value, qt) in &WRITES {
        let i = item(&mut rt, root, name);
        rt.world.borrow_mut().write(i, prop, value.clone());
        rt.settle();
        frame(&mut rt, &mut fonts, root, &mut laid);
        wrong.extend(differences(
            &mut rt,
            root,
            &format!("{name}.{prop} = {value:?}"),
            qt,
        ));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
