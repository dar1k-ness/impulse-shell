//! The caret of an input method's preedit string, as Qt 6.12 shows it for the cursor bytes of
//! zwp_text_input_v3.preedit_string (QWaylandTextInputv3::zwp_text_input_v3_done, read from
//! qtbase v6.12.0): hidden for -1/-1, at the end of the preedit for 0/0, else at `begin`.

use qml::runtime::Runtime;
use qsg::raster::Canvas;
use shell::keys::Keyboard;
use shell::scene::{self, Target};

const QML: &str = r#"
import QtQuick
Item {
    width: 200; height: 40
    Rectangle { anchors.fill: parent; color: "black" }
    TextInput {
        x: 10; y: 10; width: 180; height: 20
        color: "white"; font.pixelSize: 16
        cursorPosition: 1; text: "xy"
    }
}
"#;

/// The columns in which `a` and `b` differ.
fn differing_columns(a: &Canvas, b: &Canvas) -> Vec<usize> {
    (0..a.w)
        .filter(|x| (0..a.h).any(|y| a.px[y * a.w + x] != b.px[y * b.w + x]))
        .collect()
}

#[test]
fn preedit_caret_is_where_qt_puts_it() {
    let mut rt = Runtime::new(None);
    let win = rt.load_str(QML).unwrap();
    rt.settle();
    let mut kb = Keyboard::default();
    kb.win = Some(win);
    scene::layout_window(&mut rt, &mut kb.fonts.borrow_mut(), win, [200.0, 40.0]);
    let ti = rt.world.borrow_mut().of_type("TextInput")[0];
    rt.world.borrow_mut().force_active_focus(ti);
    kb.sync(&mut rt);
    rt.settle();

    // "x" + the preedit "abc" + "y", painted with the preedit cursor at bytes `begin`, `end`.
    let mut paint = |begin: i32, end: i32| {
        kb.ime_preedit(&mut rt, Some(("abc", begin, end)));
        rt.settle();
        scene::layout_window(&mut rt, &mut kb.fonts.borrow_mut(), win, [200.0, 40.0]);
        // The caret's blink starts over: it is in its shown half.
        rt.world.borrow_mut().caret_reset = None;
        let mut c = Canvas::new(200, 40, [0, 0, 0, 0]);
        scene::paint_window(
            &mut rt,
            &mut kb.fonts.borrow_mut(),
            win,
            &mut Target::Cpu(&mut c),
        );
        c
    };

    // Both -1: the input method hides the cursor. Every other paint differs from this one in
    // the caret alone, a line one pixel wide.
    let hidden = paint(-1, -1);
    let mut caret = |begin: i32, end: i32| {
        let cols = differing_columns(&paint(begin, end), &hidden);
        assert_eq!(cols.len(), 1, "carets at {cols:?} for {begin}, {end}");
        cols[0]
    };
    let (after_a, after_c) = (caret(1, 1), caret(3, 3));
    assert!(after_a < after_c, "{after_a} {after_c}");
    // Both 0: Qt adds no Cursor attribute and the editor puts the caret after the preedit.
    assert_eq!(caret(0, 0), after_c);
    // One of them -1: shown, at the start of the preedit.
    assert!(caret(-1, 0) < after_a);
}
