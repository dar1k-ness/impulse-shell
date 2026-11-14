//! AndroidQuickToggleButton: a Column of `anchors.left/right` Texts in a Loader turned active
//! after the first layout (the sidebar opened right after start) stacks its Texts like Qt,
//! not all at y 0; and a Text turning visible or getting its text later is placed below too.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r#"
import QtQuick
import QtQuick.Layouts
Item {
    width: 300; height: 60
    property string name: "Internet"
    property string status: ""
    property bool expanded: true
    property var col: null
    RowLayout {
        anchors.fill: parent
        Rectangle { implicitWidth: 40; implicitHeight: 40 }
        Loader {
            Layout.alignment: Qt.AlignVCenter
            Layout.fillWidth: true
            visible: parent.parent.expanded
            active: visible
            sourceComponent: Column {
                spacing: -2
                Component.onCompleted: root_.col = this
                Text { anchors { left: parent.left; right: parent.right } text: root_.name }
                Text {
                    visible: root_.status
                    anchors { left: parent.left; right: parent.right }
                    text: root_.status
                }
            }
        }
    }
    id: root_
}
"#;

fn frame(rt: &mut Runtime, fonts: &mut scene::Fonts, root: usize, laid: &mut u64) {
    for _ in 0..4 {
        let rs = std::mem::take(&mut rt.world.borrow_mut().resized);
        let mut w = rt.world.borrow_mut();
        if w.layout_rev != *laid || !rs.iter().all(|&i| scene::is_local_resize(&mut w, i)) {
            drop(w);
            scene::layout_window(rt, fonts, root, [300.0, 60.0]);
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

fn ys(rt: &mut Runtime, root: usize) -> Vec<(f64, f64, bool)> {
    let mut w = rt.world.borrow_mut();
    let Some(Value::Item(col)) = w.read(root, "col") else {
        return vec![];
    };
    let kids = w.items[col].children.clone();
    kids.into_iter()
        .map(|c| {
            let y = w.read(c, "y").map_or(-1.0, |v| v.to_number());
            let h = w.read(c, "height").map_or(-1.0, |v| v.to_number());
            let v = w.read(c, "visible").is_some_and(|v| v.truthy());
            (y, h, v)
        })
        .collect()
}

fn run(init: &[(&str, Value)], later: &[(&str, Value)]) -> Vec<(f64, f64, bool)> {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    for (k, v) in init {
        rt.world.borrow_mut().write(root, k, v.clone());
    }
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut laid = rt.world.borrow().layout_rev;
    scene::layout_window(&mut rt, &mut fonts, root, [300.0, 60.0]);
    laid = rt.world.borrow().layout_rev.max(laid);
    rt.settle();
    frame(&mut rt, &mut fonts, root, &mut laid);
    for (k, v) in later {
        rt.world.borrow_mut().write(root, k, v.clone());
        rt.settle();
        frame(&mut rt, &mut fonts, root, &mut laid);
    }
    ys(&mut rt, root)
}

fn check(k: &[(f64, f64, bool)]) {
    assert!(k[1].2, "status text visible");
    assert!(k[0].1 > 0.0, "title measured");
    assert_eq!(k[1].0, k[0].1 - 2.0, "status text placed below the title");
}

#[test]
fn status_appears_later() {
    check(&run(&[], &[("status", Value::str("SNR_67_5G"))]));
}

#[test]
fn name_and_status_appear_later() {
    let e = || Value::str("");
    check(&run(
        &[("name", e())],
        &[
            ("name", Value::str("Internet")),
            ("status", Value::str("SNR_67_5G")),
        ],
    ));
}

#[test]
fn name_appears_after_status() {
    check(&run(
        &[
            ("name", Value::str("")),
            ("status", Value::str("SNR_67_5G")),
        ],
        &[("name", Value::str("Internet"))],
    ));
}

#[test]
fn loader_expands_later() {
    check(&run(
        &[
            ("expanded", Value::Bool(false)),
            ("status", Value::str("SNR_67_5G")),
        ],
        &[("expanded", Value::Bool(true))],
    ));
}
