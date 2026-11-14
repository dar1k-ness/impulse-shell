//! `readOnly` set on TextInput and TextEdit, and the positions of a TextInput with an input
//! mask. Each `text_read_only/<name>.qml` builds a `log`; `<name>.qt` is what Qt 6.12 printed
//! for it in the tools/testenv image (`text_read_only/qt.sh` writes them).

use qml::runtime::Runtime;

fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/text_read_only")
}

/// Runs the fixture and compares its `log` with Qt's line by line.
fn same_as_qt(name: &str) {
    let f = dir().join(format!("{name}.qml"));
    let mut rt = Runtime::new(Some(dir()));
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    assert_eq!(rt.errors(), Vec::<String>::new(), "{name}");
    let ours = rt.get(root, "log").to_js_string();
    let qt = std::fs::read_to_string(dir().join(format!("{name}.qt"))).unwrap();
    let (ours, qt): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), qt.lines().collect());
    let diff: Vec<String> = (0..ours.len().max(qt.len()))
        .filter(|&k| ours.get(k) != qt.get(k))
        .map(|k| {
            let line = |v: &[&str]| v.get(k).map_or("<none>".to_string(), |l| format!("{l:?}"));
            format!(
                "line {}:\n  qt   {}\n  ours {}",
                k + 1,
                line(&qt),
                line(&ours)
            )
        })
        .collect();
    assert!(
        diff.is_empty(),
        "{name}: {} of {} lines differ from Qt\n{}",
        diff.len(),
        qt.len(),
        diff.join("\n")
    );
}

/// Before the fix the cursor and the selection stayed where they were.
#[test]
fn editable_again_puts_the_cursor_at_the_end() {
    same_as_qt("read_only");
}

/// Before the fix select() and getText() took positions in `text`, shorter than the mask,
/// cursorPosition took any position up to the mask's length, and insert() and remove() moved
/// the rest of the text.
#[test]
fn a_partly_filled_mask_takes_positions_as_qt() {
    same_as_qt("mask_positions");
}

/// Before the fix `readOnly = true` left the cursor and the selection where they were.
#[test]
fn read_only_puts_the_cursor_at_the_end() {
    same_as_qt("becomes_read_only");
}
