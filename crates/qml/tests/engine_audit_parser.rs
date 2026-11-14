//! Parser and binding findings of the QML engine audit against Qt 6.12 (3, 10, 11, 12) and
//! what their probes showed beside them: each test runs a probe and expects the `log` (or load
//! error) Qt 6.12 gives for it (PySide6 6.12.0, QQmlComponent).

use qml::runtime::Runtime;

/// Loads `main` beside `files` and returns its `log`, or the load error.
fn run(files: &[(&str, &str)], main: &str) -> Result<String, String> {
    let dir = tempfile::tempdir().unwrap();
    for (name, src) in files {
        std::fs::write(dir.path().join(name), src).unwrap();
    }
    let f = dir.path().join("main.qml");
    std::fs::write(&f, main).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&f).map_err(|e| e.to_string())?;
    rt.settle();
    assert_eq!(rt.errors(), Vec::<String>::new());
    Ok(rt.get(root, "log").to_js_string())
}

/// `Item { id: root; a: 3; b: 4; arr: [10, 20]; f(x) }` with `property var c: <binding>`.
fn binding(src: &str) -> Result<String, String> {
    run(
        &[],
        &format!(
            r#"import QtQuick
Item {{
    id: root
    property int a: 3
    property int b: 4
    property var arr: [10, 20]
    function f(x) {{ return x * 2 }}
    property var c: {src}
    property string log: String(c)
}}"#
        ),
    )
}

fn load_error(member: &str) -> String {
    let src = format!("import QtQuick\nItem {{\n    {member}\n    property int a: 1\n}}");
    match qml::parse(&src) {
        Ok(_) => "ok".into(),
        Err(e) => format!("{}:{}: {}", e.line, e.col, e.msg),
    }
}

/// Finding 3: a binding's value is the completion value of its statement, whatever the
/// statement: `switch`, `try`, a loop, a label, a declaration.
#[test]
fn statement_binding_completion_value() {
    let log = run(
        &[],
        r#"import QtQuick
Item {
    id: root
    property int a: 1
    property var c: switch (a) { case 0: "zero"; case 1: "one"; case 2: "two"; default: "other"; }
    property var d: switch (a) { case 0: "zero"; break; case 1: "one"; break; default: "other"; }
    property var e: try { nope.x } catch (e) { 5 }
    property var f: { a; }
    property var g: { if (a) "yes"; else "no" }
    property var h: { let q = 5; q + 1 }
    property var i: { for (let k = 0; k < 3; k++) k }
    property var j: { a: 7 }
    property var k: if (a) "t"
    property var l: { switch (a) { case 1: "in-block" } }
    property var m: { try { "t" } finally { "f" } }
    property var n: { var q = 5 }
    property var o: { 1; var q = 5 }
    property var p: { while (false) {} }
    property var q: { 3; if (false) 4 }
    property string log: ""
    Component.onCompleted: log = [c, d, e, f, g, h, i, j, k, l, m, n, o, p, q].map(String).join(" ")
}"#,
    );
    assert_eq!(
        log.unwrap(),
        "other one 5 1 yes 6 2 7 t in-block t undefined 1 undefined undefined"
    );
}

/// Finding 3, as `StyledRectangle.qml` writes it: a `switch` over the type's own enum. Without
/// `break` every case falls through to `default`; with it the matching case is the value.
#[test]
fn switch_binding_over_enum() {
    let styled = r#"import QtQuick
Rectangle {
    enum ContentLayer { Background, Pane, Group }
    property var contentLayer: Styled.ContentLayer.Pane
    property string shade: switch (contentLayer) {
        case Styled.ContentLayer.Background: "layer0";
        case Styled.ContentLayer.Pane: "layer1";
        case Styled.ContentLayer.Group: "layer2";
        default: "layer9";
    }
    property string picked: switch (contentLayer) {
        case Styled.ContentLayer.Background: "layer0"; break;
        case Styled.ContentLayer.Pane: "layer1"; break;
        case Styled.ContentLayer.Group: "layer2"; break;
        default: "layer9";
    }
}"#;
    let log = run(
        &[("Styled.qml", styled)],
        r#"import QtQuick
Item {
    Styled { id: a }
    Styled { id: b; contentLayer: Styled.ContentLayer.Group }
    property string log: [a.shade, a.picked, b.shade, b.picked].join(" ")
}"#,
    );
    assert_eq!(log.unwrap(), "layer9 layer1 layer9 layer2");
}

/// Finding 10: a binding goes on over a line break when the next line continues it.
#[test]
fn binding_continues_over_line_break() {
    for (src, qt) in [
        ("a\n        !== b", "true"),
        ("arr\n        instanceof Array", "true"),
        ("\"a\"\n        in ({a: 1})", "true"),
        ("f\n        (5)", "10"),
        ("arr\n        [1]", "20"),
        ("a\n        , b", "4"),
        ("a\n        ^ b", "7"),
        ("if (a > b) a; else b", "4"),
        ("if (a > b) a\n        else b", "4"),
        ("if (a > b) a;\n        else b", "4"),
        ("if (a > b) { a }\n        else { b }", "4"),
        ("if (a)\n        b\n        else\n        a", "4"),
        ("try { nope.x }\n        catch (e) { 5 }", "5"),
        ("try { 7 }\n        finally { 5 }", "7"),
        ("try\n        { 1 }\n        catch (e)\n        { 2 }", "1"),
        ("switch (a)\n        { case 3: \"three\" }", "three"),
        ("arr // c\n        /* d */ [1]", "20"),
    ] {
        assert_eq!(binding(src).as_deref(), Ok(qt), "{src}");
    }
}

/// Finding 10, the other side: a line break still ends the binding where Qt ends it, and what
/// follows is then not a member.
#[test]
fn line_break_still_ends_binding() {
    for src in [
        "a\n        else b",
        "a;\n        in arr",
        "a\n        ++b",
        "a\n        !b",
        "a;\n        (5)",
        "{ 1 }\n        (5)",
        "{ 1 }\n        [0]",
        "if (a) { f }\n        (5)",
        "if (a) { arr }\n        [0]",
        "switch (a) { case 3: \"three\" }\n        [0]",
        "if (a > b) a; else b; else 3",
    ] {
        assert!(binding(src).is_err(), "{src}");
    }
    // The next member may start with a name that begins like an operator.
    let log = run(
        &[],
        "import QtQuick\nItem {\n    property int a: 3\n    property var c: a\n    property int inx: 5\n    property var d: a\n    property int instanceofx: 6\n    property string log: [c, inx, d, instanceofx].join(\" \")\n}",
    );
    assert_eq!(log.unwrap(), "3 5 3 6");
}

/// Finding 11: `;` may follow a property declaration whatever its value is, and nothing else
/// it could not follow before.
#[test]
fn semicolon_after_property_declaration() {
    let log = run(
        &[],
        r#"import QtQuick
Item {
    property Item sub: Item {} ;
    readonly property Item ro: Item {} ;
    default property Item dflt: Item {} ;
    property list<Item> l: [Item {}] ;
    property list<Item> none: [] ;
    property var v: { } ;
    property int a: 1;
    property int two: 2;;
    property string log: "loaded " + a
}"#,
    );
    assert_eq!(log.unwrap(), "loaded 1");
    for (member, qt) in [
        (
            "property Item sub: Item {} ;;",
            "3:33: Unexpected token `;'",
        ),
        (
            "property Item sub\n    sub: Item {} ;",
            "4:18: Unexpected token `;'",
        ),
        ("property var v\n    v: { } ;", "4:12: Unexpected token `;'"),
        ("Item {} ;", "3:13: Unexpected token `;'"),
        ("data: [Item {}] ;", "3:21: Unexpected token `;'"),
        ("function f() {} ;", "3:21: Unexpected token `;'"),
        ("enum E { A } ;", "3:18: Unexpected token `;'"),
        ("component C: Item {} ;", "3:26: Unexpected token `;'"),
        ("anchors { margins: 1 } ;", "3:28: Unexpected token `;'"),
        ("NumberAnimation on x { } ;", "3:30: Unexpected token `;'"),
        ("; property int z: 2", "3:5: Unexpected token `;'"),
        ("width: 2 ;;", "3:15: Unexpected token `;'"),
    ] {
        assert_eq!(load_error(member), qt, "{member}");
    }
}

/// Finding 12: the qualifier of `import "dir" as Here` is an object in JavaScript whose
/// members are the directory's types; a document without that import does not see it.
#[test]
fn directory_import_qualifier_in_js() {
    let sub = "import QtQuick\nItem { enum Mode { A, B } property string tag: \"default\" }";
    let no_alias = r#"import QtQuick
QtObject {
    function probe() {
        try { return typeof Here + " " + Here } catch (e) { return typeof Here + " " + e.name }
    }
}"#;
    let log = run(
        &[("Sub.qml", sub), ("NoAlias.qml", no_alias)],
        r#"import "." as Here
import QtQuick
Item {
    id: root
    Here.Sub { id: s }
    NoAlias { id: na }
    property var bEnum: Here.Sub.B
    property var bType: typeof Here
    function t(f) { try { return String(f()) } catch (e) { return e.name + ": " + e.message } }
    property string log: ""
    Component.onCompleted: log = [
        bEnum, bType,
        t(() => typeof Here),
        t(() => typeof Here.Sub),
        t(() => Here.Sub.B),
        t(() => Here.Sub.Mode.B),
        t(() => JSON.stringify(Object.keys(Here))),
        t(() => "Sub" in Here),
        t(() => (Here === Here) + " " + (Here.Sub === Here.Sub)),
        t(() => Here.Sub === Sub),
        t(() => typeof Here.NoAlias),
        t(() => (s as Here.Sub) === s),
        na.probe(),
        t(() => { Here = 1; return typeof Here }),
    ].join("; ")
}"#,
    );
    assert_eq!(
        log.unwrap(),
        "1; object; object; object; 1; 1; []; true; false true; true; object; true; \
         undefined ReferenceError; Error: Invalid write to global property \"Here\""
    );
}

/// A binding is a UiScriptStatement: an expression, a block, `if`, `switch`, `try`. Qt
/// rejects every other statement in that place, with these errors.
#[test]
fn statements_that_are_no_binding() {
    for (member, qt) in [
        (
            "property var c: for (var i = 0; i < 2; i++) i",
            "3:21: Unexpected token `for'",
        ),
        (
            "property var c: for (var k in arr) k",
            "3:21: Unexpected token `for'",
        ),
        (
            "property var c: for (var v of arr) v",
            "3:21: Unexpected token `for'",
        ),
        (
            "property var c: while (false) 1",
            "3:21: Unexpected token `while'",
        ),
        (
            "property var c: do 1; while (false)",
            "3:21: Unexpected token `do'",
        ),
        ("property var c: throw 1", "3:21: Unexpected token `throw'"),
        (
            "property var c: return 1",
            "3:21: Unexpected token `return'",
        ),
        ("property var c: return", "3:21: Unexpected token `return'"),
        ("property var c: var x = 1", "3:21: Unexpected token `var'"),
        ("property var c: let x = 1", "3:21: Unexpected token `let'"),
        (
            "property var c: const x = 1",
            "3:21: Unexpected token `const'",
        ),
        ("property var c: break", "3:21: Unexpected token `break'"),
        (
            "property var c: continue",
            "3:21: Unexpected token `continue'",
        ),
        (
            "property var c: debugger",
            "3:21: Unexpected token `debugger'",
        ),
        (
            "property var c: import(\"x\")",
            "3:21: Unexpected token `import'",
        ),
        ("property var c: lbl: 5", "3:24: Expected token `,'"),
        ("property var c: lbl: { 5 }", "3:24: Expected token `,'"),
        (
            "property var c: async function g() {}",
            "3:27: Expected token `,'",
        ),
        ("property var c: async () => 1", "3:30: Expected token `,'"),
        (
            "property var c: async (a = \")\") => 1",
            "3:37: Expected token `,'",
        ),
        ("property var c: async x => 1", "3:27: Expected token `;'"),
        ("width: for (;;) {}", "3:12: Unexpected token `for'"),
        ("width: lbl: 5", "3:15: Expected token `,'"),
        ("onWidthChanged: var x = 1", "3:21: Unexpected token `var'"),
        ("onWidthChanged: return", "3:21: Unexpected token `return'"),
        (
            "onWidthChanged: while (false) {}",
            "3:21: Unexpected token `while'",
        ),
    ] {
        assert_eq!(load_error(member), qt, "{member}");
    }
}

/// What Qt accepts in that place stays accepted: names that begin like a keyword, and the
/// same statements inside a block or an `if`.
#[test]
fn statements_that_are_a_binding() {
    for member in [
        "property var c: function g() { return 1 }",
        "property var c: class A {}",
        "property var c: letter",
        "property var c: forty + returned + variable + doing",
        "property var c: async",
        "property var c: async(1)",
        "property var c: async in a",
        "property var c: async(\")\", b => 2)",
        "property var c: lbl ? 1 : 2",
        "property var c: { lbl: for (;;) break lbl }",
        "onWidthChanged: { var x = 1; for (;;) break; return }",
        "onWidthChanged: if (a) return",
        "property var c: if (a) for (;;) break",
        "property var c: if (a) var x = 1",
        "property var c: if (a) lbl: 5",
        "property var c: try { throw 1 } catch (e) { 2 }",
    ] {
        assert_eq!(load_error(member), "ok", "{member}");
    }
}

/// Every name on a directory qualifier is an object that converts to the name (Qt takes it
/// for a type of the directory); on a module qualifier the module's types and nothing else.
#[test]
fn import_qualifier_members() {
    let log = run(
        &[],
        r#"import QtQuick
import QtQuick as Q
import QtQuick.Controls as C
import "." as Here
Item {
    id: root
    function w() { Here.Nope = 1; Q.Item = 1; return typeof Here.Nope + typeof Q.Item }
    property string log: [typeof Here.Nope, String(Here.Nope), "" + Here.lower, "Nope" in Here, 0 in Here,
        typeof Here[0], Here.Nope === Here.Nope, Here.Nope == null, !Here.Nope, typeof Here.Nope.x,
        Object.keys(Here).length, Here === Here, w(),
        typeof Q.Item, Q.Item === Item, Q.Item.TopLeft, Q.Text.AlignHCenter, typeof Q.Nope, typeof Q.lower,
        typeof Q.Button, "Item" in Q, "Nope" in Q, Object.keys(Q).length, typeof Q.Component,
        typeof Q.Screen, typeof Q.Keys, typeof Q.Qt, typeof Q.ListModel, typeof C.Button, typeof C.Item].join(" ")
}"#,
    );
    assert_eq!(
        log.unwrap(),
        "object Nope lower true false undefined true false false undefined 0 false objectobject \
         object true 0 4 undefined undefined undefined true false 0 object object object object \
         object object undefined"
    );
}

/// QML's JavaScript has no async functions and no `await` (qqmljs.g has neither): `async`
/// and `await` are names, so Qt's parser stops at the token that cannot follow a name, in a
/// binding, a handler, a function and anything nested in them. These are its errors.
#[test]
fn async_and_await_are_no_qml_javascript() {
    for (member, qt) in [
        (
            "property var c: async function() {}",
            "3:27: Expected token `,'",
        ),
        ("property var c: await g()", "3:27: Expected token `,'"),
        (
            "onWidthChanged: { async (a, b) => { } }",
            "3:36: Expected token `,'",
        ),
        (
            "onWidthChanged: { async () => 1; a = 1 }",
            "3:32: Expected token `,'",
        ),
        (
            "function f() { var g = async function() {} }",
            "3:34: Expected token `;'",
        ),
        (
            "function f() { var g = await g() }",
            "3:34: Expected token `;'",
        ),
        (
            "function f() { let g = async (a, b) => { }; return g }",
            "3:41: Expected token `;'",
        ),
        (
            "function f() { const g = 1, h = async () => 1 }",
            "3:46: Expected token `;'",
        ),
        (
            "function f() { return async function() {} }",
            "3:33: Expected token `;'",
        ),
        (
            "function f() { return await g() }",
            "3:33: Expected token `;'",
        ),
        (
            "function f() { throw async (a, b) => { } }",
            "3:39: Expected token `,'",
        ),
        (
            "function f() { g(async () => 1) }",
            "3:31: Expected token `)'",
        ),
        (
            "function f() { g(1, async function() {}) }",
            "3:31: Expected token `)'",
        ),
        (
            "function f() { g(1, await g()) }",
            "3:31: Expected token `)'",
        ),
        (
            "function f() { g(async (a, b) => { }, 1) }",
            "3:35: Expected token `)'",
        ),
        (
            "function f() { return new G(async () => 1) }",
            "3:42: Expected token `)'",
        ),
        (
            "property var c: (async function() {})",
            "3:28: Expected token `,'",
        ),
        ("property var c: (await g())", "3:28: Expected token `,'"),
        (
            "property var c: [async (a, b) => { }]",
            "3:35: Expected token `,'",
        ),
        (
            "property var c: [1, async () => 1]",
            "3:34: Expected token `,'",
        ),
        (
            "property var c: [async function() {}, 1]",
            "3:28: Expected token `,'",
        ),
        ("property var c: [await g(), 1]", "3:28: Expected token `,'"),
        (
            "property var c: ({ a: async (a, b) => { } })",
            "3:40: Expected token `}'",
        ),
        (
            "property var c: ({ a: async () => 1, b: 1 })",
            "3:36: Expected token `,'",
        ),
        (
            "property var c: a ? async function() {} : 2",
            "3:31: Expected token `:'",
        ),
        ("property var c: a ? async x => 1 : 2", "3:31: Syntax error"),
        (
            "property var c: a ? await g() : 2",
            "3:31: Expected token `:'",
        ),
        (
            "property var c: a ? async function*() {} : 2",
            "3:31: Syntax error",
        ),
        (
            "property var c: a ? 1 : async (a, b) => { }",
            "3:42: Expected token `,'",
        ),
        (
            "property var c: arr[async () => 1]",
            "3:34: Expected token `,'",
        ),
        (
            "property var c: arr[async x => 1]",
            "3:31: Expected token `]'",
        ),
        (
            "property var c: arr[async function*() {}]",
            "3:31: Expected token `]'",
        ),
        (
            "function f() { if (async function() {}) {} }",
            "3:30: Expected token `,'",
        ),
        (
            "function f() { if (await g()) {} }",
            "3:30: Expected token `,'",
        ),
        (
            "function f() { while (async (a, b) => { }) {} }",
            "3:40: Expected token `,'",
        ),
        (
            "function f() { switch (async () => 1) {} }",
            "3:37: Expected token `,'",
        ),
        (
            "function f() { switch (async x => 1) {} }",
            "3:34: Syntax error",
        ),
        (
            "function f() { switch (async function*() {}) {} }",
            "3:34: Syntax error",
        ),
        (
            "function f() { switch (a) { case async function() {}: break } }",
            "3:44: Expected token `,'",
        ),
        (
            "function f() { switch (a) { case await g(): break } }",
            "3:44: Expected token `,'",
        ),
        (
            "function f() { for (var i = async (a, b) => { }; ;) {} }",
            "3:46: Expected token `;'",
        ),
        (
            "function f() { for (;async () => 1;) {} }",
            "3:35: Expected token `;'",
        ),
        (
            "property var c: `a${async function() {}}b`",
            "3:31: Expected token `,'",
        ),
        (
            "property var c: `a${async x => 1}b`",
            "3:31: Expected token `(template tail)'",
        ),
        (
            "property var c: `a${await g()}b`",
            "3:31: Expected token `,'",
        ),
        (
            "property var c: `a${async function*() {}}b`",
            "3:31: Expected token `(template tail)'",
        ),
        (
            "onWidthChanged: { x = async (a, b) => { } }",
            "3:40: Expected token `,'",
        ),
        (
            "onWidthChanged: { x += async () => 1 }",
            "3:37: Expected token `,'",
        ),
        (
            "property var c: 1 + async function() {}",
            "3:31: Expected token `,'",
        ),
        ("property var c: 1 + await g()", "3:31: Expected token `,'"),
        (
            "property var c: a && async (a, b) => { }",
            "3:39: Expected token `,'",
        ),
        ("property var c: !async () => 1", "3:31: Expected token `,'"),
        (
            "property var c: typeof async function() {}",
            "3:34: Expected token `,'",
        ),
        (
            "property var c: typeof await g()",
            "3:34: Expected token `,'",
        ),
        (
            "function f(a = async (a, b) => { }) {}",
            "3:33: Expected token `)'",
        ),
        (
            "property var c: () => async () => 1",
            "3:36: Expected token `,'",
        ),
        (
            "property var c: x => async function() {}",
            "3:32: Expected token `,'",
        ),
        ("property var c: x => await g()", "3:32: Expected token `,'"),
        (
            "property var c: (x => async (a, b) => { })",
            "3:40: Expected token `,'",
        ),
        (
            "property var c: g(x => async () => 1)",
            "3:37: Expected token `)'",
        ),
        (
            "function f() { l: async function() {} }",
            "3:29: Expected token `,'",
        ),
        ("function f() { l: await g() }", "3:29: Expected token `,'"),
        (
            "onWidthChanged: async (a, b) => { }",
            "3:34: Expected token `,'",
        ),
        ("width: async () => 1", "3:21: Expected token `,'"),
        (
            "property var c: [...async function() {}]",
            "3:31: Expected token `,'",
        ),
        ("property var c: [...await g()]", "3:31: Expected token `,'"),
        (
            "function f() { g(...async (a, b) => { }) }",
            "3:38: Expected token `)'",
        ),
        (
            "property var c: (1, async () => 1)",
            "3:34: Expected token `,'",
        ),
        (
            "function f() { 1, async function() {} }",
            "3:29: Expected token `,'",
        ),
        ("function f() { 1, await g() }", "3:29: Expected token `,'"),
        (
            "function f() { if (a) async (a, b) => { } }",
            "3:40: Expected token `,'",
        ),
        (
            "function f() { if (a) {} else async () => 1 }",
            "3:44: Expected token `,'",
        ),
        (
            "function f() { var o = { m() { async function() {} } } }",
            "3:42: Expected token `,'",
        ),
        (
            "function f() { var o = { m() { await g() } } }",
            "3:42: Expected token `,'",
        ),
        (
            "function f() { var g = function() { async (a, b) => { } } }",
            "3:54: Expected token `,'",
        ),
        (
            "function f() { var g = function() { return async () => 1 } }",
            "3:57: Expected token `;'",
        ),
        (
            "function f() { var g = () => { async function() {} } }",
            "3:42: Expected token `,'",
        ),
        (
            "function f() { var g = () => { await g() } }",
            "3:42: Expected token `,'",
        ),
        (
            "function f() { var g = 1 + async (a, b) => { } }",
            "3:45: Expected token `;'",
        ),
        (
            "function f() { var g = a ? 1 : async () => 1 }",
            "3:45: Expected token `;'",
        ),
        (
            "function f() { return a ? async function() {} : 1 }",
            "3:37: Expected token `:'",
        ),
        (
            "function f() { return a ? async x => 1 : 1 }",
            "3:37: Syntax error",
        ),
        (
            "function f() { return a ? await g() : 1 }",
            "3:37: Expected token `:'",
        ),
        (
            "function f() { return a ? async function*() {} : 1 }",
            "3:37: Syntax error",
        ),
        (
            "function f() { return [async (a, b) => { }] }",
            "3:41: Expected token `,'",
        ),
        (
            "function f() { try { async () => 1 } catch (e) {} }",
            "3:35: Expected token `,'",
        ),
        (
            "function f() { do async function() {}; while (a) }",
            "3:29: Expected token `,'",
        ),
        (
            "function f() { do await g(); while (a) }",
            "3:29: Expected token `,'",
        ),
        (
            "function f() { a.b(async (a, b) => { }).c }",
            "3:37: Expected token `)'",
        ),
        (
            "function f() { a[0](async () => 1) }",
            "3:34: Expected token `)'",
        ),
        (
            "property var c: { var g = async function() {}; return g }",
            "3:37: Expected token `;'",
        ),
        (
            "property var c: { var g = await g(); return g }",
            "3:37: Expected token `;'",
        ),
        (
            "property var c: if (a) async (a, b) => { }",
            "3:41: Expected token `,'",
        ),
        (
            "Component.onCompleted: async () => 1",
            "3:37: Expected token `,'",
        ),
        (
            "property var c: async function() {}\n    property int z: 1",
            "3:27: Expected token `,'",
        ),
        (
            "property var c: await g()\n    property int z: 1",
            "3:27: Expected token `,'",
        ),
        (
            "property var c: async (a, b) => { };",
            "3:34: Expected token `,'",
        ),
        (
            "function f() {\n        async () => 1\n    }",
            "4:18: Expected token `,'",
        ),
        (
            "function f() {\n        var q = async function() {}\n        return q\n    }",
            "4:23: Expected token `;'",
        ),
        (
            "function f() {\n        var q = await g()\n        return q\n    }",
            "4:23: Expected token `;'",
        ),
        (
            "function f() { var o = { async 'm'() {} } }",
            "3:36: Expected token `}'",
        ),
        (
            "function f() { class A { async *m() {} } }",
            "3:36: Expected token `('",
        ),
        (
            "function f() { for await (const x of []) {} }",
            "3:24: Unexpected token `identifier'",
        ),
        (
            "function f() { var a = async x\n => 1 }",
            "3:34: Expected token `;'",
        ),
        (
            "function f() { var a = async (x)\n => 1 }",
            "4:2: Unexpected token `=>'",
        ),
        (
            "function f() { await this.g() }",
            "3:26: Expected token `;'",
        ),
        (
            "function f() { await function() {} }",
            "3:26: Expected token `,'",
        ),
        (
            "function f() { return [await y, 1] }",
            "3:34: Unexpected token `identifier'",
        ),
        (
            "function f() { return `${await y}` }",
            "3:36: Unexpected token `identifier'",
        ),
        (
            "function f() { return [async x] }",
            "3:34: Unexpected token `identifier'",
        ),
        (
            "function f() { return a[async x] }",
            "3:35: Unexpected token `identifier'",
        ),
        (
            "property var c: async /* c */ function g() {}",
            "3:35: Expected token `,'",
        ),
    ] {
        assert_eq!(load_error(member), qt, "{member}");
    }
}

/// As names they stay usable, and a comment, a string, a template or a regular expression
/// that spells them is no use of them.
#[test]
fn async_and_await_are_names() {
    for member in [
        "function f() { var o = { async() {} } }",
        "function f() { var o = { get async() { return 1 } } }",
        "function f() { class A { async() {} } }",
        "function f() { async\n function g() {} }",
        "function f() { var a = async\n x => 1 }",
        "function f() { await\n 1 }",
        "function f() { await [1] }",
        "function f() { await (1) }",
        "function f() { await -1 }",
        "function f() { await `a` }",
        "function f() { return { async, x } }",
        "property var c: async\n    function g() {}",
        "property var c: \"async function\" + 'await x' + `async ${a} await x` + /async x/.source",
        "property var c: a.async(1) + a?.await",
        "function f() { var async, await; async = 1; await = 2; return async + await }",
        "function f() { return async(1) }",
        "function f() { var o = { async: 1, await: 2 }; return o.async }",
        "function f(async, await) { return async }",
        "function f() { async: for(;;) break async }",
        "function f() { async\n await\n return 1 }",
        "function f() { return async in a }",
        "function f() { return async instanceof Object }",
        "function async() { return 1 }",
        "function await() { return 1 }",
        "function f() { function async() {} return async() }",
        "property var async: 1",
        "property var await: 1",
        "signal async()",
        "property var c: 1 // async function g() {}",
        "property var c: 1 /* await 1; async () => 1 */",
        "property var c: \"async function g() {}\"",
        "property var c: `async ${a} function await 1`",
        "property var c: /async function/.test(\"await 1\")",
        "function f() { return a.async + a.await + a?.async }",
        "function f() { var o = { async: 1, await: 2 }; return o.async }",
        "function f(async, await) { return async + await }",
        "property var async: 1",
        "property var await: async",
        "function async() { return 1 }",
        "function await() { return 1 }",
    ] {
        assert_eq!(load_error(member), "ok", "{member}");
    }
}

/// A label makes a binding a LabelledStatement, which is no UiScriptStatement: Qt stops at
/// the `:`, and what it would have accepted there instead depends on what follows.
#[test]
fn a_label_is_no_binding() {
    for (member, qt) in [
        ("property var c: lbl: 5", "3:24: Expected token `,'"),
        (
            "property var c: lbl: for (;;) break lbl",
            "3:24: Expected token `;'",
        ),
        ("property var c: lbl: ;", "3:24: Unexpected token `:'"),
        ("property var c: lbl: , 1", "3:24: Unexpected token `:'"),
        ("property var c: lbl: => 1", "3:24: Expected token `;'"),
        ("property var c: lbl: ) ", "3:24: Expected token `;'"),
        ("property var c: lbl: * 2", "3:24: Expected token `;'"),
        ("property var c: lbl:", "3:24: Expected token `,'"),
        ("property var c: lbl: property", "3:24: Expected token `,'"),
        ("property var c: lbl: signal", "3:24: Expected token `,'"),
        ("property var c: lbl: of", "3:24: Expected token `,'"),
        ("property var c: lbl: get", "3:24: Expected token `,'"),
        ("property var c: lbl: yield", "3:24: Expected token `,'"),
        ("property var c: lbl: super", "3:24: Expected token `,'"),
        ("property var c: lbl: import", "3:24: Expected token `;'"),
        ("property var c: lbl: export", "3:24: Expected token `;'"),
        ("property var c: lbl: enum", "3:24: Expected token `;'"),
        ("property var c: lbl: case", "3:24: Expected token `;'"),
        ("property var c: lbl: default", "3:24: Expected token `;'"),
        ("property var c: lbl: else", "3:24: Expected token `;'"),
        ("property var c: lbl: delete a", "3:24: Expected token `,'"),
        ("property var c: lbl: void 0", "3:24: Expected token `,'"),
        ("property var c: lbl: ++a", "3:24: Expected token `,'"),
        ("property var c: lbl: --a", "3:24: Expected token `,'"),
        ("property var c: lbl: ~a", "3:24: Expected token `,'"),
        ("property var c: lbl: +a", "3:24: Expected token `,'"),
        ("property var c: lbl: /re/", "3:24: Expected token `,'"),
        ("property var c: lbl: .5", "3:24: Expected token `,'"),
        ("property var c: lbl: ...a", "3:24: Expected token `;'"),
        ("property var c: lbl: in a", "3:24: Expected token `;'"),
        (
            "property var c: lbl: instanceof a",
            "3:24: Expected token `;'",
        ),
        ("property var c: lbl: static", "3:24: Expected token `,'"),
        ("property var c: lbl: as", "3:24: Expected token `,'"),
        ("property var c: lbl: on", "3:24: Expected token `,'"),
        ("property var c: lbl: readonly", "3:24: Expected token `,'"),
        ("property var c: lbl: required", "3:24: Expected token `,'"),
        ("property var c: lbl: component", "3:24: Expected token `,'"),
        (
            "property var c: lbl: function() {}",
            "3:24: Expected token `,'",
        ),
        ("property var c: lbl: { 5 }", "3:24: Expected token `,'"),
        (
            "property var c: lbl: while (false) {}",
            "3:24: Expected token `;'",
        ),
        ("property var c: lbl: if (a) 1", "3:24: Expected token `;'"),
        ("property var c: lbl: x = 1", "3:24: Expected token `,'"),
        ("property var c: lbl: var x = 1", "3:24: Expected token `;'"),
        (
            "property var c: lbl: function g() {}",
            "3:24: Expected token `,'",
        ),
        ("property var c: lbl: -1", "3:24: Expected token `,'"),
        ("property var c: lbl: \"s\"", "3:24: Expected token `,'"),
        ("property var c: lbl: [1]", "3:24: Expected token `,'"),
        ("property var c: lbl: (1)", "3:24: Expected token `,'"),
        (
            "property var c: lbl: do {} while (false)",
            "3:24: Expected token `;'",
        ),
        (
            "property var c: lbl: switch (a) {}",
            "3:24: Expected token `;'",
        ),
        (
            "property var c: lbl: try {} catch (e) {}",
            "3:24: Expected token `;'",
        ),
        ("property var c: lbl: return", "3:24: Expected token `;'"),
        ("property var c: lbl: throw 1", "3:24: Expected token `;'"),
        ("property var c: lbl: let x = 1", "3:24: Expected token `;'"),
        (
            "property var c: lbl: const x = 1",
            "3:24: Expected token `;'",
        ),
        ("property var c: lbl: break lbl", "3:24: Expected token `;'"),
        ("property var c: lbl: lbl2: 5", "3:24: Expected token `,'"),
        (
            "property var c: lbl: new Date()",
            "3:24: Expected token `,'",
        ),
        ("property var c: lbl: typeof a", "3:24: Expected token `,'"),
        ("property var c: lbl: !a", "3:24: Expected token `,'"),
        ("property var c: lbl: this", "3:24: Expected token `,'"),
        ("property var c: lbl: null", "3:24: Expected token `,'"),
        ("property var c: lbl: true", "3:24: Expected token `,'"),
        ("property var c: lbl: `t`", "3:24: Expected token `,'"),
        (
            "property var c: lbl: class A {}",
            "3:24: Expected token `,'",
        ),
        (
            "property var c: lbl: with (a) {}",
            "3:24: Expected token `;'",
        ),
        ("property var c: lbl: debugger", "3:24: Expected token `;'"),
        ("property var c: lbl: continue", "3:24: Expected token `;'"),
        (
            "property var c: lbl:\n        5",
            "3:24: Expected token `,'",
        ),
        (
            "property var c: lbl\n        : 5",
            "4:9: Unexpected token `:'",
        ),
        ("width: lbl: 5", "3:15: Expected token `,'"),
        ("width: lbl: for (;;) break lbl", "3:15: Expected token `;'"),
        ("onWidthChanged: lbl: 5", "3:24: Expected token `,'"),
        (
            "onWidthChanged: lbl: for (;;) break lbl",
            "3:24: Expected token `;'",
        ),
        ("onWidthChanged: lbl: { }", "3:24: Expected token `,'"),
    ] {
        assert_eq!(load_error(member), qt, "{member}");
    }
}

/// `name: ;` is an empty statement and loads; a `:` that begins a line goes on with the
/// binding only as the second half of a `?` of that binding.
#[test]
fn empty_statement_and_colon_after_a_line_break() {
    for (member, qt) in [
        ("property var c: ;", "ok"),
        ("property var c: ;;", "ok"),
        ("property var c:\n        ;", "ok"),
        ("property var c: ; property var d: 2", "ok"),
        ("width: ;", "ok"),
        ("onWidthChanged: ;", "ok"),
        (
            "property var c: lbl\n        : 5",
            "4:9: Unexpected token `:'",
        ),
        ("property var c: a ? 1\n        : 2", "ok"),
        ("property var c: a\n        ? 1\n        : 2", "ok"),
        ("property var c: a ? b ? 1 : 2\n        : 3", "ok"),
        (
            "property var c: a?.b\n        : 3",
            "4:9: Unexpected token `:'",
        ),
        (
            "property var c: a ?? 1\n        : 3",
            "4:9: Unexpected token `:'",
        ),
        ("property var c: a ? .5\n        : 3", "ok"),
        ("property var c: a ?.5\n        : 3", "ok"),
        (
            "property var c: (a ? 1 : 2)\n        : 3",
            "4:9: Unexpected token `:'",
        ),
        (
            "property var c: { a: 1 }\n        : 3",
            "4:9: Unexpected token `:'",
        ),
        (
            "property var c: [a ? 1 : 2][0]\n        ? 1\n        : 3",
            "ok",
        ),
        ("property var c: a ? { b: 1 }\n        : 3", "ok"),
        ("property var c: a ? x => x ? 1 : 2\n        : 3", "ok"),
        ("property var c: a ? \"?\"\n        : 3", "ok"),
    ] {
        assert_eq!(load_error(member), qt, "{member}");
    }
}

/// The value of an empty statement is undefined: a `var` property is undefined, a typed one
/// keeps its default, a handler does nothing. An alias names nothing with it.
#[test]
fn empty_statement_binding_value() {
    let log = run(
        &[],
        r#"import QtQuick
Item {
    id: root
    property var c: ;
    property var d: ;;
    readonly property var e: ;
    property var g:
        ;
    property int n: ;
    height: 7
    property int hits: 0
    onHeightChanged: ;
    onNChanged: hits += 1
    property var after: ; property int last: 2
    property string log: [typeof c, typeof d, typeof e, typeof g, n, height, last, hits].join(" ")
}"#,
    );
    assert_eq!(
        log.unwrap(),
        "undefined undefined undefined undefined 0 7 2 0"
    );
    let e = run(
        &[],
        "import QtQuick\nItem {\n    property int a: 1\n    property alias al: ;\n}",
    );
    let e = e.unwrap_err();
    assert!(
        e.ends_with(
            "main.qml:4:24: Invalid alias reference. An alias reference must be specified as \
             <id>, <id>.<property> or <id>.<value property>.<property>"
        ),
        "{e}"
    );
}

/// A name that no `:` or `{` follows: alone on its line or before `;` it is the grammar's
/// `UiQualifiedId Semicolon`, which is there to be reported at the name; on the same line Qt
/// stops at what follows it.
#[test]
fn a_name_alone_is_an_incomplete_binding() {
    for (member, qt) in [
        (
            "foo\n    bar: 1",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        ("foo bar", "3:9: Expected token `:'"),
        ("foo;", "3:5: Incomplete binding, expected token `:` or `{`"),
        ("foo", "3:5: Incomplete binding, expected token `:` or `{`"),
        (
            "foo.bar\n    baz: 1",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        ("foo 5", "3:9: Expected token `:'"),
        (
            "Foo.bar\n    x: 1",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    function f() {}",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        ("foo function f() {}", "3:9: Expected token `:'"),
        (
            "foo\n    property int z: 1",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    Item {}",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        ("foo = 1", "3:9: Expected token `:'"),
        (
            "foo\n    = 1",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        ("foo.bar baz", "3:13: Expected token `:'"),
        (
            "foo\n    5",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo ; x: 1",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo.bar;",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "async",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "await\n    x: 1",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        ("foo /* c */ bar", "3:17: Expected token `:'"),
        (
            "foo // c\n    bar: 1",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    signal s()",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    readonly property int q: 1",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    id: x",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    (1)",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        ("foo (1)", "3:9: Expected token `:'"),
        (
            "foo\n    [1]",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo [1]",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        ("foo ,", "3:9: Expected token `:'"),
        (
            "foo\n    ,",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    \"s\"",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        ("foo \"s\"", "3:9: Expected token `:'"),
        (
            "on\n    x: 1",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    component A: Item {}",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    enum E { A }",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    required property int r",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
        (
            "foo\n    default property int r",
            "3:5: Incomplete binding, expected token `:` or `{`",
        ),
    ] {
        assert_eq!(load_error(member), qt, "{member}");
    }
}

/// `function f() { <body> }` called from a binding: its value with the type, or the error.
fn call(files: &[(&str, &str)], imports: &str, body: &str) -> String {
    let main = format!(
        r#"import QtQuick
{imports}Item {{
    id: root
    function t(f) {{ try {{ var v = f(); return typeof v === "string" ? v : typeof v + ":" + v }} catch (e) {{ return "!" + e }} }}
    function f() {{ {body} }}
    property string log: t(() => f())
}}"#
    );
    run(files, &main).unwrap()
}

/// QQmlJS has no async functions, so `async` in `for (async of xs)` is the name of the
/// variable the loop assigns, like any other name. ECMAScript forbids these three tokens
/// there (they would start `for (async of => ...;;)`).
#[test]
fn for_async_of_assigns_the_name_async() {
    for (body, qt) in [
        ("for (async of []) {} return 1", "number:1"),
        (
            r#"var async; var s = ""; for (async of [1, 2]) s += async; return s + async"#,
            "122",
        ),
        (
            r#"var s = ""; for (async of [1, 2]) s += async; return s"#,
            r#"!Error: Invalid write to global property "async""#,
        ),
        (
            "var async = 0; for (async in {a: 1, b: 2}) {} return async",
            "b",
        ),
        (
            "var async = {}; for (async.x of [1, 2]) {} return async.x",
            "number:2",
        ),
        (
            "var async = 0; for ((async) of [1, 2]) {} return async",
            "number:2",
        ),
        (
            r#"var s = ""; for (let async of [1, 2]) s += async; return s"#,
            "12",
        ),
        (
            r#"var s = ""; for (var async of [1, 2]) s += async; return s"#,
            "12",
        ),
        (
            r#"var s = ""; for (const async of "ab") s += async; return s"#,
            "ab",
        ),
        (
            "var async = 0; for (async of[3]) {} return async",
            "number:3",
        ),
        (
            "var async = 0; for ( async of [6, 7] ) { if (async > 5) break } return async",
            "number:6",
        ),
        (
            "var await = 0; for (await of [8]) {} return await",
            "number:8",
        ),
        (
            "var async = 0, of = [9]; for (async of of) {} return async",
            "number:9",
        ),
        ("var of = 0; for (of of [1, 2]) {} return of", "number:2"),
        (
            "var async = 0; for (async of [1]) for (async of [2]) {} return async",
            "number:2",
        ),
        (
            "var async = [0]; for (async[0] of [3]) {} return async[0]",
            "number:3",
        ),
        (
            "var async = 0, o = [1, 2]; for (async of o) continue; return async",
            "number:2",
        ),
        (
            "var async = 5; for (async; async < 7; async++) {} return async",
            "number:7",
        ),
        (
            "var async = 5, of = 1; for (async = of; async < 7; async++) {} return async",
            "number:7",
        ),
        (
            "var async = 0; var g = () => { for (async of [7]) {} }; g(); return async",
            "number:7",
        ),
        // The three tokens with anything between them that is no token.
        (
            "var async = 0; for(async\n of [4]) {} return async",
            "number:4",
        ),
        (
            "var async = 0; for\n(\nasync\nof\n[5]) {} return async",
            "number:5",
        ),
        (
            "var async = 0; for ( async /* x */ of [7]) {} return async",
            "number:7",
        ),
        (
            "var async = 0; for (async // c\n of [8]) {} return async",
            "number:8",
        ),
        (
            "var async = 0; for /* for (async of */ (async of [9]) {} return async",
            "number:9",
        ),
        // The same text where it is no code stays as written.
        (
            r#"var a = "for (async of [1])", b = 'for (async of x)'; return a + "|" + b + "|" + a.length"#,
            "for (async of [1])|for (async of x)|18",
        ),
        (
            r#"var c = `for (async of ${1 + 1}) ${ "for (async of y)" }`; return c + "|" + c.length"#,
            "for (async of 2) for (async of y)|33",
        ),
        (
            r#"return /for (async of)/.source + "|" + String(/for \(async of/)"#,
            r#"for (async of)|/for \(async of/"#,
        ),
        (
            "var async = 1; /* for (async of z) */ // for (async of w)\n return async",
            "number:1",
        ),
        // Code inside a template's `${}` is code.
        (
            "return `a${(() => { var async; for (async of [3]) {} return async })()}b for (async of [])`",
            "a3b for (async of [])",
        ),
        (
            r#"return eval("var async; for (async of [6]) {} async")"#,
            "number:6",
        ),
        (
            r#"return new Function("var async; for (async of [5]) {} return async")()"#,
            "number:5",
        ),
        (
            r#"return new Function("a = (() => { var async; for (async of [4]) {} return async })()", "return a")()"#,
            "number:4",
        ),
    ] {
        assert_eq!(call(&[], "", body), qt, "{body}");
    }
    let lib = "function run() { var async; for (async of [1]) {} return async + \"|\" + \"for (async of [])\" }\n";
    assert_eq!(
        call(
            &[("lib.js", lib)],
            "import \"lib.js\" as J\n",
            "return J.run()"
        ),
        "1|for (async of [])"
    );
    // In a binding and a handler, on a property of the object.
    let log = run(
        &[],
        r#"import QtQuick
Item {
    id: root
    property var async: 0
    property var b: { for (async of [3, 4]) {} return async }
    property var c: { var async = 1; for (async of [5]) {}; async }
    property string log: b + "," + c + "," + async
}"#,
    );
    assert_eq!(log.as_deref(), Ok("4,5,4"));
}

/// What follows `for (async of` on its line is where Qt has it: a syntax error there has
/// Qt's column.
#[test]
fn an_error_after_for_async_of_has_its_column() {
    for (member, qt) in [
        (
            "function f() { var async; for (async of [1]) {} var g = async function() {} }",
            "3:67: Expected token `;'",
        ),
        (
            "property var c: { var async; for (async of [1]) {} ; await g() }",
            "3:64: Expected token `,'",
        ),
        (
            "onWidthChanged: { for (async of [1]) {} async () => 1 }",
            "3:54: Expected token `,'",
        ),
        (
            "function f() { for (async of [1]) {} for await (x of y) {} }",
            "3:46: Unexpected token `identifier'",
        ),
        (
            "function f() { for (async of [1]) { for (async /* c */ of [2]) {} } var g = async x => 1 }",
            "3:87: Expected token `;'",
        ),
    ] {
        assert_eq!(load_error(member), qt, "{member}");
    }
}
