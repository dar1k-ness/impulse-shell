//! A type reached through an import qualifier (`import QtQuick as QQ`, `QQ.Timer {}`) is the
//! type itself: it runs, has its methods and defaults, and takes part in everything its plain
//! name does. Each fixture of `qualified_import/` is run in Qt 6.12 and here; what the two
//! print through `console.*` must be the same.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

use std::path::PathBuf;

/// The `console.*` lines of fixture `name` are the ones Qt prints.
#[track_caller]
fn same_console(name: &str) {
    if !qt_log::qt_available() {
        return;
    }
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/qualified_import")
        .join(format!("{name}.qml"));
    let qt = qt_log::qt_log(&file);
    assert!(!qt.is_empty(), "{name}: Qt printed nothing");
    assert_eq!(qt_log::our_log(&file), qt, "{name}");
}

/// `QQ.Timer` fires once or repeatedly, and `start()`, `stop()` and `restart()` drive it.
#[test]
fn timer_runs() {
    same_console("Timer");
}

/// `QQ.NumberAnimation` runs, pauses and resumes; `QQ.Behavior`, `QQ.SequentialAnimation`,
/// `QQ.ParallelAnimation`, `QQ.ScriptAction`, `QQ.PropertyAction`, `QQ.PauseAnimation` and
/// `QQ.ColorAnimation` do their work.
#[test]
fn animations_run() {
    same_console("Animation");
}

/// `QQ.State` with `QQ.PropertyChanges` and `QQ.StateChangeScript`, animated by a
/// `QQ.Transition`.
#[test]
fn states_and_transitions() {
    same_console("States");
}

/// Connections, Binding, Component, Loader, Repeater, ListModel, Instantiator, positioners,
/// gradients and views written with a qualifier; `instanceof`, enums and typed properties.
#[test]
fn objects_behind_a_qualifier() {
    same_console("Objects");
}

/// Layouts with their attached `L.Layout`, Controls and Window behind qualifiers of their own.
#[test]
fn other_modules_behind_a_qualifier() {
    same_console("Modules");
}

/// `Qt.createQmlObject` with a qualified import, and what a wrong qualified name reports.
#[test]
fn created_from_a_string() {
    same_console("Dynamic");
}

#[test]
fn attached_objects() {
    same_console("Attached");
}

#[test]
fn enums() {
    same_console("Enums");
}

#[test]
fn file_types_next_to_module_types() {
    same_console("Files");
}

#[test]
fn handlers_and_helpers() {
    same_console("Handlers");
}
