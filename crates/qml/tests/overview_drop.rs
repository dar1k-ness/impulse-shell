//! Overview windows follow their Hyprland data after a drop however late it reports the
//! move: the drag broke `x: initX`, and the old fixed `arbitraryRaceConditionDelay` re-read
//! (20 ms) left a window at its stale position whenever the data arrived later.
mod event_loop;

use event_loop::Loop;
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

/// The `onInitXChanged` / `onInitYChanged` handlers of the tree's OverviewWidget.qml, verbatim.
fn follow_handlers() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml/modules/rusty/overview/OverviewWidget.qml");
    let src = std::fs::read_to_string(p).unwrap();
    assert!(!src.contains("arbitraryRaceConditionDelay"));
    let h: Vec<&str> = src
        .lines()
        .filter(|l| {
            l.trim_start().starts_with("onInitXChanged:")
                || l.trim_start().starts_with("onInitYChanged:")
        })
        .collect();
    assert_eq!(h.len(), 2, "follow handlers");
    h.join("\n")
}

#[test]
fn dropped_window_follows_late_data() {
    let cfg: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("overview-drop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cfg);
    std::fs::create_dir_all(&cfg).unwrap();
    let shell = format!(
        "import QtQuick\n\nItem {{\n    property real initX: 10\n    property real initY: 20\n    x: initX\n    y: initY\n{}\n}}\n",
        follow_handlers()
    );
    std::fs::write(cfg.join("shell.qml"), shell).unwrap();
    let mut rt = Runtime::new(Some(cfg.clone()));
    let win = rt.load_file(&cfg.join("shell.qml")).unwrap();
    rt.settle();
    let num = |rt: &mut Runtime, k: &str| {
        rt.world
            .borrow_mut()
            .read(win, k)
            .map(|v| v.to_number())
            .unwrap_or(f64::NAN)
    };
    // The drag moves the window by assignment, breaking `x: initX`.
    rt.set(win, "x", Value::Num(300.0));
    rt.set(win, "y", Value::Num(400.0));
    // 200 ms of the loop after the drop, every timer run at its deadline (the virtual clock
    // jumps there): ten times the old re-read delay.
    let mut lp = Loop::new(&mut rt);
    lp.advance(&mut rt, 200.0);
    // Hyprland reports the move to another workspace well after the drop.
    rt.set(win, "initX", Value::Num(50.0));
    rt.set(win, "initY", Value::Num(60.0));
    assert_eq!((num(&mut rt, "x"), num(&mut rt, "y")), (50.0, 60.0));
    let _ = std::fs::remove_dir_all(&cfg);
}
