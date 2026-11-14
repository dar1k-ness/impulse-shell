//! A zone's names (`t`, `tt`, `ttt`, `tttt` and the formats of a locale that end in one) as
//! Qt 6.12 writes them through the system locale, the C locale and named locales, in six
//! zones under five environments and in a zone that is an offset (`TZ=UTC+3`) under two.
//! system_zone_names_qt/answers.cases has the expressions and what Qt answered in the
//! tools/testenv image (qt.py there writes the answers): a block begins with `@` and its
//! environment, then come an expression and, behind `=> `, Qt's `typeof` and text of it.
//! crates/rigen/check/cases/qml/system_zone_names.qml answers to the same file in
//! translated JS.
//!
//! system_zone_names_qt/settings_formats.cases asks the same of the four formats the shell
//! takes from its settings (qml/services/DateTime.qml), with `t`, `tt`, `ttt`, `tttt` there.
//!
//! Where the locale is C (`Qt.formatDateTime(d, "tttt")`, `Qt.locale("C").toString(d, "tttt")`)
//! Qt hands ICU the name `C`, which is no locale to ICU: a zone's long name is then in ICU's
//! default locale, the one the environment names for messages.
//! system_zone_names_qt/script_ids.cases has that for 120 environments whose `LANG` names a
//! script (`sr_Latn_RS`, `uz_Cyrl_UZ`, `ru_Latn_RU`): ICU reads the script, and Qt's own
//! name of a locale has none. system_zone_names_qt/no_icu_data.cases asks 33 environments
//! for other locales ICU has no data for (`iu_CA`, `nds_DE`): their names of a zone, long
//! and short, are in ICU's default locale too.
//! crates/rigen/check/cases/qml/script_ids.qml and no_icu_data.qml answer to the two files
//! in translated JS.
//!
//! system_zone_names_qt/offset_names.cases asks 32 environments for the short name where ICU
//! has an offset for it: Qt writes `UTC+11` and `UTC−10` as `UTC+11:00` and `UTC−10:00`
//! (six letters that begin with `UTC` get `:00`), and leaves `UTC+9` and `GMT+11` as they are.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

const TEST: &str = concat!(module_path!(), "::zones_are_named_as_qt_names_them");
/// The number of the block a process answers and where it leaves the answers; without them,
/// the process that compares them all.
const BLOCK: &str = "SYSTEM_ZONE_NAMES_BLOCK";
const OUT: &str = "SYSTEM_ZONE_NAMES_OUT";
const ANSWERS: &str = concat!(
    include_str!("system_zone_names_qt/answers.cases"),
    include_str!("system_zone_names_qt/default_locale.cases"),
    include_str!("system_zone_names_qt/no_icu_data.cases"),
    include_str!("system_zone_names_qt/offset_names.cases"),
    include_str!("system_zone_names_qt/script_ids.cases"),
    include_str!("system_zone_names_qt/settings_formats.cases")
);
/// The formats the shell takes from its settings, where a user may write `t`: an expression
/// of settings_formats.cases is one of these four of qml/services/DateTime.qml, given a
/// clock and settings.
const SETTINGS: &str = include_str!("system_zone_names_qt/settings_formats.cases");
const SHELL: &str = include_str!("../../../qml/services/DateTime.qml");
const PLACES: [&str; 4] = ["time", "shortDate", "date", "longDate"];
/// What names a locale or a zone in the environment: Qt answered with only a block's own.
const VARS: [&str; 13] = [
    "TZ",
    "TZDIR",
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
         function text(v) {{ return (typeof v + \":\" + v).split(\"\\n\")[0] }}\n  \
         function show() {{\n    var out = []\n{body}    return out.join(\"\\n\")\n  }}\n}}\n"
    );
    let mut rt = Runtime::new(None);
    let root = rt.load_str(&qml).expect("the document");
    rt.settle();
    let text = rt.call_method(root, "show", vec![]);
    let text = text.expect("show()").expect("show() returns");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    text.to_js_string()
}

/// settings_formats.cases asks what the shell asks: each line of DateTime.qml that formats
/// the clock by a setting is there, as written, with `t`, `tt`, `ttt` and `tttt` set.
#[test]
fn the_formats_of_the_settings_are_the_shells() {
    for place in PLACES {
        let line = format!("    property string {place}: ");
        let asked = SHELL.lines().find_map(|l| l.strip_prefix(&line));
        let asked = asked.unwrap_or_else(|| panic!("DateTime.qml has no `{place}`"));
        assert!(asked.contains("Config.options?.time."), "{place}: {asked}");
        for format in ["t", "tt", "ttt", "tttt"] {
            let set = format!(": \"{format}\"}}}}}})");
            let found = SETTINGS.lines().any(|l| {
                l.strip_prefix("((clock, Config) => ")
                    .and_then(|l| l.strip_prefix(asked))
                    .is_some_and(|l| l.starts_with(")(") && l.ends_with(&set))
            });
            assert!(found, "{place} with {format} is not asked");
        }
    }
}

#[test]
fn zones_are_named_as_qt_names_them() {
    if !std::path::Path::new("/usr/share/zoneinfo/Antarctica/Troll").exists() {
        testenv::equiv_missing("the time zone database in /usr/share/zoneinfo");
        return;
    }
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
    let (mut settings, mut settings_differ) = (0, 0);
    let first_of_settings = blocks.len() - SETTINGS.matches("\n@ ").count() - 1;
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
        if i >= first_of_settings {
            settings += cases.len();
            settings_differ += differ - before;
        }
    }
    println!(
        "{settings} answers to the formats of the settings in {} environments, \
         {settings_differ} differ",
        blocks.len() - first_of_settings
    );
    println!(
        "{asked} answers of {} environments compared with Qt, {differ} differ in {where_}",
        blocks.len()
    );
    assert_eq!(differ, 0, "{differ} of {asked} differ");
}
