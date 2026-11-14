//! `crates/qml/src/locale_data.rs` is what `tools/gen/locale/gen.py` writes from the QLocale of
//! Qt 6.12 (the Qt of ri-testenv's system, asked through PySide6: tools/testenv/system_qt.py),
//! byte for byte: the generator is run and its output compared with the file in the tree.

/// The Python with PySide6 in ri-testenv (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

#[test]
fn the_generator_writes_the_file_in_the_tree() {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    let written = dir.path().join("locale_data.rs");
    let out = std::process::Command::new(PYTHON)
        .arg(root.join("tools/gen/locale/gen.py"))
        .arg(&written)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "tools/gen/locale/gen.py failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let written = std::fs::read_to_string(written).unwrap();
    let kept = std::fs::read_to_string(root.join("crates/qml/src/locale_data.rs")).unwrap();
    if written == kept {
        return;
    }
    let line = written
        .lines()
        .zip(kept.lines())
        .position(|(a, b)| a != b)
        .unwrap_or(written.lines().count().min(kept.lines().count()));
    let cut = |s: &str| {
        let l = s.lines().nth(line).unwrap_or("<no such line>");
        l.chars().take(160).collect::<String>()
    };
    panic!(
        "tools/gen/locale/gen.py does not write crates/qml/src/locale_data.rs; first difference \
         at line {}:\n  generated: {}\n  in tree:   {}",
        line + 1,
        cut(&written),
        cut(&kept)
    );
}
