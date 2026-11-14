//! A press on a TextField takes active focus and is accepted by it
//! (QQuickTextInput::mousePressEvent), so a MouseArea filling the Button below never sees it;
//! a MouseArea above the field (a `contentItem` with an id inside is made first) takes it.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, keys::Keyboard, scene};

const BTN_LEFT: u32 = 0x110;

const QML: &str = include_str!("text_press/press.qml");

fn num(rt: &mut Runtime, i: usize, p: &str) -> f64 {
    rt.world
        .borrow_mut()
        .read(i, p)
        .map_or(-1.0, |v| v.to_number())
}

/// The `// qt:` line of the document: what Qt 6.12 answers to the same two clicks.
fn qt_line() -> &'static str {
    (QML.lines())
        .find_map(|l| l.strip_prefix("// qt: "))
        .expect("press.qml has a `// qt:` line")
}

#[test]
fn text_field_press_is_delivered_as_qt() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [300.0, 200.0]);
    let Some(Value::Item(plain)) = rt.world.borrow_mut().read(root, "plain") else {
        panic!("no field");
    };
    let mut ptr = Pointer::default();
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    for y in [30.0, 130.0] {
        ptr.enter(&mut rt, root, 50.0, y);
        ptr.motion(&mut rt, root, 50.0, y);
        ptr.press(&mut rt, BTN_LEFT);
        ptr.release(&mut rt, BTN_LEFT);
        rt.settle();
        kb.sync(&mut rt);
    }
    let _ = rt.call_method(root, "done", Vec::new()).unwrap();
    rt.settle();
    let result = rt.get(root, "result").to_js_string();
    assert_eq!(result, qt_line());
    // The field that took the press is the one typing goes to.
    assert_eq!(num(&mut rt, root, "presses2"), 0.0);
    assert_eq!(kb.active_editor(&rt), Some(plain));
    kb.key(&mut rt, 0x61, Some("a"), true, false);
    kb.key(&mut rt, 0x61, None, false, false);
    rt.settle();
    let text = rt
        .world
        .borrow_mut()
        .read(plain, "text")
        .map(|v| v.to_js_string());
    assert_eq!(text.as_deref(), Some("a"));
}
