//! Findings R1-1 to R1-8 of the second QML engine audit (`runtime.rs`, text editors, Item
//! defaults, tables that only grew). Each fixture in `runtime_r1/` builds a `log`; the `.qt`
//! file beside it is what Qt 6.12 printed for it in the tools/testenv image
//! (`runtime_r1/qt.sh` writes them).

use qml::runtime::{Runtime, Sym};

fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/runtime_r1")
}

/// Runs the fixture and compares its `log` with Qt's line by line.
fn same_as_qt(name: &str) {
    let f = dir().join(format!("{name}.qml"));
    let mut rt = Runtime::new(Some(dir()));
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    assert_eq!(rt.errors(), Vec::<String>::new(), "{name}");
    let ours = rt.get(root, "log").to_js_string();
    let qt = std::fs::read_to_string(dir().join(format!("{name}.qt"))).unwrap();
    let (ours, qt): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), qt.lines().collect());
    let diff: Vec<String> = (0..ours.len().max(qt.len()))
        .filter(|&k| ours.get(k) != qt.get(k))
        .map(|k| {
            let line = |v: &[&str]| v.get(k).map_or("<none>".to_string(), |l| format!("{l:?}"));
            format!(
                "line {}:\n  qt   {}\n  ours {}",
                k + 1,
                line(&qt),
                line(&ours)
            )
        })
        .collect();
    assert!(
        diff.is_empty(),
        "{name}: {} of {} lines differ from Qt\n{}",
        diff.len(),
        qt.len(),
        diff.join("\n")
    );
}

/// R1-1, R1-2, R1-3: `select`, `insert`, `remove`, `getText`, `clear` of TextInput and
/// TextEdit from four start states, with positions inside and outside the text.
#[test]
fn edit_methods_leave_the_state_qt_leaves() {
    same_as_qt("edit_state");
}

/// R1-4, R1-5: the signals each of those calls sends, in order, and the state each handler sees.
#[test]
fn edit_methods_send_the_signals_qt_sends() {
    same_as_qt("edit_signals");
}

/// R1-5: `length` is the text's length in UTF-16 code units, `lengthChanged` comes with
/// `textChanged`, and TextInput sends `textEdited` for `insert` and `remove`.
#[test]
fn length_follows_the_text() {
    same_as_qt("length");
}

/// R1-4, R1-5: a TextEdit whose `text` was read (or assigned the text it has) keeps giving
/// that text to the cursor and selection handlers of an edit, while `length` is already the
/// new one; without such a read the handlers get the new text.
#[test]
fn text_edit_keeps_the_text_it_was_read() {
    same_as_qt("text_cache");
}

/// R1-5: `onLengthChanged` and `onTextChanged` are handlers of one signal: their order across
/// a base type, its use and Connections, also for objects made again from one delegate.
#[test]
fn length_and_text_handlers_run_in_qt_order() {
    same_as_qt("handler_order");
}

/// R1-6: the properties of QQuickItem read from objects that are not items, and `radius`
/// from items that do not declare it.
#[test]
fn item_properties_exist_only_on_items() {
    same_as_qt("item_props");
}

/// R1-7: `item[key]` and `key in item` with a key no object has do not intern the key, and
/// names the object does have still resolve when built at run time.
#[test]
fn unknown_dynamic_keys_are_not_interned() {
    same_as_qt("dynamic_keys");
    for k in [
        "r1_absent_get_0",
        "r1_absent_get_1999",
        "r1_absent_in_0",
        "r1_absent_in_1999",
    ] {
        assert!(Sym::lookup(k).is_none(), "{k} was interned");
    }
}

/// Every way JS reaches a QML object's member by a computed key leaves a key no object has
/// out of the table of names: reading, writing, `in`, `delete`, `hasOwnProperty` and the other
/// Object.prototype and Reflect lookups, calling, and `?.[]`; the same paths still find the
/// names the object has.
#[test]
fn no_computed_key_path_interns_an_unknown_key() {
    same_as_qt("dynamic_key_paths");
    let interned: Vec<String> = [
        "get",
        "set",
        "compound",
        "increment",
        "in",
        "delete",
        "own",
        "own_call",
        "enumerable",
        "descriptor",
        "reflect_has",
        "reflect_get",
        "call",
        "optional",
        "optional_call",
        "group_get",
        "group_in",
    ]
    .iter()
    .flat_map(|path| {
        // A group object reads its owner's `<group>.<key>`.
        [
            format!("ev_absent_{path}"),
            format!("anchors.ev_absent_{path}"),
        ]
    })
    .filter(|k| Sym::lookup(k).is_some())
    .collect();
    assert_eq!(interned, Vec::<String>::new(), "interned");
}

/// R1-8: a template released with its Loader's item leaves `plan_seen` too.
#[test]
fn plan_seen_drops_released_templates() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("main.qml");
    std::fs::write(
        &f,
        r#"import QtQuick
Item {
    property alias active: ld.active
    Loader {
        id: ld
        sourceComponent: Component {
            Item { Repeater { model: 3; delegate: Item { Repeater { model: 2; Rectangle {} } } } }
        }
    }
}"#,
    )
    .unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    let mut sizes = Vec::new();
    for round in 0..40 {
        rt.set(root, "active", qml::value::Value::Bool(round % 2 == 1));
        rt.settle();
        rt.reap();
        rt.world.borrow_mut().purge_dead();
        if round % 2 == 0 {
            sizes.push(rt.world.borrow().plan_seen.len());
        }
    }
    // With the item unloaded only templates still alive are left, the same set every time.
    assert!(sizes.iter().all(|&n| n == sizes[0]), "{sizes:?}");
}
