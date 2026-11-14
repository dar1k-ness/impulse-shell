//! A ScrollView is laid out as Qt 6.12 lays it out (QQuickScrollView, Basic style): it keeps
//! its children in a Flickable it makes (or in the Flickable that is its first child), gives
//! that Flickable its content size, and has a vertical and a horizontal attached ScrollBar.
//!
//! `scenes/scroll_view/NAME.qt` is what Qt gives for `scenes/scroll_view/NAME.qml` (PySide6
//! 6.12.0, offscreen, `QT_QUICK_CONTROLS_STYLE=Basic`, written by `scenes/scroll_view/qt.py`
//! in the testenv image): the scene's `snap()` as loaded and after each step, with `==`
//! between them. `scenes/scroll_view/Input.qt` is the same for the pointer and the keys: the
//! `snap()` after each line of `Input.steps`, written by `scenes/scroll_view/qt_input.py`.

use qml::value::Value;
use shell::{input::Pointer, keys::Keyboard, scene};
use std::{cell::RefCell, rc::Rc};

fn check(name: &str, size: [f64; 2], steps: &[&str], qt: &str) {
    check_in(name, size, steps, qt, false);
}

/// `as_shell`: with what the shell does as a component completes, before its handlers run and
/// before the window is laid out (`scene::measure_new`); a step that makes items needs it.
fn check_in(name: &str, size: [f64; 2], steps: &[&str], qt: &str, as_shell: bool) {
    let fonts = Rc::new(RefCell::new(scene::Fonts::default()));
    let (mut rt, root) = testscenes::load_with(&format!("scroll_view/{name}.qml"), |rt| {
        if as_shell {
            rt.world.borrow_mut().track_fresh = true;
            let fonts = fonts.clone();
            rt.world.borrow_mut().pre_complete = Some(Box::new(move |w, n0| {
                fonts
                    .try_borrow_mut()
                    .is_ok_and(|mut f| scene::measure_new(w, &mut f, n0))
            }));
        }
    });
    rt.settle();
    let mut wrong = Vec::new();
    let steps = [""].iter().chain(steps);
    for (step, qt) in steps.zip(qt.trim_end().split("\n==\n")) {
        if !step.is_empty() {
            rt.call_method(root, step, vec![]).unwrap().unwrap();
            rt.settle();
        }
        // A size reaches the properties in the layout pass, and what follows it in the next.
        for _ in 0..6 {
            scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, size);
            rt.settle();
        }
        let ours = match rt.call_method(root, "snap", vec![]).unwrap().unwrap() {
            Value::Str(s) => s.to_string(),
            v => panic!("snap() gave {v:?}"),
        };
        let at = if step.is_empty() { "as loaded" } else { step };
        let (q, o): (Vec<_>, Vec<_>) = (qt.lines().collect(), ours.lines().collect());
        for n in 0..q.len().max(o.len()) {
            let (q, o) = (q.get(n).unwrap_or(&"-"), o.get(n).unwrap_or(&"-"));
            if q != o {
                wrong.push(format!("{at}\n  qt:   {q}\n  ours: {o}"));
            }
        }
    }
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert!(
        wrong.is_empty(),
        "{name}: {} rows differ\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// The ScrollViews of AssistantChat.qml, NotesContent.qml and MessageCodeBlock.qml: the view,
/// its Flickable, the TextArea in it and both bars, as loaded and scrolled to the end.
#[test]
fn the_views_of_the_shell_are_laid_out_as_in_qt() {
    let qt = include_str!("scenes/scroll_view/Places.qt");
    check("Places", [260.0, 420.0], &["end"], qt);
}

/// The content size: one TextArea, Flickable or Item, several children, `contentWidth` set
/// and bound, `availableWidth`; then after the sizes change, and after `contentWidth` is reset.
#[test]
fn a_scroll_view_has_the_content_size_qt_gives_it() {
    let qt = include_str!("scenes/scroll_view/Widths.qt");
    check("Widths", [400.0, 400.0], &["change", "reset"], qt);
}

/// The hint of the input of AssistantChat.qml, a Text in the field: as wide as the field it has
/// no width, as wide as the view it wraps and the view grows; then a long name, then a letter.
#[test]
fn a_hint_in_the_field_of_a_scroll_view_wraps_as_in_qt() {
    let qt = include_str!("scenes/scroll_view/Hint.qt");
    check("Hint", [336.0, 400.0], &["longName", "type"], qt);
}

/// The code of a message of the assistant chat (MessageCodeBlock.qml) under a paragraph that
/// wraps. The ColumnLayout the blocks are in has no width of its own, so as it completes it is
/// as wide as its implicit width, the paragraph on one line (QQuickGridLayoutBase::
/// componentComplete), and the code with it; the TextArea of the code stays as wide as the
/// view was then, less the 2 the `contentWidth: area.width - 1` of the view takes before Qt
/// stops the loop: the bar shows and the code scrolls sideways, at any later width.
#[test]
fn the_code_under_a_paragraph_keeps_the_width_the_message_had_as_it_was_made() {
    let qt = include_str!("scenes/scroll_view/Message.qt");
    check_in(
        "Message",
        [420.0, 600.0],
        &["both", "narrow", "wide"],
        qt,
        true,
    );
}

/// The same blocks given to a message that is there already: its layout has its width, the
/// code is never wider than the message and has no bar until the view is narrower than it was.
#[test]
fn the_code_added_to_a_message_is_as_wide_as_its_view() {
    let qt = include_str!("scenes/scroll_view/MessageLater.qt");
    check_in(
        "Message",
        [420.0, 600.0],
        &["empty", "fill", "narrow", "wide"],
        qt,
        true,
    );
}

/// A message that is only code: nothing in it is wider than the list.
#[test]
fn the_code_of_a_message_without_a_paragraph_is_as_wide_as_its_view() {
    let qt = include_str!("scenes/scroll_view/MessageCode.qt");
    check_in(
        "Message",
        [420.0, 600.0],
        &["codeOnly", "narrow", "wide"],
        qt,
        true,
    );
}

/// A scene of `scenes/qt_frames` (Qt's answer is written by `scenes/qt_frames/dump.sh`): its
/// `log` after `frames` frames, each one the scene's `sample(frame)`, a layout and `shown(frame)`.
/// The Flickable a ScrollView makes is `pixelAligned`: its content, and the position of its
/// bar, stand at a whole pixel. One declared in the view and a bare one stand where they are put.
#[test]
fn the_flickable_of_a_scroll_view_stands_at_a_whole_pixel() {
    let qt = include_str!("scenes/scroll_view/Aligned.qt");
    check("Aligned", [300.0, 330.0], &["part", "half", "more"], qt);
}

fn frames(name: &str, qt: &str) {
    let (mut rt, root) = testscenes::load(&format!("qt_frames/{name}.qml"));
    rt.settle();
    let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, size);
    rt.settle();
    for frame in 1..=rt.get(root, "frames").to_number() as usize {
        for step in ["sample", "shown"] {
            let frame = vec![Value::Num(frame as f64)];
            rt.call_method(root, step, frame).unwrap().unwrap();
            rt.settle();
            if step == "sample" {
                scene::layout_window(&mut rt, &mut fonts, root, size);
                rt.settle();
            }
        }
    }
    assert_eq!(rt.errors(), Vec::<String>::new());
    let ours = rt.get(root, "log").to_js_string();
    let (q, o): (Vec<_>, Vec<_>) = (qt.lines().collect(), ours.lines().collect());
    let wrong: Vec<String> = (0..q.len().max(o.len()))
        .filter(|&n| q.get(n) != o.get(n))
        .map(|n| {
            let (q, o) = (q.get(n).unwrap_or(&"-"), o.get(n).unwrap_or(&"-"));
            format!("  qt:   {q}\n  ours: {o}")
        })
        .collect();
    assert!(wrong.is_empty(), "{name}:\n{}", wrong.join("\n"));
}

/// A Flickable gives its attached vertical ScrollBar its height and the place at its right
/// edge, whatever the scene set for those; the bar's width and y stay the scene's. A
/// ScrollView leaves a bar it is given alone.
#[test]
fn a_flickable_places_its_vertical_bar_and_a_scroll_view_does_not() {
    let qt = include_str!("scenes/qt_frames/scrollbar_sizes.qt");
    frames("scrollbar_sizes", qt);
}

/// The same for a horizontal bar: the Flickable's width and the place at its bottom edge.
#[test]
fn a_flickable_places_its_horizontal_bar_and_a_scroll_view_does_not() {
    let qt = include_str!("scenes/qt_frames/scrollbar_across.qt");
    frames("scrollbar_across", qt);
}

/// One frame of the window as the shell shows it: it is laid out again only when something
/// that layout reads was written since `laid`; otherwise only the views follow their cursors.
fn frame(
    rt: &mut qml::runtime::Runtime,
    kb: &mut Keyboard,
    root: usize,
    size: [f64; 2],
    laid: Option<u64>,
) -> Option<u64> {
    let rev = rt.world.borrow().layout_rev;
    // What changed its size is laid out again too (the shell does it for those items alone).
    let resized = !std::mem::take(&mut rt.world.borrow_mut().resized).is_empty();
    if laid != Some(rev) || resized {
        scene::layout_window(rt, &mut kb.fonts.borrow_mut(), root, size);
        rt.world.borrow_mut().resized.clear();
    } else if scene::follow_cursors(&mut rt.world.borrow_mut(), &mut kb.fonts.borrow_mut(), root) {
        rt.settle();
    }
    // Scrolling moves the content without a layout: the edges and the bars follow it.
    let mut w = rt.world.borrow_mut();
    let mut scrolled = std::mem::take(&mut w.scrolled);
    scrolled.sort_unstable();
    scrolled.dedup();
    for f in scrolled {
        scene::flick_flags(&mut w, f);
    }
    Some(w.layout_rev)
}

/// The editors of AssistantChat.qml and NotesContent.qml under the pointer and the keys: the
/// bar shows while the pointer is over it, the wheel moves the text, and the view follows the
/// text cursor (QQuickTextAreaPrivate::ensureCursorVisible).
#[test]
fn the_editors_of_the_shell_scroll_as_in_qt() {
    let qt = include_str!("scenes/scroll_view/Input.qt");
    let steps = include_str!("scenes/scroll_view/Input.steps");
    input("Input", [260.0, 280.0], steps, qt);
}

/// Words typed into a TextArea that wraps and fills the ScrollView whose height follows it, as
/// the input field of AssistantChat.qml: the Flickable has a new height with each new line, the
/// TextArea then breaks its lines in the width of the Flickable, and so they stay whole. A
/// space typed at the end of a line takes no room in it.
#[test]
fn words_typed_into_the_field_of_a_scroll_view_stay_on_one_line() {
    let qt = include_str!("scenes/scroll_view/Words.qt");
    let steps = include_str!("scenes/scroll_view/Words.steps");
    input("Words", [336.0, 200.0], steps, qt);
}

/// The field of AssistantChat.qml: it fills the view and is as wide as Controls' placeholder
/// would make it (TextArea.qml: the wider of the text and the placeholder, with the paddings),
/// its hint in the placeholder's place. The view's content is then as wide as the hint, a line
/// no wider than the hint stays whole after the view has stopped growing, and one wider is
/// broken in the hint's width. Each row is also the row of the same field with
/// `placeholderText` instead of the hint.
#[test]
fn a_field_as_wide_as_its_hint_keeps_the_lines_no_wider_than_the_hint_whole() {
    let qt = include_str!("scenes/scroll_view/Hinted.qt");
    let steps = include_str!("scenes/scroll_view/Hinted.steps");
    input("Hinted", [336.0, 200.0], steps, qt);
}

/// The same field with a hint wider than the view: the field is as wide as the view, the hint
/// is broken there and so are the lines typed.
#[test]
fn a_field_of_a_hint_wider_than_the_view_is_as_wide_as_the_view() {
    let qt = include_str!("scenes/scroll_view/HintedLong.qt");
    let steps = include_str!("scenes/scroll_view/HintedLong.steps");
    input("HintedLong", [336.0, 200.0], steps, qt);
}

/// A field not anchored: the view keeps it as wide as itself, so
/// a line wider than those before it stays whole after the view has stopped growing, and the
/// view, as high as the field's implicit height, is one line high again once the text is gone.
#[test]
fn a_field_left_to_its_scroll_view_keeps_its_lines_whole_past_the_views_height() {
    let qt = include_str!("scenes/scroll_view/Tall.qt");
    let steps = include_str!("scenes/scroll_view/Tall.steps");
    input("Tall", [336.0, 200.0], steps, qt);
}

/// The same field given its text whole, next to one that is not anchored: a text longer than
/// the view is broken in the width of the view, not in the width the field had.
#[test]
fn a_long_text_set_on_the_field_of_a_scroll_view_wraps_in_the_width_of_the_view() {
    let qt = include_str!("scenes/scroll_view/Long.qt");
    let steps = include_str!("scenes/scroll_view/Long.steps");
    input("Long", [400.0, 300.0], steps, qt);
}

/// The handle of the bar of a ScrollView held and let go: the bar of the Flickable the view
/// makes stands at a whole pixel of the content while held, and where the pointer put it after.
#[test]
fn the_handle_of_a_scroll_view_moves_its_bar_as_in_qt() {
    let qt = include_str!("scenes/scroll_view/Handle.qt");
    let steps = include_str!("scenes/scroll_view/Handle.steps");
    input("Handle", [260.0, 120.0], steps, qt);
}

/// A scene under the pointer and the keys (Qt's answer is written by
/// `scenes/scroll_view/qt_input.py`): its `snap()` as loaded and after each line of `steps`.
fn input(name: &str, size: [f64; 2], steps: &str, qt: &str) {
    let (mut rt, root) = testscenes::load(&format!("scroll_view/{name}.qml"));
    rt.settle();
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    let mut ptr = Pointer::default();
    ptr.time = 1000.0;
    let mut wrong = Vec::new();
    let mut laid = None;
    let steps = [""].into_iter().chain(steps.lines());
    for (step, qt) in steps.zip(qt.trim_end().split("\n==\n")) {
        let s: Vec<&str> = step.split(' ').collect();
        let n = |k: usize| s[k].parse::<f64>().unwrap();
        match s[0] {
            "" => {}
            "move" | "wheel" => {
                ptr.enter(&mut rt, root, n(1) - 1.0, n(2) - 1.0);
                ptr.motion(&mut rt, root, n(1), n(2));
                if s[0] == "wheel" {
                    ptr.wheel(&mut rt, (0.0, -120.0), (0.0, 0.0), (ptr.time, ptr.time));
                }
            }
            "drag" | "press" => {
                ptr.enter(&mut rt, root, n(1) - 1.0, n(2) - 1.0);
                ptr.motion(&mut rt, root, n(1), n(2));
                rt.settle();
                laid = frame(&mut rt, &mut kb, root, size, laid);
                ptr.press(&mut rt, 0x110);
                for dy in (5..=n(3) as usize).step_by(5) {
                    ptr.motion(&mut rt, root, n(1), n(2) + dy as f64);
                }
                if s[0] == "drag" {
                    ptr.release(&mut rt, 0x110);
                }
            }
            "release" => ptr.release(&mut rt, 0x110),
            "key" => {
                let (sym, text) = match s[1].trim_start_matches("Ctrl+") {
                    "Return" => (0xff0d, Some("\r")),
                    "Home" => (0xff50, None),
                    "End" => (0xff57, None),
                    "Left" => (0xff51, None),
                    "Right" => (0xff53, None),
                    other => panic!("key {other}"),
                };
                kb.mods = if s[1].starts_with("Ctrl+") {
                    0x0400_0000
                } else {
                    0
                };
                for _ in 0..n(2) as usize {
                    kb.key(&mut rt, sym, text, true, false);
                    kb.key(&mut rt, sym, None, false, false);
                    // Each key is an event of its own, as in Qt: the frame after it is
                    // laid out before the next one arrives.
                    rt.settle();
                    laid = frame(&mut rt, &mut kb, root, size, laid);
                }
                kb.mods = 0;
            }
            "type" => {
                for c in s[1].chars() {
                    let c = if c == '_' { ' ' } else { c };
                    kb.key(&mut rt, c as u32, Some(&c.to_string()), true, false);
                    kb.key(&mut rt, c as u32, None, false, false);
                    rt.settle();
                    laid = frame(&mut rt, &mut kb, root, size, laid);
                }
            }
            "call" => {
                rt.call_method(root, s[1], vec![]).unwrap().unwrap();
            }
            other => panic!("step {other}"),
        }
        rt.settle();
        // Qt's answer is taken 1.5 s after the step.
        for _ in 0..94 {
            ptr.time += 16.0;
            rt.tick(ptr.time);
            laid = frame(&mut rt, &mut kb, root, size, laid);
            kb.sync(&mut rt);
            rt.settle();
        }
        let ours = match rt.call_method(root, "snap", vec![]).unwrap().unwrap() {
            Value::Str(s) => s.to_string(),
            v => panic!("snap() gave {v:?}"),
        };
        let at = if step.is_empty() { "as loaded" } else { step };
        let (q, o): (Vec<_>, Vec<_>) = (qt.lines().collect(), ours.lines().collect());
        for n in 0..q.len().max(o.len()) {
            let (q, o) = (q.get(n).unwrap_or(&"-"), o.get(n).unwrap_or(&"-"));
            if q != o {
                wrong.push(format!("{at}\n  qt:   {q}\n  ours: {o}"));
            }
        }
    }
    assert_eq!(rt.errors(), Vec::<String>::new());
    assert!(
        wrong.is_empty(),
        "{name}: {} rows differ\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
