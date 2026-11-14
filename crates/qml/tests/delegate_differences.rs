//! What a delegate's model does differently from Qt 6.12 (docs/qt-differences.md, "Models of
//! delegates"). Each test holds what the engine does now and names Qt's answer; the cases
//! that are as in Qt are `delegate_ctx_*.qml` and `list_model_*.qml` of `crates/rigen/check`.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

/// `log` of the root of `main` once loaded and settled.
fn log(main: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("main.qml");
    std::fs::write(&f, main).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&f).unwrap();
    rt.settle();
    rt.get(root, "log").to_js_string()
}

/// Qt: `2 2 y`. ObjectModel here is Quickshell's type of the same name, a list of objects
/// the shell fills: it has no default property, so the document with the items of the one of
/// QtQml.Models as its children does not load.
#[test]
fn an_object_model_of_qtqml_models_takes_no_items() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("main.qml");
    let main = r#"import QtQuick
import QtQml.Models
Item {
    id: root
    property string log: ""
    ObjectModel { id: om; Item { objectName: "x" } Item { objectName: "y" } }
    Repeater { id: rep; model: om }
    Component.onCompleted: {
        var get = "none"
        try { get = om.get(1).objectName } catch (e) { get = "throws" }
        log = rep.count + " " + om.count + " " + get
    }
}"#;
    std::fs::write(&f, main).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let err = rt.load_file(&f).err().unwrap();
    let at = err.rfind("main.qml").unwrap();
    assert_eq!(
        &err[at..],
        "main.qml:6:27: Cannot assign to non-existent default property"
    );
}

/// Qt: `2`, a view does not see a push to the array it was given. Here forceLayout reads the
/// array again once any property was written since the view last looked.
#[test]
fn a_list_view_takes_a_push_to_its_array_at_force_layout() {
    let got = log(r#"import QtQuick
Item {
    id: root
    property string log: ""
    property var rows: ["a", "b"]
    ListView { id: view; width: 100; height: 100; model: root.rows; delegate: Item { width: 10; height: 10 } }
    Component.onCompleted: {
        rows.push("c")
        log = "pushed"
        view.forceLayout()
        log = "" + view.count
    }
}"#);
    assert_eq!(got, "3");
}

/// Qt: `false a,n 2`: `modelData` of a list of JS objects is a QVariantMap, a new object at
/// every read whose write goes back to the delegate and tells `modelData`'s readers. Here
/// it is one copy, written in place and telling nobody. The keys are in order in both.
#[test]
fn the_model_data_of_a_js_object_is_one_copy() {
    let got = log(r#"import QtQuick
Item {
    id: root
    property string log: ""
    property int runs: 0
    Repeater {
        id: rep
        model: [{ n: 1, a: 2 }]
        Item {
            property var seen: { root.runs++; return modelData.n }
            function go() {
                var same = modelData === modelData
                modelData.n = 5
                return same + " " + Object.keys(modelData)
            }
        }
    }
    Component.onCompleted: log = rep.itemAt(0).go() + " " + runs
}"#);
    assert_eq!(got, "true a,n 1");
}

/// Qt: `objectName,index,row,column,model,hasModelChildren,modelData,,name,__0` (and the
/// `Changed` signals between them): the members of the model item's meta-object. Here the
/// model item lists what any object lists.
#[test]
fn the_keys_of_model_are_those_of_any_object() {
    let got = log(r#"import QtQuick
Item {
    id: root
    property string log: ""
    ListModel { id: lm; ListElement { name: "a" } }
    Repeater {
        id: rep
        model: lm
        Item { function keys() { return Object.keys(model).filter(k => !/Changed$/.test(k)).join(",") } }
    }
    Component.onCompleted: log = rep.itemAt(0).keys()
}"#);
    assert_eq!(got, "objectName");
}

/// Qt: `number`, the string "3" is the number the row has and nothing is written. Here the
/// string replaces it.
#[test]
fn a_string_that_reads_as_the_rows_number_replaces_it() {
    let got = log(r#"import QtQuick
Item {
    id: root
    property string log: ""
    Repeater {
        id: rep
        model: 4
        Item { function go() { modelData = "3"; return typeof modelData } }
    }
    Component.onCompleted: log = rep.itemAt(3).go()
}"#);
    assert_eq!(got, "string");
}
