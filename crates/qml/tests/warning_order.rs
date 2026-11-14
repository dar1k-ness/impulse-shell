//! A warning of the engine comes out inside the call that raises it, so it stands among the
//! `console.*` lines where Qt 6.12 prints it. `warning_order/order.qt612.txt` is what
//! `warning_order/qtmsg.py` printed for `warning_order/order.qml` in the PySide6 of ri-testenv,
//! `warning_order/undefined.qt612.txt` the same for `warning_order/undefined.qml`,
//! `warning_order/bindable.qt612.txt` for `warning_order/bindable.qml`.

use qml::runtime::Runtime;
use std::{cell::RefCell, path::Path};

thread_local!(static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) });

fn console(_level: u8, line: &str) {
    SAID.with_borrow_mut(|s| s.push(format!("console: {line}")));
}

fn warning(line: &str) {
    SAID.with_borrow_mut(|s| s.push(format!("warning: {line}")));
}

/// Where the engine's log goes: a binding that gives `undefined` to a property that takes
/// none is said there (`ri::binding`), a line each.
struct Logged;

impl std::io::Write for Logged {
    fn write(&mut self, text: &[u8]) -> std::io::Result<usize> {
        String::from_utf8_lossy(text).lines().for_each(warning);
        Ok(text.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// What `file` said through `console.*` and what the engine warned of and logged, in the
/// order said, until the file calls `Qt.quit()`.
fn said(file: &Path) -> Vec<String> {
    let log = qml::debug::subscriber(qml::debug::filter(None), || Logged);
    qml::debug::tracing::subscriber::with_default(log, || said_unlogged(file))
}

fn said_unlogged(file: &Path) -> Vec<String> {
    let _ = qml::builtins::CONSOLE_SINK.set(console);
    let _ = qml::warning::SINK.set(warning);
    SAID.with_borrow_mut(Vec::clear);
    let mut rt = Runtime::new(file.parent().map(Path::to_path_buf));
    rt.load_file(file).unwrap();
    rt.settle();
    let mut now = 0.0;
    while rt.world.borrow().quit.is_none() {
        assert!(now < 1000.0, "{} never called Qt.quit()", file.display());
        now += 16.0;
        rt.tick(now);
        rt.run_later();
        rt.settle();
    }
    SAID.take()
}

/// The text of a warning is not compared: few of the engine's lines are worded as Qt's yet.
/// Where each one stands is.
fn place_only(line: &str) -> &str {
    if line.starts_with("warning: ") {
        "warning"
    } else {
        line
    }
}

#[test]
fn warnings_stand_where_qt_prints_them() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/warning_order");
    let ours = said(&dir.join("order.qml"));
    let qt = std::fs::read_to_string(dir.join("order.qt612.txt")).unwrap();
    let qt: Vec<&str> = qt.lines().collect();
    assert_eq!(
        ours.iter().map(|l| place_only(l)).collect::<Vec<_>>(),
        qt.iter().map(|l| place_only(l)).collect::<Vec<_>>(),
        "ours: {ours:#?}\nQt: {qt:#?}"
    );
}

/// A warning without the place it names, which Qt gives as `file:line:column` and the engine
/// as the object and its file.
fn unplaced(line: &str) -> &str {
    match line.find("Unable to assign") {
        Some(at) if line.starts_with("warning: ") => &line[at..],
        _ => line,
    }
}

/// `undefined` given by a binding to a property that takes none, while the object is made:
/// Qt holds `Unable to assign [undefined] to <type>` back until the outermost creation is
/// over, and says then only the ones whose binding gave no value since, the latest first.
/// The original's bar has 29 such bindings (`occupied[index]` in `bar/Workspaces.qml`, a child of a
/// Row in `ToolbarTabBar.qml`) that all have a value by then: Quickshell says none of them.
#[test]
fn an_undefined_given_inside_a_creation_is_said_when_it_is_over() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/warning_order");
    let ours = said(&dir.join("undefined.qml"));
    let qt = std::fs::read_to_string(dir.join("undefined.qt612.txt")).unwrap();
    let qt: Vec<&str> = qt.lines().collect();
    assert_eq!(
        ours.iter().map(|l| unplaced(l)).collect::<Vec<_>>(),
        qt.iter().map(|l| unplaced(l)).collect::<Vec<_>>(),
        "ours: {ours:#?}\nQt: {qt:#?}"
    );
}

/// The same given to a bindable property of a Qt type (`x`, `objectName`): its binding is a
/// QQmlPropertyBinding, which names the property and says the line at once, inside a creation
/// too, also when the binding has a value by its end. The declared property beside it waits.
#[test]
fn an_undefined_given_to_a_bindable_property_is_said_at_once() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/warning_order");
    let ours = said(&dir.join("bindable.qml"));
    let qt = std::fs::read_to_string(dir.join("bindable.qt612.txt")).unwrap();
    let qt: Vec<&str> = qt.lines().collect();
    assert_eq!(
        ours.iter().map(|l| unplaced(l)).collect::<Vec<_>>(),
        qt.iter().map(|l| unplaced(l)).collect::<Vec<_>>(),
        "ours: {ours:#?}\nQt: {qt:#?}"
    );
}
