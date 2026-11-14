//! The R4 slice of the second audit of the QML engine (`runtime.rs`, delegators to
//! singletons) and the two differences found beside it. Each fixture in `audit_r4/` is run
//! in Qt 6.12 and here; everything printed up to the fixture's `END` line must be the same:
//! `console.*` lines and warnings, a warning without the `file:///…:line:column: ` Qt puts
//! before it.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

use qml::{runtime::Runtime, value::Value};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

/// (finding, fixture under `audit_r4/`). A directory is an application: its `main.qml`, with
/// the singletons its `qmldir` names created on first use.
const FIXTURES: &[(&str, &str)] = &[
    ("R4-2", "synchronizer_objects.qml"),
    ("R4-2", "synchronizer_on_property.qml"),
    ("R4-2", "synchronizer_last_write.qml"),
    ("R4-2", "synchronizer_4.qml"),
    ("R4-2", "synchronizer_5.qml"),
    ("R4-2", "synchronizer_6.qml"),
    ("R4-2", "synchronizer_9.qml"),
    ("R4-2", "synchronizer_s1.qml"),
    ("R4-2", "synchronizer_retarget.qml"),
    ("R4-4", "markdown_1.qml"),
    ("R4-4", "markdown_2.qml"),
    ("R4-4", "markdown_3.qml"),
    ("R4-5", "returned_function.qml"),
    ("R4-5", "assigned_function.qml"),
    ("R4-6", "handler_order.qml"),
    ("R4-6", "handler_order_scope.qml"),
    ("R4-7", "undefined_reset.qml"),
    ("R4-8", "underscore_names.qml"),
    ("R4-8", "underscore_handler.qml"),
    ("R4-9", "object_and_bool_models.qml"),
    ("R4-10", "delegate_choice_index.qml"),
    ("R4-11", "loader_urls"),
    ("R4-12", "list_model.qml"),
    ("R4-12", "list_model_roles.qml"),
    ("R4-12", "list_model_role_order.qml"),
    ("R4-13", "instantiator.qml"),
    ("R4-13", "repeater.qml"),
    ("R4-13", "instantiator_active.qml"),
    ("R4-14", "lazy_singletons"),
    ("X1", "container_content.qml"),
];

/// A warning only Qt prints: this engine does not load the pictures a Markdown text names.
const QT_ONLY: &str = "Cannot open: ";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/audit_r4")
        .join(name)
}

thread_local!(static CONSOLE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) });

/// What `file` prints in this engine up to its `Qt.quit()`, through the console sink.
fn our_log(file: &Path, lazy_singletons: bool) -> Vec<String> {
    qml::builtins::CONSOLE_SINK
        .get_or_init(|| |_, line| CONSOLE.with_borrow_mut(|c| c.push(line.to_owned())));
    CONSOLE.with_borrow_mut(Vec::clear);
    let dir = file.parent().unwrap().to_path_buf();
    let mut rt = Runtime::new(Some(dir.clone()));
    if lazy_singletons {
        rt.load_singletons_lazy(&[dir]);
    }
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
    let mut log = CONSOLE.take();
    log.extend(rt.errors().iter().map(|e| format!("error: {e}")));
    log
}

/// The lines before `END`.
fn until_end(mut log: Vec<String>, what: &str) -> Vec<String> {
    let end = log
        .iter()
        .position(|l| l == "END")
        .unwrap_or_else(|| panic!("{what} never printed END: {log:#?}"));
    log.truncate(end);
    log
}

#[test]
fn fixtures_print_the_same_as_qt() {
    if !qt_log::qt_available() {
        return;
    }
    let (mut lines, mut differing) = (0, Vec::new());
    for (finding, name) in FIXTURES {
        let path = fixture(name);
        let app = path.is_dir();
        let file = if app { path.join("main.qml") } else { path };
        let mut qt = until_end(qt_log::qt_log_all(&file), "Qt");
        qt.retain(|l| !l.contains(QT_ONLY));
        let ours = until_end(our_log(&file, name == &"lazy_singletons"), "this engine");
        assert!(!qt.is_empty(), "{name}: Qt printed nothing");
        lines += qt.len();
        if qt != ours {
            differing.push(format!(
                "{finding} {name}\n  Qt:   {qt:#?}\n  ours: {ours:#?}"
            ));
        }
    }
    println!(
        "compared with Qt 6.12: {} fixtures, {lines} lines, {} fixtures differ",
        FIXTURES.len(),
        differing.len()
    );
    assert!(differing.is_empty(), "{}", differing.join("\n"));
}

/// The mode tabs of the region selector (`qml/modules/rusty/regionSelector/OptionsToolbar.qml`):
/// The original's handler writes the mode while `currentIndex`, bound to the mode, is being evaluated.
/// Qt 6.12 reports that binding loop and so does this engine. With the handler of this tree
/// (no write while the mode is `undefined`) there is no loop, and the mode and the index
/// after loading, after writes of the mode and after presses on a tab are the same as with
/// the original's handler, in Qt and here.
#[test]
fn guarded_mode_tabs_differ_from_the_original_only_by_the_binding_loop() {
    if !qt_log::qt_available() {
        return;
    }
    const LOOP: &str = "Binding loop detected for property \"currentIndex\"";
    let original = fixture("mode_tabs_original").join("main.qml");
    let guarded = fixture("mode_tabs_guarded").join("main.qml");

    let qt_original = until_end(qt_log::qt_log_all(&original), "Qt");
    let at = qt_original
        .iter()
        .position(|l| l.contains(LOOP))
        .unwrap_or_else(|| panic!("Qt reports no binding loop: {qt_original:#?}"));
    // The warning and the line that names the binding.
    let mut qt_values = qt_original.clone();
    qt_values.drain(at..at + 2);
    assert_eq!(qt_values.len(), 6, "{qt_original:#?}");

    let qt_guarded = until_end(qt_log::qt_log_all(&guarded), "Qt");
    assert_eq!(qt_guarded, qt_values, "Qt: the guard changed a value");

    let ours = our_log(&original, false);
    assert!(
        ours.iter()
            .any(|l| l.starts_with("error: binding loop detected") && l.contains("currentIndex")),
        "{ours:#?}"
    );
    assert_eq!(until_end(ours, "this engine"), qt_values);

    let ours = our_log(&guarded, false);
    assert!(!ours.iter().any(|l| l.starts_with("error: ")), "{ours:#?}");
    assert_eq!(until_end(ours, "this engine"), qt_values);
    println!(
        "compared with Qt 6.12: 2 fixtures, {} lines, 0 differ",
        qt_original.len() + qt_guarded.len()
    );
}

fn call(rt: &mut Runtime, item: usize, name: &str) -> Value {
    rt.call_method(item, name, Vec::new()).unwrap().unwrap()
}

/// R4-3: a Synchronizer that is destroyed leaves the engine's tables, also one made and
/// destroyed many times (a delegate, a Loader's item).
#[test]
fn destroyed_synchronizers_are_forgotten() {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            r#"import QtQuick
import Qt.labs.synchronizer
Item {
    id: root
    property int a: 1
    Component {
        id: c
        Item { id: inner; property int v: 0; Synchronizer on v { property alias source: root.a } }
    }
    Loader { id: ld; sourceComponent: c; active: false }
    function toggle() { ld.active = !ld.active }
    function held() { return ld.item ? ld.item.v : -1 }
}"#,
        )
        .unwrap();
    rt.settle();
    assert_eq!(rt.world.borrow().synchronizers.len(), 0);
    for round in 0..200 {
        call(&mut rt, root, "toggle");
        rt.settle();
        rt.set(root, "a", Value::Num(round as f64 + 2.0));
        rt.settle();
        assert_eq!(call(&mut rt, root, "held").to_number(), round as f64 + 2.0);
        assert_eq!(rt.world.borrow().synchronizers.len(), 1, "round {round}");
        call(&mut rt, root, "toggle");
        rt.settle();
        assert_eq!(rt.world.borrow().synchronizers.len(), 0, "round {round}");
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

/// X2: a handler run by a synchronous update (a read that needs a pending binding) prints
/// through another interpreter than `Runtime::it`. The console sink, which is what the shell
/// logs, gets its lines too, in the order they were printed.
#[test]
fn a_handler_run_by_a_synchronous_update_reaches_the_console_sink() {
    qml::builtins::CONSOLE_SINK
        .get_or_init(|| |_, line| CONSOLE.with_borrow_mut(|c| c.push(line.to_owned())));
    CONSOLE.with_borrow_mut(Vec::clear);
    let mut rt = Runtime::new(None);
    rt.load_str(
        r#"import QtQuick
Item {
    id: root
    property int a: 1
    property int twice: a * 2
    onTwiceChanged: console.log("twiceChanged", twice)
    Component.onCompleted: {
        console.log("before");
        a = 5;
        console.log("after", twice);
    }
}"#,
    )
    .unwrap();
    rt.settle();
    assert_eq!(
        CONSOLE.take(),
        ["twiceChanged 2", "before", "twiceChanged 10", "after 10"]
    );
}
