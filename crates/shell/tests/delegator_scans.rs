//! A delegator (Loader, Repeater, a view) is read again only when something it reads changed:
//! its own properties, its component, its model, its window. An object destroyed elsewhere, the
//! laid-out extent of another view and the polish of another view's held model change are none
//! of those, so the 30 Loaders of this window are not read again for them. Qt has no such pass
//! at all: QQuickLoader, QQuickRepeater and QQuickItemView each act on their own signals.

use qml::runtime::{Runtime, take_deleg_scans};
use qml::value::Value;
use shell::scene;

fn qml() -> String {
    let mut q = String::from(
        r##"
import QtQuick
import Rustshell
FloatingWindow {
    id: root
    width: 300; height: 400
    property var view: lv
    property var names: ["a", "b", "c", "d", "e"]
    property var poked: pk
    Component { id: tc; Timer { interval: 7000 } }
    // What the tests write to: no delegator reads this item.
    Item {
        id: pk
        property var made: null
        property int make: 0
        property int drop: 0
        onMakeChanged: made = tc.createObject(pk)
        onDropChanged: made.destroy()
    }
    ListView {
        id: lv
        anchors.fill: parent
        model: ScriptModel { values: names }
        delegate: Rectangle {
            required property var modelData
            width: lv.width
            height: 50
        }
    }
"##,
    );
    for _ in 0..30 {
        q.push_str("    Loader { sourceComponent: Item {} }\n");
    }
    q.push_str("}\n");
    q
}

fn start() -> (Runtime, scene::Fonts, usize, usize) {
    let mut rt = Runtime::new(None);
    let win = rt.load_str(&qml()).unwrap();
    let mut fonts = scene::Fonts::default();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts, win, [300.0, 400.0]);
    rt.settle();
    let Value::Item(pk) = rt.get(win, "poked") else {
        panic!("no item");
    };
    (rt, fonts, win, pk)
}

/// One write that every pass sees, so a pass runs and reads what it has to.
fn pass(rt: &mut Runtime, pk: usize, n: f64) {
    rt.set(pk, "opacity", Value::Num(n));
    rt.settle();
}

#[test]
fn an_object_destroyed_elsewhere_reads_no_delegator_again() {
    let (mut rt, _fonts, _win, pk) = start();
    rt.set(pk, "make", Value::Num(1.0));
    rt.settle();
    assert!(matches!(rt.get(pk, "made"), Value::Item(_)));
    pass(&mut rt, pk, 0.9);
    take_deleg_scans();
    pass(&mut rt, pk, 0.85);
    assert_eq!(take_deleg_scans(), 0, "a write no delegator reads");
    let dead = rt.world.borrow().dead.len();
    rt.set(pk, "drop", Value::Num(1.0));
    rt.settle();
    pass(&mut rt, pk, 0.8);
    assert!(
        rt.world.borrow().dead.len() > dead,
        "the timer is destroyed"
    );
    assert_eq!(
        take_deleg_scans(),
        0,
        "delegators read again for a Timer destroyed beside them"
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

#[test]
fn a_view_laid_out_shorter_reads_only_itself_again() {
    let (mut rt, mut fonts, win, pk) = start();
    pass(&mut rt, pk, 0.9);
    take_deleg_scans();
    let names = |v: &[&str]| Value::array(v.iter().map(|s| Value::str(s)).collect());
    rt.set(win, "names", names(&["a", "c", "d", "e"]));
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts, win, [300.0, 400.0]);
    rt.settle();
    pass(&mut rt, pk, 0.8);
    let Value::Item(lv) = rt.get(win, "view") else {
        panic!("no view");
    };
    assert_eq!(rt.get(lv, "count").to_number(), 4.0);
    assert_eq!(rt.get(lv, "contentHeight").to_number(), 200.0);
    let scans = take_deleg_scans();
    assert!(
        (1..=4).contains(&scans),
        "{scans} delegators read again for one view's model and extent (31 are in the window)"
    );
}

#[test]
fn the_polish_of_a_held_view_reads_only_that_view_again() {
    let (mut rt, mut fonts, win, pk) = start();
    rt.set_polish_wins([win]);
    pass(&mut rt, pk, 0.9);
    let Value::Item(lv) = rt.get(win, "view") else {
        panic!("no view");
    };
    let names = |v: &[&str]| Value::array(v.iter().map(|s| Value::str(s)).collect());
    rt.set(win, "names", names(&["a", "c", "d", "e"]));
    rt.settle();
    assert_eq!(
        rt.held_windows(),
        vec![win],
        "the change waits for the frame"
    );
    assert_eq!(rt.world.borrow().delegated[&lv].1.len(), 5);
    take_deleg_scans();
    rt.polish(&[win]);
    assert_eq!(
        rt.world.borrow().delegated[&lv].1.len(),
        4,
        "applied by the polish"
    );
    scene::layout_window(&mut rt, &mut fonts, win, [300.0, 400.0]);
    rt.settle();
    pass(&mut rt, pk, 0.8);
    let scans = take_deleg_scans();
    assert!(
        (1..=4).contains(&scans),
        "{scans} delegators read again for one view's polish (31 are in the window)"
    );
}

const RESTING: &str = r##"
import QtQuick
import Rustshell
FloatingWindow {
    id: root
    width: 300; height: 400
    property var poked: pk
    property var hidden: hw
    property var lazy: ll
    property int loaded: 0
    Item { id: pk }
    LazyLoader {
        id: ll
        active: true
        component: QtObject { Component.onCompleted: root.loaded++ }
    }
    FloatingWindow {
        id: hw
        width: 100; height: 100
        ListView {
            anchors.fill: parent
            model: ScriptModel { values: ["a", "b"] }
            delegate: Item { width: 10; height: 10 }
        }
    }
}
"##;

/// A LazyLoader that has loaded keeps its object whatever its component becomes, and a view in
/// a window that is not shown keeps its delegates until the window shows: neither is read again
/// while nothing it reads changed.
#[test]
fn a_loaded_lazy_loader_and_a_view_in_a_hidden_window_are_not_read_again() {
    let mut rt = Runtime::new(None);
    let win = rt.load_str(RESTING).unwrap();
    rt.settle();
    rt.run_later();
    rt.settle();
    assert_eq!(
        rt.get(win, "loaded").to_number(),
        1.0,
        "the LazyLoader loaded"
    );
    let item = |rt: &mut Runtime, k: &str| match rt.get(win, k) {
        Value::Item(i) => i,
        v => panic!("{k}: {v:?}"),
    };
    let (pk, hw) = (item(&mut rt, "poked"), item(&mut rt, "hidden"));
    rt.set(hw, "visible", Value::Bool(false));
    rt.settle();
    pass(&mut rt, pk, 0.9);
    pass(&mut rt, pk, 0.8);
    take_deleg_scans();
    for k in 0..5 {
        pass(&mut rt, pk, 0.5 + k as f64 / 100.0);
    }
    assert_eq!(
        take_deleg_scans(),
        0,
        "delegators read again in 5 passes that changed nothing they read"
    );
    // What each reads still reaches it.
    rt.set(hw, "visible", Value::Bool(true));
    rt.settle();
    assert!(
        take_deleg_scans() >= 1,
        "the window shown reads its view again"
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
