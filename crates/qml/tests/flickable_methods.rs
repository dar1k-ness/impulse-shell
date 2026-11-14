//! QQuickFlickable's `flick(xVelocity, yVelocity)`, `cancelFlick()` and `returnToBounds()`.
//! The expected logs are what Qt 6.12 printed for the audit's `flick/f1`, `f2`, `f5`, `f6`,
//! `f7` and `f10` (a 200x200 Flickable over 200x2000): the signals of the call arrive inside
//! it, `movementStarted` before `flicking` turns true and `flickEnded` while `moving` is still
//! true, and the view ends where Qt's ends.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

/// Calls `go()` and runs 16 ms frames until nothing moves; returns the log and the last contentY.
fn run(props: &str, go: &str) -> (String, f64) {
    let src = format!(
        r#"import QtQuick
Flickable {{
    id: f
    property string log: ""
    function say(s) {{ log += s + " " + moving + " " + flicking + " " + contentY.toFixed(1) + ";" }}
    function go() {{ {go}; say("after") }}
    width: 200; height: 200; contentWidth: 200; contentHeight: 2000
    {props}
    onMovementStarted: say("movementStarted")
    onMovementEnded: log += "movementEnded " + contentY.toFixed(1) + ";"
    onFlickStarted: say("flickStarted")
    onFlickEnded: say("flickEnded")
    onMovingChanged: log += "moving " + moving + ";"
    onFlickingChanged: log += "flicking " + flicking + ";"
}}
"#
    );
    let mut rt = Runtime::new(None);
    let root = rt.load_str(&src).unwrap();
    rt.settle();
    let mut t = 1000.0;
    rt.tick(t);
    let _ = rt.call_method(root, "go", vec![]).unwrap();
    rt.settle();
    for _ in 0..1000 {
        t += 16.0;
        let busy = rt.tick(t);
        if !busy && !rt.world.borrow().flicks_active() {
            assert!(rt.errors().is_empty(), "{:?}", rt.errors());
            return (
                rt.get(root, "log").to_js_string(),
                rt.get(root, "contentY").to_number(),
            );
        }
    }
    panic!("the view never came to rest");
}

const STARTED: &str = "moving true;movementStarted true false 300.0;flicking true;\
                       flickStarted true true 300.0;";

#[test]
fn flick_runs_to_where_qt_stops() {
    let (log, y) = run("contentY: 300", "flick(0, -1500)");
    assert_eq!(
        log,
        format!(
            "{STARTED}after true true 300.0;flicking false;flickEnded true false 1050.0;moving false;\
             movementEnded 1050.0;"
        )
    );
    assert_eq!(y, 1050.0);
}

#[test]
fn flick_toward_the_start_stops_at_the_bound() {
    let (log, y) = run("contentY: 100", "flick(0, 3000)");
    assert!(
        log.ends_with("flicking false;flickEnded true false 0.0;moving false;movementEnded 0.0;"),
        "{log}"
    );
    assert_eq!(y, 0.0);
}

#[test]
fn flick_is_capped_by_maximum_flick_velocity() {
    let (_, y) = run(
        "contentY: 300; flickDeceleration: 5000; maximumFlickVelocity: 800",
        "flick(0, -5000)",
    );
    assert_eq!(y, 364.0);
}

#[test]
fn flick_on_both_axes_moves_only_the_flickable_one() {
    // contentWidth equals the width: only the vertical axis flicks, by Qt's 1 px.
    let (log, y) = run("contentY: 300", "flick(300, -40)");
    assert!(log.starts_with(STARTED), "{log}");
    assert_eq!(y, 301.0);
}

#[test]
fn cancel_flick_stops_inside_the_call() {
    let (log, y) = run("contentY: 300", "flick(0, -1500); cancelFlick()");
    assert_eq!(
        log,
        format!(
            "{STARTED}flicking false;flickEnded true false 300.0;moving false;movementEnded 300.0;\
             after false false 300.0;"
        )
    );
    assert_eq!(y, 300.0);
}

#[test]
fn return_to_bounds_brings_an_overshoot_back() {
    let (log, y) = run("", "contentY = -60; returnToBounds()");
    assert_eq!(log, "after false false -60.0;");
    assert_eq!(y, 0.0);
}
