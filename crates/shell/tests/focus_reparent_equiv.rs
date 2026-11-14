//! An item that comes into a focus scope with `focus` already set (a delegate, a created
//! object, a reparented item: its literal `focus: true` is assigned before it has a parent)
//! takes the scope's focus only when the scope has no focus item yet; otherwise it loses its own
//! (QQuickItem::setParentItem). The overlay's MangoHud widget is the case: its field says
//! `focus: true`, the overlay's content item has focus already, so the field shows no caret.
//!
//! Compared with Qt itself (`qml6`, offscreen): every case is loaded into a focused Loader of an
//! active window there and as a window of its own here. A case is a scene
//! `scenes/focus_reparent_equiv/caseNN.qml` (its first line says what it is): `step()` does
//! what brings the item in, `line()` reports `focus` and `activeFocus` of the items. The
//! scenes are compiled into the test (`testscenes`), so a build without the interpreter runs
//! them too.

use shell::keys::Keyboard;
use std::path::Path;

const DIR: &str = "focus_reparent_equiv";

/// The cases: scene names under `scenes`, in file order.
fn cases() -> Vec<String> {
    let dir = Path::new(testscenes::ROOT).join(DIR);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".qml"))
        .map(|n| format!("{DIR}/{n}"))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no scenes in {}", dir.display());
    names
}

/// What the case is: the comment on the scene's first line.
fn title(case: &str) -> String {
    let src = std::fs::read_to_string(Path::new(testscenes::ROOT).join(case)).unwrap();
    src.lines().next().unwrap_or_default().to_string()
}

/// The line of every case under Qt.
fn qt_lines(cases: &[String]) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.qml");
    let urls: Vec<String> = cases
        .iter()
        .map(|c| format!("\"file://{}/{c}\"", testscenes::ROOT))
        .collect();
    let window = format!(
        r#"import QtQuick
Window {{
    id: win
    visible: true; width: 300; height: 200
    property bool ran: false
    property var cases: [{}]
    Loader {{ id: l; focus: true; anchors.fill: parent }}
    onActiveChanged: {{
        if (!active || ran)
            return
        ran = true
        for (var i = 0; i < cases.length; i++) {{
            l.source = cases[i]
            l.item.step()
            console.log("FOCUS " + l.item.line())
        }}
        Qt.quit()
    }}
}}
"#,
        urls.join(", ")
    );
    std::fs::write(&main, window).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&main)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<String> = log
        .lines()
        .filter_map(|l| Some(l.split_once("FOCUS ")?.1.to_string()))
        .collect();
    assert_eq!(lines.len(), cases.len(), "qml6 printed:\n{log}");
    lines
}

/// The same under our engine: the case is the keyboard window. Each case has a thread of its
/// own: the scene's per-thread caches are keyed by item id, which a second Runtime reuses.
fn our_line(case: &str) -> String {
    let case = case.to_string();
    std::thread::spawn(move || our_line_here(&case))
        .join()
        .unwrap()
}

fn our_line_here(case: &str) -> String {
    let (mut rt, root) = testscenes::load(case);
    rt.settle();
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    kb.sync(&mut rt);
    rt.call_method(root, "step", vec![]).unwrap().unwrap();
    rt.settle();
    kb.sync(&mut rt);
    let line = rt
        .call_method(root, "line", vec![])
        .unwrap()
        .unwrap()
        .to_js_string();
    let errors = rt.errors();
    assert!(errors.is_empty(), "{case}: {errors:?}");
    line
}

#[test]
fn item_entering_a_scope_with_focus_matches_qt() {
    if !testenv::require("qml6") {
        return;
    }
    let cases = cases();
    let qt = qt_lines(&cases);
    // Qt 6.12: the overlay's content item keeps focus, the MangoHud field has none.
    assert_eq!(qt[0], "content=fa field=--");
    let wrong: Vec<String> = cases
        .iter()
        .enumerate()
        .map(|(i, case)| (case, &qt[i], our_line(case)))
        .filter(|(_, qt, ours)| *qt != ours)
        .map(|(case, qt, ours)| format!("{case} {}\n    Qt:   {qt}\n    ours: {ours}", title(case)))
        .collect();
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}
