//! Active focus moving between two items of one focused FocusScope: the scope keeps activeFocus
//! and is not notified (QQuickDeliveryAgentPrivate::setFocusInScope only notifies changed items).

use qml::runtime::Runtime;
use qml::value::Value;
use shell::keys::Keyboard;

const QML: &str = r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 200; height: 200
    property string log: ""
    property var first: a
    property var second: b
    FocusScope {
        focus: true
        anchors.fill: parent
        onActiveFocusChanged: root.log += "scope:" + activeFocus + ";"
        Button {
            id: a
            focus: true
            text: "a"
            onActiveFocusChanged: root.log += "a:" + activeFocus + ";"
        }
        Button {
            id: b
            y: 60
            text: "b"
            onActiveFocusChanged: root.log += "b:" + activeFocus + ";"
        }
    }
}
"#;

fn prop(rt: &mut Runtime, i: usize, name: &str) -> String {
    rt.world
        .borrow_mut()
        .read(i, name)
        .map(|v| v.to_js_string())
        .unwrap_or_default()
}

#[test]
fn enclosing_scope_keeps_active_focus_silently() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    kb.sync(&mut rt);
    rt.world.borrow_mut().write(root, "log", Value::str(""));
    let Some(Value::Item(b)) = rt.world.borrow_mut().read(root, "second") else {
        panic!("no b");
    };
    rt.world.borrow_mut().force_active_focus(b);
    kb.sync(&mut rt);
    assert_eq!(prop(&mut rt, root, "log"), "a:false;b:true;");
}
