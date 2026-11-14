//! A baseline in a height that was not set, and a popup's `anchors.centerIn`.
//!
//! QQuickTextPrivate::updateBaseline and QQuickTextInputPrivate::updateBaselineOffset move the
//! baseline by the vertical alignment only `if (q->heightValid())`: a TextField whose height is
//! its implicit one has the first line's baseline, one whose height was set (by a script or by
//! an anchor) has it where the alignment puts the text. QQuickPopupPositioner::reposition
//! places a popup that is open, and centers it in its parent only: a closed popup, and one
//! told to center in another item, stays where it is.
//!
//! `scenes/anchor_unset_height/qt.txt` is what Qt 6.12 gives for
//! `scenes/anchor_unset_height/Scene.qml` (PySide6 6.12.0, offscreen, a QQuickView sized to the
//! root): `step(0)` to `step(5)`, the event loop run between them, with `==` between their
//! results.

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
fn a_baseline_in_an_unset_height_and_a_popup_centered_when_open() {
    let qt = include_str!("scenes/anchor_unset_height/qt.txt");
    let (mut rt, root) = testscenes::load("anchor_unset_height/Scene.qml");
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
