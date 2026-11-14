//! Explicit imports take precedence over the implicit import of a file's own directory (Qt 6.12:
//! `widgets/CalendarWidget.qml` with `import ri.m.calendar` and `CalendarWidget {}` wraps the
//! imported type; a sibling `Foo.qml` loses to the imported `Foo`).
use qml::runtime::Runtime;

fn put(d: &std::path::Path, rel: &str, src: &str) {
    let p = d.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, src).unwrap();
}

#[test]
fn explicit_imports_shadow_the_files_own_directory() {
    let d = std::env::temp_dir().join(format!("import-precedence-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    put(
        &d,
        "m/calendar/CalendarWidget.qml",
        "import QtQuick\nItem { implicitHeight: 42 }\n",
    );
    put(
        &d,
        "m/calendar/Foo.qml",
        "import QtQuick\nItem { implicitHeight: 7 }\n",
    );
    put(
        &d,
        "m/widgets/Foo.qml",
        "import QtQuick\nItem { implicitHeight: 99 }\n",
    );
    put(
        &d,
        "m/widgets/CalendarWidget.qml",
        "import QtQuick\nimport ri.m.calendar\nItem {\n    implicitHeight: content.implicitHeight\n    CalendarWidget { id: content }\n}\n",
    );
    put(
        &d,
        "m/widgets/UsesFoo.qml",
        "import QtQuick\nimport ri.m.calendar\nItem {\n    implicitHeight: f.implicitHeight\n    Foo { id: f }\n}\n",
    );
    put(
        &d,
        "m/widgets/UsesLocal.qml",
        "import QtQuick\nItem {\n    implicitHeight: f.implicitHeight\n    Foo { id: f }\n}\n",
    );
    put(
        &d,
        "main.qml",
        "import QtQuick\nimport ri.m.widgets\nItem {\n    property real cal: a.implicitHeight\n    property real imported: b.implicitHeight\n    property real local: c.implicitHeight\n    CalendarWidget { id: a }\n    UsesFoo { id: b }\n    UsesLocal { id: c }\n}\n",
    );
    let mut rt = Runtime::new(Some(d.clone()));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut w = rt.world.borrow_mut();
    assert_eq!(w.read(root, "cal").unwrap_or_default().to_js_string(), "42");
    assert_eq!(
        w.read(root, "imported").unwrap_or_default().to_js_string(),
        "7"
    );
    assert_eq!(
        w.read(root, "local").unwrap_or_default().to_js_string(),
        "99"
    );
    drop(w);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
