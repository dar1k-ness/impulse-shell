//! A script reads a BINDABLE property whose binding the creation has yet to set.
//! `pending_bindable/pending.qt612.txt` is what `warning_order/qtmsg.py` printed for
//! `pending_bindable/pending.qml` in the PySide6 of ri-testenv. Qt gives the read what the
//! property holds and sets the binding after the other ones; without the `fresh_bind` check of
//! the second pending read in `World::read_inner_sym` the read ran the binding.

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
fn a_read_does_not_run_the_binding_of_a_bindable_property_the_creation_has_yet_to_set() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pending_bindable");
    let _ = qml::builtins::CONSOLE_SINK.set(console);
    let _ = qml::warning::SINK.set(warning);
    let file = dir.join("pending.qml");
    let mut rt = Runtime::new(Some(dir.clone()));
    rt.load_file(&file).unwrap();
    rt.settle();
    assert!(rt.world.borrow().quit.is_some());
    let ours = SAID.take();
    let qt = std::fs::read_to_string(dir.join("pending.qt612.txt")).unwrap();
    let qt: Vec<&str> = qt.lines().collect();
    assert_eq!(ours, qt, "ours: {ours:#?}\nQt: {qt:#?}");
}
