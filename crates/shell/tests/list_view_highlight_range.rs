//! `ListView.highlightRangeMode: StrictlyEnforceRange` (QQuickListView::viewportMoved, fixup
//! and the extents of QQuickItemView): the highlight never leaves the preferred range, so a
//! view moved by a flick, by `contentY` or by `positionViewAt*` makes the row under the
//! highlight the current one, and it rests with the current row inside the range. Each
//! expected string is what Qt 6.12 printed for the same view (a 200x200 view of 40 rows of 30).

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

/// Lays the view out, then runs each step and 16 ms frames until the view rests; returns what
/// the view logged during each step.
fn run(props: &str, steps: &[&str]) -> Vec<String> {
    let calls: String = steps
        .iter()
        .enumerate()
        .map(|(k, s)| format!("    function step{k}() {{ {s} }}\n"))
        .collect();
    let src = format!(
        r#"import QtQuick
import Rustshell
FloatingWindow {{
    id: root
    width: 200; height: 200
    property var view: f
    property string log: ""
    function say(s) {{ root.log += (root.log ? "|" : "") + s }}
    function at() {{ return f.contentY.toFixed(1) + " " + f.currentIndex }}
{calls}
    ListView {{
        id: f
        width: 200; height: 200
        model: 40
        {props}
        delegate: Rectangle {{ width: 200; height: 30 }}
        onMovementEnded: root.say("movementEnded " + root.at())
        onFlickEnded: root.say("flickEnded " + root.at())
        onCurrentIndexChanged: root.say("currentIndex " + currentIndex)
    }}
}}
"#
    );
    let mut rt = Runtime::new(None);
    let win = rt.load_str(&src).unwrap();
    let mut fonts = scene::Fonts::default();
    let mut frame = |rt: &mut Runtime| {
        for _ in 0..3 {
            rt.settle();
            scene::layout_window(rt, &mut fonts, win, [200.0, 200.0]);
        }
    };
    frame(&mut rt);
    let Some(Value::Item(lv)) = rt.world.borrow_mut().read(win, "view") else {
        panic!("no view");
    };
    let mut t = 1000.0;
    rt.tick(t);
    frame(&mut rt);
    let mut out = vec![format!(
        "{:.1} {}",
        rt.get(lv, "contentY").to_number(),
        rt.get(lv, "currentIndex").to_number()
    )];
    for k in 0..steps.len() {
        rt.world
            .borrow_mut()
            .write(win, "log", Value::Str("".into()));
        let _ = rt.call_method(win, &format!("step{k}"), vec![]).unwrap();
        frame(&mut rt);
        let mut rested = false;
        for _ in 0..1000 {
            t += 16.0;
            let busy = rt.tick(t);
            frame(&mut rt);
            if !busy && !rt.world.borrow().flicks_active() {
                rested = true;
                break;
            }
        }
        assert!(rested, "the view never came to rest in step {k}");
        root_say_rest(&mut rt, win);
        out.push(rt.get(win, "log").to_js_string());
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    out
}

/// Appends where the view rests to the log.
fn root_say_rest(rt: &mut Runtime, win: usize) {
    let at = rt
        .call_method(win, "at", vec![])
        .unwrap()
        .unwrap()
        .to_js_string();
    let _ = rt
        .call_method(win, "say", vec![Value::Str(format!("rest {at}").into())])
        .unwrap();
}

const STRICT: &str = "highlightRangeMode: ListView.StrictlyEnforceRange; preferredHighlightBegin: 60; preferredHighlightEnd: 90";

/// Qt (`flick/l5`): a flick takes `currentIndex` along row by row and ends on a row.
#[test]
fn a_flick_moves_the_current_index_with_the_highlight() {
    let got = run(
        "snapMode: ListView.SnapToItem; highlightRangeMode: ListView.StrictlyEnforceRange; preferredHighlightBegin: 0; preferredHighlightEnd: 30",
        &["f.flick(0, -1000); root.say('after ' + root.at() + ' ' + f.moving + ' ' + f.flicking)"],
    );
    assert_eq!(got[0], "0.0 0");
    assert_eq!(
        got[1],
        "after 0.0 0 true true|currentIndex 1|currentIndex 2|currentIndex 3|currentIndex 4|currentIndex 5|currentIndex 6|currentIndex 7|currentIndex 8|currentIndex 9|currentIndex 10|currentIndex 11|flickEnded 330.0 11|movementEnded 330.0 11|rest 330.0 11"
    );
}

/// Qt (`flick/l5b`): the view starts with row 0 in the range (contentY -60), a flick without
/// `snapMode` still ends with the current row in the range, and `currentIndex` scrolls the
/// row into it.
#[test]
fn the_view_rests_with_the_current_row_in_the_range() {
    let got = run(STRICT, &["f.flick(0, -1000)", "f.currentIndex = 20"]);
    assert_eq!(got[0], "-60.0 0");
    assert_eq!(
        got[1],
        "currentIndex 1|currentIndex 2|currentIndex 3|currentIndex 4|currentIndex 5|currentIndex 6|currentIndex 7|currentIndex 8|currentIndex 9|currentIndex 10|currentIndex 11|flickEnded 270.0 11|movementEnded 270.0 11|rest 270.0 11"
    );
    assert_eq!(got[2], "currentIndex 20|rest 540.0 20");
}

/// Qt (`flick/l5b`): `contentY` and `positionViewAt*` change `currentIndex` before they
/// return, `positionViewAtEnd()` through the last row first, and a flick back ends at the
/// first row's place in the range.
#[test]
fn moving_the_view_by_hand_picks_the_row_under_the_highlight() {
    let got = run(
        STRICT,
        &[
            "f.currentIndex = 20",
            "f.contentY = 100; root.say('set ' + root.at()); f.returnToBounds(); root.say('rtb ' + root.at())",
            "f.positionViewAtEnd(); root.say('pvae ' + root.at()); f.positionViewAtBeginning(); root.say('pvab ' + root.at())",
            "f.flick(0, 3000)",
        ],
    );
    assert_eq!(got[2], "currentIndex 5|set 100.0 5|rtb 90.0 5|rest 90.0 5");
    assert_eq!(
        got[3],
        "currentIndex 39|currentIndex 35|pvae 990.0 35|currentIndex 2|pvab 0.0 2|rest 0.0 2"
    );
    assert_eq!(
        got[4],
        "currentIndex 1|currentIndex 0|flickEnded -60.0 0|movementEnded -60.0 0|rest -60.0 0"
    );
}

/// Qt (`flick/l5d`): `positionViewAtIndex` jumps to a row outside the buffered rows first, and
/// a flick after `currentIndex` was set takes the current row from the highlight again.
#[test]
fn position_view_at_index_and_flicks_follow_the_highlight() {
    let got = run(
        STRICT,
        &[
            "f.positionViewAtIndex(20, ListView.Center); root.say('pvai ' + root.at())",
            "f.flick(0, 300)",
            "f.currentIndex = 30; f.flick(0, -300); root.say('after ' + root.at() + ' ' + f.moving + ' ' + f.flicking)",
        ],
    );
    assert_eq!(
        got[1],
        "currentIndex 22|currentIndex 19|pvai 510.0 19|rest 510.0 19"
    );
    assert_eq!(
        got[2],
        "currentIndex 18|flickEnded 480.0 18|movementEnded 480.0 18|rest 480.0 18"
    );
    assert_eq!(
        got[3],
        "currentIndex 30|after 480.0 30 true true|currentIndex 18|currentIndex 19|flickEnded 510.0 19|movementEnded 510.0 19|rest 510.0 19"
    );
}

/// Qt run on fixed 16 ms frames (`flick/l5c` with the steps apart): under ApplyRange the view
/// brings the current row into the range where its extents allow, `currentIndex` does not
/// follow a flick, and the flick ends where a plain view's does.
#[test]
fn apply_range_scrolls_the_current_row_into_the_range() {
    let got = run(
        "highlightRangeMode: ListView.ApplyRange; preferredHighlightBegin: 60; preferredHighlightEnd: 90",
        &[
            "f.currentIndex = 5",
            "f.currentIndex = 39",
            "f.currentIndex = 0; f.currentIndex = 20",
            "f.flick(0, -1000)",
        ],
    );
    assert_eq!(got[0], "0.0 0");
    assert_eq!(got[1], "currentIndex 5|rest 90.0 5");
    assert_eq!(got[2], "currentIndex 39|rest 1000.0 39");
    assert_eq!(got[3], "currentIndex 0|currentIndex 20|rest 540.0 20");
    assert_eq!(
        got[4],
        "flickEnded 873.0 20|movementEnded 873.0 20|rest 873.0 20"
    );
}
