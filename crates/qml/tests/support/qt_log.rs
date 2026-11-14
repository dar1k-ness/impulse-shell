//! Runs one QML file in Qt 6.12 (the PySide6 of ri-testenv) and in this engine and returns
//! what each printed through `console.*`. A fixture ends by calling `Qt.quit()`.
#![allow(dead_code)]

use qml::runtime::Runtime;
use std::{cell::RefCell, path::Path};

/// The Python with PySide6 6.12 in ri-testenv (tools/testenv/Containerfile).
pub const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// Loads argv[1] and prints every `console.*` line (message category `qml`) until the file
/// calls `Qt.quit()`, which QQmlApplicationEngine delivers through the event loop.
const QT_RUN: &str = r#"
import sys
from PySide6.QtCore import QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlApplicationEngine

def on_message(kind, context, text):
    if context.category == "qml":
        sys.stdout.write(text + "\n")
    else:
        sys.stderr.write(text + "\n")

qInstallMessageHandler(on_message)
app = QGuiApplication(sys.argv[:1])
engine = QQmlApplicationEngine()
engine.load(QUrl.fromLocalFile(sys.argv[1]))
if not engine.rootObjects():
    sys.exit(3)
sys.exit(app.exec())
"#;

/// [`QT_RUN`] printing every message, `qmlWarning` lines too, without the
/// `file:///…:line:column: ` Qt puts before a warning about an object.
const QT_RUN_ALL: &str = r#"
import re, sys
from PySide6.QtCore import QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlApplicationEngine

def on_message(kind, context, text):
    sys.stdout.write(re.sub(r"^file://.*?\.qml:\d+(:\d+)?: ", "", text) + "\n")

qInstallMessageHandler(on_message)
app = QGuiApplication(sys.argv[:1])
engine = QQmlApplicationEngine()
for path in sys.argv[2:]:
    engine.addImportPath(path)
engine.load(QUrl.fromLocalFile(sys.argv[1]))
if not engine.rootObjects():
    sys.exit(3)
sys.exit(app.exec())
"#;

/// Whether Qt 6.12 is there to compare with; reports it missing otherwise (crates/testenv).
#[track_caller]
pub fn qt_available() -> bool {
    let found = Path::new(PYTHON).exists();
    if !found {
        testenv::unavailable(format_args!("`{PYTHON}` (PySide6) is not installed"));
    }
    found
}

/// The `console.*` lines of `file` in Qt 6.12.
pub fn qt_log(file: &Path) -> Vec<String> {
    qt_run(QT_RUN, file, None)
}

/// Every line `file` prints in Qt 6.12: `console.*` and warnings.
pub fn qt_log_all(file: &Path) -> Vec<String> {
    qt_run(QT_RUN_ALL, file, None)
}

/// [`qt_log_all`] with the modules under `imports` (`qs/modules/common/qmldir` is
/// `ri.modules.common`).
pub fn qt_log_all_importing(file: &Path, imports: &Path) -> Vec<String> {
    qt_run(QT_RUN_ALL, file, Some(imports))
}

fn qt_run(script: &str, file: &Path, imports: Option<&Path>) -> Vec<String> {
    let out = std::process::Command::new(PYTHON)
        .args(["-c", script])
        .arg(file)
        .args(imports)
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "Qt failed on {}: {}",
        file.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

thread_local!(static CONSOLE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) });

/// The `console.*` lines of `file` in this engine: loaded, then frames of a virtual clock
/// until the file calls `Qt.quit()`. The lines come from the console sink: a handler run by
/// a synchronous update prints through another interpreter than `Runtime::it`.
pub fn our_log(file: &Path) -> Vec<String> {
    let (log, errors) = our_log_and_errors(file);
    assert!(errors.is_empty(), "{errors:?}");
    log
}

/// [`our_log`] for a file that makes the engine report something: the lines, then what
/// `Runtime::errors` holds at the end.
pub fn our_log_and_errors(file: &Path) -> (Vec<String>, Vec<String>) {
    our_log_and_errors_in(file, file.parent())
}

/// [`our_log_and_errors`] of an engine whose shell directory is `shell`. With one the
/// documents under it have `qs:` urls, as in a shell; with none they have `file:` urls, as
/// in the Qt [`qt_log`] runs.
pub fn our_log_and_errors_in(file: &Path, shell: Option<&Path>) -> (Vec<String>, Vec<String>) {
    qml::builtins::CONSOLE_SINK.get_or_init(|| {
        |level, line| {
            // A qmlWarning line is no `console.*` line: [`qt_log`] does not have it either.
            if level != qml::builtins::QML_WARNING {
                CONSOLE.with_borrow_mut(|c| c.push(line.to_owned()))
            }
        }
    });
    CONSOLE.with_borrow_mut(Vec::clear);
    let mut rt = Runtime::new(shell.map(Path::to_path_buf));
    rt.load_file(file).unwrap();
    rt.settle();
    let mut now = 0.0;
    while rt.world.borrow().quit.is_none() {
        assert!(
            now < 60_000.0,
            "{} never called Qt.quit(): {:?} {:?}",
            file.display(),
            rt.errors(),
            CONSOLE.take()
        );
        now += 16.0;
        rt.tick(now);
        rt.run_later();
        rt.settle();
    }
    (CONSOLE.take(), rt.errors())
}

/// Writes `src` as `name` in `dir` and returns (Qt's lines, ours).
pub fn both(dir: &Path, name: &str, src: &str) -> (Vec<String>, Vec<String>) {
    let file = dir.join(name);
    std::fs::write(&file, src).unwrap();
    (qt_log(&file), our_log(&file))
}

/// Loads argv[1] in a QQuickView, whose window gives the engine an incubation controller as a
/// shell has, and prints the root's `result` once the file has called `Qt.quit()`.
const QT_RESULT: &str = r#"
import os, sys
from PySide6.QtCore import Qt, QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView

def on_message(kind, context, text):
    sys.stderr.write(text + "\n")

qInstallMessageHandler(on_message)
app = QGuiApplication(sys.argv[:1])
view = QQuickView()

def quit():
    sys.stdout.write(str(view.rootObject().property("result")))
    app.quit()

view.engine().quit.connect(quit, Qt.ConnectionType.QueuedConnection)
view.setSource(QUrl.fromLocalFile(os.path.abspath(sys.argv[1])))
if view.errors():
    sys.exit(3)
view.show()
sys.exit(app.exec())
"#;

/// `result` of `file` in Qt 6.12.
pub fn qt_result(file: &Path) -> String {
    let out = std::process::Command::new(PYTHON)
        .args(["-c", QT_RESULT])
        .arg(file)
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "Qt failed on {}: {}",
        file.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// `result` of `file` here: loaded, then frames of a virtual clock until it calls `Qt.quit()`.
pub fn our_result(file: &Path) -> String {
    let mut rt = Runtime::new(file.parent().map(Path::to_path_buf));
    let root = rt.load_file(file).unwrap();
    rt.settle();
    let mut now = 0.0;
    while rt.world.borrow().quit.is_none() {
        assert!(now < 60_000.0, "{} never called Qt.quit()", file.display());
        now += 16.0;
        rt.tick(now);
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    rt.get(root, "result").to_js_string()
}
