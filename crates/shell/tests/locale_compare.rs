//! `String.prototype.localeCompare` compared with Qt 6.12 in the locales of
//! `scenes/locale_compare/locales`: every pair of the strings of
//! `scenes/locale_compare/scene.qml` (Latin with marks, Cyrillic, case, digits, punctuation,
//! white space, CJK, emoji, empty strings, prefixes). What Qt gives is
//! `scenes/locale_compare/scene.qt`, written by `scenes/locale_compare/dump.sh` in the testenv
//! image (`qml6`): per locale the strings in the order `localeCompare` puts them, with `<` or
//! `=` between neighbours, how many pairs that order does not answer (none), and the answers
//! that are not -1, 0 or 1 (the C locale gives the difference of two code units).
//! `scenes/locale_compare/scene.ours` holds the `order` rows the engine gives where they are not
//! Qt's: two strings stand elsewhere in the locales ICU sorts (docs/qt-differences.md), and
//! without those two the rows are Qt's.
//!
//! Qt's `localeCompare` is `QString::localeAwareCompare` (qv4stringobject.cpp), which in a Qt
//! built with ICU, as the image's is, is `QCollator::defaultCompare` (qstring.cpp): ICU's
//! `ucol_strcoll` in the locale of `LC_ALL`, `LC_COLLATE`, `LANG` (qcollator_icu.cpp,
//! qlocale_unix.cpp), and `QtPrivate::compareStrings` in the C locale. So the tables that
//! have to agree are ICU's (CLDR collation), not libc's `strcoll`.
//!
//! The scene is compiled into the test (`testscenes`): a build with the `interp` feature
//! interprets its scripts, a build without it runs the compiled ones. Each locale runs in a
//! process of its own, which is where the engine reads the environment.

const QT: &str = include_str!("scenes/locale_compare/scene.qt");
/// The `order` rows the engine gives where they are not Qt's (docs/qt-differences.md).
const OURS: &str = include_str!("scenes/locale_compare/scene.ours");
/// The strings those rows place differently: U+20000 and a lone U+D800.
const DIFFERENT: [&str; 2] = ["152", "179"];
const LOCALES: &str = include_str!("scenes/locale_compare/locales");
const TEST: &str = concat!(module_path!(), "::every_pair_compares_as_in_qt");
const ROW: &str = "LOCALE_COMPARE_ROW";
const VARS: [&str; 4] = ["LC_ALL", "LC_COLLATE", "LC_NUMERIC", "LANG"];

/// The rows of `locales`: a name and the variables its process gets.
fn locales() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    LOCALES
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut words = l.split(' ');
            let name = words.next().unwrap();
            (name, words.map(|w| w.split_once('=').unwrap()).collect())
        })
        .collect()
}

#[test]
fn every_pair_compares_as_in_qt() {
    if !testenv::in_own_process(TEST) {
        let locales = locales();
        for want in ["C", "en_US", "ru_RU", "de_DE", "sv_SE", "tr_TR", "ja_JP"] {
            assert!(locales.iter().any(|(n, _)| *n == want), "{want}");
        }
        let failed: Vec<&str> = locales
            .into_iter()
            .filter(|(name, vars)| {
                let vars = vars.iter().copied().chain([(ROW, *name)]);
                std::panic::catch_unwind(|| testenv::run_in_own_process_env(TEST, vars, &VARS))
                    .is_err()
            })
            .map(|(name, _)| name)
            .collect();
        assert!(failed.is_empty(), "differ from Qt: {failed:?}");
        return;
    }
    let name = std::env::var(ROW).unwrap();
    let qt: Vec<&str> = QT
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(n, _)| *n == name)
        .map(|(_, row)| row)
        .collect();
    assert_eq!(qt.len(), 3, "rows of Qt for {name}");
    assert!(qt[1].starts_with("pairs 32761 odd 0 "), "{}", qt[1]);

    let (mut rt, root) = testscenes::load("locale_compare/scene.qml");
    rt.settle();
    let rows = rt
        .call_method(root, "rows", vec![])
        .unwrap()
        .unwrap()
        .to_js_string();
    let errors = rt.errors();
    assert!(errors.is_empty(), "{errors:?}");
    let ours: Vec<&str> = rows.lines().collect();
    assert_eq!(ours.len(), qt.len(), "rows");
    let held: Vec<&str> = OURS
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(n, _)| *n == name)
        .map(|(_, row)| row)
        .collect();
    match held[..] {
        [] => assert!(qt[0] == ours[0], "{name}:\n{}", differences(qt[0], ours[0])),
        [held] => {
            assert!(
                held == ours[0],
                "{name}, held:\n{}",
                differences(held, ours[0])
            );
            assert_ne!(held, qt[0], "{name}: the held row is Qt's, remove it");
            let (q, o) = (without(qt[0], &DIFFERENT), without(held, &DIFFERENT));
            assert!(
                q == o,
                "{name}, the other strings:\n{}",
                differences(&q, &o)
            );
        }
        _ => panic!("{name}: {} held rows", held.len()),
    }
    for (qt, ours) in qt.iter().zip(&ours).skip(1) {
        assert!(qt == ours, "{name}:\n{}", differences(qt, ours));
    }
}

/// An `order` row without the strings `drop` names: two neighbours it leaves are `=` only when
/// everything between them was.
fn without(row: &str, drop: &[&str]) -> String {
    let words: Vec<&str> = row.strip_prefix("order ").unwrap().split(' ').collect();
    let mut out: Vec<&str> = vec![];
    let mut less = false;
    for (at, id) in words.iter().enumerate().step_by(2) {
        less |= at > 0 && words[at - 1] == "<";
        if drop.contains(id) {
            continue;
        }
        if !out.is_empty() {
            out.push(if less { "<" } else { "=" });
        }
        out.push(id);
        less = false;
    }
    out.join(" ")
}

/// Where two rows part: the words around the first that differs.
fn differences(qt: &str, ours: &str) -> String {
    let (q, o): (Vec<&str>, Vec<&str>) = (qt.split(' ').collect(), ours.split(' ').collect());
    let at = (q.iter().zip(&o)).position(|(q, o)| q != o).unwrap_or(0);
    let from = at.saturating_sub(6);
    let part = |w: &[&str]| w[from.min(w.len())..(at + 12).min(w.len())].join(" ");
    format!(
        "word {at} of {} (Qt) and {} (ours)\nQt:   … {}\nours: … {}",
        q.len(),
        o.len(),
        part(&q),
        part(&o)
    )
}
