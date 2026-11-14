//! `ListView.snapMode` (QQuickListViewPrivate::flick / fixup): a flick and `returnToBounds()`
//! rest on an item boundary. The numbers are what Qt 6.12 printed for the audit's `flick/l2`
//! (SnapToItem, 330 where a plain view stops at 333), `l3` (SnapOneItem, one item on) and
//! `l4` (47 becomes 35 inside `returnToBounds()`), each a 200x200 view of 40 rows of 30.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

/// Lays the view out, runs `go` on it and then 16 ms frames until it rests; returns contentY
/// right after `go` and at rest.
fn run(props: &str, go: &str) -> (f64, f64) {
    let src = format!(
        r#"import QtQuick
import Rustshell
FloatingWindow {{
    width: 200; height: 200
    property var view: f
    function go() {{ {go} }}
    ListView {{
        id: f
        width: 200; height: 200
        model: 40
        {props}
        delegate: Rectangle {{ width: 200; height: 30 }}
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
    let _ = rt.call_method(win, "go", vec![]).unwrap();
    let after = rt.get(lv, "contentY").to_number();
    frame(&mut rt);
    for _ in 0..1000 {
        t += 16.0;
        let busy = rt.tick(t);
        frame(&mut rt);
        if !busy && !rt.world.borrow().flicks_active() {
            assert!(rt.errors().is_empty(), "{:?}", rt.errors());
            return (after, rt.get(lv, "contentY").to_number());
        }
    }
    panic!("the view never came to rest");
}

#[test]
fn a_plain_view_stops_where_the_flick_ends() {
    assert_eq!(run("", "f.flick(0, -1000)"), (0.0, 333.0));
}

#[test]
fn snap_to_item_ends_a_flick_on_an_item() {
    assert_eq!(
        run("snapMode: ListView.SnapToItem", "f.flick(0, -1000)"),
        (0.0, 330.0)
    );
}

#[test]
fn snap_one_item_moves_one_item_per_flick() {
    assert_eq!(
        run("snapMode: ListView.SnapOneItem", "f.flick(0, -1000)"),
        (0.0, 30.0)
    );
}

#[test]
fn return_to_bounds_snaps_at_once() {
    // Rows of 30 with 5 between: 47 is nearest the row at 35.
    let go = "f.contentY = 47; f.returnToBounds()";
    assert_eq!(
        run("snapMode: ListView.SnapToItem; spacing: 5", go),
        (35.0, 35.0)
    );
    assert_eq!(run("spacing: 5", go), (47.0, 47.0));
}
