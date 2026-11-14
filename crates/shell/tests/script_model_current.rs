//! A ListView's current row while the `values` of its ScriptModel change, as Quickshell 0.3.2
//! on Qt 6.12 has it. Each case is a scene in `scenes/script_model_current/NAME.qml` that
//! leaves its findings in the window's `result`; its first line, `// qs: ...`, is what
//! Quickshell left there (`scenes/script_model_current/qs.sh`, run in the testenv image).
//!
//! The scene runs as the shell runs its windows: a view applies the changes of its model at
//! its window's polish, once per frame, and the window is laid out after each turn.

use shell::scene;

fn ours(name: &str) -> String {
    let (mut rt, win) = testscenes::load(&format!("script_model_current/{name}.qml"));
    let mut fonts = scene::Fonts::default();
    let mut drv = qml::headless::Driver::new(true).after_turn(move |rt| {
        let size = [rt.get(win, "width"), rt.get(win, "height")].map(|v| v.to_number());
        scene::layout_window(rt, &mut fonts, win, size);
    });
    drv.run(&mut rt, 1000.0);
    let result = rt.get(win, "result").to_js_string();
    let errors = rt.errors();
    assert!(errors.is_empty(), "{name}: {result}\n{errors:?}");
    result
}

fn check(name: &str, src: &str) {
    let qs = src
        .lines()
        .find_map(|l| l.strip_prefix("// qs: "))
        .expect("the scene's `// qs:` line");
    let ours = ours(name);
    assert!(
        ours == qs,
        "{name}:\n    Quickshell {qs}\n          ours {ours}"
    );
}

/// Every row replaced, then a row chosen in the same handler: the row chosen stays the current
/// one (the launcher's search chooses its first result so after each letter).
#[test]
fn swapped() {
    check(
        "swapped",
        include_str!("scenes/script_model_current/swapped.qml"),
    );
}

/// Every row replaced by more rows than the view shows: the current row, left outside the rows
/// in view, keeps its delegate and its `modelData` (ours took the delegate's `modelData` away,
/// and the launcher's emoji after its clipboard entries said `Unable to assign [undefined] to
/// LauncherSearchResult*`), and takes the new object when the model keeps the row by its key.
#[test]
fn held() {
    check("held", include_str!("scenes/script_model_current/held.qml"));
}

/// The rows replaced while the view's window is hidden: a row chosen in the same handler lays
/// the view out there and then, and stays the current one once the window shows (ours kept
/// the changes for the window, and the launcher's emoji, opened by their shortcut after the
/// clipboard, came up with their 15th row chosen, or stood still making all 1928 rows when it
/// was the last).
#[test]
fn hidden() {
    check(
        "hidden",
        include_str!("scenes/script_model_current/hidden.qml"),
    );
}
