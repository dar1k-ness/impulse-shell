//! The JS object of a QML type and of an import qualifier (QQmlTypeWrapper): what it equals,
//! what it converts to and what can be read on it. Each case is an expression and what Qt
//! 6.12 gives for it (PySide6 6.12.0, QQmlComponent); an address is written `0x`.

use qml::runtime::Runtime;

/// The expression, and `typeof: value` of its value (the value alone for a string, `!` and
/// the error when it throws).
const CASES: &[(&str, &str)] = &[
    ("Here.Nope === Here.Other", "boolean:true"),
    ("Here.Nope == Here.Other", "boolean:true"),
    ("Here.Nope !== Here.Other", "boolean:false"),
    ("Here.Nope === Here.Sub", "boolean:true"),
    ("Here.Sub === Sub", "boolean:true"),
    ("Here.Nope === Item", "boolean:true"),
    ("Item === Rectangle", "boolean:true"),
    ("Item == Rectangle", "boolean:true"),
    ("Item !== Rectangle", "boolean:false"),
    ("Item != Rectangle", "boolean:false"),
    ("Item === Item", "boolean:true"),
    ("Q.Item === C.Button", "boolean:true"),
    ("Q.Item === Q.Rectangle", "boolean:true"),
    ("Item === Keys", "boolean:false"),
    ("Keys === Keys", "boolean:true"),
    ("Keys === LayoutMirroring", "boolean:false"),
    ("Layout === Keys", "boolean:false"),
    ("Layout === Layout", "boolean:true"),
    ("Item === Screen", "boolean:false"),
    ("Item === Qt", "boolean:false"),
    ("Item === Text", "boolean:true"),
    ("Text === ListView", "boolean:false"),
    ("Component === Item", "boolean:false"),
    ("Component === Component", "boolean:true"),
    ("Here === Here", "boolean:false"),
    ("Q === Q", "boolean:false"),
    ("Here === Q", "boolean:false"),
    ("Here == Here", "boolean:false"),
    ("Item === root", "boolean:false"),
    ("Item == root", "boolean:false"),
    ("Keys == root", "boolean:false"),
    ("root == Keys", "boolean:false"),
    ("root === Keys", "boolean:false"),
    ("Item === null", "boolean:false"),
    ("Item == null", "boolean:false"),
    ("Item == undefined", "boolean:false"),
    ("Item === ({})", "boolean:false"),
    ("Item == \"QtQuick/Item\"", "boolean:true"),
    ("Item == 0", "boolean:false"),
    ("Object.is(Item, Item)", "boolean:true"),
    ("Object.is(Item, Rectangle)", "boolean:true"),
    ("[Item].indexOf(Rectangle)", "number:0"),
    ("[Item].includes(Rectangle)", "boolean:true"),
    ("[Keys, Item].indexOf(Rectangle)", "number:1"),
    ("[Item, Keys].lastIndexOf(Keys)", "number:1"),
    (
        "(function(){ switch (Rectangle) { case Keys: return \"keys\"; case Item: return \"item\"; default: return \"default\" } })()",
        "item",
    ),
    ("new Set([Item, Rectangle, Item]).size", "number:1"),
    ("new Map([[Item, 1]]).get(Rectangle)", "number:1"),
    ("new Map([[Item, 1]]).get(Item)", "number:1"),
    ("String(Item)", "QtQuick/Item"),
    ("\"\" + Item", "QtQuick/Item"),
    ("`${Rectangle}`", "QtQuick/Rectangle"),
    ("Item.toString()", "QtQuick/Item"),
    ("String(Q.Item)", "QtQuick/Item"),
    ("String(C.Button)", "Button"),
    ("String(Sub)", "Sub"),
    ("String(Here.Sub)", "Sub"),
    ("String(Here.Nope)", "Nope"),
    ("String(Keys)", "QQuickKeysAttached(0x)"),
    ("String(Layout)", "QQuickLayoutAttached(0x)"),
    ("String(RowLayout)", "QtQuick.Layouts/RowLayout"),
    ("String(Component)", "QQmlComponentAttached(0x)"),
    ("String(ListModel)", "QtQml.Models/ListModel"),
    ("String(QtObject)", "QML/QtObject"),
    ("String(Text)", "QtQuick/Text"),
    ("String(Here)", "!TypeError: Type error"),
    ("\"\" + Here", "!TypeError: Type error"),
    ("typeof Here.toString", "object"),
    ("String(Here.toString)", "toString"),
    ("typeof Here.valueOf", "object"),
    ("String(Q)", "Unknown Type"),
    ("\"\" + Q", "Unknown Type"),
    ("typeof Q.toString", "function"),
    ("Q.toString()", "Unknown Type"),
    ("String(C)", "Unknown Type"),
    ("JSON.stringify(Item)", "{}"),
    ("JSON.stringify({a: Item, b: Q})", "{\"a\":{},\"b\":{}}"),
    ("typeof Item.toString", "function"),
    ("Item.toString === Rectangle.toString", "boolean:true"),
    ("Item.toString === Q.toString", "boolean:true"),
    ("Item.toString.call(root)", "undefined:undefined"),
    ("Item.toString.call(Here)", "Unknown Type"),
    ("Item.toString.call(Q)", "Unknown Type"),
    ("Item.toString.call(Here.Nope)", "Nope"),
    ("Item.toString.call(1)", "undefined:undefined"),
    ("Item.toString.name", "toString"),
    ("Item.toString.length", "number:0"),
    ("Item.hasOwnProperty(\"toString\")", "boolean:true"),
    ("\"toString\" in Item", "boolean:true"),
    ("typeof Item.valueOf", "function"),
    ("Item.valueOf() === Item", "boolean:true"),
    (
        "Object.getPrototypeOf(Item) === Object.getPrototypeOf(Rectangle)",
        "boolean:true",
    ),
    (
        "Object.getPrototypeOf(Item) === Object.getPrototypeOf(Q)",
        "boolean:true",
    ),
    (
        "Object.getPrototypeOf(Item) === Object.getPrototypeOf(Here.Nope)",
        "boolean:true",
    ),
    (
        "Object.getPrototypeOf(Item) === Function.prototype",
        "boolean:false",
    ),
    (
        "Object.prototype.toString.call(Item)",
        "[object QMLTypeWrapper]",
    ),
    (
        "Object.prototype.toString.call(Q)",
        "[object QMLTypeWrapper]",
    ),
    (
        "Object.prototype.toString.call(Here.Nope)",
        "[object QMLTypeWrapper]",
    ),
    ("Object.keys(Item).length", "number:0"),
    ("Item + 1", "QtQuick/Item1"),
    ("Item + \"\"", "QtQuick/Item"),
    ("[Item] + \"\"", "QtQuick/Item"),
    ("String([Item, Sub, Here.Nope])", "QtQuick/Item,Sub,Nope"),
    ("[Item, Sub].join(\"+\")", "QtQuick/Item+Sub"),
    ("Item.toString.call(Text)", "QtQuick/Text"),
    ("String(Item.TopLeft)", "0"),
    ("typeof Item", "object"),
    ("typeof Q", "object"),
    ("typeof Here", "object"),
    ("!Item", "boolean:false"),
    ("Item ? 1 : 2", "number:1"),
    ("Item.toLocaleString()", "QtQuick/Item"),
    ("Number(Item)", "number:NaN"),
    ("Item < Rectangle", "boolean:true"),
    ("Item <= Rectangle", "boolean:true"),
    ("String(Object.create(null))", "!TypeError: Type error"),
    (
        "\"\" + ({toString: 1, valueOf: 2})",
        "!TypeError: Type error",
    ),
    (
        "\"\" + ({toString() { return {} }, valueOf() { return {} }})",
        "!TypeError: Type error",
    ),
    ("`${Object.create(null)}`", "!TypeError: Type error"),
    ("Number(Object.create(null))", "!TypeError: Type error"),
    ("Object.create(null) < 1", "!TypeError: Type error"),
    ("[Object.create(null)].join()", "!TypeError: Type error"),
    ("String(T.Button)", "QtQuick.Templates/Button"),
    ("String(T.Control)", "QtQuick.Templates/Control"),
    ("String(C.Control)", "Control"),
    (
        "String(C.ApplicationWindow)",
        "QQuickApplicationWindowAttached(0x)",
    ),
    ("String(Window)", "QQuickWindowAttached(0x)"),
    ("String(Q.Window)", "QQuickWindowAttached(0x)"),
    ("String(Timer)", "QtQml/Timer"),
    ("String(Connections)", "QtQml/Connections"),
    ("String(Binding)", "QtQml/Binding"),
    ("String(Animation)", "QtQuick/Animation"),
    ("String(NumberAnimation)", "QtQuick/NumberAnimation"),
    ("String(Loader)", "QtQuick/Loader"),
    ("String(Repeater)", "QtQuick/Repeater"),
    ("String(Flickable)", "QtQuick/Flickable"),
    ("String(GridLayout)", "QtQuick.Layouts/GridLayout"),
    ("String(TextInput)", "QtQuick/TextInput"),
    ("String(MouseArea)", "QtQuick/MouseArea"),
    ("String(Image)", "QtQuick/Image"),
    ("String(Font)", "QtQuick/Font"),
    ("String(Screen)", "QQuickScreenAttached(0x)"),
    ("Item === Sub", "boolean:true"),
    ("Sub === Here.Sub", "boolean:true"),
    ("Item === Flickable", "boolean:true"),
    ("ListView === ListView", "boolean:true"),
    ("Keys === Q.Keys", "boolean:true"),
    ("Item != Keys", "boolean:true"),
    ("T.Button === C.Button", "boolean:true"),
    (
        "\"\" + ({[Symbol.toPrimitive]() { return {} }})",
        "!TypeError: Type error",
    ),
    ("\"toString\" in Q", "boolean:true"),
    ("\"Item\" in Q", "boolean:true"),
    ("Object.keys(Text).length", "number:0"),
    ("Object.keys(Text.HAlignment).length", "number:0"),
    ("JSON.stringify(Text)", "{}"),
    ("Text.AlignHCenter", "number:4"),
    ("Q.Text.AlignHCenter", "number:4"),
    ("Q.toString === Item.toString", "boolean:true"),
    ("Here.toString === Here.Nope", "boolean:true"),
    (
        "new Set([Item, Rectangle, Keys, Keys, Layout]).size",
        "number:3",
    ),
    ("new Map([[Keys, 1]]).get(Item)", "undefined:undefined"),
    ("new Map([[Keys, 1]]).get(Keys)", "number:1"),
    ("Object.is(Item, Text)", "boolean:true"),
    (
        "[Item, Sub, Here.Nope].join(\"+\")",
        "QtQuick/Item+Sub+Nope",
    ),
];

#[test]
fn type_objects_are_what_qt_makes_them() {
    let cases: Vec<String> = (CASES.iter())
        .map(|(c, _)| format!("        () => {c}"))
        .collect();
    let main = format!(
        r#"import QtQuick
import QtQuick as Q
import QtQuick.Controls as C
import QtQuick.Templates as T
import QtQuick.Layouts
import "." as Here
Item {{
    id: root
    function t(f) {{
        try {{
            var v = f()
            return (typeof v === "string" ? v : typeof v + ":" + v).replace(/0x[0-9a-f]+/g, "0x")
        }} catch (e) {{
            return "!" + e
        }}
    }}
    property var cases: [
{}
    ]
    property string log: cases.map(f => t(f)).join("\n")
}}"#,
        cases.join(",\n")
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Sub.qml"), "import QtQuick\nItem {}\n").unwrap();
    let f = dir.path().join("main.qml");
    std::fs::write(&f, main).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    assert_eq!(rt.errors(), Vec::<String>::new());
    let log = rt.get(root, "log").to_js_string();
    let ours: Vec<&str> = log.split('\n').collect();
    assert_eq!(ours.len(), CASES.len());
    let wrong: Vec<String> = (CASES.iter().zip(ours))
        .filter(|((_, qt), ours)| qt != ours)
        .map(|((c, qt), ours)| format!("{c}\n  qt:   {qt}\n  ours: {ours}"))
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The files of `tests/data/type_objects`: on each line an expression, a tab and what Qt 6.12
/// gives for it, written as in [`CASES`]. The expressions of one file are the functions of
/// one array, called in order, so what one writes the next reads, as Qt was given them.
/// `eq`: one name `===` another. `string`, `typeof`: `String` and `typeof` of each name.
/// `proto`: what a type, a type with an attached type, an import qualifier, a name on it,
/// `Qt` and a singleton have in common with a function: `call`, `bind`, `apply`, `name`,
/// `length`, the prototype and its names, own names, writes. `misc`: the rest.
#[test]
fn names_of_types_are_what_qt_makes_them() {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/type_objects");
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("s")).unwrap();
    for f in ["Sub.qml", "s/qmldir", "s/Single.qml"] {
        std::fs::copy(data.join(f), dir.path().join(f)).unwrap();
    }
    let mut wrong = Vec::new();
    for name in ["eq", "misc", "proto", "string", "typeof"] {
        let text = std::fs::read_to_string(data.join(format!("{name}.txt"))).unwrap();
        let cases: Vec<(&str, &str)> = (text.lines())
            .map(|l| l.split_once('\t').unwrap())
            .collect();
        let fns: Vec<String> = (cases.iter())
            .map(|(c, _)| format!("        () => {c}"))
            .collect();
        let main = format!(
            r#"import QtQuick
import QtQuick as Q
import QtQuick.Controls as C
import QtQuick.Controls
import QtQuick.Templates as T
import QtQuick.Layouts
import QtQuick.Window
import QtQml
import QtQml.Models
import "." as Here
import "s"
import "s" as S
Item {{
    id: root
    function t(f) {{ try {{ var v = f(); return typeof v === "string" ? v : typeof v + ":" + v }} catch (e) {{ return "!" + e }} }}
    property var cases: [
{}
    ]
    property string result: cases.map(f => t(f)).join(" | ")
    Item {{
        id: other
        Keys.enabled: false
        Layout.fillWidth: true
        function kk() {{ return Keys }}
        function ll() {{ return ListView }}
        function ii() {{ return Item }}
        function ks() {{ return String(Keys) }}
    }}
}}
"#,
            fns.join(",\n")
        );
        let f = dir.path().join(format!("{name}.qml"));
        std::fs::write(&f, main).unwrap();
        let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
        let root = rt.load_file(&f).unwrap();
        rt.settle();
        let result = rt.get(root, "result").to_js_string();
        let ours: Vec<String> = (result.split(" | "))
            .map(|v| {
                let mut out = String::new();
                let mut rest = v;
                // An address: `0x` and its digits, written `0x`.
                while let Some(i) = rest.find("0x") {
                    out += &rest[..i + 2];
                    rest = rest[i + 2..].trim_start_matches(|c: char| {
                        c.is_ascii_digit() || ('a'..='f').contains(&c)
                    });
                }
                out + rest
            })
            .collect();
        assert_eq!(ours.len(), cases.len(), "{name}");
        for ((c, qt), ours) in cases.iter().zip(&ours) {
            if qt != ours {
                wrong.push(format!("{name}: {c}\n  qt:   {qt}\n  ours: {ours}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
