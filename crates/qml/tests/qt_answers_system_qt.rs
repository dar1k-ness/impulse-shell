//! Qt's answers in the fixtures of this crate and the rows of `locale_data.rs` come from one
//! Qt: the tools/testenv image's own (Arch's qt6-base, which uses the system's ICU), asked
//! through `tools/testenv/system_qt.py`. The Qt inside the PySide6 wheel carries another ICU
//! (73 against the system's 78.3), and so other names of zones and other case mappings: a
//! generator that asked it would write answers the shell's Qt does not give.

use std::path::Path;

/// The Python with PySide6 in ri-testenv (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";
/// Places that ask the Qt inside the PySide6 wheel and may: what they ask does not come of
/// ICU's data or of Unicode's. A new place goes through system_qt.py, not here. Those of pull
/// requests still open are named too, so that they land green.
const OTHER: &[(&str, &str)] = &[
    (
        "crates/colorgen/src/kde/kconfig.rs",
        "QLockFile and QSaveFile",
    ),
    (
        "crates/colorgen/tests/kde_files.rs",
        "QLockFile and QSaveFile",
    ),
    ("crates/rigen/check/qt.py", "#139: states and handlers"),
    ("crates/qml/src/json/tests.rs", "JSON numbers of QJSEngine"),
    (
        "crates/qml/tests/builtins_b2/qt.py",
        "#218: built-ins, no text of a locale",
    ),
    (
        "crates/qml/tests/component_not_ready.rs",
        "#260: a component's status",
    ),
    ("crates/qml/tests/openurl_relative/qt.py", "#308: urls"),
    (
        "crates/qml/tests/posted_order.rs",
        "#283: order of posted events",
    ),
    (
        "crates/qml/tests/qt_leftovers.rs",
        "#193: Controls' properties and errors",
    ),
    (
        "crates/qml/tests/runtime_audit_r3/qt.py",
        "#135: built-in methods of QML types",
    ),
    (
        "crates/qml/tests/state_in_handler/qt.py",
        "#139: states and handlers",
    ),
    (
        "crates/qml/tests/support/qt_log.rs",
        "#84: the log of a QML file",
    ),
    (
        "crates/qml/tests/switchwall_kde_lock.rs",
        "QLockFile and QSaveFile",
    ),
    (
        "crates/qml/tests/unset_defaults/attached.py",
        "#293: properties nothing set",
    ),
    (
        "crates/qml/tests/unset_defaults/qt.py",
        "#213: properties nothing set",
    ),
    (
        "crates/qml/tests/value_field_writes/qt.py",
        "#256: writes to value types",
    ),
    (
        "crates/qml/tests/warning_order/qtmsg.py",
        "#297: order of warnings",
    ),
    ("crates/qsg/tests/depth16_qt.rs", "pixels of images"),
    ("crates/qsg/tests/formats_qt.rs", "pixels of images"),
    ("crates/qsg/tests/scale_qt.rs", "#252: pixels of images"),
    (
        "crates/shell/tests/animated_image_qt.rs",
        "#181: frames of images",
    ),
    (
        "crates/shell/tests/qt_geometry/dump.py",
        "#284: geometry of items",
    ),
    (
        "crates/shell/tests/qt_props/qt.py",
        "#256: properties of items",
    ),
    (
        "crates/shell/tests/scenes/basic_sizes/qt.py",
        "#290: sizes of controls",
    ),
    (
        "crates/shell/tests/scenes/cursor_delegate/dump.py",
        "#292: geometry of a cursor",
    ),
    (
        "crates/shell/tests/scenes/qt_geometry/dump.py",
        "#154: geometry of items",
    ),
    (
        "crates/shell/tests/scenes/qt_view_content/qt.py",
        "#341: geometry of a view's rows",
    ),
    (
        "crates/shell/tests/scenes/scroll_view/qt.py",
        "#319: geometry of a ScrollView",
    ),
    (
        "crates/shell/tests/sync_load/qt.py",
        "#308: when a load is ready",
    ),
    (
        "crates/shell/tests/text_press/qt_press.py",
        "#311: a press on a text field",
    ),
    (
        "crates/shell/tests/unset_window/qt.py",
        "#231: properties nothing set",
    ),
    ("tools/oracle/blur_oracle.py", "pixels"),
    ("tools/oracle/canvas_oracle.py", "pixels"),
    ("tools/oracle/effects_oracle.py", "pixels"),
    ("tools/oracle/image_oracle.py", "pixels"),
    ("tools/oracle/me_oracle.py", "pixels"),
    ("tools/oracle/progress_oracle.py", "#289: pixels"),
    ("tools/oracle/rect_oracle.py", "pixels"),
    ("tools/oracle/scale_oracle.py", "pixels"),
    ("tools/oracle/srcsize_oracle.py", "pixels"),
    ("tools/oracle/steps_oracle.py", "#270: pixels"),
    ("tools/oracle/svg_oracle.py", "pixels"),
    (
        "tools/gen/qmltypes/private_methods.py",
        "#135: names of methods",
    ),
    (
        "tools/gen/qqmljs/fuzz/probe.py",
        "#301: what the parser rejects",
    ),
];
/// Places that ask the Qt inside the PySide6 wheel for what does come of ICU's data or of
/// Unicode's (collation, case, line breaks, text of both directions, charsets): their answers
/// are of ICU 73 until their owners move them to system_qt.py. Nothing is added here.
const OWED: &[(&str, &str)] = &[
    (
        "crates/qsg/tests/bidi_equiv.rs",
        "#59, #267: text of both directions",
    ),
    (
        "crates/qsg/tests/cursor_equiv.rs",
        "#244: text of both directions",
    ),
    (
        "crates/qsg/tests/hb_unicode_equiv.rs",
        "#267: Unicode properties in shaping",
    ),
    (
        "crates/shell/tests/text_line_order_equiv.rs",
        "#244: text of both directions",
    ),
];

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// system_qt.py ends with a text when the Qt that answers is not the system's, or the
/// system's ICU is not the one the answers in the tree were taken with.
#[test]
fn the_qt_that_answers_has_the_systems_icu() {
    if !Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
        return;
    }
    let out = std::process::Command::new(PYTHON)
        .arg(root().join("tools/testenv/system_qt.py"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "Qt 6.12.0 with ICU 78.3\n"
    );
}

/// The files under `dir` with a line that imports PySide6, in Python of their own or in a
/// script a Rust test holds.
fn askers(root: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                askers(root, &path, found);
            }
            continue;
        }
        if !["rs", "py", "sh"]
            .iter()
            .any(|ext| name.ends_with(&format!(".{ext}")))
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (mut through, mut asks) = (false, false);
        for line in text.lines().map(str::trim_start) {
            through |= line.starts_with("import system_qt");
            let imports = line.starts_with("from PySide6")
                || line.starts_with("import ") && line.contains("PySide6");
            if imports && !through {
                asks = true;
            }
        }
        let path = path.strip_prefix(root).unwrap();
        let path = path.to_string_lossy().into_owned();
        // system_qt.py asks under its own check.
        if asks && path != "tools/testenv/system_qt.py" {
            found.push(path);
        }
    }
}

/// Every place of the tree that imports PySide6 imports system_qt.py first, or is one of the
/// places named above. A new way to take an answer of Qt fails here with a text.
#[test]
fn every_place_that_asks_qt_asks_through_system_qt() {
    let root = root().canonicalize().unwrap();
    let mut found = Vec::new();
    for dir in ["crates", "tools"] {
        askers(&root, &root.join(dir), &mut found);
    }
    found.sort();
    let named = |path: &String| OTHER.iter().chain(OWED).any(|(other, _)| other == path);
    let bad: Vec<&String> = found.iter().filter(|path| !named(path)).collect();
    assert!(
        bad.is_empty(),
        "these import PySide6 without `import system_qt` (tools/testenv/system_qt.py) before \
         it, so the Qt inside the wheel answers them, with its ICU 73 and not the system's \
         78.3: {bad:?}"
    );
    // The check sees what it is for: the places that do go through system_qt.py are not found.
    for through in [
        "tools/gen/locale/gen.py",
        "crates/aotcheck/src/v4oracle.py",
        "crates/rigen/check/v4.py",
        "crates/qml/tests/expr_corpus_qt.rs",
        "crates/qml/tests/locale_digits_qt/qt.py",
        "crates/qml/tests/system_zone_names_qt/qt.py",
        "crates/qml/tests/qt_icu.py",
        "crates/qml/tests/xhr_equiv.rs",
        "crates/qml/tests/zone_names_qt/qt.py",
    ] {
        let text = std::fs::read_to_string(root.join(through)).unwrap();
        assert!(
            text.contains("PySide6") && text.contains("\nimport system_qt"),
            "{through}"
        );
        assert!(!found.iter().any(|path| path == through), "{through}");
    }
    assert!(
        found
            .iter()
            .any(|path| path == "crates/qsg/tests/bidi_equiv.rs"),
        "{found:?}"
    );
}
