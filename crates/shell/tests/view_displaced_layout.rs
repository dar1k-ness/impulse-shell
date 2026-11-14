//! A delegate running a displaced transition (`removeDisplaced` writes its `y`) only moves: in
//! Qt a delegate's position lays nothing out (QQuickListViewPrivate::itemGeometryChanged
//! polishes the view for a change of the delegate's size only). So a frame of the transition
//! measures and places no item: not the view, not its other delegates, not the moving ones.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r##"
import QtQuick
import QtQuick.Controls
import Rustshell
FloatingWindow {
    width: 300; height: 400
    property var view: lv
    property var names: ["a", "b", "c", "d", "e"]
    property int barChanges: 0
    property int stillMoved: 0
    ListView {
        id: lv
        anchors.fill: parent
        spacing: 4
        model: ScriptModel { values: names }
        removeDisplaced: Transition {
            NumberAnimation { property: "y"; duration: 200 }
        }
        ScrollBar.vertical: ScrollBar {
            onHeightChanged: barChanges++
            onYChanged: barChanges++
        }
        delegate: Rectangle {
            required property var modelData
            width: lv.width
            height: 50
            onYChanged: if (modelData === "a") stillMoved++
            Row {
                anchors.fill: parent
                Repeater {
                    model: 3
                    Rectangle { width: 20; height: 20 }
                }
            }
        }
    }
}
"##;

/// The shell's frame (`App::draw_frame`): a full pass when the layout revision moved or a resize
/// is not local, else up to 4 local passes. Returns (full passes, local passes, local targets).
fn frame(
    rt: &mut Runtime,
    fonts: &mut scene::Fonts,
    win: usize,
    laid: &mut u64,
) -> (usize, usize, Vec<usize>) {
    let mut local = 0;
    let mut targets = vec![];
    for pass in 0..4 {
        let mut rs = std::mem::take(&mut rt.world.borrow_mut().resized);
        let mut w = rt.world.borrow_mut();
        if w.layout_rev != *laid || !rs.iter().all(|&i| scene::is_local_resize(&mut w, i)) {
            drop(w);
            scene::layout_window(rt, fonts, win, [300.0, 400.0]);
            *laid = rt.world.borrow().layout_rev;
            rt.world.borrow_mut().resized.clear();
            return (1, local, targets);
        }
        if rs.is_empty() {
            break;
        }
        for &i in &rs {
            let t = scene::local_target(&mut w, i);
            if !targets.contains(&t) {
                targets.push(t);
            }
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
    (0, local, targets)
}

#[test]
fn displaced_transition_frames_place_only_the_moving_delegates() {
    let mut rt = Runtime::new(None);
    let win = rt.load_str(QML).unwrap();
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
    let Some(Value::Item(lv)) = rt.world.borrow_mut().read(win, "view") else {
        panic!("no view");
    };
    let names = |v: &[&str]| Value::array(v.iter().map(|s| Value::str(s)).collect());
    rt.set(win, "names", names(&["a", "c", "d", "e"]));
    // The removal itself is the view's: that frame and the next lay the view out.
    for _ in 0..2 {
        now += 16.0;
        rt.tick(now);
        frame(&mut rt, &mut fonts, win, &mut laid);
    }
    let mut moving: Vec<usize> = rt.world.borrow().view_moving.keys().copied().collect();
    moving.sort_unstable();
    assert_eq!(
        moving.len(),
        3,
        "the three delegates below the removed one move"
    );
    let count = |rt: &mut Runtime, k: &str| rt.get(win, k).to_number();
    let before = (count(&mut rt, "barChanges"), count(&mut rt, "stillMoved"));
    let ys = |rt: &mut Runtime| -> Vec<f64> {
        moving.iter().map(|&c| rt.get(c, "y").to_number()).collect()
    };
    let y0 = ys(&mut rt);
    scene::take_layout_work();
    let (mut full, mut local, mut frames) = (0, 0, 0);
    let mut targets = vec![];
    // 6 frames inside the 200 ms transition.
    for _ in 0..6 {
        now += 16.0;
        rt.tick(now);
        let (f, l, ts) = frame(&mut rt, &mut fonts, win, &mut laid);
        full += f;
        local += l;
        frames += 1;
        for t in ts {
            if !targets.contains(&t) {
                targets.push(t);
            }
        }
    }
    let (measured, placed) = scene::take_layout_work();
    assert_eq!(
        rt.world.borrow().view_moving.len(),
        3,
        "the transition still runs"
    );
    let y1 = ys(&mut rt);
    assert!(
        y0.iter().zip(&y1).all(|(a, b)| b < a),
        "the delegates move up: {y0:?} -> {y1:?}"
    );
    let after = (count(&mut rt, "barChanges"), count(&mut rt, "stillMoved"));
    assert_eq!(
        after, before,
        "(bar geometry, the delegate above) changes in {frames} frames"
    );
    assert_eq!(full, 0, "full layout passes in {frames} frames");
    assert_eq!(
        (local, targets, measured, placed),
        (0, vec![], 0, 0),
        "(local passes, their targets, items measured, items placed) in {frames} frames, in \
         each of which 3 delegates changed their y and nothing its size (view is {lv})"
    );
}

/// A delegate that changes its height while it moves is the view's to lay out, as in Qt
/// (itemGeometryChanged: heightChange in a vertical view -> forceLayoutPolish).
#[test]
fn a_moving_delegate_that_changes_its_height_lays_the_view_out() {
    let mut rt = Runtime::new(None);
    let win = rt.load_str(QML).unwrap();
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
    let Some(Value::Item(lv)) = rt.world.borrow_mut().read(win, "view") else {
        panic!("no view");
    };
    let names = |v: &[&str]| Value::array(v.iter().map(|s| Value::str(s)).collect());
    rt.set(win, "names", names(&["a", "c", "d", "e"]));
    for _ in 0..4 {
        now += 16.0;
        rt.tick(now);
        frame(&mut rt, &mut fonts, win, &mut laid);
    }
    let mut moving: Vec<usize> = rt.world.borrow().view_moving.keys().copied().collect();
    moving.sort_unstable();
    assert_eq!(
        moving.len(),
        3,
        "the three delegates below the removed one move"
    );
    let content = rt.get(lv, "contentHeight").to_number();
    rt.set(moving[0], "height", Value::Num(80.0));
    now += 16.0;
    rt.tick(now);
    let (full, _, targets) = frame(&mut rt, &mut fonts, win, &mut laid);
    assert_eq!(
        (full, targets.first()),
        (0, Some(&lv)),
        "(full passes, first local target) of the frame after the height change"
    );
    assert_eq!(
        rt.get(lv, "contentHeight").to_number(),
        content + 30.0,
        "the view's contentHeight follows the delegate's height"
    );
}

const SHRINK_QML: &str = r##"
import QtQuick
import Rustshell
FloatingWindow {
    width: 300; height: 400
    property var view: lv
    property bool expanded: true
    property var all: ["a", "b", "c"]
    ListView {
        id: lv
        anchors.fill: parent
        spacing: 4
        model: ScriptModel { values: expanded ? all : all.slice(0, 2) }
        removeDisplaced: Transition {
            NumberAnimation { property: "y"; duration: 200 }
        }
        delegate: Rectangle {
            required property var modelData
            width: lv.width
            height: expanded ? 60 : 20
            Behavior on height {
                NumberAnimation { duration: 200 }
            }
        }
    }
}
"##;

/// Only the rows after a removed one run `removeDisplaced`
/// (QQuickItemViewPrivate::applyRemovalChange: transitionNextReposition for an index past the
/// removal). A row before it that moves because the row above it shrinks is placed by each
/// layout, so it ends right under that row.
#[test]
fn a_row_before_the_removed_one_is_placed_not_displaced() {
    let mut rt = Runtime::new(None);
    let win = rt.load_str(SHRINK_QML).unwrap();
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
    let Some(Value::Item(lv)) = rt.world.borrow_mut().read(win, "view") else {
        panic!("no view");
    };
    let second = rt.world.borrow().delegated[&lv].1[1];
    assert_eq!(rt.get(second, "y").to_number(), 64.0);
    rt.set(win, "expanded", Value::Bool(false));
    let mut moved = false;
    for _ in 0..30 {
        now += 16.0;
        rt.tick(now);
        frame(&mut rt, &mut fonts, win, &mut laid);
        moved |= rt.world.borrow().view_moving.contains_key(&second);
    }
    assert!(!moved, "the second row ran the displaced transition");
    assert_eq!(rt.get(second, "y").to_number(), 24.0);
}

const NESTED_QML: &str = r##"
import QtQuick
import Rustshell
FloatingWindow {
    width: 300; height: 400
    property var view: lv
    property var names: ["b", "c"]
    ListView {
        id: lv
        anchors.fill: parent
        spacing: 3
        model: ScriptModel { values: names }
        addDisplaced: Transition {
            NumberAnimation { property: "y"; duration: 200 }
        }
        delegate: Item {
            required property var modelData
            width: lv.width
            implicitHeight: inner.implicitHeight + 20
            ListView {
                id: inner
                width: parent.width
                implicitHeight: contentHeight
                height: implicitHeight
                interactive: false
                model: 1
                delegate: Item { width: 10; implicitHeight: 19 }
            }
        }
    }
}
"##;

/// In Qt a delegate is complete when the view inserts it: a view inside it has made and laid
/// out its rows (QQuickItemView::componentComplete), so the rows the new one displaces move to
/// the place its full height leaves them. Here the inner view's content height comes with a
/// later layout pass of the same frame: the displaced rows take the place of that pass.
#[test]
fn rows_displaced_by_a_new_row_end_below_its_full_height() {
    let mut rt = Runtime::new(None);
    let win = rt.load_str(NESTED_QML).unwrap();
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
    let Some(Value::Item(lv)) = rt.world.borrow_mut().read(win, "view") else {
        panic!("no view");
    };
    let rows = |rt: &Runtime| rt.world.borrow().delegated[&lv].1.clone();
    let ys = |rt: &mut Runtime| -> Vec<f64> {
        (rows(rt).iter())
            .map(|&c| rt.get(c, "y").to_number())
            .collect()
    };
    assert_eq!(ys(&mut rt), [0.0, 42.0]);
    let names = |v: &[&str]| Value::array(v.iter().map(|s| Value::str(s)).collect());
    rt.set(win, "names", names(&["a", "b", "c"]));
    let mut moved = false;
    for _ in 0..30 {
        now += 16.0;
        rt.tick(now);
        frame(&mut rt, &mut fonts, win, &mut laid);
        moved |= !rt.world.borrow().view_moving.is_empty();
    }
    assert!(moved, "the rows below the new one ran the transition");
    assert!(rt.world.borrow().view_moving.is_empty());
    assert_eq!(ys(&mut rt), [0.0, 42.0, 84.0]);
}
