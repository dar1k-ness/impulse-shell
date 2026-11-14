//! An object's bindings to its alias properties are set after its other bindings, once its
//! children exist (QmlUnitGenerator writes `isValueBindingToAlias` bindings last), so a
//! literal written through an alias reaches a target declared further down. Values are what
//! Qt 6.12 gives for the same documents.
use qml::runtime::Runtime;

fn log_of(src: &str) -> String {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(src).unwrap();
    rt.settle();
    rt.get(root, "log").to_js_string()
}

/// The original's services/Idle.qml: `property alias inhibit: idleInhibitor.enabled; inhibit: false`
/// with the IdleInhibitor declared below.
#[test]
fn literal_through_alias_reaches_a_target_declared_below() {
    let log = log_of(
        r##"import QtQuick
Item {
    id: root
    property int src: 4
    early: 3
    property alias a: inner.enabled
    a: false
    property alias n: inner.width
    n: 5
    property alias c: rect.color
    c: "#ff0000"
    property alias scr: inner.height
    scr: root.src * 2
    property alias early: rect.width
    property alias own: rect.height
    own: 9
    property string log: a + " " + n + " " + c + " " + scr + " " + early + " " + own
    Item { id: inner; width: 100 }
    Rectangle { id: rect; color: "blue"; height: 50 }
}"##,
    );
    assert_eq!(log, "false 5 #ff0000 8 3 9");
}

/// The same in a delegate: the second instance is copied from the first one's creation plan.
#[test]
fn literal_through_alias_in_every_delegate_instance() {
    let log = log_of(
        r#"import QtQuick
Item {
    id: root
    property string log: rep.count < 3 ? "" : rep.itemAt(0).v + " " + rep.itemAt(1).v + " " + rep.itemAt(2).on
    Repeater {
        id: rep
        model: 3
        Item {
            property alias v: inner.width
            v: 7
            property alias on: inner.enabled
            on: false
            Item { id: inner; width: 100 }
        }
    }
}"#,
    );
    assert_eq!(log, "7 7 false");
}

/// The alias binding comes last whatever its place among the bindings: the target's own value
/// and the type's own alias binding are both overwritten.
#[test]
fn alias_binding_of_an_instance_overrides_the_types() {
    let d = std::env::temp_dir().join(format!("ri-alias-binding-order-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("Foo.qml"),
        "import QtQuick\nItem {\n    property alias iw: fin.width\n    iw: 3\n    \
         Item { id: fin; width: 100 }\n}\n",
    )
    .unwrap();
    std::fs::write(
        d.join("main.qml"),
        "import QtQuick\nItem {\n    property string log: f.iw + \" \" + g.iw\n    \
         Foo { id: f }\n    Foo { id: g; iw: 6; width: 5 }\n}\n",
    )
    .unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let log = rt.get(root, "log").to_js_string();
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(log, "3 6");
}
