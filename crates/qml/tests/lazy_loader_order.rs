//! Quickshell reloads a LazyLoader created after the root completed from a zero timer its
//! `Reloadable::componentComplete` starts, and loads it then when its `active` is already
//! true; Qt completes the objects of a component last created first. The
//! panels of a family (the original's `panelFamilies/*.qml`: a Scope of `PanelLoader`s whose `active`
//! is true from the start) therefore load last declared first, which decides whose
//! `IpcHandler` gets a target two panels declare. Order as quickshell logs it for the same
//! document (`crates/shell/tests/ipc_duplicate_target_equiv.rs` runs both).
use qml::runtime::Runtime;
use qml::value::Value;

const DOC: &str = r#"import QtQuick
import Rustshell
Item {
    id: root
    property bool ready: false
    property string log: ""
    function go() { ready = true }
    component PanelLoader: LazyLoader {
        property bool extraCondition: true
        active: root.ready && extraCondition
    }
    component Panel: QtObject {
        property string name
        Component.onCompleted: root.log += name + " "
    }
    component Family: Scope {
        PanelLoader { component: Panel { name: "startMenu" } }
        PanelLoader { extraCondition: false; component: Panel { name: "off" } }
        PanelLoader { component: Panel { name: "sessionScreen" } }
        PanelLoader { component: Panel { name: "taskView" } }
    }
    LazyLoader {
        active: root.ready
        component: Family {}
    }
}"#;

fn loaded() -> (Runtime, usize) {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.qml");
    std::fs::write(&main, DOC).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&main).unwrap();
    rt.settle();
    assert_eq!(rt.get(root, "log").to_js_string(), "");
    (rt, root)
}

/// The panels loaded after the next turn of the event loop, none before it.
fn order(rt: &mut Runtime, root: usize) -> String {
    rt.settle();
    assert_eq!(rt.get(root, "log").to_js_string(), "");
    rt.run_later();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    rt.get(root, "log").to_js_string()
}

#[test]
fn loaders_active_from_the_start_load_last_declared_first() {
    let (mut rt, root) = loaded();
    rt.set(root, "ready", Value::Bool(true));
    assert_eq!(order(&mut rt, root), "taskView sessionScreen startMenu ");
}

/// The family switched on by an assignment in a handler (the original: `Config.ready = true` in a
/// FileView's `onLoaded`), which loads it inside the setter.
#[test]
fn loaders_made_inside_a_setter_load_last_declared_first() {
    let (mut rt, root) = loaded();
    rt.call_method(root, "go", Vec::new()).unwrap().unwrap();
    assert_eq!(order(&mut rt, root), "taskView sessionScreen startMenu ");
}
