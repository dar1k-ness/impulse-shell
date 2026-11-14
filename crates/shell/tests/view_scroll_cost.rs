//! A frame of scrolling moves the content item and nothing else: Qt's QQuickFlickable sets the
//! content item's position (`setViewportY`), the attached ScrollBar's `position` follows in the
//! same step, and no item is measured or placed again. Qt 6.12 on these scenes, 10 writes of
//! `contentY`: the content item's `y` changes 10 times, the bar's `position` 10 times, the bar's
//! `height` 0 times.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const LIST: &str = r##"
import QtQuick
import QtQuick.Controls
import Rustshell
FloatingWindow {
    width: 300; height: 400
    property var view: v
    property int positions: 0
    property int barHeights: 0
    property int contentYs: 0
    ListView {
        id: v
        x: 50; y: 100; width: 200; height: 200
        clip: true
        model: 40
        ScrollBar.vertical: ScrollBar {
            onPositionChanged: positions++
            onHeightChanged: barHeights++
        }
        delegate: Rectangle { width: v.width; height: 40 }
    }
    Connections {
        target: v.contentItem
        function onYChanged() { contentYs++ }
    }
}
"##;

const FLICK: &str = r##"
import QtQuick
import QtQuick.Controls
import Rustshell
FloatingWindow {
    width: 300; height: 400
    property var view: v
    property int positions: 0
    property int barHeights: 0
    property int contentYs: 0
    Flickable {
        id: v
        x: 50; y: 100; width: 200; height: 200
        clip: true
        contentWidth: width; contentHeight: col.height
        ScrollBar.vertical: ScrollBar {
            onPositionChanged: positions++
            onHeightChanged: barHeights++
        }
        Column {
            id: col
            Repeater {
                model: 40
                Rectangle { width: 200; height: 40 }
            }
        }
    }
    Connections {
        target: v.contentItem
        function onYChanged() { contentYs++ }
    }
}
"##;

/// The shell's frame (`App::draw_frame`): a full pass when the layout revision moved or a resize
/// is not local, else up to 4 local passes. Returns (full passes, local passes).
fn frame(rt: &mut Runtime, fonts: &mut scene::Fonts, win: usize, laid: &mut u64) -> (usize, usize) {
    let mut local = 0;
    for pass in 0..4 {
        let mut rs = std::mem::take(&mut rt.world.borrow_mut().resized);
        let mut w = rt.world.borrow_mut();
        if w.layout_rev != *laid || !rs.iter().all(|&i| scene::is_local_resize(&mut w, i)) {
            drop(w);
            scene::layout_window(rt, fonts, win, [300.0, 400.0]);
            *laid = rt.world.borrow().layout_rev;
            rt.world.borrow_mut().resized.clear();
            return (1, local);
        }
        if rs.is_empty() {
            break;
        }
        // In the full pass's order: measure, let the bindings on the measured implicit sizes
        // answer, place.
        let rev = w.layout_rev;
        scene::measure_targets(&mut w, fonts, &rs);
        w.layout_rev = rev;
        drop(w);
        rt.settle();
        let mut w = rt.world.borrow_mut();
        let answered = std::mem::take(&mut w.resized);
        if !answered.is_empty()
            && (w.layout_rev != rev || !answered.iter().all(|&i| scene::is_local_resize(&mut w, i)))
        {
            // The next round's full pass.
            w.resized = answered;
            continue;
        }
        rs.extend(answered);
        let rev = w.layout_rev;
        scene::relayout_targets(&mut w, fonts, &rs, pass);
        w.layout_rev = rev;
        drop(w);
        local += 1;
        rt.settle();
    }
    // `App::draw_frame` after its layout passes: the scrolled Flickables' flags and bars.
    let mut w = rt.world.borrow_mut();
    let mut sc = std::mem::take(&mut w.scrolled);
    sc.sort_unstable();
    sc.dedup();
    for f in sc {
        scene::flick_flags(&mut w, f);
    }
    (0, local)
}

fn scroll(src: &str, placed: usize) {
    let mut rt = Runtime::new(None);
    // As the shell runs: an item made this frame is placed by its parent's pass.
    rt.world.borrow_mut().track_fresh = true;
    let win = rt.load_str(src).unwrap();
    let mut fonts = scene::Fonts::default();
    let mut now = 0.0;
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts, win, [300.0, 400.0]);
    let mut laid = rt.world.borrow().layout_rev;
    for _ in 0..5 {
        now += 16.0;
        rt.tick(now);
        frame(&mut rt, &mut fonts, win, &mut laid);
    }
    let Some(Value::Item(view)) = rt.world.borrow_mut().read(win, "view") else {
        panic!("no view");
    };
    let counts = |rt: &mut Runtime| {
        ["contentYs", "positions", "barHeights"].map(|k| rt.get(win, k).to_number())
    };
    let before = counts(&mut rt);
    let (mut full, mut local) = (0, 0);
    for step in 1..=10 {
        rt.set(view, "contentY", Value::Num(step as f64 * 10.0));
        now += 16.0;
        rt.tick(now);
        let (f, l) = frame(&mut rt, &mut fonts, win, &mut laid);
        full += f;
        local += l;
    }
    // The frames after the last write: nothing is left over for them either.
    for _ in 0..3 {
        now += 16.0;
        rt.tick(now);
        let (f, l) = frame(&mut rt, &mut fonts, win, &mut laid);
        full += f;
        local += l;
    }
    let errors = rt.errors();
    assert!(errors.is_empty(), "{errors:?}");
    let after = counts(&mut rt);
    let changed = [0, 1, 2].map(|k| after[k] - before[k]);
    assert_eq!(
        (changed, full, local),
        ([10.0, 10.0, 0.0], 0, placed),
        "([content item y, bar position, bar height] changes, full layout passes, local passes) \
         in 10 scrolled frames"
    );
}

#[test]
fn a_scrolled_list_frame_lays_nothing_out() {
    // Rows 14 and 15 come into the cache (contentY 40 and 80, cacheBuffer 320): each is placed
    // by two passes of the view alone.
    scroll(LIST, 4);
}

#[test]
fn a_scrolled_flickable_frame_lays_nothing_out() {
    scroll(FLICK, 0);
}
