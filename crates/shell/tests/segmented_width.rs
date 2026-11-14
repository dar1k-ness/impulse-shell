//! The row of choices of the settings (`SettingsSegmented.qml`): a choice is as wide as its
//! label and the mark lies under the chosen one, as Qt 6.12 lays `segmented_width/seg.qml` out.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::Path;

/// The sizes `qml seg.qml` of Qt 6.12 prints (the command is in the file): as loaded, and
/// with the third choice chosen.
const QT: [&str; 2] = [
    "0,57.90625,57.90625 57.90625,63.421875,63.421875 121.328125,59.171875,59.171875 | row 180.5 track 188.5 mark 61.90625,63.421875",
    "0,57.90625,57.90625 57.90625,63.421875,63.421875 121.328125,59.171875,59.171875 | row 180.5 track 188.5 mark 125.328125,59.171875",
];

/// The two lines of sizes of the scene and the errors of the engine, in a home of the test's
/// own.
fn run(test: &str) -> Option<(Vec<String>, Vec<String>)> {
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        testenv::run_in_own_process(test, testenv::home_vars(home.path()));
        return None;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/segmented_width");
    let mut rt = Runtime::new(Some(dir.clone()));
    rt.world.borrow_mut().track_fresh = true;
    // As the shell: a text is measured when a script reads its size.
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    scene::text_hooks(&mut rt.world.borrow_mut(), &fonts);
    let root = rt.load_file(&dir.join("seg.qml")).unwrap();
    rt.settle();
    let layout = |rt: &mut Runtime| {
        scene::layout_window(rt, &mut fonts.borrow_mut(), root, [400.0, 60.0]);
        rt.settle();
    };
    let mut lines = vec![];
    let mut read = |rt: &mut Runtime| {
        let sizes = rt.call_method(root, "sizes", Vec::new()).unwrap().unwrap();
        lines.push(sizes.to_js_string());
    };
    layout(&mut rt);
    read(&mut rt);
    rt.call_method(root, "choose", vec![Value::Num(2.0)])
        .unwrap()
        .unwrap();
    rt.settle();
    layout(&mut rt);
    read(&mut rt);
    let errors = rt.world.borrow().errors.clone();
    Some((lines, errors))
}

#[test]
fn a_choice_is_as_wide_as_its_label_and_the_mark_lies_under_the_chosen_one() {
    let Some((lines, errors)) =
        run("a_choice_is_as_wide_as_its_label_and_the_mark_lies_under_the_chosen_one")
    else {
        return;
    };
    assert_eq!([lines[0].as_str(), lines[1].as_str()], QT);
    assert_eq!(errors, Vec::<String>::new());
}
