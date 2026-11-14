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

/// The handlers of the chosen choice that ran by then, each with the choice's x and width as
/// the handler read them (`log` of the scene), on the same two lines of Qt 6.12: its width is
/// read as 0 before its binding ran, the text has its size before `Component.onCompleted`,
/// and the Row places the choice after that.
const QT_HANDLERS: [&str; 2] = [
    "c0,0 w0,28 w0,63.421875 d0,63.421875 x57.90625,63.421875 ",
    "c0,0 w0,28 w0,63.421875 d0,63.421875 x57.90625,63.421875 c121.328125,59.171875 ",
];

/// The two lines of sizes of the scene, its two lines of handlers and the errors of the
/// engine, in a home of the test's own.
fn run(test: &str) -> Option<(Vec<String>, Vec<String>, Vec<String>)> {
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
    let mut handlers = vec![];
    let mut read = |rt: &mut Runtime| {
        let sizes = rt.call_method(root, "sizes", Vec::new()).unwrap().unwrap();
        lines.push(sizes.to_js_string());
        handlers.push(rt.get(root, "log").to_js_string());
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
    Some((lines, handlers, errors))
}

#[test]
fn a_choice_is_as_wide_as_its_label_and_the_mark_lies_under_the_chosen_one() {
    let Some((lines, _, errors)) =
        run("a_choice_is_as_wide_as_its_label_and_the_mark_lies_under_the_chosen_one")
    else {
        return;
    };
    assert_eq!([lines[0].as_str(), lines[1].as_str()], QT);
    assert_eq!(errors, Vec::<String>::new());
}

#[test]
fn the_handlers_of_a_choice_run_in_the_order_of_qt() {
    let Some((_, handlers, _)) = run("the_handlers_of_a_choice_run_in_the_order_of_qt") else {
        return;
    };
    assert_eq!([handlers[0].as_str(), handlers[1].as_str()], QT_HANDLERS);
}
