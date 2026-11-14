//! Slice R3 of the second QML engine audit (runtime built-in methods). Each fixture of
//! `runtime_audit_r3/` records what it sees in its `log`; its `.qt` file is the log Qt 6.12
//! gives (`runtime_audit_r3/qt.py`).
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/runtime_audit_r3")
}

/// Loads fixture `name` and runs its first 700 ms on a clock stepped by hand.
fn load(name: &str) -> (Runtime, Result<usize, String>) {
    let mut rt = Runtime::new(Some(dir()));
    let root = rt.load_file(&dir().join(format!("{name}.qml")));
    rt.settle();
    for t in 0..=700 {
        rt.poll_native();
        rt.tick(f64::from(t));
    }
    (rt, root)
}

fn qt(name: &str) -> String {
    std::fs::read_to_string(dir().join(format!("{name}.qt"))).unwrap()
}

/// The fixture's log is the one Qt gives.
fn same_as_qt(name: &str) {
    let (rt, root) = load(name);
    let log = rt.get(root.unwrap(), "log").to_js_string();
    assert_eq!(log, qt(name), "{name}; errors: {:?}", rt.errors());
}

/// R3-3: `restart()` of a running Timer notifies `running` false, then true.
#[test]
fn r3_03_timer_restart() {
    same_as_qt("R03TimerRestart");
}

/// R3-4: `click()` presses, toggles a checkable button, releases, then clicks; a disabled
/// button does nothing.
#[test]
fn r3_04_button_click() {
    same_as_qt("R04ButtonClick");
}

/// R3-5: enumerating a QML object lists its properties, signals and methods in meta-object
/// order; `JSON.stringify` writes the properties.
#[test]
fn r3_05_keys() {
    same_as_qt("R05Keys");
}

/// R3-6: a built-in method read as a value is a function.
#[test]
fn r3_06_method_value() {
    same_as_qt("R06MethodValue");
}

/// R3-6: Object.prototype is behind the object's own methods: quickshell's `PamResult` has
/// a `toString(result)` of its own.
#[test]
fn r3_06_own_method_before_object_prototype() {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Services.Pam\n\
             Item { property string s: PamResult.toString(PamResult.Failed) + ' ' + \
             typeof PamResult.hasOwnProperty }",
        )
        .unwrap();
    rt.settle();
    assert_eq!(rt.get(root, "s").to_js_string(), "Failed function");
}

/// R3-7: the point and rectangle overloads of `mapToItem`/`mapFromItem`, `mapToGlobal` and
/// `mapFromGlobal`; the results are `point` and `rect` values.
#[test]
fn r3_07_map() {
    same_as_qt("R07Map");
}

/// R3-8: an initial property the type does not have is not created.
#[test]
fn r3_08_initial_props() {
    same_as_qt("R08InitialProps");
}

/// R3-9: `destroy(delay)` deletes after `delay` milliseconds.
#[test]
fn r3_09_destroy_delay() {
    same_as_qt("R09DestroyDelay");
}

/// R3-10: hexadecimal enum values.
#[test]
fn r3_10_enum_hex() {
    same_as_qt("R10EnumHex");
}

/// R3-11: `Qt.createComponent` resolves a relative url against the calling document; `url`
/// is a url value.
#[test]
fn r3_11_create_component() {
    same_as_qt("R11CreateComponent");
}

/// R3-12: the component `Qt.createQmlObject` compiles is released with the object.
#[test]
fn r3_12_create_qml_object_template() {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            "import QtQuick\n\
             Item {\n\
             function make(n) { for (var i = 0; i < n; i++) \
             Qt.createQmlObject('import QtQuick; QtObject {}', this, 'dyn').destroy() }\n\
             function bad() { try { Qt.createQmlObject('import QtQuick; Nope {}', this, 'dyn') } \
             catch (e) { return 'threw' } }\n\
             }",
        )
        .unwrap();
    rt.settle();
    let live = |rt: &mut Runtime| {
        rt.settle();
        let mut w = rt.world.borrow_mut();
        let reaped = std::mem::take(&mut w.reaped);
        w.sweep_dead(&reaped);
        w.purge_dead();
        w.templates.live()
    };
    let before = live(&mut rt);
    rt.call_method(root, "make", vec![qml::value::Value::Num(300.0)])
        .unwrap()
        .unwrap();
    assert_eq!(live(&mut rt), before);
    let threw = rt.call_method(root, "bad", vec![]).unwrap().unwrap();
    assert_eq!(threw.to_js_string(), "threw");
    assert_eq!(live(&mut rt), before);
}

/// R3-13: an unknown type is a load error, as in Qt, with the line and the column.
#[test]
fn r3_13_unknown_type() {
    let (_, root) = load("R13UnknownType");
    let at = dir().join("R13UnknownType.qml");
    assert_eq!(
        root.unwrap_err(),
        format!("{}:3:5: Nope is not a type", at.display())
    );
    assert_eq!(
        qt("R13UnknownType"),
        "R13UnknownType.qml:3:5: Nope is not a type\n"
    );
}

/// R3-14: an object assigned to a property is wrapped in a Component only when the property
/// is of type Component, whatever its name.
#[test]
fn r3_14_component_wrap() {
    same_as_qt("R14ComponentWrap");
}

/// R3-15: Container's `setCurrentIndex` takes any index; `itemAt`, `addItem`, `insertItem`,
/// `moveItem`, `removeItem`, `takeItem` and `count`.
#[test]
fn r3_15_container() {
    same_as_qt("R15Container");
}

/// R3-16: StackView's `push`, `pop`, `replace`, `get`, `find` and the Qt 6.7 methods with
/// their argument forms.
#[test]
fn r3_16_stack_view() {
    same_as_qt("R16StackView");
}
