//! A Qt type nothing is set in, as an item of a laid out window, has in every property what
//! Qt 6.12 gives it.
//!
//! `crates/qml/tests/unset_defaults/<module>.txt` has what Qt 6.12 gives a fresh instance
//! (see `crates/qml/tests/unset_defaults.rs`), and `tests/unset_window/window.txt` the few
//! lines a shown window changes (`tests/unset_window/qt.py`). That test reads the engine
//! without a window, so a size that the layout or the text gives is not there; this one reads
//! the same paths after the window was laid out.
//!
//! `tests/unset_window/leftover.txt` names the lines that still differ: every other line is
//! held here. `UNSET_DEFAULTS_WRITE=1` writes that file again.

use qml::runtime::Runtime;
use shell::scene;
use std::collections::{BTreeMap, HashMap, HashSet};

const JS: &str = include_str!("../../qml/tests/unset_defaults/desc.js");

fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/unset_window")
}

/// The directory the scene of type `ty` is loaded from.
fn scene_dir(ty: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("unset_window_{}_{ty}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What the scene `src` reads for each of its `n` paths once its window was laid out.
fn read(ty: &str, src: &str, n: usize) -> Result<Vec<String>, String> {
    let file = scene_dir(ty).join("x.qml");
    std::fs::write(&file, src).unwrap();
    let mut rt = Runtime::new(None);
    let root = rt.load_file(&file)?;
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut now = 0.0;
    rt.tick(now);
    for _ in 0..20 {
        scene::layout_window(&mut rt, &mut fonts, root, [400.0, 300.0]);
        now += 16.0;
        rt.poll_native();
        rt.tick(now);
    }
    let result = rt.get(root, "result").to_js_string();
    let ours: Vec<String> = result.split('\n').map(str::to_string).collect();
    assert_eq!(ours.len(), n, "{ty}: {:?}", rt.errors());
    Ok(ours)
}

/// `Type.path`, Qt's value and ours, for every line of `<module>.txt` where they differ.
fn differences(module: &str, window: &HashMap<&str, &str>) -> Vec<(String, String, String)> {
    let fresh = dir().join(format!("../../../qml/tests/unset_defaults/{module}.txt"));
    let text = std::fs::read_to_string(fresh).unwrap();
    let mut types: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for l in text.lines() {
        let (path, qt) = l.split_once('\t').unwrap();
        let qt = window.get(path).copied().unwrap_or(qt);
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
            "import QtQuick\n{import}Item {{\n    id: root\n    width: 400\n    height: 300\n    \
             property string result\n    {ty} {{ id: t }}\n{JS}\n    function c(f) {{ try {{ \
             return desc(f()) }} catch (e) {{ return \"!\" + e }} }}\n    Timer {{ interval: 200; \
             running: true; onTriggered: root.result = [\n{}\n].join(\"\\n\") }}\n}}\n",
            reads.join(",\n")
        );
        // A thread for each scene: the layout keeps what it knows of an item by its number
        // for the thread's one runtime.
        let (name, n) = (ty.to_string(), cases.len());
        let ours = std::thread::spawn(move || read(&name, &src, n));
        match ours.join().unwrap() {
            Err(e) => out.push((ty.to_string(), "an object".to_string(), e)),
            Ok(ours) => {
                let cwd = std::env::current_dir().unwrap();
                let up = cwd.parent().unwrap().to_string_lossy().into_owned();
                let scene = scene_dir(ty).to_string_lossy().into_owned();
                for ((path, qt), ours) in cases.iter().zip(ours) {
                    let ours = ours
                        .replace(&scene, "$DIR")
                        .replace(&*cwd.to_string_lossy(), "$CWD")
                        .replace(&up, "$UP");
                    if *qt != ours {
                        out.push((format!("{ty}.{path}"), qt.to_string(), ours));
                    }
                }
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
    "Qt.labs.folderlistmodel",
    "Qt.labs.synchronizer",
];

#[test]
fn unset_properties_in_a_window_read_what_qt_gives() {
    let window = std::fs::read_to_string(dir().join("window.txt")).unwrap();
    let window: HashMap<&str, &str> = (window.lines())
        .map(|l| l.split_once('\t').unwrap())
        .collect();
    let diff: Vec<_> = (MODULES.iter())
        .flat_map(|m| differences(m, &window))
        .collect();
    let path = dir().join("leftover.txt");
    if std::env::var_os("UNSET_DEFAULTS_WRITE").is_some() {
        // `owners.tsv` of the fresh test: the end of a path and the open PR that fixes it,
        // named in a fourth column.
        let owners = dir().join("../../../qml/tests/unset_defaults/owners.tsv");
        let owners = std::fs::read_to_string(owners).unwrap();
        let owners: Vec<(&str, &str)> = (owners.lines())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.split_once('\t').unwrap())
            .filter(|(_, pr)| pr.starts_with('#'))
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
