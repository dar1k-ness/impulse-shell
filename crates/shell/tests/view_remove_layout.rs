//! A delegate running its `remove` transition is out of the view's layout
//! (QQuickItemView::releaseItem took it out of visibleItems): what the transition writes to it
//! places that delegate alone. The view, its other delegates and its attached ScrollBar are not
//! laid out again for a frame of the transition, as in Qt, where the bar's height changes 0
//! times while a popup notification slides out.

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
    property var names: ["a", "b", "c", "d"]
    property int barHeights: 0
    property int barYs: 0
    property int moved: 0
    ListView {
        id: lv
        anchors.fill: parent
        spacing: 4
        model: ScriptModel { values: names }
        remove: Transition {
            NumberAnimation { property: "x"; to: 300; duration: 200 }
            NumberAnimation { property: "opacity"; to: 0; duration: 200 }
        }
        ScrollBar.vertical: ScrollBar {
            onHeightChanged: barHeights++
            onYChanged: barYs++
        }
        delegate: Rectangle {
            required property var modelData
            width: lv.width
            height: 50
            onYChanged: if (modelData !== "b") moved++
            onWidthChanged: if (modelData !== "b") moved++
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
fn remove_transition_frames_place_only_the_leaving_delegate() {
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
    rt.set(win, "names", names(&["a", "c", "d"]));
    // The removal itself moves the delegates below up: that frame and the next are the view's.
    for _ in 0..2 {
        now += 16.0;
        rt.tick(now);
        frame(&mut rt, &mut fonts, win, &mut laid);
    }
    let leaving: Vec<usize> = rt.world.borrow().view_removing.keys().copied().collect();
    assert_eq!(leaving.len(), 1, "one delegate runs the remove transition");
    let count = |rt: &mut Runtime, k: &str| rt.get(win, k).to_number();
    let before = (
        count(&mut rt, "barHeights"),
        count(&mut rt, "barYs"),
        count(&mut rt, "moved"),
    );
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
    assert!(
        !rt.world.borrow().view_removing.is_empty(),
        "the transition still runs"
    );
    let x = rt.get(leaving[0], "x").to_number();
    assert!(x > 0.0 && x < 300.0, "the leaving delegate moves: x={x}");
    let after = (
        count(&mut rt, "barHeights"),
        count(&mut rt, "barYs"),
        count(&mut rt, "moved"),
    );
    assert_eq!(
        after, before,
        "(bar height, bar y, other delegates) changes in {frames} frames"
    );
    assert_eq!(full, 0, "full layout passes in {frames} frames");
    assert_eq!(local, frames, "one local pass per frame");
    assert_eq!(
        targets, leaving,
        "local targets: only the leaving delegate (view is {lv})"
    );
}
