//! What a `string` property holds after a value that is not a string is written to it, as
//! Qt 6.12 answers: a number, a `bool`, `undefined` and `null`, assigned by a script and
//! through a binding, to `text` of Text, TextInput, TextEdit and Label, to `objectName` and
//! to a `property string` of the document. Each case reads the property back (its `typeof`,
//! its value, its `length`) and counts the change signal. The document runs in this engine
//! and in Qt itself (QQmlEngine through the PySide6 of ri-testenv on the image's own Qt,
//! tools/testenv/system_qt.py), and the two answers must be the same lines.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

/// The Python with PySide6 in ri-testenv (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// Loads the document argv[1], calls its `run()` and prints its `out` property. argv[2] is
/// tools/testenv, where system_qt.py is.
const QT_RUN: &str = r#"
import sys
sys.path.insert(0, sys.argv[2])
import system_qt
from PySide6.QtCore import QUrl, QMetaObject, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlEngine, QQmlComponent
qInstallMessageHandler(lambda mode, context, message: None)
app = QGuiApplication(sys.argv[:1])
engine = QQmlEngine()
component = QQmlComponent(engine, QUrl.fromLocalFile(sys.argv[1]))
root = component.create()
system_qt.checked()
if root is None:
    sys.exit("; ".join(e.toString() for e in component.errors()))
QMetaObject.invokeMethod(root, "run")
sys.stdout.write(root.property("out"))
"#;

/// (name in the answers, type, its `string` property, the handler of its change signal).
const TARGETS: [(&str, &str, &str, &str); 6] = [
    ("Text", "Text", "text", "onTextChanged"),
    ("TextInput", "TextInput", "text", "onTextChanged"),
    ("TextEdit", "TextEdit", "text", "onTextChanged"),
    ("Label", "Label", "text", "onTextChanged"),
    ("QtObject", "QtObject", "objectName", "onObjectNameChanged"),
    ("own", "QtObject", "s", "onSChanged"),
];

/// Expressions a script assigns.
const VALUES: [&str; 29] = [
    "3",
    "-0",
    "2.5",
    "0.1 + 0.2",
    "1 / 3",
    "123456.789",
    // A whole number is written by its digits, a double that happens to be whole as a
    // double (`1e+05`).
    "100000",
    "Math.round(100000.4)",
    "200000 * 0.5",
    "12345678.5",
    "0.0001",
    "0.00001234",
    "0.000001",
    "1e-7",
    "5e-324",
    "2147483648",
    "123456789012",
    "1e15",
    "1e20",
    "123456789012345680000",
    "1e21",
    "1.5e300",
    "NaN",
    "Infinity",
    "-Infinity",
    "true",
    "false",
    "undefined",
    "null",
];

/// Properties of the root a binding reads: an `int`, a `real`, a `bool`, two `var`s holding
/// `undefined` and `null`, a `real` that is whole and an empty `string`.
const SOURCES: [&str; 7] = ["whole", "half", "flag", "unset", "none", "round", "empty"];

fn document() -> String {
    let (mut objects, mut writes, mut bound) = (String::new(), String::new(), String::new());
    for (name, ty, prop, handler) in TARGETS {
        let id = name.to_lowercase();
        let own = if name == "own" {
            "property string s; "
        } else {
            ""
        };
        let counted = format!("{own}property int hits: 0; {handler}: hits++");
        objects += &format!("    property {ty} {id}Written: {ty} {{ {counted} }}\n");
        for v in VALUES {
            writes += &format!(
                "        write(\"{name}.{prop} = {v}\", {id}Written, \"{prop}\", () => {v});\n"
            );
        }
        for s in SOURCES {
            objects +=
                &format!("    property {ty} {id}_{s}: {ty} {{ {counted}; {prop}: root.{s} }}\n");
            bound += &format!("        bound(\"{name}.{prop}: {s}\", {id}_{s}, \"{prop}\");\n");
        }
    }
    format!(
        r#"import QtQuick
import QtQuick.Controls

Item {{
    id: root
    property string out
    property var lines: []
    property int whole: 3
    property real half: 2.5
    property bool flag: true
    property var unset: undefined
    property var none: null
    property real round: 100000
    property string empty
{objects}
    function show(v) {{
        return typeof v + " " + (typeof v === "string" ? JSON.stringify(v) : String(v))
            + " length " + (v === undefined || v === null ? "none" : String(v.length));
    }}
    function attempt(o, p, v) {{
        try {{ o[p] = v(); return "ok"; }} catch (e) {{ return "threw " + e.name + ": " + e.message; }}
    }}
    // The property holds "x", the value is assigned twice: what the first assignment did,
    // what the property reads, how often it changed on the first and on the second.
    function write(what, o, p, v) {{
        o[p] = "x";
        const before = o.hits;
        const first = attempt(o, p, v);
        const once = o.hits - before;
        attempt(o, p, v);
        lines.push(what + " -> " + first + ", " + show(o[p]) + ", changed " + once + " then "
            + (o.hits - before - once));
    }}
    function bound(what, o, p) {{
        lines.push(what + " -> " + show(o[p]) + ", changed " + o.hits);
    }}
    function run() {{
{writes}{bound}
        whole = 4; half = 0.5; flag = false; unset = 5; none = 6; round = 7; empty = "e";
        lines.push("after the sources changed");
{bound}
        unset = undefined; none = null; round = 100000; empty = "";
        lines.push("after undefined and null came back");
{bound}
        out = lines.join("\n");
    }}
}}
"#
    )
}

fn qt_answer(dir: &std::path::Path, document: &str) -> String {
    let (script, file) = (dir.join("run.py"), dir.join("Case.qml"));
    std::fs::write(&script, QT_RUN).unwrap();
    std::fs::write(&file, document).unwrap();
    let out = std::process::Command::new(PYTHON)
        .arg(&script)
        .arg(&file)
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/testenv"))
        .output()
        .unwrap();
    match out.status.success() {
        true => String::from_utf8(out.stdout).unwrap(),
        false => format!(
            "load error: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
    }
}

fn our_answer(document: &str) -> String {
    let mut rt = Runtime::new(None);
    let root = match rt.load_str(document) {
        Ok(root) => root,
        Err(e) => return format!("load error: {}", e.replace('\n', " ")),
    };
    rt.settle();
    match rt.call_method(root, "run", vec![]) {
        Some(Ok(_)) => rt.get(root, "out").to_js_string(),
        Some(Err(e)) => format!("run() threw: {}", e.replace('\n', " ")),
        None => "no run()".into(),
    }
}

/// A whole number written by a script is written by its digits, in Qt and here.
const WHOLE: &str = "Text.text = 100000 -> ok, string \"100000\" length 6, changed 1 then 0";

/// The lines that differ from Qt 6.12, (Qt's, ours): a double that happens to be whole is
/// a double to QV4, and QString::number writes one of 100000 as `1e+05`; a number here is
/// one kind of value, written by its digits (docs/qt-differences.md).
fn whole_doubles() -> Vec<(String, String)> {
    let (qt, ours) = ("string \"1e+05\" length 5", "string \"100000\" length 6");
    let mut lines = Vec::new();
    for (name, _, prop, _) in TARGETS {
        for what in [
            " = Math.round(100000.4) -> ok, $, changed 1 then 0",
            " = 200000 * 0.5 -> ok, $, changed 1 then 0",
            ": round -> $, changed 1",
            ": round -> $, changed 3",
        ] {
            // A binding to objectName (a QProperty) gives it the digits in Qt too.
            if prop == "objectName" && what.starts_with(':') {
                continue;
            }
            let line = format!("{name}.{prop}{what}");
            lines.push((line.replace('$', qt), line.replace('$', ours)));
        }
    }
    lines
}

/// `document` answers here what it answers in Qt, in at least `least` lines, but for the
/// `known` pairs of lines (Qt's, ours), each of which is answered so.
fn same_as_qt(document: &str, least: usize, known: &[(String, String)]) {
    let dir = tempfile::tempdir().unwrap();
    let (qt, ours) = (qt_answer(dir.path(), document), our_answer(document));
    let (qt, ours): (Vec<&str>, Vec<&str>) = (qt.lines().collect(), ours.lines().collect());
    assert!(qt.len() >= least, "Qt answered:\n{}", qt.join("\n"));
    let pair = |k: usize| (qt.get(k).copied(), ours.get(k).copied());
    let is_known = |k: usize| {
        known
            .iter()
            .any(|(a, b)| pair(k) == (Some(a.as_str()), Some(b.as_str())))
    };
    let lines = qt.len().max(ours.len());
    let mut wrong: Vec<String> = (0..lines)
        .filter(|&k| qt.get(k) != ours.get(k) && !is_known(k))
        .map(|k| {
            format!(
                "  Qt:   {}\n  ours: {}",
                qt.get(k).unwrap_or(&"(no line)"),
                ours.get(k).unwrap_or(&"(no line)")
            )
        })
        .collect();
    let count = wrong.len();
    for (a, b) in known {
        if !(0..lines).any(|k| pair(k) == (Some(a.as_str()), Some(b.as_str()))) {
            wrong.push(format!(
                "  a known difference is not answered so:\n  Qt:   {a}\n  ours: {b}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{count} of {} lines differ from Qt 6.12:\n{}",
        qt.len(),
        wrong.join("\n")
    );
    assert!(qt.len() < 250 || qt.contains(&WHOLE), "Qt: no `{WHOLE}`");
}

/// Whether the caller is the test `name` in a process of its own, with Qt there to ask.
fn alone_with_qt(name: &str) -> bool {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
        return false;
    }
    if !testenv::in_own_process(name) {
        testenv::run_in_own_process(name, [("QT_QPA_PLATFORM", "offscreen")]);
        return false;
    }
    true
}

#[test]
fn a_string_property_holds_what_qt_holds() {
    if alone_with_qt(concat!(
        module_path!(),
        "::a_string_property_holds_what_qt_holds"
    )) {
        same_as_qt(&document(), 250, &whole_doubles());
    }
}

/// (type, its property, the handler of its change signal, what the binding gives): bindings
/// that give, when the object is made, `undefined`, `null` or the value the property holds
/// already.
const UNCHANGED: [(&str, &str, &str, &str); 21] = [
    ("Text", "text", "onTextChanged", "root.unset"),
    ("Text", "text", "onTextChanged", "root.none"),
    ("Text", "text", "onTextChanged", "root.empty"),
    ("Text", "text", "onTextChanged", "undefined"),
    ("Text", "text", "onTextChanged", "\"\""),
    ("TextInput", "text", "onTextChanged", "root.unset"),
    ("TextInput", "text", "onTextChanged", "root.empty"),
    ("TextEdit", "text", "onTextChanged", "root.unset"),
    ("TextEdit", "text", "onTextChanged", "root.empty"),
    ("Text", "color", "onColorChanged", "root.unset"),
    ("Text", "color", "onColorChanged", "root.black"),
    ("Text", "elide", "onElideChanged", "root.unset"),
    ("Text", "elide", "onElideChanged", "root.zero"),
    ("Text", "wrapMode", "onWrapModeChanged", "root.zero"),
    ("Item", "objectName", "onObjectNameChanged", "root.unset"),
    ("Item", "objectName", "onObjectNameChanged", "root.empty"),
    ("Item", "objectName", "onObjectNameChanged", "root.none"),
    ("Item", "width", "onWidthChanged", "root.unset"),
    ("Item", "width", "onWidthChanged", "root.zero"),
    ("Item", "opacity", "onOpacityChanged", "root.one"),
    ("Item", "visible", "onVisibleChanged", "root.yes"),
];

fn unchanged_document() -> String {
    let (mut objects, mut lines) = (String::new(), String::new());
    for (k, (ty, prop, handler, source)) in UNCHANGED.into_iter().enumerate() {
        objects += &format!(
            "    property {ty} o{k}: {ty} {{ property int hits: 0; {handler}: hits++; {prop}: {source} }}\n"
        );
        let what = format!("{ty}.{prop}: {source}").replace('"', "'");
        lines += &format!(
            "        lines.push(\"{what} -> \" + typeof o{k}.{prop} + \" \" + JSON.stringify(o{k}.{prop}) + \", changed \" + o{k}.hits);\n"
        );
    }
    format!(
        r#"import QtQuick

Item {{
    id: root
    property string out
    property var lines: []
    property var unset: undefined
    property var none: null
    property string empty
    property color black: "black"
    property int zero: 0
    property real one: 1
    property bool yes: true
{objects}
    function run() {{
{lines}
        out = lines.join("\n");
    }}
}}
"#
    )
}

#[test]
fn a_binding_that_gives_what_the_property_holds_changes_nothing() {
    if alone_with_qt(concat!(
        module_path!(),
        "::a_binding_that_gives_what_the_property_holds_changes_nothing"
    )) {
        same_as_qt(&unchanged_document(), UNCHANGED.len(), &[]);
    }
}
