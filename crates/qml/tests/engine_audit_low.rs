//! Low-severity findings of the QML engine audit against Qt 6.12: each test runs the audit's
//! probe and expects the `log` (or load error) Qt 6.12 gives for it.

use qml::runtime::Runtime;

/// Loads `main` beside `files` and returns its `log` once the scene, and timers due within
/// the next few frames, have settled.
fn run(files: &[(&str, &str)], main: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    for (name, src) in files {
        std::fs::write(dir.path().join(name), src).unwrap();
    }
    let f = dir.path().join("main.qml");
    std::fs::write(&f, main).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    let mut t = 0.0;
    for _ in 0..8 {
        t += 16.0;
        rt.tick(t);
        rt.run_later();
        rt.settle();
    }
    rt.get(root, "log").to_js_string()
}

fn load_error(src: &str) -> String {
    match qml::parse(src) {
        Ok(_) => "ok".into(),
        Err(e) => format!("{}:{}: {}", e.line, e.col, e.msg),
    }
}

/// Finding 5: calling a declared property's `oChanged()` notifies its readers and handler.
#[test]
fn calling_changed_signal_notifies() {
    let log = run(
        &[],
        r#"import QtQuick
Item { id: r; property string log: ""; property var o: ({a: 1}); property int seen: o.a
  onOChanged: log += "oc;"
  onSeenChanged: log += "s" + seen + ";"
  Component.onCompleted: { o.a = 5; oChanged(); log += seen } }"#,
    );
    assert_eq!(log, "oc;s1;oc;s5;5");
    let log = run(
        &[],
        r#"import QtQuick
Item { id: r; property string log: seen + "," + n; property var o: ({a: 1}); property int seen: o.a; property int n: 0
  onOChanged: n++
  Component.onCompleted: { o.a = 5; oChanged() } }"#,
    );
    assert_eq!(log, "5,2");
}

/// Finding 6: an assigned `state` stays while no other state's `when` holds.
#[test]
fn explicit_state_beside_when() {
    let states = r#"states: [ State { name: "a"; PropertyChanges { target: r; k: 2 } }, State { name: "b"; when: r.f; PropertyChanges { target: r; k: 3 } } ]"#;
    let log = run(
        &[],
        &format!(
            r#"import QtQuick
Item {{ id: r; property string log: k + state; property bool f: false; property int k: 1
  {states}
  Component.onCompleted: state = "a" }}"#
        ),
    );
    assert_eq!(log, "2a");
    let log = run(
        &[],
        &format!(
            r#"import QtQuick
Item {{ id: r; property string log: ""; property bool f: false; property int k: 1
  {states}
  Component.onCompleted: {{ state = "a"; log += k + state + ";"; f = true; log += k + state + ";"; f = false; log += k + state + ";" }} }}"#
        ),
    );
    assert_eq!(log, "2a;3b;1;");
}

/// Finding 7: writing the object a `var` already holds is not a change.
#[test]
fn var_same_reference_is_no_change() {
    let log = run(
        &[],
        r#"import QtQuick
Item { id: r; property string log: ""; property var v: null; property var o: ({k: 1}); property int n: 0
  onVChanged: n++
  Component.onCompleted: { var n0 = n; v = o; v = o; log += (n - n0) + ";"; var arr = [1, 2]; v = arr; v = arr; log += (n - n0) + ";"; v = [1, 2]; v = [1, 2]; log += (n - n0) + ";"; v = "s"; v = "s"; log += (n - n0) + ";"; v = r; v = r; log += (n - n0) + ";"; v = undefined; v = undefined; log += (n - n0) } }"#,
    );
    assert_eq!(log, "1;2;4;5;6;7");
}

/// Finding 8: `Timer.start()/stop()/restart()` emit `runningChanged` inside the call.
#[test]
fn timer_methods_emit_running_changed_at_once() {
    let log = run(
        &[],
        r#"import QtQuick
Item { id: r; property string log: ""
  Timer { id: t; interval: 100000; repeat: false; onTriggered: r.log += "t;"; onRunningChanged: r.log += "r" + running + ";" }
  Component.onCompleted: { t.start(); t.stop(); t.restart(); t.stop() } }"#,
    );
    assert_eq!(log, "rtrue;rfalse;rtrue;rfalse;");
    let log = run(
        &[],
        r#"import QtQuick
Item { id: r; property string log: ""
  Timer { id: t; interval: 100000; onRunningChanged: r.log += "r" + running + ";" }
  Component.onCompleted: { t.start(); log += "a" + t.running + ";"; t.stop(); log += "b" + t.running + ";"; t.running = true; log += "c;"; t.running = false; log += "d;" } }"#,
    );
    assert_eq!(log, "rtrue;atrue;rfalse;bfalse;rtrue;c;rfalse;d;");
}

/// Finding 9: a `sig.connect(f)` outliving the object `f` was made in runs in a dead context:
/// its ids read as undefined (Qt throws `Cannot read property ... of undefined`).
#[test]
fn connect_closure_dies_with_its_object() {
    let log = run(
        &[],
        r#"import QtQuick
Item { id: r; property string log: ""; signal a()
  Component { id: c; Item { id: it; property int k: 3; Component.onCompleted: r.a.connect(function() { r.log += "closure" + it.k + ";" }) } }
  Timer { interval: 20; running: true; onTriggered: { r.a(); r.log += "after" } }
  Component.onCompleted: { var o = c.createObject(r); a(); o.destroy() } }"#,
    );
    assert_eq!(log, "closure3;after");
}

/// Finding 10: `Connections` run after the object's own handler, the later one first.
#[test]
fn connections_run_in_qt_order() {
    let log = run(
        &[],
        r#"import QtQuick
Item { id: r; property string log: ""; signal foo()
  Connections { target: r; function onFoo() { r.log += "c1;" } }
  onFoo: log += "h;"
  Connections { target: r; function onFoo() { r.log += "c2;" } }
  function s() { log += "s;" }
  Component.onCompleted: { foo.connect(s); foo() } }"#,
    );
    assert_eq!(log, "h;c2;c1;s;");
}

/// Finding 11: each import of a script without `.pragma library` has its own state.
#[test]
fn script_without_pragma_library_per_import() {
    let log = run(
        &[
            ("lib.js", "var n = 0;\nfunction counter() { return ++n }\n"),
            (
                "shared.js",
                ".pragma library\nvar n = 0;\nfunction bump() { return ++n }\n",
            ),
            (
                "Sub5.qml",
                "import QtQuick\nimport \"shared.js\" as S\nimport \"lib.js\" as L\n\
                 Item { function bump() { return S.bump() } function count() { return L.counter() } }\n",
            ),
        ],
        r#"import QtQuick
import "lib.js" as A
import "lib.js" as B
import "shared.js" as S1
Item { id: r; property string log: ""
  Sub5 { id: s }
  Component.onCompleted: { A.counter(); log = A.counter() + "," + B.counter() + "," + S1.bump() + "," + s.bump() + "," + s.count() } }"#,
    );
    assert_eq!(log, "2,1,1,2,1");
}

/// Finding 12: `Instantiator` reports its objects and lets go of them while inactive.
#[test]
fn instantiator_signals_and_object_at() {
    let log = run(
        &[],
        r#"import QtQuick
import QtQml
Item { id: r; property string log: ""; property var m: ["a", "b"]
  Instantiator { id: inst; model: r.m; delegate: QtObject { required property string modelData; Component.onCompleted: r.log += "+" + modelData + ";"; Component.onDestruction: r.log += "-" + modelData + ";" }
    onObjectAdded: (i, o) => r.log += "A" + i + o.modelData + ";"; onObjectRemoved: (i, o) => r.log += "R" + i + ";" }
  Component.onCompleted: { log += inst.count + "|"; m = ["c"]; log += "|" + inst.count + inst.objectAt(0).modelData; inst.active = false; log += "|" + inst.count } }"#,
    );
    assert_eq!(log, "+a;A0a;+b;A1b;2|R0;-a;R1;-b;+c;A0c;|1cR0;-c;|0");
}

/// Finding 13: `ListModel` rows from `ListElement`s, its methods, and views over it.
#[test]
fn list_model() {
    let model =
        r#"ListModel { id: lm; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } }"#;
    let log = run(
        &[],
        &format!(
            r#"import QtQuick
Item {{ id: r; property string log: ""
  {model}
  Component.onCompleted: {{ log += lm.count + lm.get(1).name; lm.append({{name: "c", n: 3}}); lm.remove(0); lm.setProperty(0, "name", "B"); lm.move(0, 1, 1); lm.insert(0, {{name: "z", n: 0}}); log += "|"; for (var i = 0; i < lm.count; i++) log += lm.get(i).name + lm.get(i).n + ","; lm.set(1, {{name: "q"}}); log += lm.get(1).name + lm.get(1).n; lm.clear(); log += lm.count }} }}"#
        ),
    );
    assert_eq!(log, "2b|z0,c3,B2,q30");
    let log = run(
        &[],
        &format!(
            r#"import QtQuick
Item {{ id: r; property string log: ""
  {model}
  Repeater {{ id: rep; model: lm; delegate: Item {{ required property string name; required property int index; property string tag: name + index; Component.onCompleted: r.log += "+" + name + index + ";"; Component.onDestruction: r.log += "-" + name + ";" }} }}
  Component.onCompleted: {{ lm.append({{name: "c", n: 3}}); lm.remove(0); lm.setProperty(0, "name", "B"); lm.move(0, 1, 1); lm.insert(0, {{name: "z", n: 0}}); log += "|" + rep.count + ":"; for (var i = 0; i < rep.count; i++) log += rep.itemAt(i).tag + ","; log += lm.get(1).name + lm.count }} }}"#
        ),
    );
    assert_eq!(log, "+a0;+b1;+c2;-a;+z0;|3:z0,c1,B2,c3");
}

/// Finding 14: a type cut off after `<` is a parse error, not a panic.
#[test]
fn truncated_generic_type_is_an_error() {
    for src in [
        "Item { property list<",
        "Item { property list<int",
        "Item { property list",
    ] {
        assert_ne!(load_error(src), "ok", "{src}");
    }
}

/// Finding 15: nesting is bounded as in Qt (1364 objects), with no stack overflow.
#[test]
fn nesting_limit() {
    let nested = |n: usize| format!("import QtQuick\n{}{}", "Item { ".repeat(n), "}".repeat(n));
    assert_eq!(load_error(&nested(1364)), "ok");
    for n in [1365, 100_000] {
        let e = load_error(&nested(n));
        assert!(
            e.ends_with("Maximum statement or expression depth exceeded"),
            "{n}: {e}"
        );
    }
}

/// Finding 16: QML Qt rejects is a load error with Qt's message and position.
#[test]
fn rejects_what_qt_rejects() {
    let body = |b: &str| format!("import QtQuick\nItem {{\n{b}\n}}\n");
    for (b, want) in [
        (
            "  property int as: 13",
            "3:16: Reserved keyword \"as\" cannot be used as a QML identifier.",
        ),
        (
            "property int a: 1.5",
            "3:17: Invalid property assignment: int expected",
        ),
        (
            "property int a: -1.5",
            "3:17: Invalid property assignment: int expected",
        ),
        (
            "property int a: \"x\"",
            "3:17: Invalid property assignment: int expected",
        ),
        (
            "property int a: 2147483648",
            "3:17: Invalid property assignment: int expected",
        ),
        (
            "property real a: \"x\"",
            "3:18: Invalid property assignment: number expected",
        ),
        (
            "property bool a: 1",
            "3:18: Invalid property assignment: boolean expected",
        ),
        (
            "property string a: 1",
            "3:20: Invalid property assignment: string expected",
        ),
        (
            "function f() {}; function g() {}",
            "3:16: Unexpected token `;'",
        ),
        ("Item {};", "3:8: Unexpected token `;'"),
        ("width: { 3 }; height: 2", "3:13: Unexpected token `;'"),
        (";", "3:1: Unexpected token `;'"),
    ] {
        assert_eq!(load_error(&body(b)), want, "{b}");
    }
    for b in [
        "property int a: 2.0",
        "property int a: 1e3",
        "property int a: 0x10",
        "property int a: (1.5)",
        "property int a: 1.5 + 1",
        "property var a: 1.5",
        "property int a: 1;;",
        "property int a: { return 5 }; width: 1",
        "property int a; property int b",
        "signal s; signal t",
        "id: r; width: 3",
        "property int on: 1; property int get: 2",
    ] {
        assert_eq!(load_error(&body(b)), "ok", "{b}");
    }
}

/// `this` of the top-level code of an imported `.js` file is the global object, as in Qt 6.12
/// (`crates/rigen/check/cases/library_this.js`).
#[test]
fn js_file_top_level_this_is_the_global_object() {
    let src = "var g = this;\nvar top = typeof this;\nvar arrow = () => typeof this;\n\
               function id() { return this }\n\
               function r() { return [top, arrow(), id() === g, typeof g.Math].join() }\n";
    let log = run(
        &[
            ("lib.js", src),
            ("plib.js", &format!(".pragma library\n{src}")),
        ],
        r#"import QtQuick
import "lib.js" as L
import "plib.js" as P
Item { property string log: L.r() + ";" + P.r() }"#,
    );
    assert_eq!(log, "object,object,true,object;object,object,true,object");
}
