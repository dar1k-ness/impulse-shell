//! Expression corpora answered the way Qt 6.12 answers them: `String.prototype.arg`, number
//! parsing and rounding, direct `eval`, `with`, `Qt.formatDate`/`formatTime`/`formatDateTime`,
//! `Date.parse` and a Date's strings, and non-ASCII names in a QML document. Every case runs
//! in this engine and in Qt itself (the QQmlEngine of ri-testenv's system Qt, asked through
//! PySide6 and tools/testenv/system_qt.py) in the same time zone and locale, and the two
//! answers must be the same string: the type and value of the result, or the name and message
//! of what it threw.
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
from PySide6.QtCore import QCoreApplication, QUrl, QMetaObject, qInstallMessageHandler
from PySide6.QtQml import QQmlEngine, QQmlComponent
qInstallMessageHandler(lambda mode, context, message: None)
app = QCoreApplication(sys.argv[:1])
engine = QQmlEngine()
component = QQmlComponent(engine, QUrl.fromLocalFile(sys.argv[1]))
system_qt.checked()
root = component.create()
if root is None:
    sys.exit("; ".join(e.toString() for e in component.errors()))
QMetaObject.invokeMethod(root, "run")
sys.stdout.write(root.property("out"))
"#;

/// One function body per line; `#` starts a comment line.
const CORPORA: [(&str, &str); 8] = [
    ("string_arg", include_str!("expr_corpus/string_arg.txt")),
    ("numbers", include_str!("expr_corpus/numbers.txt")),
    ("eval", include_str!("expr_corpus/eval.txt")),
    ("with", include_str!("expr_corpus/with.txt")),
    ("format_date", include_str!("expr_corpus/format_date.txt")),
    ("date_parse", include_str!("expr_corpus/date_parse.txt")),
    ("date_zones", include_str!("expr_corpus/date_zones.txt")),
    ("date_bounds", include_str!("expr_corpus/date_bounds.txt")),
];

/// Whole documents: `run()` leaves the answer in `out`.
const DOCUMENTS: [(&str, &str); 4] = [
    (
        "a property, its change handler and an id with non-ASCII names",
        r#"import QtQml
QtObject {
    id: корень
    property string out
    property int ёж: 1
    property int n: 0
    property int δx: ёж * 2
    onЁжChanged: n++
    function сумма(a, b) { return a + b }
    function run() {
        ёж = 5;
        var größe = сумма(ёж, n);
        out = [ёж, n, δx, größe, корень.ёж, typeof ёжChanged].join(" ");
    }
}
"#,
    ),
    (
        "signals and handlers with non-ASCII names, a letter number and an astral letter",
        r#"import QtQml
QtObject {
    property string out
    property int n: 0
    property int ǆa: 1
    property int 𝒳: 7
    property int ⅷ: 8
    signal сигнал(int число)
    onСигнал: (число) => { n += число }
    onǄaChanged: n += 1000
    on𝒳Changed: n += 100
    function run() {
        сигнал(10);
        ǆa = 2;
        𝒳 = 3;
        out = [n, 𝒳, ⅷ, typeof ǆaChanged].join(" ");
    }
}
"#,
    ),
    (
        "a with statement in a handler of a document",
        r#"import QtQml
QtObject {
    property string out
    property int wq: 1
    property var seen: []
    onWqChanged: with ({wq: 5}) { seen.push("with => " + wq) }
    function run() {
        wq = 2;
        out = seen.join();
    }
}
"#,
    ),
    (
        "a literal with a leading zero in a binding",
        r#"import QtQml
QtObject {
    property string out
    property int a: 010
    property var b: [010, 08, 0o10, 0x10, 010 === 10]
    function run() { out = a + " " + b.join() }
}
"#,
    ),
];

fn cases(corpus: &str) -> Vec<&str> {
    corpus
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .collect()
}

/// The document answering every case of a corpus, one line each: `type:value`, or
/// `throws Name: message` (the first line of the message).
fn corpus_document(cases: &[&str]) -> String {
    let calls: Vec<String> = cases
        .iter()
        .map(|c| format!("            show(function () {{ {c} }})"))
        .collect();
    format!(
        r#"import QtQml
QtObject {{
    id: root
    property string out
    function show(f) {{
        try {{
            var r = f();
            return typeof r + ":" + (r === 0 && 1 / r < 0 ? "-0" : String(r)).replace(/\n/g, "\\n");
        }} catch (e) {{
            return "throws " + e.name + ": " + String(e.message).split("\n")[0];
        }}
    }}
    function run() {{
        var d = new Date(1710064800123);
        out = [
{}
        ].join("\n");
    }}
}}
"#,
        calls.join(",\n")
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
    match rt.call_method(root, "run", vec![]) {
        Some(Ok(_)) => rt.get(root, "out").to_js_string(),
        Some(Err(e)) => format!("run() threw: {}", e.replace('\n', " ")),
        None => "no run()".into(),
    }
}

#[test]
fn expressions_answer_as_qt() {
    const TEST: &str = concat!(module_path!(), "::expressions_answer_as_qt");
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
        return;
    }
    if !testenv::in_own_process(TEST) {
        // The answers depend on the time zone and the locale; both engines read them from
        // the environment.
        testenv::run_in_own_process(
            TEST,
            [
                ("TZ", "Europe/Moscow"),
                ("LC_ALL", "C.UTF-8"),
                ("QT_QPA_PLATFORM", "offscreen"),
            ],
        );
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut compared, mut wrong) = (0, vec![]);
    for (name, corpus) in CORPORA {
        let cases = cases(corpus);
        let document = corpus_document(&cases);
        let (qt, mut ours) = (qt_answer(dir.path(), &document), our_answer(&document));
        if ours.split('\n').count() != cases.len() {
            // A case this engine cannot load takes the whole document with it: one each then.
            let each = cases.iter().map(|c| our_answer(&corpus_document(&[c])));
            ours = each.collect::<Vec<_>>().join("\n");
        }
        let (qt, ours): (Vec<&str>, Vec<&str>) =
            (qt.split('\n').collect(), ours.split('\n').collect());
        assert_eq!(
            qt.len(),
            cases.len(),
            "{name}: Qt answered\n{}",
            qt.join("\n")
        );
        compared += cases.len();
        for (i, case) in cases.iter().enumerate() {
            if qt[i] != ours[i] {
                wrong.push(format!(
                    "{name}: {case}\n    Qt:   {}\n    ours: {}",
                    qt[i], ours[i]
                ));
            }
        }
    }
    for (name, document) in DOCUMENTS {
        let (qt, ours) = (qt_answer(dir.path(), document), our_answer(document));
        assert!(!qt.starts_with("load error"), "{name}: Qt: {qt}");
        compared += 1;
        if qt != ours {
            wrong.push(format!("{name}\n    Qt:   {qt}\n    ours: {ours}"));
        }
    }
    println!("{compared} cases compared with Qt, {} differ", wrong.len());
    assert!(
        wrong.is_empty(),
        "{} of {compared} cases differ from Qt:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// A TZif file (RFC 9636) of a zone that is Moscow's where the dates of
/// `date_text_is_qts` fall: local mean time (+2:30:17) until 1916-07-03, 3 h ahead
/// of UTC since.
fn moscow_tzif() -> Vec<u8> {
    let header = |changes: u32| {
        let counts = [0, 0, 0, changes, 2, 8u32];
        let mut header = [b"TZif2".as_slice(), &[0; 15]].concat();
        header.extend(counts.iter().flat_map(|c| c.to_be_bytes()));
        header
    };
    let (lmt, msk) = (9017i32.to_be_bytes(), 10800i32.to_be_bytes());
    let kinds = [lmt.as_slice(), &[0, 0], &msk, &[0, 4], b"LMT\0MSK\0"].concat();
    // The 32-bit block, which a reader of version 2 skips, has no change.
    let mut file = [header(0), kinds.clone()].concat();
    file.extend(header(1));
    file.extend((-1_688_265_017i64).to_be_bytes());
    file.push(1);
    file.extend(kinds);
    file.extend(b"\n<+03>-3\n");
    file
}

/// `Date.prototype.toString` and its kin print what Qt 6.12 prints (`TZ=Europe/Moscow qml6`),
/// not what ECMA-262 21.4.4.41 and 21.4.4.43 spell out: V4 puts the year after the time,
/// does not pad the day, leaves the zone out of `toTimeString` and prints `toUTCString` in
/// the same order. `Date.parse` reads both forms. The ends of the time value range
/// (±8.64e15 ms) have texts too, the lower one in Moscow's local mean time (+02:30:17), with
/// the year QDate counts, whose calendar has no year 0 (-271822; ECMA-262 counts -271821);
/// one millisecond past either end is no date, and its date and time texts are empty. The zone is the
/// test's own, under `TZDIR`: the machine needs no time zone database.
#[test]
fn date_text_is_qts() {
    const TEST: &str = concat!(module_path!(), "::date_text_is_qts");
    const CASES: [(&str, &str); 22] = [
        ("return d.toString()", "Sun Mar 10 13:00:00 2024 GMT+0300"),
        ("return String(d)", "Sun Mar 10 13:00:00 2024 GMT+0300"),
        ("return \"\" + d", "Sun Mar 10 13:00:00 2024 GMT+0300"),
        ("return d.toTimeString()", "13:00:00"),
        ("return d.toUTCString()", "Sun Mar 10 10:00:00 2024 GMT"),
        ("return d.toGMTString()", "Sun Mar 10 10:00:00 2024 GMT"),
        (
            "return new Date(-1).toString()",
            "Thu Jan 1 02:59:59 1970 GMT+0300",
        ),
        ("return Date.parse(d.toString())", "1710064800000"),
        (
            "return Date.parse(\"Sun Mar 10 13:00:00 2024 GMT+0300\")",
            "1710064800000",
        ),
        (
            "return Date.parse(\"Sun Mar 10 2024 13:00:00 GMT+0300\")",
            "1710064800000",
        ),
        (
            "return new Date(8.64e15).toString()",
            "Sat Sep 13 03:00:00 275760 GMT+0300",
        ),
        (
            "return new Date(-8.64e15).toString()",
            "Tue Apr 20 02:30:17 -271822 GMT+0230",
        ),
        (
            "return new Date(8.64e15).toDateString()",
            "Sat Sep 13 275760",
        ),
        (
            "return new Date(-8.64e15).toDateString()",
            "Tue Apr 20 -271822",
        ),
        ("return new Date(8.64e15).toTimeString()", "03:00:00"),
        ("return new Date(-8.64e15).toTimeString()", "02:30:17"),
        (
            "return new Date(8.64e15).toUTCString()",
            "Sat Sep 13 00:00:00 275760 GMT",
        ),
        (
            "return new Date(-8.64e15).toUTCString()",
            "Tue Apr 20 00:00:00 -271822 GMT",
        ),
        (
            "return new Date(Date.UTC(270000, 6, 15, 12, 30, 45)).toString()",
            "Sat Jul 15 15:30:45 270000 GMT+0300",
        ),
        ("return new Date(8.64e15 + 1).toString()", "Invalid Date"),
        ("return new Date(-8.64e15 - 1).toString()", "Invalid Date"),
        (
            "return new Date(8.64e15 + 1).toDateString() + new Date(-8.64e15 - 1).toTimeString()",
            "",
        ),
    ];
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Test")).unwrap();
        std::fs::write(dir.path().join("Test/Moscow"), moscow_tzif()).unwrap();
        let vars = [
            ("TZDIR", dir.path().as_os_str()),
            ("TZ", "Test/Moscow".as_ref()),
        ];
        testenv::run_in_own_process(TEST, vars);
        return;
    }
    let cases: Vec<&str> = CASES.iter().map(|(case, _)| *case).collect();
    let ours = our_answer(&corpus_document(&cases));
    let want: Vec<String> = CASES
        .iter()
        .map(|(_, text)| match text.parse::<f64>() {
            Ok(_) => format!("number:{text}"),
            Err(_) => format!("string:{text}"),
        })
        .collect();
    assert_eq!(ours.split('\n').collect::<Vec<_>>(), want);
}

/// A direct `eval` in strict code and the `delete` of a name it declared do what ECMA-262
/// 19.2.1.1 (PerformEval) says, which is not what Qt 6.12 does.
/// In a strict function or a class method V4 runs the text as a function of its own: it
/// sees only those names of the caller that V4 happened to keep in a context instead of a
/// register (`o`, `B` and `A` below are not defined there), and its `this` is an object
/// that is not the caller's `this`. (In an arrow function its `this` is not the arrow's,
/// here as in V4: the fixture `arrow` of builtins_b1_qt_equiv.rs holds that.)
/// `delete` of a name `eval` declared answers true in V4 and leaves the name. Which names
/// V4 keeps in a context is a matter of its code generator, so these answers cannot be
/// reproduced; the cases stay out of the comparison with Qt, and Qt's answer is beside each.
#[test]
fn eval_in_strict_code_is_ecma_262s() {
    const CASES: [(&str, &str); 6] = [
        // Qt: throws ReferenceError: o is not defined
        (
            r#"var o = {}; function f() { "use strict"; return eval("this === o") } return f.call(o)"#,
            "boolean:true",
        ),
        // Qt: string:object
        (
            r#"function f() { "use strict"; return eval("typeof this") } return f.call(5)"#,
            "string:number",
        ),
        // Qt: string:undefined
        (
            r#"class A { m() { var B = A; return eval("typeof B") } } return new A().m()"#,
            "string:function",
        ),
        // Qt: string:undefined
        (
            r#"class A { m() { return eval("typeof A") } } return new A().m()"#,
            "string:function",
        ),
        // Qt: undefined:undefined
        (
            r#"class A { constructor() { this.v = 3 } m() { return eval("this.v") } } return new A().m()"#,
            "number:3",
        ),
        // Qt: string:true number
        (
            r#"eval("var q = 1"); return (delete q) + " " + typeof q"#,
            "string:true undefined",
        ),
    ];
    let cases: Vec<&str> = CASES.iter().map(|(case, _)| *case).collect();
    let ours = our_answer(&corpus_document(&cases));
    let want: Vec<&str> = CASES.iter().map(|(_, answer)| *answer).collect();
    assert_eq!(ours.split('\n').collect::<Vec<_>>(), want);
}
