//! A layout with a panel loads the original's horizontal and vertical bar together, and each declares the
//! global shortcut `barToggle`. Hyprland sends one press, the shell passes it to every
//! `GlobalShortcut` of that name: the bar is toggled once, as with one bar loaded.

use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

/// The tree with `main` as its main.qml.
fn tree(tag: &str, main: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("bar-toggle-shortcut-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("zzmain.qml"), main).unwrap();
    d
}

fn start(d: &Path) -> Runtime {
    let mut rt = Runtime::new(Some(d.to_path_buf()));
    rt.load_singletons(&[
        d.to_path_buf(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.settle();
    rt.load_file(&d.join("zzmain.qml")).unwrap();
    rt.settle();
    rt
}

fn bar_open(rt: &Runtime) -> bool {
    let states = *rt.world.borrow().singletons.get("GlobalStates").unwrap();
    matches!(rt.get(states, "barOpen"), Value::Bool(true))
}

/// One press of the global shortcut `name`, delivered as the shell does: `pressed` on every
/// live `GlobalShortcut` of that name. Returns how many there are.
fn press(rt: &mut Runtime, name: &str) -> usize {
    let items: Vec<usize> = {
        let mut w = rt.world.borrow_mut();
        let mut items = w.of_type("GlobalShortcut");
        items.retain(|i| !w.dead.contains(i));
        items.retain(|&i| matches!(w.read(i, "name"), Some(Value::Str(s)) if &*s == name));
        items
    };
    {
        let mut w = rt.world.borrow_mut();
        for &i in &items {
            w.write(i, "pressed", Value::Bool(true));
            qml::native::emit(&mut w, i, "pressed", &[], Vec::new());
        }
    }
    rt.settle();
    items.len()
}

fn toggles(tag: &str, main: &str, shortcuts: usize) {
    let d = tree(tag, main);
    let mut rt = start(&d);
    assert!(bar_open(&rt), "the bar starts open");
    assert_eq!(press(&mut rt, "barToggle"), shortcuts);
    assert!(!bar_open(&rt), "one press hides the bar");
    assert_eq!(press(&mut rt, "barToggle"), shortcuts);
    assert!(bar_open(&rt), "the next press shows it again");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn both_bars_loaded_toggle_once() {
    toggles(
        "both",
        "import QtQuick\nimport ri.modules.rusty.bar\nimport ri.modules.rusty.verticalBar\nItem { Bar {} VerticalBar {} }\n",
        2,
    );
}

#[test]
fn horizontal_bar_alone_toggles() {
    toggles(
        "one",
        "import QtQuick\nimport ri.modules.rusty.bar\nItem { Bar {} }\n",
        1,
    );
}
