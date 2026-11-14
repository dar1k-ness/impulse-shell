//! `name: undefined` where an object is declared, by the kind of the property: one with a
//! RESET function is reset (QQmlBinding::doStore), one with none keeps what it had and logs
//! `Unable to assign [undefined] to <type>`, a `var` takes it; an alias, a field of a group
//! and an attached property go by the property they stand for. The binding of the type that
//! the declaration took the place of is never run, so a property that keeps what it had is at
//! its default.
//!
//! `scenes/undefined_declared/qt.txt` has, on each line, an expression of the `cases` of
//! `scenes/undefined_declared/Declared.qml`, a tab and what Qt 6.12 gives for it (PySide6
//! 6.12.0, QQmlComponent, offscreen, `QT_QUICK_CONTROLS_STYLE=Basic`) as `typeof:value`, `!`
//! and the error when it throws.

use shell::scene;

#[test]
fn undefined_in_a_declaration_by_kind() {
    let qt = include_str!("scenes/undefined_declared/qt.txt");
    let cases: Vec<(&str, &str)> = (qt.lines()).map(|l| l.split_once('\t').unwrap()).collect();
    let (mut rt, root) = testscenes::load("undefined_declared/Declared.qml");
    rt.settle();
    let mut fonts = scene::Fonts::default();
    // A Text's implicit size and a control's reach the properties in the layout pass.
    for _ in 0..4 {
        scene::layout_window(&mut rt, &mut fonts, root, [400.0, 400.0]);
        rt.settle();
    }
    let result = rt.get(root, "result").to_js_string();
    let ours: Vec<&str> = result.split(" | ").collect();
    assert_eq!(ours.len(), cases.len());
    let wrong: Vec<String> = (cases.iter().zip(ours))
        .filter(|((_, qt), ours)| qt != ours)
        .map(|((c, qt), ours)| format!("{c}\n  qt:   {qt}\n  ours: {ours}"))
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} differ\n{}",
        wrong.len(),
        cases.len(),
        wrong.join("\n")
    );
    println!(
        "undefined_declared: {} of {} as in Qt",
        cases.len(),
        cases.len()
    );
}
