//! A view releasing the delegate whose Loader item is still being created: the item's type
//! names a lazy singleton, and creating that singleton resets the view's model (settings.qml
//! preloading its pages with `language.ui` set, Translation then rereading). The release must
//! not delete the object tree under construction, and its objects, never completed, get no
//! `Component.onDestruction`.
use qml::runtime::Runtime;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ri-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pump(rt: &mut Runtime, ms: u64) {
    let mut now = 0f64;
    for _ in 0..ms / 8 {
        rt.poll_native();
        now += 8.0;
        rt.tick(now);
    }
}

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
}

#[test]
fn model_reset_by_lazy_singleton_mid_creation() {
    let d = temp_dir("view-release-mid-creation");
    write(
        &d,
        &[
            (
                "qmldir",
                "singleton Store 1.0 Store.qml\nsingleton Lazy 1.0 Lazy.qml\n",
            ),
            (
                "Store.qml",
                "pragma Singleton\nimport QtQuick\nQtObject { property var pages: [\"a\", \"b\"]; property string out: \"\" }\n",
            ),
            (
                "Lazy.qml",
                "pragma Singleton\nimport QtQuick\nQtObject {\n    property int n: 1\n    Component.onCompleted: { Store.out += \"lazy;\"; Store.pages = [\"c\"] }\n}\n",
            ),
            (
                "Page.qml",
                "import QtQuick\nItem {\n    default property alias contentData: col.data\n    Column { id: col }\n}\n",
            ),
            (
                "Group.qml",
                "import QtQuick\nItem { property int n: Lazy.n }\n",
            ),
            (
                "Pg.qml",
                r#"import QtQuick
Page {
    id: pg
    Component.onCompleted: Store.out += "pg" + (parent ? parent.modelData : "-") + pg.children[0].children.length + ";"
    Component.onDestruction: Store.out += "pgd" + (parent ? parent.modelData : "-") + ";"
    Group {}
    Item {}
}
"#,
            ),
            (
                "main.qml",
                r#"import QtQuick
Item {
    id: root
    property bool go: false
    Repeater {
        id: rep
        model: Store.pages
        Loader {
            required property var modelData
            required property int index
            active: root.go && index === 0
            source: "Pg.qml"
            onLoaded: Store.out += "loaded" + modelData + ";"
            Component.onDestruction: Store.out += "ld" + modelData + ";"
        }
    }
    Component.onCompleted: {
        root.go = true;
        Store.out += "after:" + rep.count + ";";
        Qt.callLater(() => Store.out += "later:" + rep.count + ";");
    }
}
"#,
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons_lazy(std::slice::from_ref(&d));
    rt.load_file(&d.join("main.qml")).unwrap();
    pump(&mut rt, 400);
    let store = *rt.world.borrow().singletons.get("Store").unwrap();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    // Qt 6.12 (QQmlApplicationEngine, PySide6 6.12.0).
    assert_eq!(
        rt.get(store, "out").to_js_string(),
        "lazy;ldb;lda;pgc2;loadedc;after:1;later:1;"
    );
}

#[test]
fn model_reset_by_a_binding_mid_creation() {
    // The view's model is reset from a binding of the Loader item under construction, whose
    // settle builds no delegators of its own: the view still rebuilds inside the write, before
    // that creation returns, and the released item, never completed, gets no onDestruction.
    let d = temp_dir("view-reset-binding-mid-creation");
    write(
        &d,
        &[
            ("qmldir", "singleton Store 1.0 Store.qml\n"),
            (
                "Store.qml",
                r#"pragma Singleton
import QtQuick
QtObject {
    property var pages: ["a", "b"]
    property var log: []
    function reset() {
        if (log.indexOf("reset") < 0) {
            log.push("reset");
            pages = ["c"];
        }
        return 1;
    }
}
"#,
            ),
            (
                "Group.qml",
                "import QtQuick\nItem { property int n: Store.reset() }\n",
            ),
            (
                "Pg.qml",
                r#"import QtQuick
Item {
    Component.onCompleted: Store.log.push("pgc" + (parent ? parent.modelData : "-"))
    Component.onDestruction: Store.log.push("pgd" + (parent ? parent.modelData : "-"))
    Group {}
    Item {}
}
"#,
            ),
            (
                "main.qml",
                r#"import QtQuick
Item {
    id: root
    property bool go: false
    Repeater {
        id: rep
        model: Store.pages
        Loader {
            required property var modelData
            required property int index
            active: root.go && index === 0
            source: "Pg.qml"
            onLoaded: Store.log.push("loaded" + modelData)
            Component.onDestruction: Store.log.push("ld" + modelData)
        }
    }
    Component.onCompleted: {
        root.go = true;
        Store.log.push("after:" + rep.count);
        Qt.callLater(() => Store.log.push("later:" + rep.count));
    }
}
"#,
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons_lazy(std::slice::from_ref(&d));
    rt.load_file(&d.join("main.qml")).unwrap();
    pump(&mut rt, 400);
    let store = *rt.world.borrow().singletons.get("Store").unwrap();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    // Qt 6.12 (QQmlApplicationEngine, PySide6 6.12.0).
    assert_eq!(
        rt.get(store, "log").to_js_string(),
        "reset,ldb,lda,pgcc,loadedc,after:1,later:1"
    );
}

#[test]
fn a_view_inside_a_released_delegate_mid_creation() {
    // Each delegate holds a Repeater of its own, which gets another delegate right before
    // the delegates are released. A Repeater inside an object that waits for its deletion
    // builds nothing and is no work left to the settles inside the creation (they went on
    // for ever, the released delegates being kept until the creation returns); the released
    // delegates are deleted.
    let d = temp_dir("view-inside-released-delegate");
    write(
        &d,
        &[
            ("qmldir", "singleton Store 1.0 Store.qml\n"),
            (
                "Store.qml",
                r#"pragma Singleton
import QtQuick
QtObject {
    property var pages: ["a", "b"]
    property var log: []
    property bool alt: false
    function reset() {
        if (log.indexOf("reset") < 0) {
            log.push("reset");
            // Every delegate's Repeater gets another delegate, then the delegates are released.
            alt = true;
            pages = ["c"];
        }
        return 1;
    }
}
"#,
            ),
            (
                "Group.qml",
                "import QtQuick\nItem { property int n: Store.reset() }\n",
            ),
            (
                "Pg.qml",
                r#"import QtQuick
Item {
    Component.onCompleted: Store.log.push("pgc" + (parent ? parent.parent.modelData : "-"))
    Component.onDestruction: Store.log.push("pgd" + (parent ? parent.parent.modelData : "-"))
    Group {}
}
"#,
            ),
            (
                "main.qml",
                r#"import QtQuick
Item {
    id: root
    property bool go: false
    Component { id: one; Item {} }
    Component { id: other; Item {} }
    Repeater {
        id: rep
        model: Store.pages
        Item {
            id: page
            required property var modelData
            required property int index
            Loader {
                active: root.go && page.index === 0
                source: "Pg.qml"
                onLoaded: Store.log.push("loaded" + page.modelData)
            }
            Repeater {
                model: 0
                delegate: Store.alt ? other : one
            }
            Component.onDestruction: Store.log.push("ld" + modelData)
        }
    }
    Component.onCompleted: {
        root.go = true;
        Store.log.push("after:" + rep.count);
        Qt.callLater(() => Store.log.push("later:" + rep.count));
    }
}
"#,
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons_lazy(std::slice::from_ref(&d));
    rt.load_file(&d.join("main.qml")).unwrap();
    pump(&mut rt, 400);
    let store = *rt.world.borrow().singletons.get("Store").unwrap();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    // Qt 6.12 (QQmlApplicationEngine, PySide6 6.12.0).
    assert_eq!(
        rt.get(store, "log").to_js_string(),
        "reset,ldb,lda,pgcc,loadedc,after:1,later:1"
    );
    // The released delegates are gone: the one of "c" is the only one left. The object of a
    // delegate's row (QQmlDMListAccessorData) has a `modelData` too and is no delegate.
    let w = rt.world.borrow();
    let left: Vec<String> = (0..w.items.len())
        .filter(|i| !w.dead.contains(i))
        .filter(|&i| !w.items[i].types.iter().any(|t| t.starts_with("QQmlDM")))
        .filter_map(|i| w.items[i].props.get("modelData"))
        .map(|p| p.value.to_js_string())
        .collect();
    assert_eq!(left, ["c"]);
}

#[test]
fn settings_preload_with_russian_ui() {
    const TEST: &str = concat!(module_path!(), "::settings_preload_with_russian_ui");
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join(".config/rusty-impulse")).unwrap();
        std::fs::write(
            home.join(".config/rusty-impulse/config.json"),
            "{\"language\":{\"ui\":\"ru_RU\"}}\n",
        )
        .unwrap();
        testenv::run_in_own_process(TEST, testenv::home_vars(home));
        return;
    }
    let r = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut rt = Runtime::new(Some(r.clone()));
    let dirs: Vec<PathBuf> = [
        "modules/common",
        "modules/common/functions",
        "modules/common/utils",
        "services",
        "",
    ]
    .iter()
    .map(|d| r.join(d))
    .collect();
    rt.load_singletons_lazy(&dirs);
    let root = rt.load_file(&r.join("settings.qml")).unwrap();
    pump(&mut rt, 3000);
    let tr = *rt.world.borrow().singletons.get("Translation").unwrap();
    assert_eq!(rt.get(tr, "languageCode").to_js_string(), "ru_RU");
    let pages = match rt.world.borrow_mut().read(root, "pages") {
        Some(qml::value::Value::Obj(o)) => o.borrow().array.as_ref().map_or(0, |a| a.len()),
        _ => 0,
    };
    assert!(pages > 1, "settings.qml lists no pages");
    let w = rt.world.borrow();
    let loaded = (0..w.items.len())
        .filter(|&i| !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "Loader"))
        .filter(|&i| w.items[i].props.contains_key("pageIndex"))
        .filter(|&i| {
            matches!(
                w.items[i].props.get("item").map(|p| &p.value),
                Some(qml::value::Value::Item(_))
            )
        })
        .count();
    assert_eq!(loaded, pages, "not every settings page preloaded");
}
