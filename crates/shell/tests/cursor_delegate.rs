//! `cursorDelegate` of TextInput, TextEdit, TextField and TextArea makes the cursor item Qt 6.12
//! makes: created in the editor when `cursorVisible` first turns true, kept (and left shown)
//! when it turns false, moved to `cursorRectangle` and given its height, destroyed when the
//! delegate is replaced or cleared. Each case is a scene in `scenes/cursor_delegate/NAME.qml`
//! whose `step(k)` logs the state and then does step `k`; what real Qt logged is in
//! `scenes/cursor_delegate/NAME.qt` (`scenes/cursor_delegate/dump.py`, run in the testenv
//! image with the Basic style).
//!
//! The scenes are compiled into the test (`testscenes`): a build with the `interp` feature
//! interprets their scripts, a build without it runs the compiled ones.

use qml::value::Value;
use qsg::raster::Canvas;
use shell::keys::Keyboard;
use shell::scene::{self, Target};
use std::cell::RefCell;
use std::rc::Rc;

/// Scene `name` loaded as the shell loads a document: the text measure and the sizes of new
/// items are installed first, so `cursorRectangle` is computed when read, in
/// `Component.onCompleted` too.
fn load(name: &str) -> (qml::runtime::Runtime, usize, Rc<RefCell<scene::Fonts>>) {
    let fonts = Rc::new(RefCell::new(scene::Fonts::default()));
    let (measure, sizes) = (fonts.clone(), fonts.clone());
    let (rt, root) = testscenes::load_with(&format!("cursor_delegate/{name}.qml"), |rt| {
        rt.world.borrow_mut().pre_complete = Some(Box::new(move |w, n0| {
            sizes
                .try_borrow_mut()
                .is_ok_and(|mut f| scene::measure_new(w, &mut f, n0))
        }));
        rt.world.borrow_mut().text_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = measure.try_borrow_mut() {
                scene::measure_text(w, &mut f, i);
            }
        }));
    });
    (rt, root, fonts)
}

/// The scene's log after its steps, each in a focused window with a frame after it.
fn ours(name: &str) -> String {
    let (mut rt, root, fonts) = load(name);
    rt.settle();
    let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    kb.fonts = fonts.clone();
    let mut now = 0.0;
    let mut frame = |rt: &mut qml::runtime::Runtime| {
        kb.sync(rt);
        scene::layout_window(rt, &mut fonts.borrow_mut(), root, size);
        now += 16.0;
        rt.tick(now);
        scene::layout_window(rt, &mut fonts.borrow_mut(), root, size);
    };
    frame(&mut rt);
    let steps = rt.get(root, "steps").to_number() as usize;
    for k in 0..steps {
        rt.call_method(root, "step", vec![Value::Num(k as f64)])
            .unwrap()
            .unwrap();
        rt.settle();
        frame(&mut rt);
    }
    let errors = rt.errors();
    assert!(errors.is_empty(), "{name}: {errors:?}");
    rt.get(root, "out").to_js_string()
}

fn check(name: &str, qt: &str) {
    let ours = ours(name);
    let (ours, qt): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), qt.lines().collect());
    let mut wrong = vec![];
    for k in 0..ours.len().max(qt.len()) {
        let (o, q) = (ours.get(k).unwrap_or(&"-"), qt.get(k).unwrap_or(&"-"));
        if o != q {
            wrong.push(format!("  ours {o}\n    Qt {q}"));
        }
    }
    assert!(wrong.is_empty(), "{name}:\n{}", wrong.join("\n"));
}

macro_rules! case {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[test]
        fn $name() {
            check(
                stringify!($name),
                include_str!(concat!("scenes/cursor_delegate/", stringify!($name), ".qt")),
            );
        }
    };
}

case!(
    /// TextInput.
    input
);
case!(
    /// TextEdit.
    edit
);
case!(
    /// TextField (Basic style), with padding.
    field
);
case!(
    /// TextArea (Basic style), with padding.
    area
);
case!(
    /// A delegate with its own `x`, `y` and `height`: numbers give way to the cursor rectangle,
    /// bindings stand until the cursor next moves.
    sized
);
case!(
    /// The cursor item read by the script that moved the cursor or changed the text: Qt has
    /// moved it by then, no frame in between.
    moved
);
case!(
    /// `text` and `cursorPosition` written in the document, as literals and as bindings, in
    /// both orders: Qt writes the literals last to first and the bindings after them, a
    /// TextEdit takes its text when the component is complete.
    literal
);
case!(
    /// `cursorRectangle` and the cursor item read in `Component.onCompleted`, before any
    /// frame.
    early
);
case!(
    /// A delegate that is a `component` of the document, one that extends another, and a
    /// `Component` holding one.
    local
);

/// Home and Right from the keyboard move the cursor item with no layout in between: a frame
/// in which nothing was resized paints it where the cursor is. The positions are the ones Qt
/// logs in `moved.qt` for the same field with the cursor at 0 and at 3.
#[test]
fn keys_move_the_cursor_item_without_a_layout() {
    const HOME: u32 = 0xff50;
    const RIGHT: u32 = 0xff53;
    let qt = include_str!("scenes/cursor_delegate/moved.qt");
    let qt_x = |pos: usize| -> f64 {
        let tag = format!("field pos={pos} cur ");
        let at = qt.find(&tag).expect("moved.qt has the position") + tag.len();
        qt[at..].split(' ').next().unwrap().parse().unwrap()
    };
    let (mut rt, root, fonts) = load("moved");
    rt.settle();
    let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    // As in the shell: the keyboard edits with the font cache the measure uses.
    kb.fonts = fonts.clone();
    for now in [16.0, 32.0] {
        kb.sync(&mut rt);
        scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, size);
        rt.tick(now);
    }
    let Value::Item(field) = rt.get(root, "field") else {
        panic!("the scene names its field");
    };
    // What a frame paints: the stored value, not a script's read.
    let stored_x = |rt: &qml::runtime::Runtime| -> f64 {
        let w = rt.world.borrow();
        let (_, item) = w.cursor_items[&field];
        w.items[item]
            .props
            .get("x")
            .map_or(f64::NAN, |p| p.value.to_number())
    };
    let near = |a: f64, b: f64| (a - b).abs() < 0.005;
    assert!(
        near(stored_x(&rt), qt_x(11)),
        "at the end: {}",
        stored_x(&rt)
    );
    let mut press = |rt: &mut qml::runtime::Runtime, sym: u32| {
        kb.key(rt, sym, None, true, false);
        kb.key(rt, sym, None, false, false);
        rt.settle();
    };
    press(&mut rt, HOME);
    assert!(
        near(stored_x(&rt), qt_x(0)),
        "after Home: {}, Qt {}",
        stored_x(&rt),
        qt_x(0)
    );
    for _ in 0..3 {
        press(&mut rt, RIGHT);
    }
    assert!(
        near(stored_x(&rt), qt_x(3)),
        "after Right x3: {}, Qt {}",
        stored_x(&rt),
        qt_x(3)
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

/// The To Do field of `qml/` with focus (`todo.qml`) is, pixel for pixel with no allowance,
/// the picture Qt 6.12 draws of it (`todo.png`, by `dump.py`): the text, the rounded border
/// and the cursor item made from `cursorDelegate`.
/// Qt's window is 260 x 70, the size of the scene: in one stretched to the output Qt itself
/// draws two corner pixels of the border one of 255 off.
#[test]
fn todo_field_is_the_picture_qt_draws() {
    let (mut rt, root, fonts) = load("todo");
    rt.settle();
    let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    kb.fonts = fonts.clone();
    for frame in 1..=8 {
        kb.sync(&mut rt);
        scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, size);
        rt.tick(16.0 * frame as f64);
        rt.settle();
    }
    scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, size);
    let errors = rt.errors();
    assert!(errors.is_empty(), "{errors:?}");
    let (w, h) = (size[0] as usize, size[1] as usize);
    let mut ours = Canvas::new(w, h, [0, 0, 0, 0]);
    scene::paint_window(
        &mut rt,
        &mut fonts.borrow_mut(),
        root,
        &mut Target::Cpu(&mut ours),
    );
    let qt = qsg::image::load_png(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/scenes/cursor_delegate/todo.png"
    ))
    .unwrap();
    assert_eq!((qt.w, qt.h), (w, h));
    // (x, y, ours, Qt)
    let differ: Vec<(usize, usize, [u8; 4], [u8; 4])> = (0..w * h)
        .filter(|&k| ours.px[k] != qt.px[k])
        .map(|k| (k % w, k / w, ours.px[k], qt.px[k]))
        .collect();
    assert_eq!(differ, []);
    // The cursor is in the picture: yellow among the white text, as tall as the line.
    let rows = (20..37)
        .filter(|y| (30..230).any(|x| qt.px[y * w + x][0] > qt.px[y * w + x][2] + 100))
        .count();
    assert!(rows >= 15, "the cursor in Qt's picture: {rows} rows");
}
