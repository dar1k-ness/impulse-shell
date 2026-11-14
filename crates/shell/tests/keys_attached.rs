//! Keys attached handlers (QQuickKeysAttached::keyPressed/keyReleased), TextInput edit signal
//! order and Menu focus hand-over.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::keys::Keyboard;

const ESCAPE: u32 = 0xff1b;
const RETURN: u32 = 0xff0d;

const QML: &str = r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    property string log: ""
    property var keys: k
    property var field: f
    property var menu: m
    Item {
        id: k
        focus: true
        onActiveFocusChanged: root.log += "af" + activeFocus + ";"
        Keys.onEscapePressed: root.log += "esc;"
        Keys.onReturnPressed: event => { root.log += "ret;"; event.accepted = false }
        Keys.onPressed: event => root.log += "p" + event.key + ";"
        Keys.onReleased: event => root.log += "r" + event.key + ";"
    }
    TextField {
        id: f
        onTextEdited: root.log += "edited:" + text + ";"
        onTextChanged: root.log += "text:" + text + ";"
    }
    Menu {
        id: m
        onOpened: root.log += "opened;"
        onClosed: root.log += "closed;"
        MenuItem { text: "A" }
    }
}
"#;

fn item(rt: &mut Runtime, root: usize, n: &str) -> usize {
    match rt.world.borrow_mut().read(root, n) {
        Some(Value::Item(i)) => i,
        v => panic!("{n}: {v:?}"),
    }
}

fn take_log(rt: &mut Runtime, root: usize) -> String {
    let mut w = rt.world.borrow_mut();
    let s = w
        .read(root, "log")
        .map(|v| v.to_js_string())
        .unwrap_or_default();
    w.write(root, "log", Value::str(""));
    s
}

fn setup() -> (Runtime, Keyboard, usize) {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    kb.sync(&mut rt);
    rt.settle();
    take_log(&mut rt, root);
    (rt, kb, root)
}

fn tap(rt: &mut Runtime, kb: &mut Keyboard, sym: u32) {
    kb.key(rt, sym, None, true, false);
    kb.key(rt, sym, None, false, false);
    rt.settle();
}

#[test]
fn special_key_handler_accepts_and_skips_on_pressed() {
    let (mut rt, mut kb, root) = setup();
    tap(&mut rt, &mut kb, ESCAPE);
    assert_eq!(take_log(&mut rt, root), "esc;r16777216;");
    // A special handler that rejects the event lets Keys.onPressed see it.
    tap(&mut rt, &mut kb, RETURN);
    assert_eq!(take_log(&mut rt, root), "ret;p16777220;r16777220;");
}

#[test]
fn text_edited_runs_before_text_changed() {
    let (mut rt, mut kb, root) = setup();
    let field = item(&mut rt, root, "field");
    rt.world.borrow_mut().force_active_focus(field);
    kb.sync(&mut rt);
    take_log(&mut rt, root);
    kb.key(&mut rt, 0x61, Some("a"), true, false);
    kb.key(&mut rt, 0x61, None, false, false);
    rt.settle();
    assert_eq!(take_log(&mut rt, root), "edited:a;text:a;");
}

#[test]
fn menu_takes_active_focus_and_gives_it_back() {
    let (mut rt, mut kb, root) = setup();
    let menu = item(&mut rt, root, "menu");
    rt.world.borrow_mut().popup_set_visible(menu, true);
    rt.settle();
    kb.sync(&mut rt);
    assert_eq!(take_log(&mut rt, root), "affalse;opened;");
    rt.world.borrow_mut().popup_set_visible(menu, false);
    rt.settle();
    kb.sync(&mut rt);
    assert_eq!(take_log(&mut rt, root), "aftrue;closed;");
}
