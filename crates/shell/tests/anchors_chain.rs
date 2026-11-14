//! QQuickAnchors between siblings follow the anchor graph, not declaration order: a chain
//! declared target-last, a cycle across axes (p.left: q.right, q.top: p.bottom), an item whose
//! width follows its anchored height with a sibling anchored to it, and fill/centerIn chains,
//! first laid out and after the chain's root moves or resizes (a local relayout re-places every
//! sibling that follows it, also transitively). A real loop ends instead of hanging.
//! Expected numbers are Qt 6.12's (PySide6-Essentials 6.12.0, offscreen) for the same QML.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r#"
import QtQuick
Item {
    id: root_
    width: 400; height: 400
    property real m: 5
    property var o_a: a
    property var o_b: b
    property var o_c: c
    property var o_d: d
    property var o_e: e
    property var o_p: p
    property var o_q: q
    property var o_s: s
    property var o_s2: s2
    property var o_f0: f0
    property var o_f1: f1
    property var o_f2: f2
    Rectangle { id: e; anchors.left: d.right; width: 10; height: 10 }
    Rectangle { id: d; anchors.left: c.right; width: 10; height: 10 }
    Rectangle { id: c; anchors.left: b.right; width: 10; height: 10 }
    Rectangle { id: b; anchors.left: a.right; width: 10; height: 10 }
    Rectangle { id: a; anchors.left: parent.left; anchors.leftMargin: root_.m; width: 10; height: 10 }
    Rectangle { id: p; anchors.left: q.right; anchors.top: parent.top; anchors.topMargin: 50; width: 10; height: 10 }
    Rectangle { id: q; anchors.top: p.bottom; x: 100; width: 10; height: 10 }
    Rectangle { id: s2; anchors.left: s.right; anchors.verticalCenter: s.verticalCenter; width: 5; height: 5 }
    Rectangle { id: s; anchors.right: parent.right; anchors.top: parent.top; anchors.bottom: parent.bottom; anchors.topMargin: 300; width: height }
    Rectangle { id: f2; anchors.centerIn: f1; width: 4; height: 4 }
    Rectangle { id: f1; anchors.fill: f0; anchors.margins: 2 }
    Rectangle { id: f0; x: 200; y: 200; width: 40; height: 20 }
}
"#;

fn item(rt: &Runtime, root: usize, n: &str) -> usize {
    match rt.world.borrow_mut().read(root, &format!("o_{n}")) {
        Some(Value::Item(i)) => i,
        v => panic!("o_{n}: {v:?}"),
    }
}

fn get(rt: &Runtime, root: usize, n: &str, p: &str) -> f64 {
    let i = item(rt, root, n);
    rt.world
        .borrow_mut()
        .read(i, p)
        .map_or(f64::NAN, |v| v.to_number())
}

fn chain(rt: &Runtime, root: usize) -> Vec<f64> {
    ["a", "b", "c", "d", "e"]
        .iter()
        .map(|n| get(rt, root, n, "x"))
        .collect()
}

/// Everything but the chain, as (name.prop, value).
fn rest(rt: &Runtime, root: usize) -> Vec<(String, f64)> {
    [
        ("p", "x"),
        ("p", "y"),
        ("q", "x"),
        ("q", "y"),
        ("s", "x"),
        ("s", "y"),
        ("s", "width"),
        ("s", "height"),
        ("s2", "x"),
        ("s2", "y"),
        ("f1", "x"),
        ("f1", "y"),
        ("f1", "width"),
        ("f1", "height"),
        ("f2", "x"),
        ("f2", "y"),
    ]
    .iter()
    .map(|(n, p)| (format!("{n}.{p}"), get(rt, root, n, p)))
    .collect()
}

fn expect(v: &[f64]) -> Vec<(String, f64)> {
    let names = [
        "p.x",
        "p.y",
        "q.x",
        "q.y",
        "s.x",
        "s.y",
        "s.width",
        "s.height",
        "s2.x",
        "s2.y",
        "f1.x",
        "f1.y",
        "f1.width",
        "f1.height",
        "f2.x",
        "f2.y",
    ];
    names
        .iter()
        .map(|n| n.to_string())
        .zip(v.iter().copied())
        .collect()
}

fn layout(rt: &mut Runtime, fonts: &mut scene::Fonts, root: usize) {
    rt.settle();
    scene::layout_window(rt, fonts, root, [400.0, 400.0]);
    rt.settle();
}

fn write(rt: &Runtime, root: usize, n: Option<&str>, p: &str, v: f64) {
    let i = n.map_or(root, |n| item(rt, root, n));
    rt.world.borrow_mut().write(i, p, Value::Num(v));
}

#[test]
fn sibling_anchors_follow_the_anchor_graph() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    let mut fonts = scene::Fonts::default();
    layout(&mut rt, &mut fonts, root);
    let first = [
        110.0, 50.0, 100.0, 60.0, 300.0, 300.0, 100.0, 100.0, 400.0, 347.0, 202.0, 202.0, 36.0,
        16.0, 218.0, 208.0,
    ];
    assert_eq!(chain(&rt, root), [5.0, 15.0, 25.0, 35.0, 45.0]);
    assert_eq!(rest(&rt, root), expect(&first));

    write(&rt, root, None, "m", 25.0);
    layout(&mut rt, &mut fonts, root);
    assert_eq!(chain(&rt, root), [25.0, 35.0, 45.0, 55.0, 65.0]);
    assert_eq!(rest(&rt, root), expect(&first));

    write(&rt, root, Some("f0"), "width", 60.0);
    write(&rt, root, Some("q"), "x", 7.0);
    layout(&mut rt, &mut fonts, root);
    let third = [
        17.0, 50.0, 7.0, 60.0, 300.0, 300.0, 100.0, 100.0, 400.0, 347.0, 202.0, 202.0, 56.0, 16.0,
        228.0, 208.0,
    ];
    assert_eq!(chain(&rt, root), [25.0, 35.0, 45.0, 55.0, 65.0]);
    assert_eq!(rest(&rt, root), expect(&third));

    write(&rt, root, Some("a"), "width", 20.0);
    layout(&mut rt, &mut fonts, root);
    assert_eq!(chain(&rt, root), [25.0, 45.0, 55.0, 65.0, 75.0]);
    assert_eq!(rest(&rt, root), expect(&third));
}

#[test]
fn anchor_loop_ends() {
    // Qt warns "Possible anchor loop detected on horizontal anchor." and stops re-placing.
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            r#"
import QtQuick
Item {
    width: 400; height: 400
    property var o_l1: l1
    property var o_l2: l2
    Rectangle { id: l1; anchors.left: l2.right; width: 10; height: 10 }
    Rectangle { id: l2; anchors.left: l1.right; width: 10; height: 10 }
}
"#,
        )
        .unwrap();
    let mut fonts = scene::Fonts::default();
    layout(&mut rt, &mut fonts, root);
    let (x1, x2) = (get(&rt, root, "l1", "x"), get(&rt, root, "l2", "x"));
    assert!(x1.is_finite() && x2.is_finite(), "{x1} {x2}");
    assert!((x1 - x2).abs() <= 10.0 * 6.0, "{x1} {x2}");
}
