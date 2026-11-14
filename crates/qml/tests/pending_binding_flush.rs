//! A JS read of a binding still pending on a just-created object evaluates it first
//! (QQmlData::flushPendingBinding from QV4::QObjectWrapper::getProperty).
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ri-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

fn pump(rt: &mut Runtime, ms: u64) {
    let mut now = 0f64;
    for _ in 0..ms / 8 {
        rt.poll_native();
        now += 8.0;
        rt.tick(now);
    }
}

/// MediaControls.qml: the Loader's `active: GlobalStates.mediaControlsOpen` turns its default
/// `true` to `false` while the panel is enabled, and `onActiveChanged` reads
/// `root.realPlayers.length` before `realPlayers: MprisController.players` had its turn.
#[test]
fn change_handler_reads_a_pending_binding_through_its_value() {
    let d = temp_dir("pending-binding-flush");
    write(
        &d,
        &[
            (
                "qmldir",
                "singleton GS 1.0 GS.qml\nsingleton Ctl 1.0 Ctl.qml\nsingleton Src 1.0 Src.qml\n",
            ),
            (
                "GS.qml",
                "pragma Singleton\nimport QtQuick\nQtObject { property bool open: false; property string log: \"\" }\n",
            ),
            (
                "Src.qml",
                "pragma Singleton\nimport QtQuick\nQtObject { property var values: [] }\n",
            ),
            (
                "Ctl.qml",
                "pragma Singleton\nimport QtQuick\nQtObject {\n    property list<QtObject> players: Src.values.filter(p => isReal(p))\n    function isReal(p) { return true }\n}\n",
            ),
            (
                "main.qml",
                r#"import QtQuick
Item {
    id: root
    readonly property var real: Ctl.players
    Loader {
        id: ld
        active: GS.open
        onActiveChanged: {
            if (!ld.active && root.real.length === 0)
                GS.open = false
            GS.log += ld.active + ":" + typeof root.real + ";"
        }
        sourceComponent: Item {}
    }
}
"#,
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons_lazy(std::slice::from_ref(&d));
    rt.load_file(&d.join("main.qml")).unwrap();
    pump(&mut rt, 50);
    let gs = *rt.world.borrow().singletons.get("GS").unwrap();
    rt.set(gs, "open", Value::Bool(true));
    pump(&mut rt, 50);
    rt.set(gs, "open", Value::Bool(false));
    pump(&mut rt, 50);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(
        rt.get(gs, "log").to_js_string(),
        "false:object;true:object;false:object;"
    );
}

/// A Loader enables before the root's later bindings; its handler reads `later` as bound (7), not
/// as its default (0) or before the handler ran (""), as Qt 6.12 does.
#[test]
fn scope_read_of_a_pending_binding_gives_its_value() {
    let d = temp_dir("pending-binding-scope");
    write(
        &d,
        &[(
            "main.qml",
            r#"import QtQuick
Item {
    id: root
    property string seen: ""
    property int k: 1
    Loader {
        active: root.k > 5
        onActiveChanged: seen = "" + later
    }
    property int later: 7 * k
}
"#,
        )],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    pump(&mut rt, 50);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(rt.get(root, "seen").to_js_string(), "7");
}

/// SettingsTextRow.qml in a page Loader: the placeholder's `visible: input.text.length === 0`
/// flushes the TextInput's pending `text: root.text`. The setter re-checking mask, length and
/// selection is not a read of the placeholder: no binding loop, and cursor moves leave it alone.
#[test]
fn flushed_text_input_setter_reads_stay_out_of_the_reader() {
    let d = temp_dir("pending-binding-text-input");
    write(
        &d,
        &[
            (
                "TextRow.qml",
                r#"import QtQuick
Item {
    id: root
    property string text: ""
    readonly property var evals: ({ n: 0 })
    property alias input: input
    property alias placeholder: placeholder
    function evaluated() { return evals.n }
    TextInput {
        id: input
        text: root.text
    }
    Text {
        id: placeholder
        visible: { root.evals.n += 1; return input.text.length === 0 }
    }
}
"#,
            ),
            (
                "main.qml",
                r#"import QtQuick
Item {
    id: root
    property bool on: false
    property string text: "abc"
    property alias loader: ld
    Loader {
        id: ld
        active: root.on
        sourceComponent: TextRow { text: root.text }
    }
}
"#,
            ),
        ],
    );
    let mut rt = Runtime::new(Some(d.clone()));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    pump(&mut rt, 50);
    rt.set(root, "on", Value::Bool(true));
    pump(&mut rt, 50);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let item = |rt: &Runtime, i: usize, name: &str| match rt.get(i, name) {
        Value::Item(i) => i,
        v => panic!("{name}: {v:?}"),
    };
    let row = item(&rt, item(&rt, root, "loader"), "item");
    let (input, placeholder) = (item(&rt, row, "input"), item(&rt, row, "placeholder"));
    assert!(!rt.get(placeholder, "visible").truthy());
    let evaluated = |rt: &mut Runtime| {
        rt.call_method(row, "evaluated", vec![])
            .unwrap()
            .unwrap()
            .to_number()
    };
    let evals = evaluated(&mut rt);
    rt.set(input, "cursorPosition", Value::Num(1.0));
    rt.set(input, "selectionStart", Value::Num(0.0));
    pump(&mut rt, 50);
    assert_eq!(
        evaluated(&mut rt),
        evals,
        "cursor moves re-ran the placeholder"
    );
    rt.set(root, "text", Value::str(""));
    pump(&mut rt, 50);
    assert!(rt.get(placeholder, "visible").truthy());
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
