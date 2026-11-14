//! Numbers, dates and times are written in a locale's digits and read back from them as
//! Qt 6.12 does it, in every locale Qt knows that has digits other than ASCII's (76 locales, 11
//! sets of digits, two of them outside the BMP) and in system locales put together from
//! the environment. locale_digits_qt/show.qml has what is asked of a locale, systems.txt the
//! environments, and answers.qt what Qt answered (locale_digits_qt/qt.sh writes it, in the
//! tools/testenv image).
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use qml::value::Value;
use std::collections::BTreeSet;

const TEST: &str = concat!(
    module_path!(),
    "::numbers_and_dates_take_a_locales_digits_as_in_qt"
);
/// Where the process of a system locale leaves its answers; without it, the process that
/// compares them all.
const OUT: &str = "LOCALE_DIGITS_OUT";
const SHOW: &str = include_str!("locale_digits_qt/show.qml");
const ANSWERS: &str = include_str!("locale_digits_qt/answers.qt");
/// What names a locale in the environment: qt.sh clears them too.
const LOCALE_VARS: [&str; 8] = [
    "TZDIR",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_TIME",
    "LC_NUMERIC",
    "LC_MESSAGES",
    "LC_MONETARY",
];
const ZONE: (&str, &str) = ("TZ", "Asia/Kolkata");

#[test]
fn numbers_and_dates_take_a_locales_digits_as_in_qt() {
    if !std::path::Path::new("/usr/share/zoneinfo/Asia/Kolkata").exists() {
        testenv::equiv_missing("the time zone database in /usr/share/zoneinfo");
        return;
    }
    if !testenv::in_own_process(TEST) {
        testenv::run_in_own_process_env(TEST, [ZONE, ("LC_ALL", "C.UTF-8")], &LOCALE_VARS[..3]);
    } else if let Some(out) = std::env::var_os(OUT) {
        std::fs::write(out, show(&mut document(), "system", "")).expect("write the answers");
    } else {
        compare();
    }
}

fn document() -> (Runtime, usize) {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(SHOW).expect("show.qml");
    rt.settle();
    (rt, root)
}

/// show.qml's answers for the locale `name`, each line behind `group`.
fn show((rt, root): &mut (Runtime, usize), group: &str, name: &str) -> String {
    let text = rt.call_method(*root, "show", vec![Value::Str(name.into())]);
    let text = text
        .expect("show()")
        .expect("show() returns")
        .to_js_string();
    assert!(rt.errors().is_empty(), "{group}: {:?}", rt.errors());
    text.lines().map(|l| format!("{group}\t{l}\n")).collect()
}

/// Asks every group of answers.qt what Qt was asked, a named locale here and a system locale
/// in a process with its environment, and compares the answers with Qt's line by line.
fn compare() {
    let mut groups: Vec<&str> = Vec::new();
    for line in ANSWERS.lines() {
        let group = line.split('\t').next().unwrap_or("");
        if groups.last() != Some(&group) {
            groups.push(group);
        }
    }
    let dir = tempfile::tempdir().expect("a directory for answers");
    let out = dir.path().join("answers");
    let mut document = document();
    let mut ours = String::new();
    for group in &groups {
        let Some(vars) = group.strip_prefix("system ") else {
            ours += &show(&mut document, group, group);
            continue;
        };
        let mut process = testenv::own_process_command(TEST);
        for unset in LOCALE_VARS {
            process.env_remove(unset);
        }
        process.envs(vars.split(' ').filter_map(|v| v.split_once('=')));
        let ran = process.env(ZONE.0, ZONE.1).env(OUT, &out).output();
        let ran = ran.expect("run");
        let stdout = String::from_utf8_lossy(&ran.stdout);
        assert!(ran.status.success(), "{group}:\n{stdout}");
        let answers = std::fs::read_to_string(&out).expect("a system locale's answers");
        ours += &answers.replace("system\t", &format!("{group}\t"));
    }

    let (qt, ours): (Vec<_>, Vec<_>) = (ANSWERS.lines().collect(), ours.lines().collect());
    let differ: Vec<_> = qt
        .iter()
        .zip(&ours)
        .filter(|(qt, ours)| qt != ours)
        .collect();
    for (qt, ours) in &differ {
        println!("Qt:   {qt}\nours: {ours}");
    }
    let locales: BTreeSet<_> = differ
        .iter()
        .map(|(qt, _)| qt.split('\t').next().unwrap_or(""))
        .collect();
    println!(
        "{} answers of {} locales compared with Qt, {} differ in {} locales",
        qt.len(),
        groups.len(),
        differ.len(),
        locales.len()
    );
    assert_eq!(qt.len(), ours.len(), "answers");
    assert!(differ.is_empty(), "{} of {} differ", differ.len(), qt.len());

    // What the answers cover: the zero digit of each locale, `<digit> length <UTF-16 units>`.
    let zeros = |units: &str| {
        let zeros = qt.iter().filter_map(|l| l.split_once("\tzeroDigit\t"));
        let zeros = zeros.filter(|(group, zero)| {
            !group.starts_with("system ") && !zero.starts_with('0') && zero.contains(units)
        });
        zeros
            .map(|(group, zero)| (zero.chars().next(), group))
            .collect::<Vec<_>>()
    };
    let systems = |units| {
        zeros(units)
            .iter()
            .map(|z| z.0)
            .collect::<BTreeSet<_>>()
            .len()
    };
    assert!(
        zeros(" length ").len() >= 76,
        "locales: {:?}",
        zeros(" length ")
    );
    assert_eq!(systems(" length "), 11, "sets of digits");
    assert_eq!(systems(" length 2 "), 2, "sets of digits outside the BMP");
}
