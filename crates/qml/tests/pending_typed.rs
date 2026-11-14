//! A handler that runs while its object is created reads the object's own bound properties,
//! `var`, `int` and `string`, and its `implicitWidth`: Qt 6.12 runs each pending binding on
//! the read (QQmlData::flushPendingBinding), also after the handler made other objects first
//! (`createObject`, `ListModel.append` under a Repeater, a Loader).
//!
//! The documents are the `rigen` cases `crates/rigen/check/cases/qml/pending_typed_<name>.qml`
//! (there compiled code = interpreter = Qt); their `// qt:` line is what Qt 6.12 leaves in
//! `result` (PySide6 6.12.0, QQmlComponent): `typeof v`, `v`, `n`, `s`, `implicitWidth` as
//! the handler saw them.

use qml::runtime::Runtime;

fn check(name: &str) {
    let cases = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rigen/check/cases/qml");
    let file = cases.join(format!("pending_typed_{name}.qml"));
    let text = std::fs::read_to_string(&file).unwrap();
    let qt = (text.lines().find_map(|l| l.strip_prefix("// qt: "))).unwrap();
    let mut rt = Runtime::new(Some(cases.clone()));
    let root = rt.load_file(&file).unwrap();
    rt.settle();
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert_eq!(rt.get(root, "result").to_js_string(), qt);
}

#[test]
fn nothing_made_before_the_read() {
    check("plain");
}

#[test]
fn createobject_in_a_binding_of_a_child() {
    check("create");
}

#[test]
fn createobject_in_the_handler() {
    check("hcreate");
}

#[test]
fn a_delegate_made_by_listmodel_append_in_the_handler() {
    check("append");
}

#[test]
fn a_loader_made_active_in_the_handler() {
    check("loader");
}

#[test]
fn createobject_in_a_binding_and_an_append_in_the_handler() {
    check("both");
}
