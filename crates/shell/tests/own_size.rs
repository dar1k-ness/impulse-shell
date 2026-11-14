//! A view's size bound to the size of its own content item, as the notification popup of the
//! waffle panels has it (`height: Math.min(contentItem.height + topMargin + bottomMargin,
//! parent.height)`): on the way the binding reads the property it is about to write, which
//! holds nothing yet. It reads what is held, and is no binding loop.
//!
//! Qt 6.12 (PySide6 6.12.0, QQmlComponent, offscreen) gives `72 40 40 100 90` for `result` of
//! `scenes/own_size/Own.qml` and logs nothing.

use shell::scene;

#[test]
fn a_size_from_the_own_content_item_is_no_binding_loop() {
    let (mut rt, root) = testscenes::load("own_size/Own.qml");
    rt.settle();
    let mut fonts = scene::Fonts::default();
    for _ in 0..4 {
        scene::layout_window(&mut rt, &mut fonts, root, [300.0, 400.0]);
        rt.settle();
    }
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert_eq!(rt.get(root, "result").to_js_string(), "72 40 40 100 90");
}
