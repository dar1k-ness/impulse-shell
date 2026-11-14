//! Second builtins audit, slice B3: the findings that depend on the process environment
//! (time zone, LC_* variables). Each case runs this test binary again as a child with the
//! environment under test and reads the value it prints, so no test changes its own process.
//! The fixtures in `audit_b3/` are that slice's value type and locale findings, recorded from
//! Qt 6.12.
//! The expected values are Qt 6.12's (tools/testenv), or ECMA-262's where the audit names it.
use std::process::Command;

const SRC: &str = "AUDIT_B3_SRC";
const LOCALE_VARS: [&str; 10] = [
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_MESSAGES",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_MONETARY",
    "LC_MEASUREMENT",
    "LC_COLLATE",
    "TZ",
];

/// Child mode: prints the value of the JS expression in `AUDIT_B3_SRC`.
#[test]
fn child() {
    let Ok(src) = std::env::var(SRC) else { return };
    let body = qml::ir::lower_program(&format!("return {src}")).unwrap();
    let mut it = qml::eval::Interp::default();
    let g = it.global.clone();
    let v = match it.run(&body, &g) {
        Ok(v) => v.to_js_string(),
        Err(_) => "THROW".into(),
    };
    println!("\nRESULT[{v}]");
}

const FIXTURE: &str = "AUDIT_B3_FIXTURE";

/// The expressions of a `tests/audit_b3/*.cases` file and Qt 6.12's answer to each, and the
/// QML document that logs ours. In the file `@` lines are members of the root item, `#`
/// lines comments, and every other line an expression followed by `=> ` and what Qt printed
/// for it in tools/testenv (TZ=Europe/Moscow, and LC_ALL=C.UTF-8 or the variables the test
/// names). The expressions run in order
/// in one document, so each sees the state the earlier ones left.
fn fixture(text: &str) -> (String, Vec<(&str, &str)>) {
    let (mut members, mut body, mut cases) = (String::new(), String::new(), Vec::new());
    let mut lines = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    while let Some(line) = lines.next() {
        if let Some(member) = line.strip_prefix('@') {
            members += &format!("  {member}\n");
            continue;
        }
        let want = lines.next().and_then(|l| l.strip_prefix("=> "));
        let want = want.unwrap_or_else(|| panic!("no `=> ` line after {line}"));
        let i = cases.len();
        body += &format!(
            "    try {{ console.log(\"{i}| \" + show((function() {{ return ({line}) }})())) }} \
             catch (e) {{ console.log(\"{i}| THROW \" + e) }}\n"
        );
        cases.push((line, want));
    }
    let qml = format!(
        "import QtQuick\nItem {{\n  id: root\n{members}  \
         function show(v) {{ return v === undefined ? \"undefined\" : v === null ? \"null\" : typeof v + \":\" + v }}\n  \
         function w(f) {{ try {{ return f() }} catch (e) {{ return \"THROW \" + e }} }}\n  \
         Component.onCompleted: {{\n{body}  }}\n}}\n"
    );
    (qml, cases)
}

/// Child mode: loads the document of the fixture named by `AUDIT_B3_FIXTURE` and prints
/// the first line of each of its console messages.
#[test]
fn fixture_child() {
    let Ok(name) = std::env::var(FIXTURE) else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/audit_b3");
    let text = std::fs::read_to_string(dir.join(format!("{name}.cases"))).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("main.qml");
    std::fs::write(&file, fixture(&text).0).unwrap();
    // The engine keeps no console lines: the child takes them as they are printed.
    thread_local! {
        static PRINTED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    let _ = qml::builtins::CONSOLE_SINK.set(|_, line| {
        PRINTED.with_borrow_mut(|p| p.push(line.to_string()));
    });
    let mut rt = qml::runtime::Runtime::new(Some(tmp.path().to_path_buf()));
    rt.load_file(&file).unwrap();
    rt.settle();
    for line in &PRINTED.take() {
        // A fixture records the first line of an answer.
        println!("\nLOG[{}]", line.lines().next().unwrap_or(""));
    }
}

/// Runs fixture `name` in a child with Qt's recording environment and fails with every case
/// whose answer is not Qt's.
fn check_fixture(name: &str) {
    check_fixture_in(name, &[("LC_ALL", "C.UTF-8")]);
}

/// [`check_fixture`] for a fixture Qt answered under the locale variables `env`.
fn check_fixture_in(name: &str, env: &[(&str, &str)]) {
    let wrong: String = fixture_answers(name, env)
        .iter()
        .filter(|(_, want, got)| got.as_ref() != Some(want))
        .map(|(case, want, got)| format!("{case}\n    qt:   {want}\n    ours: {got:?}\n"))
        .collect();
    assert!(wrong.is_empty(), "{name}: differs from Qt 6.12\n{wrong}");
}

/// Each case of fixture `name`, Qt's answer to it and ours, run in a child under `env`.
fn fixture_answers(name: &str, env: &[(&str, &str)]) -> Vec<(String, String, Option<String>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/audit_b3");
    let text = std::fs::read_to_string(dir.join(format!("{name}.cases"))).unwrap();
    let (_, cases) = fixture(&text);
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args([
        "fixture_child",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ]);
    for k in LOCALE_VARS {
        c.env_remove(k);
    }
    c.env("TZ", "Europe/Moscow").envs(env.iter().copied());
    let out = c.env(FIXTURE, name).output().unwrap();
    let out = String::from_utf8(out.stdout).unwrap();
    let mut got = vec![None; cases.len()];
    for line in out.lines() {
        let Some(line) = line.strip_prefix("LOG[").and_then(|l| l.strip_suffix(']')) else {
            continue;
        };
        if let Some((i, answer)) = line.split_once("| ")
            && let Ok(i) = i.parse::<usize>()
        {
            got[i] = Some(answer);
        }
    }
    cases
        .iter()
        .zip(got)
        .map(|((case, want), got)| (case.to_string(), want.to_string(), got.map(String::from)))
        .collect()
}

/// B3-19..B3-26, B3-28: value type text, equality and conversions, and font fields.
#[test]
fn value_types_match_qt() {
    check_fixture("valuetypes");
    check_fixture("valuetypes_more");
}

/// B3-22: a write that leaves a value type property as it was does not notify.
#[test]
fn value_type_writes_notify_as_qt() {
    check_fixture("signals");
}

/// A `vector2d`, `vector3d`, `vector4d`, `quaternion`, `matrix4x4` or `font` property declared
/// without a value notifies on its first write even of the value it shows; `point`, `size`
/// and `rect` do not, nor does a property declared with a value.
#[test]
fn value_types_without_a_value_notify_on_the_first_write_as_qt() {
    check_fixture("unset_signals");
}

/// An array stores the default value of a value type; a font's spacings are whole 64ths,
/// its `capitalization` three bits and its `hintingPreference` two.
#[test]
fn arrays_and_font_fields_store_what_qt_stores() {
    check_fixture("font_fixed_arrays");
}

/// A font read from text keeps the fields of QFont::toString that QML has no property for.
#[test]
fn font_strings_match_qt() {
    check_fixture("font_strings");
    check_fixture("font_strings_more");
}

/// A write to a method's name writes the property that has the method's index.
#[test]
fn method_name_writes_match_qt() {
    check_fixture("method_name_writes");
    check_fixture("method_name_writes_more");
}

/// B3-27, B3-34: `date` properties hold dates and `url` properties refuse what is no URL.
#[test]
fn date_and_url_properties_match_qt() {
    check_fixture("date_url_props");
    check_fixture("date_url_props_more");
    check_fixture("date_url_props_gap");
}

/// B3-1..B3-3: the text, fields, setters and parsing of dates, far dates, years past 9999
/// and local times a clock change skips or repeats.
#[test]
fn dates_match_qt() {
    check_fixture("dates");
    check_fixture("dates_more");
    check_fixture("dates_gap");
    check_fixture("dates_text");
}

/// B3-32: a write to a field of a color changes the color where it was read, also when the
/// field is named by a string, a variable or inside `with`.
#[test]
fn color_field_writes_match_qt() {
    check_fixture("color_fields");
    check_fixture("color_fields_more");
}

/// A red, green or blue outside 0..1 makes an extended RGB color; alpha is clamped.
#[test]
fn colors_out_of_range_match_qt() {
    check_fixture("color_out_of_range");
    check_fixture("color_extended");
}

/// A `color` property refuses `undefined`, a function, a symbol and a regular expression
/// with Qt's text, reads an object's color fields and takes a `Qt.binding`.
#[test]
fn color_assignments_match_qt() {
    check_fixture("color_assign");
    check_fixture("color_from_object");
    check_fixture("binding_assign");
}

/// A color is an object with the twelve properties of its type: `in`, `hasOwnProperty`,
/// `Object.keys`, descriptors, the value types' prototype and `JSON.stringify` see them.
#[test]
fn a_color_is_an_object_as_in_qt() {
    check_fixture("color_object");
}

/// The same questions asked of a `point`; a strict write to a name a value type has no
/// property for throws; Map, Set, WeakMap and WeakSet find a value type by value.
#[test]
fn value_types_are_objects_as_in_qt() {
    check_fixture("point_object");
    check_fixture("value_type_objects");
    check_fixture("value_type_objects_more");
    check_fixture("value_type_own");
}

/// What script defines on a value type with `Object.defineProperty` or an index is kept
/// beside its properties: read and listed after them, and gone from a copy. Freezing it
/// leaves the properties writable.
#[test]
fn script_defined_members_of_a_value_type_match_qt() {
    check_fixture("value_type_members");
}

/// `Easing` is a type: it lists the three members of the object behind it, prints as
/// that object, refuses a strict write with "Type error" and answers `delete` with true.
#[test]
fn the_easing_type_matches_qt() {
    check_fixture("easing_type");
    check_fixture("easing_type_more");
}

/// `Easing` has a prototype with a `toString` before %Function.prototype%, reads its own
/// names and then %Object.prototype%, and keeps its prototype and extensibility.
#[test]
fn the_easing_type_has_qts_prototype() {
    check_fixture("easing_prototype");
}

/// A Locale's `weekDays` and `uiLanguages` are read-only sequences: they list their
/// elements only, have no own descriptors, and share a prototype with `valueOf`, `length`
/// and the four methods that would change their length.
#[test]
fn locale_lists_match_qt() {
    check_fixture("locale_lists");
    check_fixture("locale_lists_more");
    check_fixture("locale_lists_proto");
}

/// A write to a property of `undefined` or `null` throws Qt's text.
#[test]
fn writes_to_no_object_match_qt() {
    check_fixture("no_object_writes");
}

/// B3-7..B3-12: the Locale type's properties, methods, names, errors and equality.
#[test]
fn locale_objects_match_qt() {
    check_fixture("locale");
    check_fixture("locale_more");
    check_fixture("locale_object");
}

/// B3-4..B3-6: the system locale is named by LC_ALL, LC_MESSAGES or LANG, and takes its
/// numbers and the sizes of their groups from LC_NUMERIC, its dates and first day of the week
/// from LC_TIME, its currency from LC_MONETARY and its measurement system from LC_MEASUREMENT.
#[test]
fn system_locale_reads_the_environment_as_qt() {
    const EN: (&str, &str) = ("LANG", "en_US.UTF-8");
    let fixtures: [(&str, &[(&str, &str)]); 18] = [
        (
            "system_collate_c",
            &[("LANG", "ru_RU.UTF-8"), ("LC_COLLATE", "C")],
        ),
        (
            "system_grouping",
            &[("LANG", "de_DE.UTF-8"), ("LC_NUMERIC", "bn_BD.UTF-8")],
        ),
        (
            "system_grouping_of_numeric",
            &[
                ("LANG", "en_IN.UTF-8"),
                ("LC_NUMERIC", "de_CH.UTF-8"),
                ("LC_MONETARY", "bn_BD.UTF-8"),
                ("LC_TIME", "hi_IN.UTF-8"),
            ],
        ),
        (
            "system_every_category",
            &[
                ("LANG", "de_DE.UTF-8"),
                ("LC_NUMERIC", "bn_BD.UTF-8"),
                ("LC_MONETARY", "en_IN.UTF-8"),
                ("LC_TIME", "ar_EG.UTF-8"),
                ("LC_MESSAGES", "fr_FR.UTF-8"),
                ("LC_MEASUREMENT", "en_US.UTF-8"),
                ("LC_COLLATE", "C"),
            ],
        ),
        ("system_lang", &[EN]),
        ("system_lc_numeric", &[EN, ("LC_NUMERIC", "de_DE.UTF-8")]),
        ("system_lc_messages", &[EN, ("LC_MESSAGES", "ru_RU.UTF-8")]),
        ("system_lc_time", &[EN, ("LC_TIME", "ru_RU.UTF-8")]),
        ("system_lc_monetary", &[EN, ("LC_MONETARY", "ja_JP.UTF-8")]),
        (
            "system_lc_measurement",
            &[EN, ("LC_MEASUREMENT", "de_DE.UTF-8")],
        ),
        (
            "system_lc_all",
            &[("LC_ALL", "de_DE.UTF-8"), EN, ("LC_NUMERIC", "ru_RU.UTF-8")],
        ),
        ("system_language", &[EN, ("LANGUAGE", "fr:de")]),
        ("system_unset", &[]),
        ("system_c", &[("LANG", "C")]),
        (
            "system_lc_collate",
            &[("LANG", "ru_RU.UTF-8"), ("LC_COLLATE", "C")],
        ),
        (
            "system_mixed",
            &[
                ("LANG", "en_GB.UTF-8"),
                ("LC_MEASUREMENT", "en_US.UTF-8"),
                ("LC_TIME", "ar_EG.UTF-8"),
            ],
        ),
        ("system_script", &[("LANG", "sr_RS.UTF-8@latin")]),
        (
            "system_unknown",
            &[("LANG", "zz_ZZ.UTF-8"), ("LC_NUMERIC", "de_DE.UTF-8")],
        ),
    ];
    for (name, env) in fixtures {
        check_fixture_in(name, env);
    }
}

fn probe(env: &[(&str, &str)], src: &str) -> String {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["child", "--exact", "--nocapture", "--test-threads=1"]);
    for k in LOCALE_VARS {
        c.env_remove(k);
    }
    let out = c.envs(env.iter().copied()).env(SRC, src).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let start = text.find("RESULT[").expect("child printed no result") + 7;
    let end = text[start..].rfind(']').unwrap();
    text[start..start + end].to_string()
}

const MSK: [(&str, &str); 1] = [("TZ", "Europe/Moscow")];

/// B3-1: every time value TimeClip allows (±8.64e15 ms) is a date with local fields.
#[test]
fn far_dates_have_local_fields() {
    let src = "[String(new Date(8.64e15)), new Date(8.64e15).getTimezoneOffset(), \
               new Date(8.64e15).toDateString(), \
               Qt.formatDateTime(new Date(8.64e15), 'yyyy-MM-dd hh:mm'), \
               Qt.formatDateTime(new Date(8.64e15), 'ddd d MMM yy hh:mm:ss.zzz AP'), \
               new Date(8.64e15).toLocaleDateString(Qt.locale('en_US')), \
               String(new Date(-8.64e15)), new Date(-8.64e15).getTimezoneOffset(), \
               Qt.formatDateTime(new Date(-8.64e15), 'yyyy-MM-dd hh:mm'), \
               Qt.formatDate(new Date(-8.64e15), 'dddd d MMMM yyyy'), \
               Qt.formatDateTime(new Date(8.3e15), 'yyyy-MM-dd hh:mm:ss'), \
               Qt.formatDateTime(new Date(-8.3e15), 'yyyy-MM-dd hh:mm:ss')].join('|')";
    assert_eq!(
        probe(&MSK, src),
        "Sat Sep 13 03:00:00 275760 GMT+0300|-180|Sat Sep 13 275760|+275760-09-13 03:00|\
         Sat 13 Sep 60 03:00:00.000 AM|Saturday, September 13, +275760|\
         Tue Apr 20 02:30:17 -271822 GMT+0230|-150.28333333333333|-271822-04-20 02:30|\
         Tuesday 20 April -271822|+264986-07-12 22:33:20|-261048-06-21 06:56:57"
    );
}

/// B3-3: a local time in the hour a DST transition skips or repeats is the instant Qt 6.12
/// makes of it. A reading in the ECMAScript format or given to the Date constructor goes
/// by V4's UTC(t) (the offset in force at the reading less the zone's standard time: an
/// hour earlier than ECMA-262 §21.4.1.26 in the skipped hour, the later instant in the
/// repeated one); a reading QDateTime parses goes by QDateTime (the offset before the
/// skip, the earlier instant).
#[test]
fn skipped_local_time_is_the_instant_qt_makes_of_it() {
    let src = "[Date.parse('2010-03-28T02:30:00'), new Date(2010, 2, 28, 2, 30).getTime(), \
               Date.parse('2010-03-28T02:00:00'), Date.parse('2010-03-28T02:59:59.999'), \
               new Date('2010-03-28T02:30:00').getHours(), Date.parse('2011-03-27T02:30:00'), \
               Date.parse('2010-03-28T02:30'), Date.parse('Mar 28 2010 02:30:00'), \
               Date.parse('2010-03-28T01:59:59'), Date.parse('2010-03-28T03:00:00'), \
               Date.parse('2010-10-31T02:30:00'), new Date(2010, 9, 31, 2, 30).getTime(), \
               Date.parse('Oct 31 2010 02:30:00'), Date.parse('2014-10-26T01:30:00'), \
               new Date(2010, 2, 28, 2, 30).getTimezoneOffset()].join('|')";
    assert_eq!(
        probe(&MSK, src),
        "1269729000000|1269729000000|1269727200000|1269730799999|1|1301178600000|\
         1269729000000|1269732600000|1269730799000|1269730800000|\
         1288481400000|1288481400000|1288477800000|1414276200000|-180"
    );
}

/// B3-13: `Easing` holds QQuickEasingValueType::Type, where `Bezier` is `BezierSpline` and
/// QEasingCurve's `TCBSpline` and `Custom` do not exist.
#[test]
fn easing_enum_is_the_one_qt_exports() {
    let src = "[Easing.Bezier, Easing.TCBSpline, Easing.Custom, Easing.BezierSpline, \
               Easing.Linear, Easing.SineCurve, Easing.CosineCurve, Easing.OutInBounce, \
               Easing.InCurve, Easing.OutCurve, Easing.NCurveTypes].join('|')";
    assert_eq!(probe(&[], src), "45|||45|0|43|44|40|41|42|");
}

/// `Easing.valueForProgress(type, progress)` is the value of a QEasingCurve of that type, as
/// Qt 6.12 (PySide6) prints it; with fewer arguments it throws.
#[test]
fn easing_value_for_progress_is_the_curve_s_value() {
    let src = "(function () {
        function t(f) { try { return String(f()) } catch (e) { return 'throws ' + e } }
        var E = Easing
        return [
            () => E.valueForProgress(), () => E.valueForProgress(0.5),
            () => E.valueForProgress(E.InQuad, 0.5), () => E.valueForProgress(E.OutBounce, 0.3),
            () => E.valueForProgress(0, 0.5), () => E.valueForProgress('x', 0.5),
            () => E.valueForProgress(E.InQuad, '0.5'), () => E.valueForProgress(E.InQuad, 2),
            () => E.valueForProgress(E.InQuad, -1), () => E.valueForProgress(999, 0.5),
            () => E.valueForProgress(-1, 0.5), () => E.valueForProgress(E.InQuad, 0.5, 7),
            () => E.valueForProgress(E.BezierSpline, 0.5),
            () => E.valueForProgress(E.InElastic, 0.5), () => E.valueForProgress(E.OutBack, 0.5),
            () => E.valueForProgress(undefined, 0.5),
            () => E.valueForProgress(E.InQuad, undefined), () => E.valueForProgress(null, null),
            () => E.valueForProgress(1.9, 0.5), () => E.valueForProgress('1', 0.5),
            () => E.valueForProgress(E.InQuad, {}), () => E.valueForProgress.length,
            () => E.valueForProgress.name, () => E.valueForProgress(E.SineCurve, 0.3),
            () => E.valueForProgress(E.InOutCubic, 0.3), () => typeof E.valueForProgress,
        ].map(t).join('|')
    })()";
    assert_eq!(
        probe(&[], src),
        "throws Error: Insufficient arguments|throws Error: Insufficient arguments|0.25|\
         0.6806249999999999|0.5|0.5|0.25|1|0|0.5|0.5|0.25|0.5|-0.015625000000000045|\
         1.0876975|0.5|0|0|0.25|0.25|0|0||0.6545084971874737|0.108|function"
    );
}

/// `Component.createObject(parent, properties)` writes its initial properties as
/// QQmlProperty::write does and makes no object when one is refused.
#[test]
fn create_object_refuses_an_initial_property_as_qt() {
    check_fixture("create_object");
}

/// The initial properties go in the order of their names, before the bindings and
/// Component.onCompleted; the object one of them refused is deleted once it completed.
#[test]
fn create_object_orders_its_initial_properties_as_qt() {
    check_fixture("create_object_order");
}

/// Twenty properties by twenty values, each as an initial property.
#[test]
fn create_object_converts_initial_properties_as_qt() {
    check_fixture("create_object_matrix");
}

/// The same twenty properties by twenty values, each written by a script: the rule of
/// QV4::QObjectWrapper::setProperty, not that of an initial property.
#[test]
fn a_script_write_converts_its_value_as_qt() {
    check_fixture("property_write_matrix");
}

#[test]
fn create_object_writes_each_kind_of_property_as_qt() {
    check_fixture("create_object_values");
}

/// A number written to an enumeration property reads back as the enumeration's unsigned
/// type: -1 is 4294967295.
#[test]
fn create_object_writes_a_number_to_an_enumeration_as_qt() {
    check_fixture("create_object_enum");
}

/// The two places of qml/ whose initial properties come from outside data (a notification
/// read back from its file, a message of the assistant program): a string for a number, a
/// missing key (`undefined`) and `null`, as Qt answers them.
#[test]
fn create_object_takes_outside_data_as_qt() {
    check_fixture("create_object_outside");
}

/// An enumeration with an explicit C++ type (a quint8) is not narrowed to it, and the key of
/// an enumeration another class declares is refused: docs/qt-differences.md. The other
/// cases of the fixture answer as Qt.
#[test]
fn create_object_does_not_narrow_an_enumeration_of_an_explicit_type() {
    let ours: Vec<(usize, Option<String>)> =
        fixture_answers("create_object_enum_typed", &[("LC_ALL", "C.UTF-8")])
            .into_iter()
            .enumerate()
            .filter(|(_, (_, want, got))| got.as_ref() != Some(want))
            .map(|(i, (_, _, got))| (i, got))
            .collect();
    let held = [
        (2, "number:4294967295"),
        (4, "number:256"),
        (5, "number:257"),
        (6, "number:4294967295"),
        (8, "string:refused"),
        (9, "number:4294967295"),
    ];
    let held: Vec<_> = held
        .iter()
        .map(|(i, a)| (*i, Some(a.to_string())))
        .collect();
    assert_eq!(ours, held);
}
