//! QQuickFlow wraps only at a valid width (height for TopToBottom): set explicitly, by anchors
//! or by a parent Layout. Without one it lays a single line and grows its implicit size, so a
//! Flow that gains children does not wrap at the width it computed for fewer.
//! Expected sizes are Qt 6.12's (PySide6-Essentials 6.12.0, offscreen) for the same QML.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r#"
import QtQuick
import QtQuick.Layouts
Item {
    id: root_
    width: 400; height: 400
    property int n: 2
    property var o_f: f
    property var o_g: g
    property var o_h: h
    property var o_k: k
    property var o_m: m
    property var o_l: l
    Flow { id: f; spacing: 5; Repeater { model: root_.n; Rectangle { width: 50; height: 20 } } }
    Flow { id: g; width: 105; spacing: 5; Repeater { model: root_.n; Rectangle { width: 50; height: 20 } } }
    Item {
        width: 160; height: 400
        Flow { id: h; anchors.left: parent.left; anchors.right: parent.right; spacing: 5; Repeater { model: root_.n; Rectangle { width: 50; height: 20 } } }
    }
    Flow { id: k; flow: Flow.TopToBottom; spacing: 5; Repeater { model: root_.n; Rectangle { width: 50; height: 20 } } }
    Flow { id: m; flow: Flow.TopToBottom; height: 45; spacing: 5; Repeater { model: root_.n; Rectangle { width: 50; height: 20 } } }
    ColumnLayout {
        width: 160
        Flow { id: l; Layout.fillWidth: true; spacing: 5; Repeater { model: root_.n; Rectangle { width: 50; height: 20 } } }
    }
}
"#;

fn frame(rt: &mut Runtime, fonts: &mut scene::Fonts, root: usize, laid: &mut u64) {
    for _ in 0..4 {
        let rs = std::mem::take(&mut rt.world.borrow_mut().resized);
        let mut w = rt.world.borrow_mut();
        if w.layout_rev != *laid || !rs.iter().all(|&i| scene::is_local_resize(&mut w, i)) {
            drop(w);
            scene::layout_window(rt, fonts, root, [400.0, 400.0]);
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

/// Loads `QML`, lays it out like the shell's first frame, then applies each write in `steps`
/// followed by a frame, calling `probe` after the first frame and after every step.
fn run<T>(steps: &[(&str, Value)], mut probe: impl FnMut(&mut Runtime, usize) -> T) -> Vec<T> {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut laid = rt.world.borrow().layout_rev;
    scene::layout_window(&mut rt, &mut fonts, root, [400.0, 400.0]);
    laid = rt.world.borrow().layout_rev.max(laid);
    rt.settle();
    frame(&mut rt, &mut fonts, root, &mut laid);
    let mut out = vec![probe(&mut rt, root)];
    for (k, v) in steps {
        rt.world.borrow_mut().write(root, k, v.clone());
        rt.settle();
        frame(&mut rt, &mut fonts, root, &mut laid);
        out.push(probe(&mut rt, root));
    }
    out
}

fn item_num(rt: &mut Runtime, root: usize, item: &str, prop: &str) -> f64 {
    let mut w = rt.world.borrow_mut();
    let Some(Value::Item(i)) = w.read(root, &format!("o_{item}")) else {
        panic!("o_{item} is not an item");
    };
    w.read(i, prop).map_or(f64::NAN, |v| v.to_number())
}

const FLOWS: [&str; 6] = ["f", "g", "h", "k", "m", "l"];

fn sizes(rt: &mut Runtime, root: usize) -> Vec<(f64, f64)> {
    FLOWS
        .iter()
        .map(|n| {
            (
                item_num(rt, root, n, "width"),
                item_num(rt, root, n, "height"),
            )
        })
        .collect()
}

#[test]
fn flow_wraps_only_at_valid_size() {
    let got = run(&[("n", Value::Num(5.0))], sizes);
    let qt2 = [
        (105.0, 20.0),
        (105.0, 20.0),
        (160.0, 20.0),
        (50.0, 45.0),
        (50.0, 45.0),
        (160.0, 20.0),
    ];
    let qt5 = [
        (270.0, 20.0),
        (105.0, 70.0),
        (160.0, 45.0),
        (50.0, 120.0),
        (160.0, 45.0),
        (160.0, 45.0),
    ];
    for (when, (g, q)) in [
        ("2 children", (&got[0], qt2)),
        ("5 children", (&got[1], qt5)),
    ] {
        for ((n, g), q) in FLOWS.iter().zip(g).zip(q) {
            assert_eq!(*g, q, "{when}: Flow {n} (width, height), Qt 6.12 has {q:?}");
        }
    }
}
