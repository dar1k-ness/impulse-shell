//! `toLocaleUpperCase` and `toLocaleLowerCase` as Qt 6.12 answers them: the case mapping of
//! ICU in the locale the argument names (a name, a locale of `Qt.locale`, the first of an
//! array) and, with no argument, in the system's locale, with the rules Turkish, Azerbaijani,
//! Lithuanian, Greek and Armenian have of their own. `toUpperCase` and `toLowerCase` are
//! asked beside them: they know no locale.
//! locale_case_qt/answers.cases has the expressions and what Qt answered in the
//! tools/testenv image (qt.py there writes the answers): a block begins with `@` and its
//! environment, then come an expression and, behind `=> `, Qt's `typeof` and text of it.
//! An expression gives its answer through `escape`, so each is a line of ASCII.
//!
//! locale_case_qt/code_points.cases asks every code point of Unicode, a range at a time:
//! which ones `toUpperCase` and `toLowerCase` change and into what, which ones the locale's
//! methods map otherwise in English, and which ones Turkish, Azerbaijani, Lithuanian, Greek
//! and Armenian map otherwise than English.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

const TEST: &str = concat!(module_path!(), "::case_is_mapped_as_qt_maps_it");
/// The number of the block a process answers and where it leaves the answers; without them,
/// the process that compares them all.
const BLOCK: &str = "LOCALE_CASE_BLOCK";
const OUT: &str = "LOCALE_CASE_OUT";
const ANSWERS: &str = concat!(
    include_str!("locale_case_qt/answers.cases"),
    include_str!("locale_case_qt/code_points.cases")
);
/// What names a locale in the environment: Qt answered with only a block's own.
const VARS: [&str; 11] = [
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_IDENTIFICATION",
    "LC_MEASUREMENT",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
];

/// The blocks of answers.cases: the environment, then each expression with Qt's answer.
fn blocks() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    let mut blocks: Vec<(&str, Vec<_>)> = Vec::new();
    let mut lines = ANSWERS.lines().filter(|l| !l.is_empty());
    while let Some(line) = lines.next() {
        if let Some(vars) = line.strip_prefix("@ ") {
            blocks.push((vars, Vec::new()));
            continue;
        }
        let qt = lines.next().and_then(|l| l.strip_prefix("=> "));
        let qt = qt.unwrap_or_else(|| panic!("no `=> ` line after {line}"));
        let block = blocks.last_mut().expect("an `@` line first");
        block.1.push((line, qt));
    }
    blocks
}

/// Our answers to a block's expressions, a line each.
fn answer(cases: &[(&str, &str)]) -> String {
    let mut body = String::new();
    for (case, _) in cases {
        body += &format!(
            "    try {{ out.push(text((function() {{ return ({case}) }})())) }} \
             catch (e) {{ out.push(\"THROW \" + e) }}\n"
        );
    }
    let qml = format!(
        "import QtQuick\nItem {{\n  \
         function text(v) {{ return typeof v + \":\" + v }}\n  \
         function show() {{\n    var out = []\n{body}    return out.join(\"\\n\")\n  }}\n}}\n"
    );
    // A block is one evaluation, and one that asks every code point is a long one.
    qml::watchdog::set_timeout(None);
    let mut rt = Runtime::new(None);
    let root = rt.load_str(&qml).expect("the document");
    rt.settle();
    let text = rt.call_method(root, "show", vec![]);
    let text = text.expect("show()").expect("show() returns");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    text.to_js_string()
}

#[test]
fn case_is_mapped_as_qt_maps_it() {
    let blocks = blocks();
    if !testenv::in_own_process(TEST) {
        testenv::run_in_own_process_env(TEST, [("LC_ALL", "C.UTF-8")], &VARS[..2]);
        return;
    }
    if let (Ok(block), Some(out)) = (std::env::var(BLOCK), std::env::var_os(OUT)) {
        let block: usize = block.parse().expect("a block's number");
        std::fs::write(out, answer(&blocks[block].1)).expect("write the answers");
        return;
    }

    let dir = tempfile::tempdir().expect("a directory for answers");
    let out = dir.path().join("answers");
    let (mut asked, mut differ, mut where_) = (0, 0, 0);
    for (i, (vars, cases)) in blocks.iter().enumerate() {
        let mut process = testenv::own_process_command(TEST);
        for unset in VARS {
            process.env_remove(unset);
        }
        process.envs(vars.split(' ').filter_map(|v| v.split_once('=')));
        let ran = process.env(BLOCK, i.to_string()).env(OUT, &out).output();
        let ran = ran.expect("run");
        let stdout = String::from_utf8_lossy(&ran.stdout);
        assert!(ran.status.success(), "{vars}:\n{stdout}");
        let ours = std::fs::read_to_string(&out).expect("a block's answers");
        let ours: Vec<_> = ours.lines().collect();
        assert_eq!(ours.len(), cases.len(), "{vars}: answers");
        let before = differ;
        for ((case, qt), ours) in cases.iter().zip(ours) {
            asked += 1;
            if *qt != ours {
                differ += 1;
                println!("{vars}\n  {case}\n    Qt:   {qt}\n    ours: {ours}");
            }
        }
        where_ += usize::from(differ > before);
    }
    println!(
        "{asked} answers of {} environments compared with Qt, {differ} differ in {where_}",
        blocks.len()
    );
    assert_eq!(differ, 0, "{differ} of {asked} differ");
}
