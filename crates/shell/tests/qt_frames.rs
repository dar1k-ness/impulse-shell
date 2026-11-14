//! Scenes stepped frame by frame give what Qt 6.12 gives on the same frames. Each case is a
//! scene in `scenes/qt_frames/NAME.qml` with Qt's answer in `scenes/qt_frames/NAME.qt`
//! (`scenes/qt_frames/dump.sh`, run in the testenv image): the scene's `log` after `frames`
//! frames of a 16 ms animation clock, each one a clock step, the scene's `sample(frame)` and
//! then a layout and the hover of a still pointer (Qt: hover, polish, sync and render). A scene
//! with `function shown(frame)` has it called after that: what is on the screen. Qt applies an
//! item's anchors when its geometry is written and we at the layout, so the two differ in
//! `sample` and not in `shown`. Every case runs twice: with a full layout pass on each frame,
//! and with the shell's frame, which places only what was resized when that is local.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::input::Pointer;
use shell::scene;

/// The shell's frame (`App::draw_frame`): a full pass when the layout revision moved or a resize
/// is not local, else up to 4 local passes.
fn shell_frame(
    rt: &mut Runtime,
    fonts: &mut scene::Fonts,
    root: usize,
    size: [f64; 2],
    laid: &mut u64,
) {
    for pass in 0..4 {
        let rs = std::mem::take(&mut rt.world.borrow_mut().resized);
        let mut w = rt.world.borrow_mut();
        if w.layout_rev != *laid || !rs.iter().all(|&i| scene::is_local_resize(&mut w, i)) {
            drop(w);
            scene::layout_window(rt, fonts, root, size);
            *laid = rt.world.borrow().layout_rev;
            rt.world.borrow_mut().resized.clear();
            return;
        }
        if rs.is_empty() {
            return;
        }
        let rev = w.layout_rev;
        scene::relayout_targets(&mut w, fonts, &rs, pass);
        w.layout_rev = rev;
        drop(w);
        rt.settle();
    }
}

fn ours(name: &str, local: bool) -> String {
    let (mut rt, root) = testscenes::load(&format!("qt_frames/{name}.qml"));
    rt.settle();
    rt.run_later();
    let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
    let frames = rt.get(root, "frames").to_number() as usize;
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, size);
    rt.settle();
    let mut laid = rt.world.borrow().layout_rev;
    // `property int hoverInterval`: Qt's interval of frame-synchronous hover in ms, and the
    // scene runs in wall time, `wait(frame)` ms from a frame to the next. Else the interval
    // is 0, every frame delivering the hover, and no time passes.
    let every = match rt.get(root, "hoverInterval") {
        Value::Num(ms) => Some(ms as i64),
        _ => None,
    };
    let mut wall = std::time::Instant::now();
    let ms = |ms: f64| std::time::Duration::from_secs_f64(ms / 1000.0);
    let mut pointer = Pointer::default();
    pointer.hover_every = Some(every.unwrap_or(0));
    // The window's first frame, which starts its interval; dump.cpp gives the scene's first
    // frame after it.
    pointer.frame(root, wall);
    wall += ms(300.0);
    // `property real mouseX` and `mouseY`: the pointer enters the window there and stays.
    if let Value::Num(x) = rt.get(root, "mouseX") {
        let y = rt.get(root, "mouseY").to_number();
        pointer.enter(&mut rt, root, x, y);
        rt.settle();
    }
    for i in 1..=frames {
        rt.tick(i as f64 * 16.0);
        rt.settle();
        rt.call_method(root, "sample", vec![Value::Num(i as f64)])
            .unwrap()
            .unwrap();
        rt.settle();
        if local {
            shell_frame(&mut rt, &mut fonts, root, size, &mut laid);
        } else {
            scene::layout_window(&mut rt, &mut fonts, root, size);
        }
        rt.settle();
        // The shell's frame: `App::draw` then `Pointer::rehover`.
        pointer.frame(root, wall);
        pointer.rehover(&mut rt);
        rt.settle();
        if let Some(r) = rt.call_method(root, "shown", vec![Value::Num(i as f64)]) {
            r.unwrap();
            rt.settle();
        }
        if every.is_some() {
            let wait = rt.call_method(root, "wait", vec![Value::Num(i as f64)]);
            let next = wall + ms(wait.unwrap().unwrap().to_number());
            // The delay timer of the hover fires between the frames, as in the shell's loop.
            while let Some(due) = pointer.hover_due(wall)
                && wall + ms(due) <= next
            {
                wall += ms(due);
                pointer.hover_timer(wall);
                pointer.rehover(&mut rt);
                rt.settle();
            }
            wall = next;
        }
    }
    let errors = rt.errors();
    assert!(errors.is_empty(), "{name}: {errors:?}");
    rt.get(root, "log").to_js_string()
}

fn check(name: &str, qt: &str) {
    for local in [false, true] {
        let ours = ours(name, local);
        let (ours, qt): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), qt.lines().collect());
        let wrong: Vec<String> = (0..ours.len().max(qt.len()))
            .filter(|&i| ours.get(i) != qt.get(i))
            .map(|i| {
                format!(
                    "  ours {}\n    Qt {}",
                    ours.get(i).unwrap_or(&"(no line)"),
                    qt.get(i).unwrap_or(&"(no line)")
                )
            })
            .collect();
        let frame = if local {
            "the shell's frame"
        } else {
            "a full pass"
        };
        assert!(wrong.is_empty(), "{name}, {frame}:\n{}", wrong.join("\n"));
    }
}

macro_rules! case {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[test]
        fn $name() {
            check(
                stringify!($name),
                include_str!(concat!("scenes/qt_frames/", stringify!($name), ".qt")),
            );
        }
    };
}

case!(
    /// A delegate running its `remove` transition is released at the view's next forced
    /// layout: a change of the view's geometry (unless the view is empty), of its spacing, of
    /// a delegate's height, or of the model. Not by a scroll, a moved parent, or a delegate's
    /// width, x or opacity.
    remove_release
);
case!(
    /// A Flickable gives its attached vertical ScrollBar its height and the place at its
    /// right edge, whatever the scene set for those; the bar's width and y stay the scene's.
    /// A ScrollView leaves a bar it is given alone.
    scrollbar_sizes
);
case!(
    /// The same for a horizontal bar.
    #[ignore = "the engine has no attached horizontal ScrollBar yet"]
    scrollbar_across
);
case!(
    /// Frame by frame of a `remove` transition: the leaving delegate, the delegates closing
    /// the gap and the attached ScrollBar are where Qt has them.
    remove_geometry
);
case!(
    /// An item that an animation moves from under a still pointer, or under it, loses or gets
    /// `containsMouse` or `hovered` in the frame Qt does it.
    still_pointer
);
case!(
    /// The same at Qt's real interval of frame-synchronous hover, 100 ms: a frame that comes
    /// sooner after the last delivery leaves the hover to a timer that fires between frames,
    /// a frame that comes later delivers it.
    still_pointer_100ms
);
