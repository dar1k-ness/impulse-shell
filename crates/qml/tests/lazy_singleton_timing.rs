//! When a lazy QML singleton is created: Qt 6.12 creates it on the first lookup of its name,
//! in a binding, a function or a handler alike, never before the file naming it is
//! instantiated. Expected orders are Qt 6.12's (PySide6 6.12.0, QQmlApplicationEngine).
use qml::runtime::Runtime;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ri-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

fn pump(rt: &mut Runtime, ms: u64) {
    let mut now = 0f64;
    for _ in 0..ms / 8 {
        rt.poll_native();
        now += 8.0;
        rt.tick(now);
    }
}

/// A singleton named only in the root's `Component.onCompleted` is created there, after the
/// Repeater's Loaders have loaded, so its model reset rebuilds them only then.
#[test]
fn singleton_named_only_in_on_completed_is_created_there() {
    let d = temp_dir("lazy-on-completed");
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
                "pragma Singleton\nimport QtQuick\nQtObject {\n    property int n: 1\n    Component.onCompleted: { Store.out += \"lazy;\"; Store.pages = [\"c\"]; Store.out += \"reset;\" }\n}\n",
            ),
            (
                "It.qml",
                "import QtQuick\nItem { Component.onCompleted: Store.out += \"it\" + parent.modelData + \";\" }\n",
            ),
            (
                "main.qml",
                r#"import QtQuick
Item {
    Repeater {
        model: Store.pages
        Loader {
            required property var modelData
            source: "It.qml"
            onLoaded: Store.out += "loaded" + modelData + ";"
        }
    }
    Component.onCompleted: { Store.out += "rootc;"; var x = Lazy.n; Store.out += "x" + x + ";" }
}
"#,
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons_lazy(std::slice::from_ref(&d));
    rt.load_file(&d.join("main.qml")).unwrap();
    pump(&mut rt, 100);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let store = *rt.world.borrow().singletons.get("Store").unwrap();
    assert_eq!(
        rt.get(store, "out").to_js_string(),
        "ita;loadeda;itb;loadedb;rootc;lazy;itc;loadedc;reset;x1;"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Each singleton is created by the first read of its name: in the binding that reads it, in
/// the function when it is called, through an import alias (`NS.Al`) and in a signal handler.
/// `Store.log` records into a closure, so the bindings calling it depend on nothing.
#[test]
fn singletons_are_created_by_the_first_read_of_their_name() {
    let d = temp_dir("lazy-first-read");
    let single = |n: &str, v: u32| {
        format!(
            "pragma Singleton\nimport QtQuick\nQtObject {{ property int n: {v}; Component.onCompleted: Store.log(\"{n}c\") }}\n"
        )
    };
    let (lazy, fun, al, hd) = (
        single("lazy", 1),
        single("fn", 2),
        single("al", 3),
        single("hd", 4),
    );
    write(
        &d,
        &[
            (
                "services/qmldir",
                "module ri.services\nsingleton Store 1.0 Store.qml\nsingleton Lazy 1.0 Lazy.qml\nsingleton Fn 1.0 Fn.qml\nsingleton Al 1.0 Al.qml\nsingleton Hd 1.0 Hd.qml\n",
            ),
            (
                "services/Store.qml",
                "pragma Singleton\nimport QtQuick\nQtObject { readonly property var log: (function () { var a = []; return function (s) { if (s === undefined) return a.join(\" \"); a.push(s) } })() }\n",
            ),
            ("services/Lazy.qml", &lazy),
            ("services/Fn.qml", &fun),
            ("services/Al.qml", &al),
            ("services/Hd.qml", &hd),
            (
                "main.qml",
                r#"import QtQuick
import ri.services
import ri.services as NS
Item {
    id: root
    property string got: ""
    function f() { return Fn.n }
    property int a: { Store.log("ba"); return 0 }
    Item { property int c: { Store.log("bc"); return 0 } Component.onCompleted: Store.log("c1") }
    Item { property int n: { Store.log("bn"); return Lazy.n } Component.onCompleted: Store.log("c2") }
    property int b: { Store.log("bb"); return 0 }
    property bool go: false
    onGoChanged: Store.log("hd" + Hd.n)
    Component.onCompleted: { Store.log("rootc"); Store.log("f" + f()); Store.log("ns" + NS.Al.n); go = true }
    Timer { interval: 50; running: true; onTriggered: root.got = Store.log() }
}
"#,
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons_lazy(&[d.clone(), d.join("services")]);
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    pump(&mut rt, 100);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(
        rt.get(root, "got").to_js_string(),
        "bn lazyc bc ba bb rootc fnc f2 alc ns3 hdc hd4 c2 c1"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Name lookups during a lazy singleton's creation (Qt 6.12, PySide6 6.12.0): the singleton
/// is visible by its own name, and to a singleton it creates in turn, from its first binding
/// on; a real ReferenceError in its `Component.onCompleted`, or in another object's handler
/// one of its writes runs, is reported.
#[test]
fn reference_errors_during_lazy_creation_are_reported() {
    let d = temp_dir("lazy-reference-errors");
    write(
        &d,
        &[
            (
                "qmldir",
                "singleton Store 1.0 Store.qml\nsingleton Lazy 1.0 Lazy.qml\nsingleton A 1.0 A.qml\nsingleton B 1.0 B.qml\n",
            ),
            (
                "Store.qml",
                "pragma Singleton\nimport QtQuick\nQtObject { property string out: \"\"; property int flag: 0 }\n",
            ),
            (
                "Lazy.qml",
                r#"pragma Singleton
import QtQuick
QtObject {
    property int n: 1
    property int m: Lazy.n + 1
    onMChanged: Store.out += "lm" + Lazy.m + ";"
    Component.onCompleted: { Store.out += "lazyc" + Lazy.m + ";"; Store.flag = 1; missingFn() }
}
"#,
            ),
            (
                "A.qml",
                "pragma Singleton\nimport QtQuick\nQtObject {\n    property int x: 5\n    property int y: B.n\n    Component.onCompleted: Store.out += \"ac\" + y + \";\"\n}\n",
            ),
            (
                "B.qml",
                "pragma Singleton\nimport QtQuick\nQtObject {\n    property int n: A.x * 2\n    Component.onCompleted: Store.out += \"bc\" + A.x + \",\" + n + \";\"\n}\n",
            ),
            (
                "main.qml",
                r#"import QtQuick
Item {
    property int v: Lazy.n
    property int f: Store.flag
    onFChanged: { Store.out += "f" + f + ";"; if (f === 1) otherMissing() }
    property int w: A.y
    Component.onCompleted: Store.out += "rootc;"
}
"#,
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons_lazy(std::slice::from_ref(&d));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    pump(&mut rt, 16);
    let store = *rt.world.borrow().singletons.get("Store").unwrap();
    assert_eq!(
        rt.get(store, "out").to_js_string(),
        "lm2;lazyc2;f1;bc5,10;ac10;rootc;"
    );
    assert_eq!(rt.get(root, "v").to_number(), 1.0);
    assert_eq!(rt.get(root, "w").to_number(), 10.0);
    assert_eq!(
        rt.errors(),
        [
            "Lazy.Component.onCompleted: missingFn is not defined",
            "Item.onFChanged: otherMissing is not defined",
        ]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Eager loading creates singletons in dependency order (B before A, which names it), but one
/// read before its turn (A, by B's binding in this cycle) is created by that read, as in Qt
/// with B read first: A's handler sees B registered before its bindings ran (`ac0`), no
/// ReferenceError for either name, and a real one is reported.
#[test]
fn eager_loading_creates_a_singleton_read_before_its_turn() {
    let d = temp_dir("eager-cycle");
    write(
        &d,
        &[
            (
                "qmldir",
                "singleton Store 1.0 Store.qml\nsingleton A 1.0 A.qml\nsingleton B 1.0 B.qml\n",
            ),
            (
                "Store.qml",
                "pragma Singleton\nimport QtQuick\nQtObject { property string out: \"\" }\n",
            ),
            (
                "A.qml",
                "pragma Singleton\nimport QtQuick\nQtObject {\n    property int x: 5\n    Component.onCompleted: { var n = B.n; Store.out += \"ac\" + n + \";\"; missingA() }\n}\n",
            ),
            (
                "B.qml",
                "pragma Singleton\nimport QtQuick\nQtObject {\n    property int n: A.x * 2\n    Component.onCompleted: Store.out += \"bc\" + A.x + \";\"\n}\n",
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(std::slice::from_ref(&d));
    let store = *rt.world.borrow().singletons.get("Store").unwrap();
    assert_eq!(rt.get(store, "out").to_js_string(), "ac0;bc5;");
    assert_eq!(
        rt.errors(),
        ["A.Component.onCompleted: missingA is not defined"]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// A singleton first read by a binding and creating objects as it starts (the original's Notifications
/// builds its wrappers in `onLoaded`) leaves that binding an ordinary one: it keeps its
/// dependencies and follows later changes, as in Qt, where those objects are the singleton's.
#[test]
fn binding_creating_a_singleton_that_creates_objects_keeps_its_dependencies() {
    let d = temp_dir("lazy-binding-deps");
    write(
        &d,
        &[
            ("qmldir", "singleton Lazy 1.0 Lazy.qml\n"),
            (
                "Lazy.qml",
                "pragma Singleton\nimport QtQuick\nQtObject {\n    id: root\n    property list<QtObject> list: []\n    property Component c: Component { QtObject {} }\n    function clear() { root.list = [] }\n    Component.onCompleted: root.list = [c.createObject(root)]\n}\n",
            ),
            (
                "main.qml",
                "import QtQuick\nItem {\n    property int count: Lazy.list.length\n    function clear() { Lazy.clear() }\n}\n",
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons_lazy(std::slice::from_ref(&d));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    assert_eq!(rt.get(root, "count").to_number(), 1.0);
    rt.call_method(root, "clear", vec![]).unwrap().unwrap();
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(rt.get(root, "count").to_number(), 0.0);
    let _ = std::fs::remove_dir_all(&d);
}

/// A singleton first read by a binding that a write re-evaluates is created inside that update,
/// but its own handlers' writes still update the bindings depending on them inside the write
/// (Qt's QQmlNotifier): `seen = d` right after `n = 5` reads the new `d`.
#[test]
fn singleton_created_in_a_binding_update_updates_its_bindings_inside_its_writes() {
    let d = temp_dir("lazy-in-update");
    write(
        &d,
        &[
            ("qmldir", "singleton Lazy 1.0 Lazy.qml\n"),
            (
                "Lazy.qml",
                "pragma Singleton\nimport QtQuick\nQtObject {\n    property int n: 0\n    property int d: n * 2\n    property int seen: -1\n    Component.onCompleted: { n = 5; seen = d }\n}\n",
            ),
            (
                "main.qml",
                "import QtQuick\nItem {\n    property bool on: false\n    property int v: on ? Lazy.seen : -2\n    function go() { on = true }\n}\n",
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons_lazy(std::slice::from_ref(&d));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    pump(&mut rt, 16);
    assert_eq!(rt.get(root, "v").to_number(), -2.0);
    rt.call_method(root, "go", vec![]).unwrap().unwrap();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(rt.get(root, "v").to_number(), 10.0);
    let _ = std::fs::remove_dir_all(&d);
}
