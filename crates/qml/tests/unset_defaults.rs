//! A property of a Qt type that the document never sets reads what Qt 6.12 gives for it.
//!
//! `tests/unset_defaults/<module>.txt` has, for every type of that module the `qml` tree
//! creates, each property of a fresh instance (and the fields of its groups, value types and
//! non-item objects), a tab and what Qt 6.12 gives for it: PySide6 6.12.0, QQmlComponent, the
//! Basic style, written by `tests/unset_defaults/qt.py`.
//!
//! `tests/unset_defaults/leftover.txt` names the lines the engine still reads differently
//! (with the open PR that fixes the line or the test that holds it, where `owners.tsv` names
//! one): every other line is
//! held here. `UNSET_DEFAULTS_WRITE=1` writes that file again from what
//! the engine reads now.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use std::collections::{BTreeMap, HashSet};

const JS: &str = include_str!("unset_defaults/desc.js");

fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/unset_defaults")
}

/// `Type.path`, Qt's value and ours, for every line of `<module>.txt` where they differ.
fn differences(module: &str) -> Vec<(String, String, String)> {
    let text = std::fs::read_to_string(dir().join(format!("{module}.txt"))).unwrap();
    let mut types: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for l in text.lines() {
        let (path, qt) = l.split_once('\t').unwrap();
        let (ty, path) = path.split_once('.').unwrap();
        types.entry(ty).or_default().push((path, qt));
    }
    let import = if module == "QtQuick" {
        String::new()
    } else {
        format!("import {module}\n")
    };
    let mut out = Vec::new();
    for (ty, cases) in types {
        let reads: Vec<String> = (cases.iter())
            .map(|(p, _)| format!("c(() => t.{p})"))
            .collect();
        let src = format!(
            "import QtQuick\n{import}Item {{\n    id: root\n    property string result\n    \
             {ty} {{ id: t }}\n{JS}\n    function c(f) {{ try {{ return desc(f()) }} catch (e) \
             {{ return \"!\" + e }} }}\n    Component.onCompleted: result = [\n{}\n].join(\"\\n\")\n}}\n",
            reads.join(",\n")
        );
        let mut rt = Runtime::new(None);
        let root = match rt.load_str(&src) {
            Ok(root) => root,
            Err(e) => {
                out.push((ty.to_string(), "an object".to_string(), e));
                continue;
            }
        };
        rt.settle();
        let result = rt.get(root, "result").to_js_string();
        let ours: Vec<&str> = result.split('\n').collect();
        assert_eq!(ours.len(), cases.len(), "{ty}: {:?}", rt.errors());
        let cwd = std::env::current_dir().unwrap();
        for ((path, qt), ours) in cases.iter().zip(ours) {
            // What depends on the directory the test runs in is written `$CWD`.
            let ours = ours.replace(&*cwd.to_string_lossy(), "$CWD");
            if *qt != ours {
                out.push((format!("{ty}.{path}"), qt.to_string(), ours));
            }
        }
    }
    out
}

const MODULES: &[&str] = &[
    "QtQuick",
    "QtQuick.Controls",
    "QtQuick.Layouts",
    "QtQuick.Effects",
    "QtQuick.Shapes",
    "Qt5Compat.GraphicalEffects",
    "Qt.labs.folderlistmodel",
    "Qt.labs.synchronizer",
];

#[test]
fn unset_properties_read_what_qt_gives() {
    let diff: Vec<_> = MODULES.iter().flat_map(|m| differences(m)).collect();
    let path = dir().join("leftover.txt");
    if std::env::var_os("UNSET_DEFAULTS_WRITE").is_some() {
        // `owners.tsv`: the end of a path and the open PR that fixes it, named in a
        // fourth column.
        let owners = std::fs::read_to_string(dir().join("owners.tsv")).unwrap();
        let owners: Vec<(&str, &str)> = (owners.lines())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.split_once('\t').unwrap())
            .collect();
        let owner = |p: &str| {
            let rest = p.split_once('.').unwrap().1;
            (owners.iter())
                .find(|(k, _)| *k == p || *k == rest || rest.contains(&format!("{k}.")))
                .map_or(String::new(), |(_, pr)| format!("\t{pr}"))
        };
        let lines: Vec<String> = (diff.iter())
            .map(|(p, qt, ours)| format!("{p}\t{qt}\t{ours}{}\n", owner(p)))
            .collect();
        std::fs::write(&path, lines.concat()).unwrap();
        return;
    }
    let text = std::fs::read_to_string(path).unwrap();
    let leftover: HashSet<&str> = (text.lines())
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    let wrong: Vec<String> = (diff.iter())
        .filter(|(p, ..)| !leftover.contains(p.as_str()))
        .map(|(p, qt, ours)| format!("{p}\n  qt:   {qt}\n  ours: {ours}"))
        .collect();
    assert!(
        wrong.is_empty(),
        "{} differ\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
