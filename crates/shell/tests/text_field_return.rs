//! Return in a single-line TextField emits accepted and then climbs to the parents' Keys
//! handlers (QQuickTextInput ignores it), as the wallpaper selector's filter field relies on:
//! typing a filter, moving the selection and pressing Enter applies the selected item.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::keys::Keyboard;

const RETURN: u32 = 0xff0d;
const KP_ENTER: u32 = 0xff8d;

const QML: &str = r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    property string log: ""
    property var field: f
    Keys.onPressed: event => {
        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            root.log += "activate:" + f.text + ";";
            event.accepted = true;
        }
    }
    TextField {
        id: f
        focus: true
        onAccepted: root.log += "accepted;"
        Keys.onPressed: event => { event.accepted = false; }
    }
}
"#;

fn log(rt: &mut Runtime, root: usize) -> String {
    let mut w = rt.world.borrow_mut();
    let s = w
        .read(root, "log")
        .map(|v| v.to_js_string())
        .unwrap_or_default();
    w.write(root, "log", Value::str(""));
    s
}

#[test]
fn return_in_text_field_reaches_parent_keys_handler() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let Some(Value::Item(f)) = rt.world.borrow_mut().read(root, "field") else {
        panic!("no field");
    };
    rt.world.borrow_mut().force_active_focus(f);
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    kb.sync(&mut rt);
    rt.settle();
    for c in "wall_3".chars() {
        let t = c.to_string();
        kb.key(&mut rt, c as u32, Some(&t), true, false);
        kb.key(&mut rt, c as u32, None, false, false);
    }
    rt.settle();
    log(&mut rt, root);
    for sym in [RETURN, KP_ENTER] {
        kb.key(&mut rt, sym, None, true, false);
        kb.key(&mut rt, sym, None, false, false);
        rt.settle();
        assert_eq!(log(&mut rt, root), "accepted;activate:wall_3;");
    }
}
