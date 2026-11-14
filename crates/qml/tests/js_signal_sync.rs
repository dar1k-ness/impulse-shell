//! A signal called from JS (`root.sig(x)`) runs its handlers and Connections inside the call,
//! as Qt's direct connections do (checked on Qt 6.12: after `root.ping(7)` the caller reads
//! `got == 7`).

use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap()
}

fn str_of(rt: &Runtime, item: usize, name: &str) -> String {
    rt.get(item, name).to_js_string()
}

#[test]
fn handlers_and_connections_run_inside_the_call() {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            r#"import QtQuick
Item {
    id: root
    signal ping(int v)
    property int got: 0
    property int conn: 0
    property string log: ""
    onPing: (v) => { got = v; log += "h" + v + " " }
    Item { id: other; signal pong() }
    Connections { id: c; target: root; function onPing(v) { root.conn = v; root.log += "c" + v + " " } }
    function run() {
        root.ping(7);
        log += "after:" + got + "," + conn + " ";
        // Retargeting right after the emit must not lose the event already delivered.
        c.target = other;
        root.ping(8);
        log += "end:" + got + "," + conn;
    }
}"#,
        )
        .unwrap();
    rt.settle();
    rt.call_method(root, "run", vec![]).unwrap().unwrap();
    assert_eq!(str_of(&rt, root, "log"), "h7 c7 after:7,7 h8 end:8,7");
    rt.settle();
    assert_eq!(str_of(&rt, root, "log"), "h7 c7 after:7,7 h8 end:8,7");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

/// waffle screenSnip: the tree's DragManager.qml `onReleased` emits `dragReleased` and then
/// `resetDrag()`; WRegionSelectionPanel's handler bails on `selectionWidth === 0`, so with a
/// queued signal it never ran the snip.
#[test]
fn drag_manager_release_handler_sees_the_selection() {
    let home = std::env::temp_dir().join(format!("js-signal-sync-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    // The tree's DragManager.qml beside the scene (its `qs.*` imports are unused there).
    let dir = home.join("scene");
    std::fs::create_dir_all(&dir).unwrap();
    let src = std::fs::read_to_string(ri().join("modules/common/widgets/DragManager.qml")).unwrap();
    let src: String = src
        .lines()
        .filter(|l| !l.starts_with("import ri."))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(dir.join("DragManager.qml"), src).unwrap();
    std::fs::write(
        dir.join("Main.qml"),
        r#"import QtQuick
Item {
    id: root
    width: 200; height: 200
    property var area: dragArea
    property var command: []
    DragManager {
        id: dragArea
        anchors.fill: parent
        property int selectionX: regionTopLeftX
        property int selectionY: regionTopLeftY
        property int selectionWidth: regionWidth
        property int selectionHeight: regionHeight
        onDragReleased: (diffX, diffY) => {
            if (selectionWidth === 0 || selectionHeight === 0) {
                return;
            }
            root.command = [selectionX, selectionY, selectionWidth, selectionHeight, diffX, diffY];
        }
    }
}"#,
    )
    .unwrap();
    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join("Main.qml")).unwrap();
    rt.settle();
    let Value::Item(area) = rt.get(root, "area") else {
        panic!("no DragManager")
    };
    let mouse = |x: f64, y: f64, buttons: f64| {
        vec![Runtime::js_object(&[
            ("x", Value::Num(x)),
            ("y", Value::Num(y)),
            ("button", Value::Num(1.0)),
            ("buttons", Value::Num(buttons)),
            ("modifiers", Value::Num(0.0)),
            ("accepted", Value::Bool(true)),
        ])]
    };
    rt.fire(area, "onPressed", &["mouse"], mouse(10.0, 20.0, 1.0));
    rt.fire(
        area,
        "onPositionChanged",
        &["mouse"],
        mouse(40.0, 35.0, 1.0),
    );
    rt.fire(area, "onReleased", &["mouse"], mouse(40.0, 35.0, 0.0));
    rt.settle();
    assert_eq!(str_of(&rt, root, "command"), "10,20,30,15,30,15");
    assert_eq!(str_of(&rt, area, "regionWidth"), "0", "resetDrag ran after");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
