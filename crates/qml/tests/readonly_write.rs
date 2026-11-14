//! A JavaScript write to a property that is not writable throws QV4's
//! `TypeError: Cannot assign to read-only property "x"` whatever the strictness or
//! assignment form, and leaves the value unchanged (Qt 6.12, PySide6 probe).
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

fn run(body: &str) -> String {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(&format!("{PRE}{body}")).unwrap();
    rt.call_method(root, "run", vec![]).unwrap().unwrap();
    rt.get(root, "log").to_js_string()
}

const PRE: &str = r#"import QtQuick
Item {
    id: root
    property string log: ""
    function t(label, fn) {
        try { fn(); log += label + ":ok\n" } catch (e) { log += label + ":" + e.name + ": " + e.message + "\n" }
    }
"#;

#[test]
fn cpp_property_without_write_is_refused() {
    let log = run(r#"
    Item { width: 5; height: 5 }
    function run() {
        t("member", () => { root.childrenRect = Qt.rect(0, 0, 1, 1) })
        t("unqualified", () => { childrenRect = Qt.rect(0, 0, 1, 1) })
        t("strict", () => { "use strict"; root.childrenRect = root.childrenRect })
        t("compound", () => { root.childrenRect += 1 })
        t("update", () => { root.activeFocus++ })
        t("Reflect.set", () => { Reflect.set(root, "activeFocus", true) })
        t("Object.assign", () => { Object.assign(root, { activeFocus: true }) })
        log += root.childrenRect.width + " " + root.activeFocus + "\n"
    }
}"#);
    assert_eq!(
        log,
        "member:TypeError: Cannot assign to read-only property \"childrenRect\"\n\
         unqualified:TypeError: Cannot assign to read-only property \"childrenRect\"\n\
         strict:TypeError: Cannot assign to read-only property \"childrenRect\"\n\
         compound:TypeError: Cannot assign to read-only property \"childrenRect\"\n\
         update:TypeError: Cannot assign to read-only property \"activeFocus\"\n\
         Reflect.set:TypeError: Cannot assign to read-only property \"activeFocus\"\n\
         Object.assign:TypeError: Cannot assign to read-only property \"activeFocus\"\n\
         5 false\n"
    );
}

#[test]
fn singleton_property_without_write_is_refused() {
    let log = run(r#"
    function run() {
        t("visible", () => { Qt.inputMethod.visible = true })
        t("keyboardRectangle", () => { Qt.inputMethod.keyboardRectangle = Qt.rect(0, 0, 1, 1) })
        t("state", () => { Qt.application.state = 0 })
        t("name", () => { Qt.application.name = "x" })
        log += Qt.inputMethod.visible + " " + Qt.inputMethod.keyboardRectangle.width + " " + Qt.application.name + "\n"
    }
}"#);
    assert_eq!(
        log,
        "visible:TypeError: Cannot assign to read-only property \"visible\"\n\
         keyboardRectangle:TypeError: Cannot assign to read-only property \"keyboardRectangle\"\n\
         state:TypeError: Cannot assign to read-only property \"state\"\n\
         name:ok\n\
         false 0 x\n"
    );
}

/// A `readonly property`, and an alias to a property that is not writable, are refused the
/// same way; writable properties, list properties (assigned by clear + append) and in-place
/// changes of a read-only property's value are not.
#[test]
fn qml_readonly_and_alias_are_refused() {
    let log = run(r#"
    readonly property int ro: 3
    readonly property var bag: [1]
    property alias rect: root.childrenRect
    property alias alro: root.ro
    property int w: 1
    Item { width: 5; height: 5 }
    function run() {
        t("ro", () => { root.ro = 4 })
        t("ro unqualified", () => { ro = 5 })
        t("alias", () => { root.rect = Qt.rect(0, 0, 1, 1) })
        t("alias readonly", () => { root.alro = 9 })
        t("bag.push", () => { bag.push(2) })
        t("w", () => { root.w = 2 })
        t("children", () => { root.children = [] })
        log += ro + " " + alro + " " + bag.length + " " + w + "\n"
    }
}"#);
    assert_eq!(
        log,
        "ro:TypeError: Cannot assign to read-only property \"ro\"\n\
         ro unqualified:TypeError: Cannot assign to read-only property \"ro\"\n\
         alias:TypeError: Cannot assign to read-only property \"rect\"\n\
         alias readonly:TypeError: Cannot assign to read-only property \"alro\"\n\
         bag.push:ok\n\
         w:ok\n\
         children:ok\n\
         3 3 2 2\n"
    );
}
