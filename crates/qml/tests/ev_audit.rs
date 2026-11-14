//! Findings EV of the second QML engine audit that need files Qt refuses or the engine's own
//! state: script directives, a `.pragma library` that throws, the template map. Expected
//! values are Qt 6.12's (PySide6). The other EV findings are cases of `crates/rigen/check`,
//! which runs them interpreted and compiled.
#![cfg(feature = "interp")]

use qml::eval::{ScriptDirectives, ScriptImport, script_directives};
use qml::runtime::{Runtime, Sym};
use qml::value::Value;

/// Loads `main.qml` beside `files`: its root's `log` and the engine's errors.
fn run(files: &[(&str, &str)], main: &str) -> (String, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    for (name, src) in files {
        std::fs::write(dir.path().join(name), src).unwrap();
    }
    let f = dir.path().join("main.qml");
    std::fs::write(&f, main).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    let prefix = format!("{}/", dir.path().canonicalize().unwrap().display());
    let errors = rt.errors().iter().map(|e| e.replace(&prefix, "")).collect();
    (rt.get(root, "log").to_js_string(), errors)
}

/// EV-13: directives are the ones before the first token of the program
/// (QQmlJS::Lexer::scanDirectives).
#[test]
fn script_directives_come_before_the_program() {
    let file = |f, alias| (ScriptImport::File(f), alias);
    let module = |m, alias| (ScriptImport::Module(m), alias);
    let read = |src| script_directives(src).map(|d| (d.library, d.imports));
    for (src, library, imports) in [
        ("var c = 1", false, vec![]),
        (
            "/* how to use:\n.import \"Nope.js\" as Z\n*/\nvar c = 1",
            false,
            vec![],
        ),
        (
            "// .import \"Nope2.js\" as Z\nvar s = '\\\n.import \"Nope3.js\" as Y'",
            false,
            vec![],
        ),
        (
            "/* a leading comment */ .pragma library\n// another\n  .import \"C.js\" as CC\nvar e",
            true,
            vec![file("C.js", "CC")],
        ),
        (
            ".import QtQuick as QQ\n.import QtQml 2.15 as QM\n.import QtQuick.Controls 2 as C\n",
            false,
            vec![
                module("QtQuick", "QQ"),
                module("QtQml", "QM"),
                module("QtQuick.Controls", "C"),
            ],
        ),
        // A chained call is no directive.
        ("var f = [1]\n.concat([2])", false, vec![]),
    ] {
        assert_eq!(read(src), Ok((library, imports)), "{src}");
    }
    // The program is what follows the last directive.
    let src = ".pragma library\n.import \"a.js\" as A\nvar x";
    let d: ScriptDirectives = script_directives(src).unwrap();
    assert_eq!(&src[d.end..], "\nvar x");
    for (src, error) in [
        (
            ".import \"C.js\" as lower\nvar g = 1",
            "line 1: Invalid import qualifier",
        ),
        (
            ".import \"C.js\"\nvar i = 1",
            "line 1: File import requires a qualifier",
        ),
        (
            "\n.import QtQuick\nvar i = 1",
            "line 2: Module import requires a qualifier",
        ),
        (
            ".import \"C.txt\" as C",
            "line 1: Imported file must be a script",
        ),
        (".pragma strict", "line 1: Syntax error"),
    ] {
        assert_eq!(read(src), Err(error.to_string()), "{src}");
    }
}

const USE: &str = r#"import QtQuick
import "Lib.js" as Lib
QtObject {
    property string result: {
        try { return JSON.stringify([Lib.v, Object.keys(Lib)]) } catch (e) { return "THROWS " + e.name }
    }
}
"#;

/// Loads `Use.qml`, which imports `lib` as `Lib.js`, from a Loader.
fn import_lib(lib: &str, others: &[(&str, &str)]) -> (String, Vec<String>) {
    let main = r#"import QtQuick
Item {
    property string log: ""
    Loader { id: l; source: "Use.qml" }
    Component.onCompleted: log = l.item ? l.item.result : "LOADFAIL"
}
"#;
    let mut files = vec![("Lib.js", lib), ("Use.qml", USE)];
    files.extend(others);
    run(&files, main)
}

/// EV-13: a `.import` in a comment or a string imports nothing, so the file loads (the
/// audit's fixture H, files C, D and E).
#[test]
fn import_in_a_comment_is_no_import() {
    let c = "/* how to use:\n.import \"Nope.js\" as Z\n*/\nvar v = 1\n";
    assert_eq!(import_lib(c, &[]), (r#"[1,["v"]]"#.to_string(), vec![]));
    let d = "// .import \"Nope2.js\" as Z\nvar s = '\\\n.import \"Nope3.js\" as Y'\nvar v = 2\n";
    assert_eq!(import_lib(d, &[]), (r#"[2,["s","v"]]"#.to_string(), vec![]));
    let e = "/* a leading comment */ .pragma library\n// another\n  .import \"C.js\" as CC\n\
             var v = CC.v + 2\nfunction shared() { return typeof v }\n";
    assert_eq!(
        import_lib(e, &[("C.js", c)]),
        (r#"[3,["shared","v"]]"#.to_string(), vec![])
    );
}

/// EV-13: a file with a directive after code, an ill-formed one or one naming a missing file
/// is not imported, and the engine says why (Qt 6.12 gives the importing component no script
/// either: "Script ... unavailable").
#[test]
fn ill_formed_directive_is_an_error() {
    for (lib, error) in [
        (
            "var f = 1\n.import \"C.js\" as LateC\nvar v = typeof LateC\n",
            "Lib.js: ",
        ),
        (
            ".import \"C.js\" as lower\nvar v = 1\n",
            "Lib.js: line 1: Invalid import qualifier",
        ),
        (".import \"Missing.js\" as M\nvar v = 1\n", "Missing.js: "),
        (
            ".import \"C.js\"\nvar v = typeof C\n",
            "Lib.js: line 1: File import requires a qualifier",
        ),
    ] {
        let (log, errors) = import_lib(lib, &[("C.js", "var v = 1\n")]);
        assert_eq!(log, "THROWS ReferenceError", "{lib}");
        assert_eq!(errors.len(), 1, "{lib}: {errors:?}");
        assert!(errors[0].starts_with(error), "{lib}: {errors:?}");
    }
}

/// EV-14: a `.pragma library` that throws while it runs is imported by everyone with what it
/// declared and ran until then, and so is a file that is no library; the error is reported
/// (the audit's fixture I).
#[test]
fn library_that_throws_keeps_what_ran() {
    let bad = ".pragma library\nvar before = 1\nfunction early() { return before }\nnull.x\n\
               var after = 2\nfunction late() { return 1 }\n";
    let bad2 = "var before = 1\nfunction early() { return before }\nundefinedFn()\nvar after = 2\n";
    let child = r#"import QtQuick
import "Bad.js" as Bad
import "Bad2.js" as Bad2
QtObject {
    property string result: {
        try { return [typeof Bad, Bad.before, Bad.after, typeof Bad.early, typeof Bad.late, Object.keys(Bad).sort(), typeof Bad2, Bad2.before, Bad2.after, typeof Bad2.early].join(",") } catch (e) { return "THROWS " + e.name }
    }
}
"#;
    let main = r#"import QtQuick
Item {
    property string log: ""
    Loader { id: l1; source: "Child.qml" }
    Loader { id: l2; source: "Child.qml" }
    Component.onCompleted: log = l1.item.result + "\n" + l2.item.result
}
"#;
    let (log, errors) = run(
        &[("Bad.js", bad), ("Bad2.js", bad2), ("Child.qml", child)],
        main,
    );
    let qt = "object,1,,function,function,after,before,early,late,object,1,,function";
    assert_eq!(log, format!("{qt}\n{qt}"));
    // The library ran once, the other file once for each import.
    assert_eq!(
        errors,
        [
            "Bad.js: Cannot read property 'x' of null",
            "Bad2.js: undefinedFn is not defined",
            "Bad2.js: undefinedFn is not defined",
        ]
    );
}

/// EV-16: the template object of a tagged template goes with the code holding the template:
/// each `new Function` parses its own (the audit's fixture K kept 1001 after 1000 of them).
#[test]
fn template_map_drops_the_sites_of_dropped_code() {
    let mut it = qml::eval::Interp::default();
    let g = it.global.clone();
    let mut run = |src: &str| {
        let body = qml::ir::lower_program(src).unwrap();
        assert!(it.run(&body, &g).is_ok());
        let realm = it.realm();
        let templates = realm.borrow().templates.as_ref().map_or(0, |t| t.len());
        (g.borrow().vars["result"].to_js_string(), templates)
    };
    let functions = "var same = []\n\
                     for (var i = 0; i < 1000; i++) { var f = new Function('t', 'return t`a${1}b`'); \
                     same.push(f(tag) === f(tag)) }\n\
                     var result = same.every(Boolean) && kept() === first";
    let first = format!(
        "function tag(s) {{ return s }}\nfunction kept() {{ return tag`x${{1}}y` }}\n\
         var first = kept()\n{functions}"
    );
    assert_eq!(run(&first), ("true".to_string(), 1001));
    // The functions are garbage once the collector has run, as it does every turn of the
    // host's loop; the map lets go of their sites as it grows again.
    qml::eval::collect_cycles();
    let (result, templates) = run(functions);
    assert_eq!(result, "true");
    assert!(
        (1001..1100).contains(&templates),
        "{templates} template objects"
    );
}

/// A name script gives a group object (`item.anchors[key] = v`) is a JavaScript property of
/// that object, as V4 keeps it on its wrapper of the QObject: 10 000 such keys, written, read,
/// tested with `in` and half of them deleted, add nothing to the table of names, and the
/// values go with the object.
#[test]
fn new_keys_of_a_group_object_are_not_interned() {
    const MAIN: &str = r#"import QtQuick
Rectangle {
    id: root
    property var made
    property string log: ""
    Component { id: comp; Item {} }
    function fill(o, prefix, n) {
        var a = o.anchors, sum = 0, found = 0
        for (var i = 0; i < n; i++) {
            a[prefix + i] = i
            sum += a[prefix + i]
            if ((prefix + i) in a) found++
            if (i % 2) delete a[prefix + i]
        }
        var kept = Object.keys(a).filter(k => k.startsWith(prefix)).length
        return [sum, found, kept, a[prefix + 0], a[prefix + 1]].join(",")
    }
    function make() { made = comp.createObject(root) }
    function run(n) { log = fill(root, "ev_key_", n) + ";" + fill(made, "ev_made_", n) }
    function drop() { made.destroy(); made = null }
}"#;
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("main.qml");
    std::fs::write(&f, MAIN).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&f).unwrap();
    let call = |rt: &mut Runtime, name: &str, n: f64| {
        rt.call_method(root, name, vec![Value::Num(n)])
            .unwrap()
            .unwrap();
        rt.settle();
    };
    call(&mut rt, "make", 0.0);
    // The names the fixture's own code needs are there after one small round.
    call(&mut rt, "run", 4.0);
    assert_eq!(rt.get(root, "log").to_js_string(), "6,4,2,0,;6,4,2,0,");
    let names = Sym::count();
    let kept = rt.world.borrow().extras.len();
    assert_eq!(kept, 2, "the objects holding new keys");
    call(&mut rt, "run", 10000.0);
    let all = "49995000,10000,5000,0,";
    assert_eq!(rt.get(root, "log").to_js_string(), format!("{all};{all}"));
    assert_eq!(Sym::count(), names, "the table of names grew");
    for k in [
        "ev_key_0",
        "ev_key_9999",
        "ev_made_5000",
        "anchors.ev_key_0",
    ] {
        assert!(Sym::lookup(k).is_none(), "{k} was interned");
    }
    // The values go with the object that holds them.
    call(&mut rt, "drop", 0.0);
    assert_eq!(rt.world.borrow().extras.len(), 1);
    assert_eq!(rt.errors(), Vec::<String>::new());
}

/// `WlrLayershell.exclusiveEdge` is this shell's own property of Quickshell's attached type
/// (Quickshell's `.qmltypes` do not list it): a write from script through the window keeps
/// it where the shell reads it, and a name neither has is still refused.
#[test]
fn a_write_to_the_shells_own_attached_property_is_kept() {
    const MAIN: &str = r#"import QtQuick
import Rustshell
import Rustshell.Wayland
PanelWindow {
    id: bar
    function edge() {
        bar.WlrLayershell.exclusiveEdge = Edges.Bottom
        return bar.WlrLayershell.exclusiveEdge === Edges.Bottom
    }
    function none() {
        bar.WlrLayershell.evNoSuchMember = 1
        return bar.WlrLayershell.evNoSuchMember
    }
    function signal() {
        try { bar.WlrLayershell.exclusiveEdgeChanged = 1 } catch (e) { return String(e) }
        return "kept"
    }
}"#;
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("main.qml");
    std::fs::write(&f, MAIN).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&f).unwrap();
    let kept = rt.call_method(root, "edge", vec![]).unwrap().unwrap();
    assert_eq!(kept.to_js_string(), "true");
    let read = rt.get(root, "WlrLayershell.exclusiveEdge").to_js_string();
    assert_ne!(read, "undefined", "what the shell reads for the panel");
    let none = rt.call_method(root, "none", vec![]).unwrap().unwrap();
    assert_eq!(none.to_js_string(), "undefined");
    // The type table declares it (`Impulse/wlr-layershell.qmltypes`), so it is a property like
    // Quickshell's own: its change signal is a member too.
    let signal = rt.call_method(root, "signal", vec![]).unwrap().unwrap();
    assert_eq!(
        signal.to_js_string(),
        r#"TypeError: Cannot assign to read-only property "exclusiveEdgeChanged""#
    );
}
