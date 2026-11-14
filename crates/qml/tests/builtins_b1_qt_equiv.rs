//! The JavaScript built-ins of slice B1 of the second engine audit against Qt 6.12. Each
//! `data/builtins_b1/NAME.qml` prints one console line per case; `NAME.qt612` holds the lines
//! `qml6` of Qt 6.12 prints for it (`TZ=UTC QT_QPA_PLATFORM=offscreen qml6 NAME.qml`, the
//! `qml: ` prefix removed). A case passes when this engine prints the same line.
//!
//! [`KNOWN`] lists the cases that differ and why; they are not compared. `NAME.here` holds
//! the lines of a deliberate difference (docs/qt-differences.md).

use qml::runtime::Runtime;

/// Why a listed case differs from Qt 6.12: it waits for another pull request. `seen` names
/// the pull requests and the head of each on which this fixture's case printed Qt's line;
/// it is empty while no head has been seen to print it. Once one of them is merged the case
/// is equal here, and [`compare`] fails until its row is taken out.
struct Why {
    what: &'static str,
    seen: &'static [(u32, &'static str)],
}

impl std::fmt::Display for Why {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let seen: Vec<String> = self
            .seen
            .iter()
            .map(|(pr, head)| format!("#{pr} at {head}"))
            .collect();
        match seen.is_empty() {
            true => write!(f, "{} (not seen equal to Qt on any head)", self.what),
            false => write!(f, "{} (equal to Qt on {})", self.what, seen.join(", ")),
        }
    }
}

/// (fixture, case, why): the case is the text before ` => `, or the first word of the line.
const KNOWN: &[(&str, &str, Why)] = &[
    // A debt: this case answered as Qt before a colour got its members (pull request #360).
    // A value type read from a property as a reference gives it back
    // (`audit_b3_env::value_types_read_from_a_property_are_references_as_in_qt`, pull
    // request #262), and this fixture has not been run with that.
    (
        "misc",
        "Z6",
        Why {
            what: "`Object.isFrozen` of a frozen `Qt.rgba` colour is true",
            seen: &[],
        },
    ),
    // What the tail of long arrays (B2-17) took from the array storage of B1: both were
    // merged in #308, and without the tail these cases are equal to Qt.
    (
        "longtail",
        "t4",
        Why {
            what: "QV4 appends to the storage of a frozen or non-extensible array (`push`), the 65537 or more holes at its end stored: here they are a tail without storage and `push` throws; known from the merge of the storage of B1 with the tail of B2, docs/qt-differences.md",
            seen: &[(335, "4e932a8b")],
        },
    ),
    (
        "longtail",
        "t7",
        Why {
            what: "QV4 appends to the storage of a frozen or non-extensible array (`push`), the 65537 or more holes at its end stored: here they are a tail without storage and `push` throws; known from the merge of the storage of B1 with the tail of B2, docs/qt-differences.md",
            seen: &[(335, "4e932a8b")],
        },
    ),
    (
        "longtail",
        "t24",
        Why {
            what: "QV4 appends to the storage of a frozen or non-extensible array (`push`), the 65537 or more holes at its end stored: here they are a tail without storage and `push` throws; known from the merge of the storage of B1 with the tail of B2, docs/qt-differences.md",
            seen: &[(335, "4e932a8b")],
        },
    ),
    (
        "longtail",
        "t31",
        Why {
            what: "QV4 appends to the storage of a frozen or non-extensible array (`push`), the 65537 or more holes at its end stored: here they are a tail without storage and `push` throws; known from the merge of the storage of B1 with the tail of B2, docs/qt-differences.md",
            seen: &[(335, "4e932a8b")],
        },
    ),
    (
        "longtail",
        "t18",
        Why {
            what: "QV4 sorts the storage of a frozen array, the 65537 or more holes at its end stored: here they are a tail without storage and `sort` throws; known from the merge of the storage of B1 with the tail of B2, docs/qt-differences.md",
            seen: &[(335, "4e932a8b")],
        },
    ),
];

thread_local! {
    /// The console lines of this thread's runtime.
    static CONSOLE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn ours(src: &str) -> Vec<String> {
    let _ = qml::builtins::CONSOLE_SINK.set(|_, line| {
        CONSOLE.with_borrow_mut(|c| c.push(line.to_string()));
    });
    CONSOLE.with_borrow_mut(|c| c.clear());
    let mut rt = Runtime::new(None);
    rt.load_str(src).unwrap();
    rt.settle();
    // Qt's message handler writes a line to stderr as a C string: it ends at a NUL.
    let shown = |l: &str| l.split('\0').next().unwrap_or_default().to_string();
    CONSOLE
        .take()
        .iter()
        .flat_map(|l| l.split('\n'))
        .map(shown)
        .collect()
}

/// `line` with the address of every QObject in it (`Class(0x5b28e3f9ae80)`) as `0xADDR`: an
/// address is compared by its shape, `0x` and hexadecimal digits, not by its value.
fn without_addresses(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find("(0x") {
        let digits = rest[at + 3..]
            .bytes()
            .take_while(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
            .count();
        out.push_str(&rest[..at + 3]);
        if digits > 0 {
            out.push_str("ADDR");
        }
        rest = &rest[at + 3 + digits..];
    }
    out + rest
}

fn case(line: &str) -> &str {
    match line.split_once(" => ") {
        Some((id, _)) => id,
        None => line.split(' ').next().unwrap_or(line),
    }
}

/// Runs fixture `name` and compares every case not in [`KNOWN`] with Qt's line. `here` holds
/// the lines of the deliberate difference (docs/qt-differences.md): such a case has to print
/// that line, and Qt another.
fn compare(name: &str, src: &str, qt: &str, here: &str) {
    let ours = ours(src);
    let qt: Vec<&str> = qt.lines().collect();
    let here: Vec<&str> = here.lines().collect();
    assert_eq!(
        ours.len(),
        qt.len(),
        "{name}: number of console lines; ours:\n{}",
        ours.join("\n")
    );
    let mut bad = Vec::new();
    let (mut skipped, mut deliberate) = (0, 0);
    for (a, b) in ours.iter().zip(&qt) {
        let (a, b) = (without_addresses(a), without_addresses(b));
        let (a, b) = (&a.as_str(), &b.as_str());
        assert_eq!(case(a), case(b), "{name}: order of cases");
        if let Some(h) = here.iter().find(|h| case(h) == case(b)) {
            assert_ne!(h, b, "{name}: Qt 6.12 prints this line of {name}.here too");
            if a != h {
                bad.push(format!("{name}:\n  ours: {a}\n  here: {h}\n  qt:   {b}"));
            }
            deliberate += 1;
        } else if let Some((.., why)) = KNOWN.iter().find(|(f, c, _)| *f == name && *c == case(b)) {
            assert_ne!(
                a,
                b,
                "{name} {}: equal to Qt 6.12, take it out of KNOWN (it waited for: {why})",
                case(b)
            );
            println!("{name} {}: differs, waits for: {why}", case(b));
            skipped += 1;
        } else if a != b {
            bad.push(format!("{name}:\n  ours: {a}\n  qt:   {b}"));
        }
    }
    assert_eq!(
        deliberate,
        here.len(),
        "{name}.here names a case {name}.qml lacks"
    );
    assert!(
        bad.is_empty(),
        "{} cases differ from Qt 6.12\n{}",
        bad.len(),
        bad.join("\n")
    );
    println!(
        "{name}: {} equal to Qt 6.12, {deliberate} deliberate differences, {skipped} known differences",
        qt.len() - skipped - deliberate
    );
}

/// Whether this is the process of test `test` whose zone is `zone`; otherwise runs the test
/// in such a process. A Date prints in the system's zone, and Qt's lines are those of `zone`.
fn in_zone(test: &str, zone: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    testenv::run_in_own_process(test, [("TZ", zone)]);
    false
}

macro_rules! fixture {
    ($($test:ident => $name:literal $(+ $here:literal)?),* $(,)?) => {$(
        #[test]
        fn $test() {
            if in_zone(concat!(module_path!(), "::", stringify!($test)), "UTC") {
                compare(
                    $name,
                    include_str!(concat!("data/builtins_b1/", $name, ".qml")),
                    include_str!(concat!("data/builtins_b1/", $name, ".qt612")),
                    concat!($(include_str!(concat!("data/builtins_b1/", $here)),)? ""),
                );
            }
        }
    )*};
}

/// The fixture `date` in a zone: `date.ZONE.qt612` holds Qt's lines there (`TZ=$zone`).
macro_rules! date_in_zone {
    ($($test:ident => $zone:literal, $qt:literal),* $(,)?) => {$(
        #[test]
        fn $test() {
            if in_zone(concat!(module_path!(), "::", stringify!($test)), $zone) {
                compare(
                    "date",
                    include_str!("data/builtins_b1/date.qml"),
                    include_str!(concat!("data/builtins_b1/", $qt)),
                    "",
                );
            }
        }
    )*};
}

date_in_zone! {
    date_text_utc => "UTC", "date.utc.qt612",
    date_text_ahead_of_utc => "Europe/Moscow", "date.europe_moscow.qt612",
    date_text_behind_utc_with_summer_time => "America/New_York", "date.america_new_york.qt612",
    date_text_half_hour_zone => "Asia/Kolkata", "date.asia_kolkata.qt612",
}

fixture! {
    audit_fixture_2_4 => "audit_2_4" + "audit_2_4.here",
    console_log => "console",
    color_methods_and_parsing => "color",
    color_rgba => "col2",
    builtin_descriptors => "obj",
    builtins_are_locked => "lock",
    regexp_object => "t4" + "t4.here",
    regexp_without_u => "re" + "re.here",
    function_to_string => "fn",
    function_name_and_conversion => "n3",
    connect_freeze_arg => "misc" + "misc.here",
    qml_object_integrity => "frz",
    string_arg => "arg",
    string_arg_numbers => "argnum" + "argnum.here",
    messages => "msg" + "msg.here",
    messages_more => "msg2" + "msg2.here",
    function_text_and_regexp_source => "fnre",
    own_keys => "keys",
    regexp_folding_exec_template_bound => "ecma" + "ecma.here",
    array_species_replace_receiver_super => "species",
    derived_constructors => "derived" + "derived.here",
    frozen_array_writes => "frozen",
    frozen_array_methods => "frozen2",
    frozen_array_like_objects => "frozen3",
    array_storage_slots_and_attribute_bytes => "arraystore" + "arraystore.here",
    array_storage_hidden_elements_and_sparse => "arraystore2" + "arraystore2.here",
    array_storage_defined_element_of_an_empty_array => "emptydefine",
    array_storage_methods_of_a_long_frozen_array => "longtail",
    arrow_this_and_super => "arrow",
    typed_array_base_and_collection_callbacks => "typedbase" + "typedbase.here",
    not_a_function_and_error_keys => "notfn" + "notfn.here",
    own_key_order_of_every_builtin => "keyorder" + "keyorder.here",
    qml_object_names_define_property_type_objects => "qobj",
}

/// The `.qml`, `.js` and `.mjs` files of the shell's `qml/` with their text.
fn shell_sources() -> Vec<(std::path::PathBuf, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(std::path::PathBuf, String)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else if matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("qml" | "js" | "mjs")
            ) {
                let text =
                    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                out.push((p, text));
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml"),
        &mut out,
    );
    assert!(out.len() > 100, "qml/ was not found: {} files", out.len());
    out
}

/// `file:line: text` of every line of `qml/` that `found` accepts.
fn shell_lines(found: impl Fn(&str) -> bool) -> Vec<String> {
    let mut lines = Vec::new();
    for (path, text) in shell_sources() {
        for (n, line) in text.lines().enumerate() {
            if found(line) {
                lines.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
            }
        }
    }
    lines
}

/// docs/qt-differences.md, "An array in V4's sparse storage": an array V4 holds in its
/// sparse storage answers `freeze`, `seal` and `sort` differently here, and that is left so
/// because `qml/` calls none of the functions that make such an array. This holds the count
/// at 0. The text does not say what the receiver is, so any such call fails the test.
#[test]
fn shell_qml_does_not_freeze_seal_or_define_properties() {
    const CALLS: [&str; 7] = [
        "Object.freeze",
        "Object.seal",
        "Object.preventExtensions",
        "Object.defineProperty",
        "Object.defineProperties",
        "Reflect.defineProperty",
        "Reflect.preventExtensions",
    ];
    let lines = shell_lines(|line| CALLS.iter().any(|c| line.contains(c)));
    assert!(
        lines.is_empty(),
        "qml/ calls Object.freeze, seal, preventExtensions or defineProperty in {} place(s). \
         If the receiver can be an array, see docs/qt-differences.md, \"An array in V4's \
         sparse storage\": an array with an accessor element or a gap of 4096 answers \
         differently from Qt 6.12 after these calls.\n{}",
        lines.len(),
        lines.join("\n")
    );
}

/// docs/qt-differences.md, "`forEach` of a Map or a Set without a callback": the count of
/// `forEach()` with no argument in `qml/` is 0.
#[test]
fn shell_qml_has_no_for_each_without_a_callback() {
    let lines = shell_lines(|line| {
        line.match_indices("forEach(")
            .any(|(at, call)| line[at + call.len()..].trim_start().starts_with(')'))
    });
    assert!(
        lines.is_empty(),
        "qml/ calls forEach() without a callback in {} place(s). If the receiver can be a \
         Map or a Set, see docs/qt-differences.md, \"`forEach` of a Map or a Set without a \
         callback\".\n{}",
        lines.len(),
        lines.join("\n")
    );
}
