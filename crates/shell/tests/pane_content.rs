//! The content size of a QQuickPane (Pane, Frame, GroupBox, Page, ScrollView): `contentWidth`
//! and `contentHeight` that nothing set follow what is in the pane, `implicitContentWidth` and
//! `implicitContentHeight` are that size, and a reset (`contentWidth = undefined`) goes back
//! to it.
//!
//! `scenes/pane_content/qt.txt` is what Qt 6.12 gives for `scenes/pane_content/Panes.qml` (PySide6 6.12.0,
//! offscreen, `QT_QUICK_CONTROLS_STYLE=Basic`), five `snap()`s with `==` between them: as
//! loaded, then after `reset()`, `change()`, `set()` and `reset()` again, called in that order
//! from the `Component.onCompleted` of a document whose root is `Panes {}`.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

fn frame(rt: &mut Runtime, fonts: &mut scene::Fonts, root: usize) {
    // A pane's size reaches the properties in the layout pass, and what follows it in the next.
    for _ in 0..4 {
        scene::layout_window(rt, fonts, root, [400.0, 400.0]);
        rt.settle();
    }
}

#[test]
fn a_pane_has_the_content_size_qt_gives_it() {
    let qt = include_str!("scenes/pane_content/qt.txt");
    let (mut rt, root) = testscenes::load("pane_content/Panes.qml");
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut snap = |rt: &mut Runtime, step: &str| {
        if !step.is_empty() {
            rt.call_method(root, step, vec![]).unwrap().unwrap();
            rt.settle();
        }
        frame(rt, &mut fonts, root);
        match rt.call_method(root, "snap", vec![]).unwrap().unwrap() {
            Value::Str(s) => s.to_string(),
            v => panic!("snap() gave {v:?}"),
        }
    };
    let steps = ["", "reset", "change", "set", "reset"];
    let mut wrong = Vec::new();
    for (step, qt) in steps.into_iter().zip(qt.trim_end().split("\n==\n")) {
        let ours = snap(&mut rt, step);
        for (q, o) in qt.lines().zip(ours.lines()) {
            if q != o {
                wrong.push(format!("after `{step}`\n  qt:   {q}\n  ours: {o}"));
            }
        }
        assert_eq!(qt.lines().count(), ours.lines().count(), "after `{step}`");
    }
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert!(
        wrong.is_empty(),
        "{} lines differ\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
