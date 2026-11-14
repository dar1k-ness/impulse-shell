//! The second engine audit's findings in the JavaScript built-ins (`builtins.rs`, slice B2):
//! every expression of `builtins_b2/cases.tsv` gives the value, or throws the error, that
//! Qt 6.12 gives for it. Qt runs the same QML file here: the PySide6 of ri-testenv.
//!
//! A line of the file is `finding<TAB>expression`. Where Qt's message is only `Type error`,
//! the name of the error is compared: the engine says what was wrong. A third field is what
//! the expression gives here where Qt 6.12 gives something else on purpose
//! (docs/qt-differences.md): the engine has to give exactly that and Qt something else.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

/// The Python with PySide6 in ri-testenv (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";
const CASES: &str = include_str!("builtins_b2/cases.tsv");
const QT_LOG: &str = include_str!("builtins_b2/qt.py");

/// One QML file that evaluates every expression and joins what each gave, a thrown error
/// as `!name: message`, into `log`.
fn document(exprs: &[&str]) -> String {
    let fns: Vec<String> = exprs
        .iter()
        .map(|e| format!("function() {{ return {e} }}"))
        .collect();
    format!(
        r#"import QtQuick
QtObject {{
    id: root
    property string log: ""
    property url u
    property Loader ld: Loader {{}}
    property Image im: Image {{}}
    property FontLoader fl: FontLoader {{}}
    property Component comp: Component {{ QtObject {{ property url u; property string s; property var v }} }}
    function show(v) {{ return typeof v === "string" ? JSON.stringify(v) : String(v) }}
    Component.onCompleted: {{
        var cases = [{}];
        var out = [];
        for (var i = 0; i < cases.length; i++) {{
            try {{ out[i] = show(cases[i]()) }} catch (e) {{ out[i] = "!" + e.name + ": " + e.message }}
        }}
        log = out.join("\u0001")
    }}
}}"#,
        fns.join(",\n")
    )
}

fn qt_log(dir: &std::path::Path, main: &std::path::Path) -> String {
    let script = dir.join("qt.py");
    std::fs::write(&script, QT_LOG).unwrap();
    let out = std::process::Command::new(PYTHON)
        .arg(&script)
        .arg(main)
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{PYTHON} failed:\n{err}");
    String::from_utf8(out.stdout).unwrap()
}

/// Without a shell directory: under one a url goes through Quickshell's interceptor, which
/// the Qt of `qt_log` does not have.
fn our_log(main: &std::path::Path) -> String {
    let mut rt = Runtime::new(None);
    let root = rt.load_file(main).unwrap();
    rt.settle();
    let log = rt.get(root, "log").to_js_string();
    assert!(
        !log.is_empty(),
        "the handler did not finish: {:?}",
        rt.errors()
    );
    log
}

#[test]
fn expressions_give_what_qt_gives() {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6 6.12)"));
        return;
    }
    let cases: Vec<(&str, &str, Option<&str>)> = CASES
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut f = l.split('\t');
            (f.next().unwrap(), f.next().unwrap(), f.next())
        })
        .collect();
    let exprs: Vec<&str> = cases.iter().map(|c| c.1).collect();
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.qml");
    std::fs::write(&main, document(&exprs)).unwrap();
    let qt = qt_log(dir.path(), &main);
    let ours = our_log(&main);
    let (qt, ours): (Vec<&str>, Vec<&str>) =
        (qt.split('\u{1}').collect(), ours.split('\u{1}').collect());
    assert_eq!(qt.len(), cases.len(), "Qt evaluated every case");
    assert_eq!(ours.len(), cases.len(), "we evaluated every case");
    let same = |i: usize| match qt[i] {
        "!TypeError: Type error" => ours[i].starts_with("!TypeError: "),
        qt => ours[i] == qt,
    };
    let report = |i: usize| {
        format!(
            "{} {}\n    Qt:   {:?}\n    ours: {:?}",
            cases[i].0, cases[i].1, qt[i], ours[i]
        )
    };
    let (known, compared): (Vec<usize>, Vec<usize>) =
        (0..cases.len()).partition(|&i| cases[i].2.is_some());
    let differ: Vec<String> = (compared.iter().filter(|&&i| !same(i)))
        .map(|&i| report(i))
        .collect();
    let moved: Vec<String> = (known.iter())
        .filter(|&&i| Some(ours[i]) != cases[i].2 || same(i))
        .map(|&i| report(i))
        .collect();
    println!(
        "compared with Qt 6.12: {} cases, {} differ; {} more are the differences of docs/qt-differences.md",
        compared.len(),
        differ.len(),
        known.len()
    );
    assert!(differ.is_empty(), "{}", differ.join("\n"));
    assert!(
        moved.is_empty(),
        "no longer the difference docs/qt-differences.md describes:\n{}",
        moved.join("\n")
    );
}
