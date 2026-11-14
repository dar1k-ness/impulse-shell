//! `Component.createObject` and `Component.incubateObject` of a component that is not Ready
//! (status Error or Loading), the arguments they take, and the incubator of a Ready one. Each
//! fixture of `component_not_ready/` is run in Qt 6.12 and here; the `console.*` lines and
//! the warnings must be the same.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

use std::path::{Path, PathBuf};

/// Loads argv[1] in a QQuickView, whose window gives the engine an incubation controller as
/// a shell has, and prints until the file calls `Qt.quit()`: `console: ` and a `console.*`
/// line, or `message: ` and another message (qWarning, qmlWarning) without the url of the
/// file's directory.
const QT_VIEW: &str = r#"
import os, sys
from PySide6.QtCore import Qt, QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView

path = os.path.abspath(sys.argv[1])
here = QUrl.fromLocalFile(os.path.dirname(path)).toString() + "/"

def on_message(kind, context, text):
    tag = "console: " if context.category == "qml" else "message: "
    sys.stdout.write(tag + text.replace(here, "") + "\n")

qInstallMessageHandler(on_message)
app = QGuiApplication(sys.argv[:1])
view = QQuickView()
view.engine().quit.connect(app.quit, Qt.ConnectionType.QueuedConnection)
view.setSource(QUrl.fromLocalFile(path))
if view.errors():
    sys.exit(3)
view.show()
sys.exit(app.exec())
"#;

/// (console lines, messages) of a run.
type Printed = (Vec<String>, Vec<String>);

/// Seconds a run of Qt has. 8 of them end before nextest ends the test.
const QT_LIMIT: &str = "30";

/// What Qt 6.12 prints for `file`, or None for a run that is not over after [`QT_LIMIT`].
fn qt_run(name: &str, file: &Path) -> Option<Printed> {
    let out = std::process::Command::new("timeout")
        .args([QT_LIMIT, qt_log::PYTHON, "-c", QT_VIEW])
        .arg(file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    if out.status.code() == Some(124) {
        return None;
    }
    assert!(
        out.status.success(),
        "Qt failed on {name}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let tagged = |tag: &str| -> Vec<String> {
        text.lines()
            .filter_map(|l| l.strip_prefix(tag))
            .map(str::to_owned)
            .collect()
    };
    let qt = (tagged("console: "), tagged("message: "));
    assert!(!qt.0.is_empty(), "{name}: Qt printed nothing");
    Some(qt)
}

/// The `// qt: ` lines of a fixture: console lines that Qt 6.12 prints one after another in
/// the case the fixture is about.
fn pinned(file: &Path) -> Vec<String> {
    let src = std::fs::read_to_string(file).unwrap();
    src.lines()
        .filter_map(|l| l.trim_start().strip_prefix("// qt: "))
        .map(str::to_owned)
        .collect()
}

/// A message of Qt's own that a run of Qt on a loaded machine may hold beside the ones the
/// fixture is about (docs/qt-differences.md, "An asynchronous component on a loaded machine").
const QT_LATE: &str =
    "QBasicTimer::start: negative intervals aren't allowed; the interval will be set to 1ms.";

/// What Qt 6.12 prints for fixture `name`.
///
/// The document of a `Component.Asynchronous` component comes from Qt's loader thread: on a
/// loaded machine the component may be Ready at the first read, or its events may come after
/// ones posted later (docs/qt-differences.md, "An asynchronous component on a loaded
/// machine"). A fixture with such a component pins its case, and Qt's answer is the first of
/// 8 runs in which that case came out. A run that is not over in time is one of the 8, and
/// so is one whose messages hold [`QT_LATE`].
fn qt_answer(name: &str, file: &Path) -> Printed {
    let pin = pinned(file);
    let runs = if pin.is_empty() { 1 } else { 8 };
    let mut last = String::new();
    for run in 1..=runs {
        last = match qt_run(name, file) {
            None => format!("not over after {QT_LIMIT} s"),
            Some(qt) if pin.is_empty() => return qt,
            Some(qt) if qt.1.iter().any(|m| m == QT_LATE) => {
                format!("dropped for Qt's own message {QT_LATE:?}")
            }
            Some(qt) if qt.0.windows(pin.len()).any(|w| w == pin) => {
                eprintln!("{name}: Qt 6.12 answered on run {run} of {runs}");
                return qt;
            }
            Some(qt) => format!("{:?}", qt.0),
        };
        eprintln!("{name}: run {run} of Qt 6.12 is not the pinned case: {last}");
    }
    panic!("{name}: Qt 6.12 never said {pin:?} in {runs} runs; its last run: {last}");
}

/// What fixture `name` prints in Qt 6.12 and here. Qt loads the fixture by its `file:` url,
/// so the engine here has no shell directory: under one the documents have `qs:` urls, and
/// what is said about those is Quickshell's to answer, not plain Qt's.
fn both(name: &str) -> (Printed, Printed) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/component_not_ready");
    let file = dir.join(format!("{name}.qml"));
    let qt = qt_answer(name, &file);
    let here = format!("file://{}/", dir.display());
    let (log, errors) = qt_log::our_log_and_errors_in(&file, None);
    let errors = errors.iter().map(|e| e.replace(&here, "")).collect();
    (qt, (log, errors))
}

#[track_caller]
fn same(name: &str, messages: usize) {
    if !qt_log::qt_available() {
        return;
    }
    let (qt, ours) = both(name);
    assert_eq!(qt.1.len(), messages, "{name}: {:?}", qt.1);
    assert_eq!(ours.0, qt.0, "{name}: console");
    assert_eq!(ours.1, qt.1, "{name}: messages");
}

/// A component whose status is Error (no such file, a syntax error, an unknown type) or
/// Loading gives null and "QQmlComponent: Component is not ready" for every `createObject`
/// and `incubateObject`, and makes nothing; once the loading one is Ready it makes objects.
#[test]
fn component_that_is_not_ready_warns_and_gives_null() {
    same("NotReady", 60);
}

/// What `createObject` and `incubateObject` say about arguments that are no parent and no
/// properties, on a declared Component, on one of `Qt.createComponent`, and on ones with
/// status Error and Loading.
#[test]
fn unsuitable_arguments_are_named_before_the_component_is_asked() {
    same("Args", 90);
}

/// Every pair of arguments of `createObject`: which the method taking a QVariantMap accepts,
/// which give null.
#[test]
fn create_object_arguments_fit_a_parent_and_a_map() {
    same("Matrix", 129);
}

/// The incubator of a Ready component: Loading until the loop turns (the newest first),
/// Ready at once for `Qt.Synchronous` and after `forceCompletion()`, `onStatusChanged` called
/// with the status; and a `Component.Asynchronous` component is Loading until the loop
/// turns unless the engine already has its document.
#[test]
fn incubator_makes_its_object_on_a_later_turn() {
    same("Incubate", 1);
}

/// The first argument of `createObject` and `incubateObject` is the QObject parent of what is
/// made: the object is destroyed with it. Only an Item made for an Item is its child: one
/// made for a QtObject or a Timer has no `parent` ("Created graphical object was not placed
/// in the graphics scene") until `parent` is written or the properties name one, and a
/// QtObject made for an Item is not in its `data`.
#[test]
fn an_item_created_for_an_object_that_is_no_item_has_no_parent() {
    same("CreateParent", 5);
}

/// That message names the type as `qmlWarning` does (QQmlMetaType::prettyTypeName): without
/// the qualifier of its import, and by its document when it is the root of one.
#[test]
fn an_item_without_a_parent_is_named_as_qml_warning_names_it() {
    same("CreateNamed", 2);
}
