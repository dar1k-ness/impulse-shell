//! A JS file a document imports (`import "x.js" as X`, not a `.pragma library`) runs once for
//! each instance of the document: its top-level scope, its functions and its namespace object
//! are that instance's. They go when the instance is destroyed. The scope holds the namespace
//! object and the namespace object's handler holds the scope's bindings, a cycle the
//! collector has to see: a Loader that makes the document again and again, as the desktop
//! does with its widgets on every edit of the layout, must not leave one context per round.
use qml::value::Value;

const LIB: &str = "var table = [{ n: 1 }, { n: 2 }]\n\
                   function twice(x) { return x * 2 }\n\
                   function sum() { return table.reduce((a, c) => a + c.n, 0) }\n";

const USES: &str = r#"import QtQuick
import "lib.js" as Lib
Item {
    property int n: Lib.twice(Lib.sum())
    property var row: Lib.table.find(c => c.n === 2)
}
"#;

const SHELL: &str = r#"import QtQuick
Item {
    id: root
    property bool on: true
    property int seen: loader.item ? loader.item.n + loader.item.row.n : -1
    Loader {
        id: loader
        active: root.on
        source: "Uses.qml"
    }
}
"#;

/// The host's maintenance between frames: destroyed instances swept, cycles collected.
fn tidy(rt: &mut qml::runtime::Runtime) {
    let mut w = rt.world.borrow_mut();
    let mut ids = std::mem::take(&mut w.reaped);
    ids.append(&mut w.reap_pending);
    w.sweep_dead(&ids);
    w.purge_dead();
    drop(w);
    qml::eval::collect_cycles();
}

#[test]
fn the_context_of_an_imported_js_file_goes_with_its_instance() {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in [("lib.js", LIB), ("Uses.qml", USES), ("shell.qml", SHELL)] {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let mut rt = qml::runtime::Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt
        .load_file(&dir.path().join("shell.qml"))
        .expect("scene loads");
    let seen = |rt: &mut qml::runtime::Runtime| match rt.world.borrow_mut().read(root, "seen") {
        Some(Value::Num(n)) => n,
        v => panic!("seen is {v:?}"),
    };
    let round = |rt: &mut qml::runtime::Runtime| {
        rt.set(root, "on", Value::Bool(false));
        assert_eq!(seen(rt), -1.0);
        rt.set(root, "on", Value::Bool(true));
        assert_eq!(seen(rt), 8.0);
        tidy(rt);
        qml::value::live_objects().len()
    };
    // What the first rounds leave is what one instance needs.
    round(&mut rt);
    let before = round(&mut rt);
    let after = (0..20).map(|_| round(&mut rt)).last().unwrap();
    assert_eq!(after, before, "JS objects left by 20 more instances");
}
