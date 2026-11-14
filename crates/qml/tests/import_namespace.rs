//! `import X as NS` qualifies module X only (QQmlTypeNamespace): `NS.Other` is undefined for a
//! singleton of another module and does not create it, the qualifier exists only in documents
//! that declare it, and its members are not enumerable. Expected values are Qt 6.12's (PySide6
//! 6.12.0, QQmlApplicationEngine with `qs/a`, `qs/b` modules).
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

/// Module `qs.a` with singleton `S`, module `qs.b` with singleton `Other`; `main.qml` imports
/// `qs.a as A` and `qs.b`, `NoAlias.qml` neither.
fn modules(d: &Path) {
    write(
        d,
        &[
            ("a/qmldir", "module ri.a\nsingleton S 1.0 S.qml\n"),
            (
                "a/S.qml",
                "pragma Singleton\nimport QtQuick\nQtObject { property int n: 1 }\n",
            ),
            ("b/qmldir", "module ri.b\nsingleton Other 1.0 Other.qml\n"),
            (
                "b/Other.qml",
                "pragma Singleton\nimport QtQuick\nQtObject { property int n: 2 }\n",
            ),
            (
                "NoAlias.qml",
                r#"import QtQuick
QtObject {
    function probe() {
        try { return typeof A + " " + A } catch (e) { return typeof A + " " + e.name }
    }
}
"#,
            ),
            (
                "main.qml",
                r#"import QtQuick
import ri.a as A
import ri.b
QtObject {
    property var na: NoAlias {}
    property string other: ""
    property string keys: ""
    property string s: ""
    property string noAlias: ""
    Component.onCompleted: {
        other = typeof A.Other + " " + A.Other
        keys = JSON.stringify(Object.keys(A)) + " " + ("S" in A) + " " + ("Other" in A)
        s = typeof A.S + " " + A.S.n + " " + (A.S === A.S)
        noAlias = na.probe()
    }
}
"#,
            ),
        ],
    );
}

fn check(rt: &mut Runtime, root: usize) {
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(rt.get(root, "other").to_js_string(), "undefined undefined");
    assert_eq!(rt.get(root, "keys").to_js_string(), "[] true false");
    assert_eq!(rt.get(root, "s").to_js_string(), "object 1 true");
    assert_eq!(
        rt.get(root, "noAlias").to_js_string(),
        "undefined ReferenceError"
    );
}

#[test]
fn namespace_has_only_its_modules_lazy_singletons() {
    let d = temp_dir("ns-lazy");
    modules(&d);
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons_lazy(&[d.join("a"), d.join("b")]);
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    check(&mut rt, root);
    let w = rt.world.borrow();
    assert!(w.singletons.contains_key("S"));
    assert!(
        !w.singletons.contains_key("Other"),
        "reading A.Other created qs.b's Other"
    );
    drop(w);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn namespace_has_only_its_modules_created_singletons() {
    let d = temp_dir("ns-eager");
    modules(&d);
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[d.join("a"), d.join("b")]);
    assert!(rt.world.borrow().singletons.contains_key("Other"));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    check(&mut rt, root);
    let _ = std::fs::remove_dir_all(&d);
}

/// The `qmldir` of a directory imported by its path (`import "q"`) names its types
/// (QQmlImportInstance::resolveType over QQmlTypeLoaderQmldirContent): `singleton Renamed
/// Uno.qml` is a singleton under both names, `Other 1.0 Plain.qml` a type under both,
/// `internal Hidden Hid.qml` a type only for the files of that directory, a file the `qmldir`
/// does not list a type under its own name, and a `singleton` whose file has no `pragma
/// Singleton` makes every document importing the directory fail, used or not.
///
/// Each `tests/data/qmldir_names/<name>.qml` is a document; `qt.txt` has its name, a tab and
/// what Qt 6.12 gives (PySide6 6.12.0, QQmlComponent): the document's `result`, or `!` and
/// the descriptions of QQmlComponent::errors() joined by ` ## `.
#[test]
fn a_directory_qmldir_names_its_types() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/qmldir_names");
    let qt = std::fs::read_to_string(data.join("qt.txt")).unwrap();
    let mut wrong = Vec::new();
    for (name, qt) in qt.lines().map(|l| l.split_once('\t').unwrap()) {
        let f = data.join(format!("{name}.qml"));
        let mut rt = Runtime::new(Some(data.clone()));
        let ours = match rt.load_file(&f) {
            Ok(root) => {
                rt.settle();
                assert_eq!(rt.errors(), Vec::<String>::new(), "{name}");
                rt.get(root, "result").to_js_string()
            }
            // Before the first description comes the document's path and the line.
            Err(e) => {
                let e = e.replace('\n', " ## ");
                let at = qt.split(" ## ").next().unwrap().trim_start_matches('!');
                let from = e.find(at).unwrap_or(0);
                format!("!{}", &e[from..])
            }
        };
        if qt != ours {
            wrong.push(format!("{name}\n  qt:   {qt}\n  ours: {ours}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A directory with no `qmldir`, imported by its path: its files with `pragma Singleton` are
/// singletons, as in Quickshell 0.3.2, which writes the `qmldir` of such a directory itself
/// (one `singleton` line for each of those files). Qt 6.12 alone registers no singleton there:
/// the name is an object with no members, and `Solo {}` fails with another message.
///
/// `tests/data/no_qmldir/quickshell.txt` has a document's name, a tab, what Quickshell 0.3.2
/// gives (`qs -p` on a `ShellRoot` with the same `cases`), a tab and what Qt 6.12 gives
/// (PySide6 6.12.0, QQmlComponent): the `result`, or `!` and the error.
#[test]
fn a_directory_without_qmldir_has_the_singletons_quickshell_gives_it() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/no_qmldir");
    let text = std::fs::read_to_string(data.join("quickshell.txt")).unwrap();
    let mut wrong = Vec::new();
    for line in text.lines() {
        let mut cols = line.split('\t');
        let (name, quickshell) = (cols.next().unwrap(), cols.next().unwrap());
        let mut rt = Runtime::new(Some(data.clone()));
        let ours = match rt.load_file(&data.join(format!("{name}.qml"))) {
            Ok(root) => {
                rt.settle();
                assert_eq!(rt.errors(), Vec::<String>::new(), "{name}");
                rt.get(root, "result").to_js_string()
            }
            Err(e) => {
                let at = quickshell.trim_start_matches('!');
                format!("!{}", &e[e.find(at).unwrap_or(0)..])
            }
        };
        if quickshell != ours {
            wrong.push(format!(
                "{name}\n  rustshell: {quickshell}\n  ours:       {ours}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
