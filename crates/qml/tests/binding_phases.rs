//! The order a creation enables its bindings in. `binding_phases/phases.qt612.txt` is what
//! `warning_order/qtmsg.py` printed for `binding_phases/phases.qml` in the PySide6 of
//! ri-testenv.

use qml::runtime::Runtime;
use std::{cell::RefCell, path::Path};

thread_local!(static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) });

fn console(_level: u8, line: &str) {
    SAID.with_borrow_mut(|s| s.push(format!("console: {line}")));
}

fn warning(line: &str) {
    SAID.with_borrow_mut(|s| s.push(format!("warning: {line}")));
}

#[test]
fn the_bindings_of_the_geometry_are_set_after_the_other_ones() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/binding_phases");
    let _ = qml::builtins::CONSOLE_SINK.set(console);
    let _ = qml::warning::SINK.set(warning);
    let file = dir.join("phases.qml");
    let mut rt = Runtime::new(Some(dir.clone()));
    rt.load_file(&file).unwrap();
    rt.settle();
    assert!(rt.world.borrow().quit.is_some());
    let ours = SAID.take();
    let qt = std::fs::read_to_string(dir.join("phases.qt612.txt")).unwrap();
    let qt: Vec<&str> = qt.lines().collect();
    assert_eq!(ours, qt, "ours: {ours:#?}\nQt: {qt:#?}");
}
