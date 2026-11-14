//! A control, a label and a text field are as wide as their text in the font they take from
//! the control or window they are in (QQuickControlPrivate::parentFont), a plain Text takes
//! none. `font_inherit/scene.qml` compares each width with the width the same text has in a
//! control that sets the font itself; Qt 6.12 (qml6, offscreen, the tools/testenv image)
//! leaves `true` for each of them in `result`.

use qml::runtime::Runtime;
use shell::scene;

#[test]
fn a_control_is_as_wide_as_its_text_in_the_font_of_the_control_it_is_in() {
    let file =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/font_inherit/scene.qml");
    let mut rt = Runtime::new(None);
    let root = rt.load_file(&file).expect("scene loads");
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut now = 0.0;
    rt.tick(now);
    for _ in 0..20 {
        scene::layout_window(&mut rt, &mut fonts, root, [400.0, 300.0]);
        now += 16.0;
        rt.poll_native();
        rt.tick(now);
    }
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert_eq!(
        rt.get(root, "result").to_js_string(),
        "true true true true true true true",
        "in the pane: button, label, text field, button in a frame in an item; in the window: \
         button; in the pane: text, button moved into it"
    );
}
