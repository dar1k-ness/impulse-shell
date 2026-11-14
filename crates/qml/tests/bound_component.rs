//! `pragma ComponentBehavior: Bound`: an inline Component's objects resolve the declaring
//! document's ids before their own properties (waffle StartMenuContent's
//! `Component { SearchPageContent { context: context } }` with `StartMenuContext { id: context }`).
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use qml::value::Value;

fn run(pragma: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!(
        "ri-bound-{}-{}",
        std::process::id(),
        pragma.is_empty()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let qml = format!(
        "{pragma}\nimport QtQuick\n\
         Item {{\n\
         id: root\n\
         component Page: Item {{ id: page; property alias context: inner.context; property var cats: inner.context ? inner.context.categories : null\n\
             Item {{ id: inner; property QtObject context }} }}\n\
         QtObject {{ id: context; property var categories: [\"All\"] }}\n\
         property bool set: loader.item ? loader.item.context === context : false\n\
         property string cats: loader.item ? JSON.stringify(loader.item.cats) : \"\"\n\
         Loader {{ id: loader; sourceComponent: comp }}\n\
         Component {{ id: comp; Page {{ context: context }} }}\n\
         }}"
    );
    std::fs::write(dir.join("shell.qml"), qml).unwrap();
    let mut rt = Runtime::new(Some(dir.clone()));
    let r = rt.load_file(&dir.join("shell.qml")).unwrap();
    rt.settle();
    let set = matches!(rt.get(r, "set"), Value::Bool(true));
    let cats = rt.get(r, "cats").to_js_string();
    std::fs::remove_dir_all(&dir).unwrap();
    (set, cats)
}

#[test]
fn bound_component_sees_document_ids_first() {
    assert_eq!(
        run("pragma ComponentBehavior: Bound"),
        (true, "[\"All\"]".to_string())
    );
}

#[test]
fn unbound_component_keeps_scope_properties_first() {
    // Qt: without Bound the scope object's own `context` (the alias, null) wins.
    assert_eq!(run(""), (false, "null".to_string()));
}
