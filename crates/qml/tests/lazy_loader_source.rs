//! Quickshell's `LazyLoader.source`: a LazyLoader loads the file its `source` names (relative
//! to the file declaring it) once `active`, as the original's family loads ri-only files.
use qml::runtime::Runtime;
use qml::value::Value;

#[test]
fn lazy_loader_loads_its_source_when_active() {
    let d = std::env::temp_dir().join(format!("lazy-loader-source-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(
        d.join("sub/Child.qml"),
        "import QtQuick\nQtObject { property int answer: 42 }\n",
    )
    .unwrap();
    std::fs::write(
        d.join("main.qml"),
        "import QtQuick\nimport Quickshell\nItem {\n    id: root\n    property bool on: false\n    property var loader: l\n    LazyLoader { id: l; active: root.on; source: Qt.resolvedUrl(\"sub/Child.qml\") }\n}\n",
    )
    .unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let loader = match rt.world.borrow_mut().read(root, "loader") {
        Some(Value::Item(i)) => i,
        v => panic!("{v:?}"),
    };
    let item = |rt: &Runtime| rt.world.borrow_mut().read(loader, "item");
    assert!(!matches!(item(&rt), Some(Value::Item(_))));
    rt.world.borrow_mut().write(root, "on", Value::Bool(true));
    rt.settle();
    let Some(Value::Item(c)) = item(&rt) else {
        panic!("not loaded: {:?}", item(&rt))
    };
    assert_eq!(
        rt.world
            .borrow_mut()
            .read(c, "answer")
            .map(|v| v.to_number()),
        Some(42.0)
    );
}
