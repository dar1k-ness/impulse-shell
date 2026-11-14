//! The size anchors gave an item is a set one (QQuickAnchorsPrivate::setItemWidth is
//! QQuickItem::setWidth): it stays when the anchor is reset, by a script or by a binding that
//! gives `undefined`, does not follow the implicit size or the parent afterwards, and goes back
//! to the implicit size with `width = undefined`. `widthChanged` comes once for each change.
//!
//! `scenes/anchor_reset_size/qt.txt` is what Qt 6.12 gives for `scenes/anchor_reset_size/Reset.qml`
//! (PySide6 6.12.0, offscreen, a QQuickView sized to the root): `step(0)` to `step(5)`, the
//! event loop run between them, with `==` between their results.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

fn frame(rt: &mut Runtime, fonts: &mut scene::Fonts, root: usize) {
    for _ in 0..4 {
        let size = ["width", "height"].map(|k| rt.get(root, k).to_number());
        scene::layout_window(rt, fonts, root, size);
        rt.settle();
    }
}

#[test]
fn a_size_anchors_gave_stays_after_the_anchor_is_reset() {
    let qt = include_str!("scenes/anchor_reset_size/qt.txt");
    let (mut rt, root) = testscenes::load("anchor_reset_size/Reset.qml");
    rt.settle();
    let mut fonts = scene::Fonts::default();
    frame(&mut rt, &mut fonts, root);
    let mut wrong = Vec::new();
    for (n, qt) in qt.trim_end().split("\n==\n").enumerate() {
        let ours = match rt.call_method(root, "step", vec![Value::Num(n as f64)]) {
            Some(Ok(Value::Str(s))) => s.to_string(),
            v => panic!("step({n}) gave {v:?}"),
        };
        rt.settle();
        frame(&mut rt, &mut fonts, root);
        for (q, o) in qt.lines().zip(ours.lines()) {
            if q != o {
                wrong.push(format!("step({n})\n  qt:   {q}\n  ours: {o}"));
            }
        }
        assert_eq!(qt.lines().count(), ours.lines().count(), "step({n})");
    }
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert!(
        wrong.is_empty(),
        "{} lines differ\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
