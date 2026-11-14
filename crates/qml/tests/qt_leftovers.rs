//! What the comparison of slice R3 (`runtime_audit_r3.rs`) with Qt 6.12 left different. Each
//! fixture of `qt_leftovers/` is run in Qt 6.12 and here; what the two print must be the same.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

use qml::runtime::Runtime;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/qt_leftovers")
}

/// The `console.*` lines of fixture `name` are the ones Qt prints.
#[track_caller]
fn same_console(name: &str) {
    if !qt_log::qt_available() {
        return;
    }
    let file = dir().join(format!("{name}.qml"));
    let qt = qt_log::qt_log(&file);
    assert!(!qt.is_empty(), "{name}: Qt printed nothing");
    assert_eq!(qt_log::our_log(&file), qt, "{name}");
}

/// Loads argv[1] and prints its load errors, or runs it until `Qt.quit()` and prints the
/// messages that are not `console.*` lines (qmlWarning). The url of the file's directory is
/// cut from both.
const QT_MESSAGES: &str = r#"
import os, sys
from PySide6.QtCore import Qt, QUrl, qInstallMessageHandler
from PySide6.QtGui import QGuiApplication
from PySide6.QtQml import QQmlComponent, QQmlEngine

path = os.path.abspath(sys.argv[1])
here = QUrl.fromLocalFile(os.path.dirname(path)).toString() + "/"

def on_message(kind, context, text):
    if context.category != "qml":
        sys.stdout.write(text.replace(here, "") + "\n")

qInstallMessageHandler(on_message)
app = QGuiApplication(sys.argv[:1])
engine = QQmlEngine()
engine.quit.connect(app.quit, Qt.ConnectionType.QueuedConnection)
component = QQmlComponent(engine, QUrl.fromLocalFile(path))
root = None if component.isError() else component.create()
if root is None:
    for e in component.errors():
        sys.stdout.write(e.toString().replace(here, "") + "\n")
    sys.exit(0)
sys.exit(app.exec())
"#;

/// What [`QT_MESSAGES`] prints for `file`.
fn qt_messages(file: &Path) -> Vec<String> {
    let out = std::process::Command::new(qt_log::PYTHON)
        .args(["-c", QT_MESSAGES])
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
    let text = String::from_utf8(out.stdout).unwrap();
    text.lines().map(str::to_owned).collect()
}

/// An empty Container, TabBar and SwipeView have `currentIndex` -1; the first item added
/// becomes current, and removing items moves the index as QQuickContainer does.
#[test]
fn container_without_items_has_no_current_index() {
    same_console("ContainerEmpty");
}

/// `Container.currentItem` is the item at `currentIndex`, null when there is none.
#[test]
fn container_current_item() {
    same_console("ContainerCurrentItem");
}

/// The items declared in a container make the first one current before any handler runs.
#[test]
fn container_declared_items_make_the_first_current_silently() {
    same_console("ContainerDeclared");
}

/// A `currentIndex` binding of a container with declared items keeps its value.
#[test]
fn container_current_index_binding_stands() {
    same_console("ContainerBound");
}

/// A container hears of each item a Repeater makes and releases: `count` changes as the item
/// arrives after the others, the item then moves to its place and the current item stays
/// current; the items a Repeater released leave first first, after all of them were released.
#[test]
fn repeater_in_a_container() {
    same_console("RepeaterInContainer");
}

/// A container hears of the items a Repeater unparents when its model is replaced or its
/// delegate taken away: each released item is counted out as the Repeater takes it from its
/// parent, and an object `createObject` makes is counted in before its initial properties.
#[test]
fn repeater_leaves_a_container() {
    same_console("RepeaterLeavesContainer");
}

/// A Repeater given a delegate again, or another one: QQmlDelegateModel::setDelegate inserts
/// the rows, which the Repeater makes, and QQuickRepeater::regenerate then releases them and
/// makes them again. Here they are made once (docs/qt-differences.md).
#[test]
#[ignore = "deferred: a Repeater whose delegate is assigned at run time makes its rows once, \
            Qt twice; nothing in qml/ assigns one (docs/qt-differences.md)"]
fn repeater_given_a_delegate_at_run_time() {
    same_console("RepeaterDelegateBack");
}

/// The container hears of a delegate before the delegate's bindings are enabled, and the
/// delegate has its row by then: a read runs the pending binding of a property, with `index`,
/// `modelData` and the roles; `objectName` and `x` (BINDABLE) have their defaults.
#[test]
fn a_delegate_told_to_a_container_reads_its_row() {
    same_console("DelegateInContainer");
}

/// `CheckBox.checkState` and `checked` follow each other; a click goes through the three
/// states of a `tristate` box or through `nextCheckState`; `toggle()` flips `checked`.
#[test]
fn checkbox_check_state() {
    same_console("CheckBox");
}

/// Of a literal `checked` and a literal `checkState` that contradict each other the one
/// written first in the file stands (QmlIR stores an object's literals last written first,
/// and each setter sets the other property); a binding is evaluated after both.
#[test]
fn checkbox_literals_that_contradict() {
    same_console("checkbox/Literals");
}

/// The handler of a signal whose name starts with underscores has the first letter after
/// them in upper case: `on_ZChanged` of the property `_z`, `on__S` of the signal `__s`.
#[test]
fn handlers_of_names_that_start_with_an_underscore() {
    same_console("HandlerUnderscore");
}

/// `createObject(parent, properties)` sets the properties in the order of their names, so
/// `checkState` before `checked` however the object lists them.
#[test]
fn create_object_sets_initial_properties_by_name() {
    same_console("checkbox/Initial");
}

/// `on_ZChanged` is the handler of `_zChanged`, of the object and in a Connections: the
/// first letter of the signal is the one in upper case, whatever stands before it.
#[test]
fn a_handler_name_upper_cases_the_first_letter_of_the_signal() {
    same_console("handler/Underscore");
}

/// A C++ method called with fewer arguments than any of its overloads takes throws
/// "Insufficient arguments" or lists the candidates.
#[test]
fn cpp_method_with_too_few_arguments_throws() {
    if !qt_log::qt_available() {
        return;
    }
    // What ListModel refuses is a `qmlWarning` here and in Qt (`audit_r4.rs` holds the
    // texts), not a `console.*` line.
    let file = dir().join("TooFewArguments.qml");
    let mut ours = qt_log::our_log(&file);
    ours.retain(|l| !l.starts_with("QML ListModel: "));
    assert_eq!(ours, qt_log::qt_log(&file));
}

/// `console.log` prints an array in brackets, nested arrays too; `JSON.stringify` of a
/// circular structure throws "Cannot convert circular structure to JSON".
#[test]
fn console_prints_arrays_in_brackets() {
    same_console("ConsoleArrays");
}

/// The same for typed `list<T>` properties and the arrays built-in functions return.
#[test]
fn console_prints_lists_in_brackets() {
    same_console("ConsoleLists");
}

/// `layer.effect` is a Component: the effect item exists while the layer is enabled and is
/// made again when the component changes.
#[test]
fn layer_effect_is_a_component() {
    same_console("LayerEffect");
}

/// QQuickItemLayer: an item whose layer is enabled has a ShaderEffectSource after it among its
/// siblings and the item made from `layer.effect` after that. Both follow the item to another
/// parent, the one that is shown has its geometry, `z`, opacity, transform and visibility, and
/// a container counts the effect item it was made under among its content items.
#[test]
fn layer_parts_are_siblings_of_the_item() {
    same_console("LayerPlace");
}

/// Every copy a Repeater makes of an item with `layer.enabled: true` and a `layer.effect` has
/// its own effect item, the copies made from the creation plan of the delegate too (the
/// third and later). The shell's RippleButton masks its background that way.
#[test]
fn each_repeated_item_has_its_layer_effect() {
    same_console("LayerRepeated");
}

/// The methods of StackView say why they did nothing, as `qmlWarning(view)` lines:
/// `url:line:column: QML StackView: text`.
#[test]
fn stack_view_warns() {
    if !qt_log::qt_available() {
        return;
    }
    let file = dir().join("StackViewWarnings.qml");
    let here = format!("file://{}/", dir().display());
    // The text an item prints as is left to `toString()` of objects.
    let text = |line: &str| {
        let line = line.replace(&here, "");
        let item = "can't find item to pop: ";
        match line.find(item) {
            Some(i) => line[..i + item.len()].to_owned(),
            None => line,
        }
    };
    let qt: Vec<String> = qt_messages(&file).iter().map(|l| text(l)).collect();
    assert_eq!(qt.len(), 15, "{qt:?}");
    assert!(
        qt.iter().all(|l| l.starts_with("StackViewWarnings.qml:")),
        "{qt:?}"
    );

    let (log, errors) = qt_log::our_log_and_errors(&file);
    let ours: Vec<String> = errors.iter().map(|l| text(l)).collect();
    assert_eq!(ours, qt);
    // What the calls return.
    assert_eq!(log, qt_log::qt_log(&file));
}

/// The place `qmlWarning(view)` names is where the outermost document declares the view, and
/// the type is the name it has there: of the type used, of the inline component, or of the
/// document itself for the root of one loaded on its own whose name starts in upper case.
#[test]
fn qml_warning_names_the_place_and_the_type() {
    if !qt_log::qt_available() {
        return;
    }
    let file = dir().join("stackwarn/Places.qml");
    let here = format!("file://{}/", dir().join("stackwarn").display());
    let qt: Vec<String> = qt_messages(&file)
        .iter()
        .map(|l| l.replace(&here, ""))
        .collect();
    assert_eq!(qt.len(), 20, "{qt:?}");
    assert!(qt.iter().all(|l| l.contains(": QML ")), "{qt:?}");
    let (_, errors) = qt_log::our_log_and_errors(&file);
    let ours: Vec<String> = errors.iter().map(|l| l.replace(&here, "")).collect();
    assert_eq!(ours, qt);
}

/// The error of a type that does not exist names the line and the column of the type name.
#[test]
fn unknown_type_error_has_the_column() {
    if !qt_log::qt_available() {
        return;
    }
    for n in 1..=10 {
        let file = dir().join(format!("unknown/U{n}.qml"));
        let qt = qt_messages(&file);
        assert_eq!(qt.len(), 1, "{qt:?}");
        let mut rt = Runtime::new(Some(dir().join("unknown")));
        let ours = rt.load_file(&file).unwrap_err();
        let here = format!("{}/", dir().join("unknown").display());
        assert_eq!(ours.replace(&here, ""), qt[0], "U{n}");
    }
}

/// A module imported only behind a qualifier gives no bare names: with `import QtQuick as Q`
/// Qt knows `Q.Item` and no `Item`. Here the bare name is still the type
/// (docs/qt-differences.md, "A bare name of a module imported only behind a qualifier").
#[test]
#[ignore = "a bare name of a module imported only behind a qualifier is still a type here: \
            docs/qt-differences.md"]
fn bare_name_of_a_module_behind_a_qualifier_is_no_type() {
    if !qt_log::qt_available() {
        return;
    }
    let file = dir().join("unknown/Bare.qml");
    let qt = qt_messages(&file);
    assert_eq!(qt, ["Bare.qml:3:2: Item is not a type"]);
    let mut rt = Runtime::new(Some(dir().join("unknown")));
    let ours = rt.load_file(&file).err().unwrap_or_default();
    let here = format!("{}/", dir().join("unknown").display());
    assert_eq!(ours.replace(&here, ""), qt[0]);
}

/// A name that is no type fails the load of the document wherever it stands, also in a
/// Component nothing creates: an inline component, a delegate of an empty model, the
/// `sourceComponent` of an inactive Loader, a Behavior or State of such a component. A type
/// whose own document has one is "unavailable" where it is named.
#[test]
fn unknown_type_in_a_component_nobody_creates_fails_the_load() {
    if !qt_log::qt_available() {
        return;
    }
    let dir = dir().join("uncreated");
    let here = format!("{}/", dir.display());
    for n in 1..=9 {
        let file = dir.join(format!("C{n}.qml"));
        let qt = qt_messages(&file);
        assert!(!qt.is_empty(), "C{n}: Qt loaded it");
        let mut rt = Runtime::new(Some(dir.clone()));
        let ours = rt.load_file(&file).unwrap_err().replace(&here, "");
        assert_eq!(ours.lines().collect::<Vec<_>>(), qt, "C{n}");
    }
}

/// The same for documents loaded at run time: `Qt.createComponent` gives a component with
/// status Error and the errors in `errorString()`, a Loader becomes `Loader.Error` when it is
/// activated and warns, `Qt.createQmlObject` throws.
#[test]
fn unknown_type_in_a_component_fails_documents_loaded_at_run_time() {
    if !qt_log::qt_available() {
        return;
    }
    let file = dir().join("uncreated/Dynamic.qml");
    let qt = qt_log::qt_log(&file);
    assert_eq!(qt.len(), 19, "{qt:?}");
    let (log, errors) = qt_log::our_log_and_errors(&file);
    assert_eq!(log, qt);
    // What `createObject` of a component that is not ready and the activated Loader warn.
    let here = format!("file://{}/", dir().join("uncreated").display());
    let warned = qt_messages(&file);
    assert_eq!(warned.len(), 6, "{warned:?}");
    let ours: Vec<String> = errors.iter().map(|e| e.replace(&here, "")).collect();
    assert_eq!(ours, warned);
}
