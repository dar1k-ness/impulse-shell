//! `undefined` written to a property, by a binding (QQmlBinding::doStore) and by an assignment
//! (QV4::QObjectWrapper::setProperty): a property with a RESET function goes back to what it
//! is when nothing is written (`width: undefined` is the implicit width), a `var` property
//! takes it, any other keeps its value; the assignment throws `Cannot assign [undefined] to
//! <type>` there, the binding logs `Unable to assign [undefined] to <type>`.
//!
//! `tests/data/undefined_writes/<name>.qml` is the document; `<name>.txt` has, on each line,
//! an expression of its `cases`, a tab and what Qt 6.12 gives for it (PySide6 6.12.0,
//! QQmlComponent) as `typeof: value`, `!` and the error when it throws.

use qml::runtime::Runtime;

fn check(name: &str) {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/undefined_writes");
    let text = std::fs::read_to_string(data.join(format!("{name}.txt"))).unwrap();
    let cases: Vec<(&str, &str)> = (text.lines())
        .map(|l| l.split_once('\t').unwrap())
        .collect();
    let mut rt = Runtime::new(Some(data.clone()));
    let root = rt.load_file(&data.join(format!("{name}.qml"))).unwrap();
    rt.settle();
    assert_eq!(rt.errors(), Vec::<String>::new());
    let result = rt.get(root, "result").to_js_string();
    let ours: Vec<&str> = result.split(" | ").collect();
    assert_eq!(ours.len(), cases.len());
    let wrong: Vec<String> = (cases.iter().zip(ours))
        .filter(|((_, qt), ours)| qt != ours)
        .map(|((c, qt), ours)| format!("{c}\n  qt:   {qt}\n  ours: {ours}"))
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Written where the object is declared: `width: undefined`, `width: ;` and the same for the
/// other kinds of property, then assignments in `Component.onCompleted`.
#[test]
fn undefined_written_at_creation() {
    check("w");
}

/// Written after a value: a binding whose value becomes `undefined`, and an assignment over
/// a value the property already had.
#[test]
fn undefined_written_over_a_value() {
    check("wu");
}

/// QQuickItem::setWidth and setHeight ignore NaN: the size stays what it was and no signal is
/// sent, whether the binding gives NaN at creation or after a value.
#[test]
fn nan_written_to_a_size() {
    check("nan");
}

/// A change handler run while its object is created reads the sizes whose bindings are not
/// enabled yet as numbers, and writes one to a `real` property: an object made with
/// `createObject` by a binding of the same creation leaves those bindings pending.
#[test]
fn a_size_read_before_its_binding_is_enabled() {
    check("pending");
}

/// `createObject` in a binding of a child, while the parent is being made, enables the
/// bindings of the object it makes only: a handler of the parent that runs after it reads the
/// parent's own typed properties, whose bindings are still pending, with their values.
#[test]
fn bindings_stay_pending_over_a_createobject_in_a_binding() {
    check("created");
}

/// `undefined` written to the properties whose RESET function does more than put the default
/// back: a corner and `antialiasing` of a RectangularShadow follow `radius` and the implicit
/// value again, a Grid's spacings and a Container's content size keep their value, a padding
/// has the wider padding, a popup's margin its `margins`, `margins` is -1, `closePolicy` 17,
/// a control's `font` the default font; a Text's `font` has no RESET and throws.
#[test]
fn undefined_written_to_a_property_with_its_own_reset() {
    check("reset");
}

/// A value written after such a reset is a set one again: a binding that gave a value, then
/// `undefined`, then the value, and the same by assignments, for a padding, a popup's margins,
/// a corner radius, a ComboBox's `displayText` and a popup's `parent`
/// (QQuickPopup::resetParentItem: the item it is declared in).
#[test]
fn a_value_written_after_a_reset() {
    check("again");
}

/// A script's `undefined` to a property that takes none, with a binding and with none: the
/// write throws, the value stays, no change signal comes and the binding is gone
/// (QV4::QObjectWrapper::setProperty takes it off before the value is refused). The document
/// is the `rigen` case `rejected_undefined.qml` (there compiled code = interpreter = Qt); its
/// `// qt:` line is what Qt 6.12 leaves in `result`.
#[test]
fn a_rejected_undefined_takes_the_binding_off() {
    let cases = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rigen/check/cases/qml");
    let file = cases.join("rejected_undefined.qml");
    let text = std::fs::read_to_string(&file).unwrap();
    let qt = (text.lines().find_map(|l| l.strip_prefix("// qt: "))).unwrap();
    let mut rt = Runtime::new(Some(cases.clone()));
    let root = rt.load_file(&file).unwrap();
    rt.settle();
    let result = rt.get(root, "result").to_js_string();
    let wrong: Vec<String> = (qt.split(" | ").zip(result.split(" | ")))
        .filter(|(qt, ours)| qt != ours)
        .map(|(qt, ours)| format!("  qt:   {qt}\n  ours: {ours}"))
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert_eq!(result.split(" | ").count(), qt.split(" | ").count());
}
