//! A control of the Basic style takes its size and its baseline from the parts the document
//! did not set, as Qt 6.12 does: `Button {}` is 100x40 with its baseline at 6, from the style's
//! own `background` (100x40) and `contentItem` (the label) and the style's paddings.
//!
//! `scenes/basic_sizes/sizes.qt` is what Qt 6.12 gives for `scenes/basic_sizes/sizes.qml`
//! (`scenes/basic_sizes/qt.py`, run in the testenv image): `snap()` as loaded and after each
//! of `grow()`, `restyle()`, `strip()` and `shrink()`, with `==` between them. A line is a
//! control: implicit size, size, baseline, the implicit sizes of its parts, the room for its
//! content, its paddings; the last one is how often `live` said each of them changed since the step
//! began (not compared as loaded: that is the order the style's file is bound in).
//!
//! `scenes/basic_sizes/fields.qml` is the same for the width a TextArea and a TextField take
//! from their text as it is wrapped and from their placeholder (`type()`, `narrow()`, `clear()`).
//! `scenes/basic_sizes/bars.qml` is a ProgressBar of the style of the file that names it:
//! Basic's, and Material's in a file that imports that style (`strip()`).
//! `scenes/basic_sizes/combo.qml` is the side paddings of a ComboBox as a script reads them
//! (`write()`, `reset()`, `strip()`).
//!
//! The scenes are compiled into the test (`testscenes`): a build with the `interp` feature
//! interprets its scripts, a build without it runs the compiled ones.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

fn frames(rt: &mut Runtime, fonts: &mut scene::Fonts, root: usize) {
    for _ in 0..6 {
        scene::layout_window(rt, fonts, root, [400.0, 300.0]);
        rt.settle();
    }
}

/// The scene with the files next to it as its types: `testscenes::load` interprets a scene as
/// one text, which has no files next to it.
fn load(scene: &str) -> (Runtime, usize) {
    if !qml::INTERP {
        return testscenes::load(scene);
    }
    println!("scene {scene}: interpreted scripts");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenes");
    let mut rt = Runtime::new(Some(root.clone()));
    let top = rt
        .load_file(&root.join(scene))
        .unwrap_or_else(|e| panic!("scene {scene}: {e}"));
    (rt, top)
}

/// `snap()` of `scene` as loaded and after each of `steps`, against what Qt gave (`qt`).
/// A line that starts with one of `still` is known to differ, and has to (`step/start`: after
/// that step only).
fn like_qt(scene: &str, qt: &str, steps: &[&str], still: &[&str]) {
    let (mut rt, root) = load(scene);
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut wrong = Vec::new();
    let mut lines = 0;
    let steps = [""].iter().chain(steps);
    for (step, qt) in steps.zip(qt.trim_end().split("\n==\n")) {
        if !step.is_empty() {
            rt.call_method(root, step, vec![]).unwrap().unwrap();
            rt.settle();
        }
        frames(&mut rt, &mut fonts, root);
        let ours = match rt.call_method(root, "snap", vec![]).unwrap().unwrap() {
            Value::Str(s) => s.to_string(),
            v => panic!("snap() gave {v:?}"),
        };
        assert_eq!(qt.lines().count(), ours.lines().count(), "after `{step}`");
        for (q, o) in qt.lines().zip(ours.lines()) {
            lines += 1;
            // What `live` said while it was made depends on the order Qt binds the style's
            // file in, which is not what this test is about.
            if step.is_empty() && q.starts_with("signals:") {
                continue;
            }
            let known = still.iter().any(|s| match s.split_once('/') {
                Some((at, s)) => at == *step && q.starts_with(s),
                None => q.starts_with(s),
            });
            if q != o && !known {
                wrong.push(format!("after `{step}`\n  qt:   {q}\n  ours: {o}"));
            } else if q == o && known {
                wrong.push(format!(
                    "after `{step}`\n  as in Qt now, not to be listed: {q}"
                ));
            }
        }
    }
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert!(
        wrong.is_empty(),
        "{} of {lines} lines differ\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn a_control_has_the_size_and_baseline_the_basic_style_gives_it() {
    like_qt(
        "basic_sizes/sizes.qml",
        include_str!("scenes/basic_sizes/sizes.qt"),
        &["grow", "restyle", "strip", "shrink"],
        &[],
    );
}

#[test]
fn a_text_area_is_as_wide_as_its_wrapped_text_or_its_placeholder() {
    like_qt(
        "basic_sizes/fields.qml",
        include_str!("scenes/basic_sizes/fields.qt"),
        &["type", "narrow", "clear"],
        &[],
    );
}

#[test]
fn a_progress_bar_has_the_size_of_the_style_its_file_imports() {
    like_qt(
        "basic_sizes/bars.qml",
        include_str!("scenes/basic_sizes/bars.qt"),
        &["strip"],
        &[],
    );
}

#[test]
fn a_combo_box_has_the_side_paddings_its_style_binds() {
    like_qt(
        "basic_sizes/combo.qml",
        include_str!("scenes/basic_sizes/combo.qt"),
        &["write", "reset", "strip"],
        &[],
    );
}

/// A document that has no file (`Runtime::load_str`) names the style by its imports too: the
/// sizes are those of `b0` and `m0` of `bars.qt`.
#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "a document of text; AOT-only refuses it"
)]
fn a_document_without_a_file_names_the_style_too() {
    let size = |imports: &str| {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(&format!(
                "import QtQuick\n{imports}Item {{ property Item bar: b; ProgressBar {{ id: b }} }}\n"
            ))
            .expect("loads");
        rt.settle();
        frames(&mut rt, &mut scene::Fonts::default(), root);
        let bar = match rt.get(root, "bar") {
            Value::Item(i) => i,
            v => panic!("bar is {v:?}"),
        };
        ["implicitWidth", "implicitHeight"].map(|k| rt.get(bar, k).to_number())
    };
    assert_eq!(size("import QtQuick.Controls\n"), [200.0, 6.0]);
    assert_eq!(
        size("import QtQuick.Controls.Material\nimport QtQuick.Controls\n"),
        [200.0, 4.0]
    );
}
