//! A write to a field of a value type property nothing set (`img.sourceClipRect.x = 3`)
//! reaches the property, whose signal comes once for a new value and not for the same one.
//!
//! `tests/value_field_writes/qt.tsv` has, for every such property of each Qt type the `qml`
//! tree creates, what Qt 6.12 gives for `probe` of `tests/value_field_writes/probe.js`:
//! PySide6 6.12.0, QQmlComponent, written by `tests/value_field_writes/qt.py`.
//!
//! `tests/value_field_writes/leftover.txt` names the lines the engine still gives another
//! answer for; `VALUE_FIELD_WRITES_WRITE=1` writes it again from what the engine gives now.
//! The three there differ for a write of the whole property as well: Qt's Image sends
//! `sourceSizeChanged` from the setter and again from the load that follows, and its
//! DragHandler has no `persistentTranslationChanged` (the signal is `translationChanged`).
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use std::collections::HashSet;

const JS: &str = include_str!("value_field_writes/probe.js");
const QT: &str = include_str!("value_field_writes/qt.tsv");

fn leftover() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/value_field_writes/leftover.txt")
}

#[test]
fn a_field_written_to_an_unset_value_type_property_does_what_qt_does() {
    let mut diff = Vec::new();
    for l in QT.lines() {
        let [module, path, field, qt] = l.split('\t').collect::<Vec<_>>()[..] else {
            panic!("{l}")
        };
        let (ty, prop) = path.split_once('.').unwrap();
        let import = if module == "QtQuick" {
            String::new()
        } else {
            format!("import {module}\n")
        };
        let src = format!(
            "import QtQuick\n{import}Item {{\n    id: root\n    property string result\n    \
             {ty} {{ id: t }}\n{JS}\n    Component.onCompleted: result = \
             probe(t, \"{prop}\", \"{field}\")\n}}\n"
        );
        let mut rt = Runtime::new(None);
        let ours = match rt.load_str(&src) {
            Ok(root) => {
                rt.settle();
                let r = rt.get(root, "result").to_js_string();
                match rt.errors().first() {
                    Some(e) if r.is_empty() => format!("!{e}"),
                    _ => r,
                }
            }
            Err(e) => e,
        };
        if ours != qt {
            diff.push((path, qt, ours));
        }
    }
    if std::env::var_os("VALUE_FIELD_WRITES_WRITE").is_some() {
        let lines: Vec<String> = (diff.iter())
            .map(|(p, qt, ours)| format!("{p}\t{qt}\t{ours}\n"))
            .collect();
        std::fs::write(leftover(), lines.concat()).unwrap();
        return;
    }
    let text = std::fs::read_to_string(leftover()).unwrap();
    let known: HashSet<&str> = (text.lines())
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    let wrong: Vec<String> = (diff.iter())
        .filter(|(p, ..)| !known.contains(p))
        .map(|(p, qt, ours)| format!("{p}\n  qt:   {qt}\n  ours: {ours}"))
        .collect();
    assert!(
        wrong.is_empty(),
        "{} differ\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
