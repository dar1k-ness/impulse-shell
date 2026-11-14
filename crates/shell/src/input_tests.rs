//! Pointer delivery against Qt Quick semantics, driven through `Pointer` on a loaded scene.
use super::*;

const BTN_LEFT: u32 = 0x110;

fn load(name: &str, qml: &str) -> (Runtime, usize) {
    let d = std::env::temp_dir().join(format!("shell-ptr-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("main.qml"), qml).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let _ = std::fs::remove_dir_all(&d);
    (rt, root)
}

fn log(rt: &Runtime, root: usize) -> String {
    rt.get(root, "log").to_js_string()
}

/// A frame of window `win` with Qt's interval of frame-synchronous hover at 0: every frame
/// delivers it.
fn frame(p: &mut Pointer, win: usize) {
    p.hover_every = Some(0);
    p.frame(win, std::time::Instant::now());
}

fn click(p: &mut Pointer, rt: &mut Runtime) {
    p.press(rt, BTN_LEFT);
    p.release(rt, BTN_LEFT);
}

fn drag(p: &mut Pointer, rt: &mut Runtime, root: usize, from: (f64, f64), to: (f64, f64)) {
    p.motion(rt, root, from.0, from.1);
    p.press(rt, BTN_LEFT);
    for i in 1..=12 {
        let f = i as f64 / 12.0;
        p.motion(
            rt,
            root,
            from.0 + (to.0 - from.0) * f,
            from.1 + (to.1 - from.1) * f,
        );
    }
    p.release(rt, BTN_LEFT);
}

#[test]
fn mouse_area_double_click_replaces_second_clicked() {
    let (mut rt, root) = load(
        "dclick",
        r#"import QtQuick
        Item {
            width: 200; height: 200
            property string log: ""
            MouseArea {
                width: 200; height: 200
                onPressed: parent.log += "p"
                onReleased: parent.log += "r"
                onClicked: parent.log += "c"
                onDoubleClicked: parent.log += "d"
            }
        }"#,
    );
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 50.0, 50.0);
    p.motion(&mut rt, root, 50.0, 50.0);
    click(&mut p, &mut rt);
    click(&mut p, &mut rt);
    assert_eq!(log(&rt, root), "prcpdr");
    // QGuiApplicationPrivate resets mousePressButton after a double click.
    click(&mut p, &mut rt);
    assert_eq!(log(&rt, root), "prcpdrprc");
    // Moving more than mouseDoubleClickDistance from the press cancels the double click.
    p.motion(&mut rt, root, 60.0, 50.0);
    click(&mut p, &mut rt);
    assert_eq!(log(&rt, root), "prcpdrprcprc");
}

#[test]
fn mouse_area_without_double_click_handler_clicks_twice() {
    let (mut rt, root) = load(
        "dclick-none",
        r#"import QtQuick
        Item {
            width: 200; height: 200
            property string log: ""
            MouseArea {
                width: 200; height: 200
                onClicked: parent.log += "c"
            }
        }"#,
    );
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 50.0, 50.0);
    p.motion(&mut rt, root, 50.0, 50.0);
    click(&mut p, &mut rt);
    click(&mut p, &mut rt);
    assert_eq!(log(&rt, root), "cc");
}

#[test]
fn mouse_area_drag_target_emits_drag_active_changed() {
    let (mut rt, root) = load(
        "dragactive",
        r#"import QtQuick
        Item {
            width: 400; height: 200
            property string log: ""
            Rectangle {
                id: t
                width: 60; height: 60
                MouseArea {
                    width: 60; height: 60
                    drag.target: t
                    drag.axis: Drag.XAxis
                    onReleased: root.log += "r" + t.x + " "
                    onClicked: root.log += "c "
                    drag.onActiveChanged: root.log += (drag.active ? "on " : "off ")
                }
            }
            id: root
        }"#,
    );
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 30.0, 30.0);
    drag(&mut p, &mut rt, root, (30.0, 30.0), (130.0, 30.0));
    let l = log(&rt, root);
    assert!(l.starts_with("on r"), "{l}");
    assert!(l.ends_with(" off "), "{l}");
    assert!(!l.contains('c'), "{l}");
}

/// A `RowLayout` whose dragged child is `drag.target` of its own MouseArea, with a neighbour
/// that shows while the drag lasts (the tray of the waffle bar). Qt 6.12: the layout places the
/// child once the neighbour shows, then the child follows the pointer from there, 3 px a move.
#[test]
fn a_child_of_a_layout_dragged_by_its_mouse_area_follows_the_pointer() {
    let (mut rt, root) = load(
        "layoutdrag",
        r#"import QtQuick
        import QtQuick.Layouts
        Item {
            id: host
            width: 400; height: 60
            property string log: ""
            function xs() { return over.visible + ":" + over.x + "," + rep.itemAt(0).x + "," + rep.itemAt(1).x }
            RowLayout {
                id: row
                x: 100
                property bool dragging: false
                spacing: 0
                Rectangle {
                    id: over
                    visible: row.dragging
                    implicitWidth: 40; implicitHeight: 40
                    DropArea {
                        id: drop
                        anchors.fill: parent
                        property bool will: false
                        onEntered: will = true
                        onExited: will = false
                    }
                }
                Repeater {
                    id: rep
                    model: 2
                    delegate: Rectangle {
                        id: b
                        implicitWidth: 40; implicitHeight: 40
                        property real initialX
                        MouseArea {
                            id: area
                            anchors.fill: parent
                            drag.target: parent
                            drag.axis: Drag.XAxis
                            drag.threshold: 2
                            onPressed: event => {
                                b.Drag.hotSpot.x = event.x
                                b.initialX = b.x
                                row.dragging = true
                                b.Drag.active = true
                                host.log += "p" + b.x + " "
                            }
                            onPositionChanged: host.log += "m" + b.x + " "
                            onReleased: {
                                host.log += "r" + area.drag.active + "," + b.x + "," + drop.containsDrag + "," + drop.will + " "
                                if (area.drag.active)
                                    b.x = b.initialX
                                b.Drag.active = false
                                row.dragging = false
                                host.log += "e" + b.x + " "
                            }
                        }
                    }
                }
            }
        }"#,
    );
    // A frame after every event, as the shell lays a window out.
    let frame = |rt: &mut Runtime| {
        rt.settle();
        crate::scene::layout_window(rt, &mut crate::scene::Fonts::default(), root, [400.0, 60.0]);
        rt.settle();
    };
    frame(&mut rt);
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 120.0, 20.0);
    p.motion(&mut rt, root, 120.0, 20.0);
    p.press(&mut rt, BTN_LEFT);
    frame(&mut rt);
    for i in 1..=20 {
        p.motion(&mut rt, root, 120.0 - 3.0 * f64::from(i), 20.0);
        frame(&mut rt);
    }
    p.release(&mut rt, BTN_LEFT);
    frame(&mut rt);
    // qt_drag.py main.qml 120 20 60 20 20, Qt 6.12.0.
    assert_eq!(
        log(&rt, root),
        "p0 m40 m37 m34 m31 m28 m25 m22 m19 m16 m13 m10 m7 m4 m1 m-2 m-5 m-8 m-11 m-14 m-17 \
         rtrue,-17,true,true e0 "
    );
    let xs = rt.call_method(root, "xs", vec![]).unwrap().unwrap();
    assert_eq!(xs.to_js_string(), "false:0,0,40");
}

/// QQuickMouseArea::mouseMoveEvent maps the press, the pointer and the target's start into the
/// target's parent. Under parents at a fraction of a pixel the mapped points round, and a
/// pointer exactly the threshold away is past it one way and not the other.
#[test]
fn a_drag_under_parents_at_a_fraction_of_a_pixel_starts_where_qt_starts_it() {
    let qml = r#"import QtQuick
        Item {
            id: host
            width: 400; height: 120
            function xs() { return a.x + "," + b.x }
            component Spot: Rectangle {
                width: 40; height: 30
                MouseArea {
                    anchors.fill: parent
                    drag.target: parent
                    drag.axis: Drag.XAxis
                }
            }
            Item { x: 0.1; Item { x: 0.2; Spot { id: a; x: 100 } } }
            Item { x: 0.3; y: 40; Item { x: 0.4; Spot { id: b; x: 100 } } }
        }"#;
    // qt_drag.py main.qml 120 <y> <to> <y> 20, Qt 6.12.0: one run a row.
    let rows = [
        (10.0, 100.0, "89.99999999999999,100"),
        (10.0, 140.0, "109,100"),
        (50.0, 100.0, "100,91.00000000000001"),
        (50.0, 140.0, "100,110"),
    ];
    for (y, to, xs) in rows {
        let (mut rt, root) = load("fracdrag", qml);
        let frame = |rt: &mut Runtime| {
            rt.settle();
            let mut fonts = crate::scene::Fonts::default();
            crate::scene::layout_window(rt, &mut fonts, root, [400.0, 120.0]);
            rt.settle();
        };
        frame(&mut rt);
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 120.0, y);
        p.motion(&mut rt, root, 120.0, y);
        p.press(&mut rt, BTN_LEFT);
        frame(&mut rt);
        for i in 1..=20 {
            p.motion(&mut rt, root, 120.0 + (to - 120.0) / 20.0 * f64::from(i), y);
            frame(&mut rt);
        }
        p.release(&mut rt, BTN_LEFT);
        frame(&mut rt);
        let got = rt.call_method(root, "xs", vec![]).unwrap().unwrap();
        assert_eq!(got.to_js_string(), xs, "to {to} at y {y}");
    }
}

/// QQuickDragAttached::hotSpot is a QPointF: the point a script wrote a field of, the one a
/// binding gave whole and the one whose fields were bound one by one are all where a DropArea
/// sees the drag.
#[test]
fn a_drag_hot_spot_written_by_field_or_bound_whole_is_where_the_drop_area_sees_the_drag() {
    let qml = r#"import QtQuick
        Item {
            id: host
            width: 400; height: 120
            property string log: ""
            function xs() { return a.x + "," + b.x + "," + c.x }
            component Spot: Rectangle {
                id: r
                property string name
                property bool byScript: false
                property bool whole: true
                x: 100; width: 40; height: 30
                MouseArea {
                    id: area
                    anchors.fill: parent
                    drag.target: parent
                    drag.axis: Drag.XAxis
                    drag.threshold: 2
                    onPressed: event => {
                        if (r.byScript) {
                            r.Drag.hotSpot.x = event.x
                            r.Drag.hotSpot.y = event.y
                        }
                        r.Drag.active = true
                        host.log += r.name + " p" + (r.whole ? r.Drag.hotSpot + " " : "") + r.Drag.hotSpot.x + "," + r.Drag.hotSpot.y + " "
                    }
                    onReleased: {
                        host.log += "r" + r.x + " "
                        r.Drag.active = false
                    }
                }
            }
            component Target: DropArea {
                property string name
                width: 80; height: 30
                onEntered: drag => host.log += name + " in" + drag.x + "," + drag.y + " "
                onPositionChanged: drag => host.log += "at" + drag.x + "," + drag.y + " "
                onExited: host.log += "out "
            }
            Target { name: "A"; y: 0 }
            Spot { id: a; name: "a"; y: 0; byScript: true }
            Target { name: "B"; y: 40 }
            Spot { id: b; name: "b"; y: 40; Drag.hotSpot: Qt.point(30, 5) }
            Target { name: "C"; y: 80 }
            // No whole value in the log here: with its fields bound one by one the engine reads
            // Drag.hotSpot as an object, Qt as QPointF(30, 5).
            Spot { id: c; name: "c"; y: 80; whole: false; Drag.hotSpot.x: 30; Drag.hotSpot.y: 5 }
        }"#;
    // qt_drag.py main.qml 120 <y> 60 <y> 20, Qt 6.12.0: one run a row.
    let rows = [
        (
            10.0,
            "a pQPointF(20, 10) 20,10 A in78,10 at75,10 at72,10 at69,10 at66,10 at63,10 r43 out ",
            "43,100,100",
        ),
        (
            50.0,
            "b pQPointF(30, 5) 30,5 B in79,5 at76,5 at73,5 r43 out ",
            "100,43,100",
        ),
        (
            90.0,
            "c p30,5 C in79,5 at76,5 at73,5 r43 out ",
            "100,100,43",
        ),
    ];
    for (y, want, xs) in rows {
        let (mut rt, root) = load("hotspot", qml);
        let frame = |rt: &mut Runtime| {
            rt.settle();
            let mut fonts = crate::scene::Fonts::default();
            crate::scene::layout_window(rt, &mut fonts, root, [400.0, 120.0]);
            rt.settle();
        };
        frame(&mut rt);
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 120.0, y);
        p.motion(&mut rt, root, 120.0, y);
        p.press(&mut rt, BTN_LEFT);
        frame(&mut rt);
        for i in 1..=20 {
            p.motion(&mut rt, root, 120.0 - 3.0 * f64::from(i), y);
            frame(&mut rt);
        }
        p.release(&mut rt, BTN_LEFT);
        frame(&mut rt);
        assert_eq!(log(&rt, root), want);
        let got = rt.call_method(root, "xs", vec![]).unwrap().unwrap();
        assert_eq!(got.to_js_string(), xs);
    }
}

#[test]
fn drag_handler_without_target_activates() {
    let (mut rt, root) = load(
        "draghandler",
        r#"import QtQuick
        Item {
            id: root
            width: 400; height: 200
            property string log: ""
            property real px: dh.persistentTranslation.x
            Rectangle {
                width: 80; height: 80
                DragHandler {
                    id: dh
                    xAxis.enabled: true
                    yAxis.enabled: false
                    target: null
                    onActiveChanged: root.log += active + " " + Math.round(translation.x) + " "
                }
            }
        }"#,
    );
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 40.0, 40.0);
    drag(&mut p, &mut rt, root, (40.0, 40.0), (90.0, 40.0));
    assert_eq!(log(&rt, root), "true 0 false 0 ");
    assert_eq!(rt.get(root, "px").to_number(), 50.0);
}

#[test]
fn wheel_handler_gets_every_notch() {
    let (mut rt, root) = load(
        "wheelhandler",
        r#"import QtQuick
        Item {
            id: root
            width: 200; height: 200
            property string log: ""
            Rectangle {
                width: 100; height: 100
                WheelHandler {
                    id: wh
                    acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                    onWheel: event => root.log += event.angleDelta.y + " "
                }
            }
            property real rot: wh.rotation
        }"#,
    );
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 50.0, 50.0);
    p.motion(&mut rt, root, 50.0, 50.0);
    p.wheel(&mut rt, (0.0, -120.0), (0.0, 0.0), (0.0, 0.0));
    p.wheel(&mut rt, (0.0, -120.0), (0.0, 0.0), (60.0, 60.0));
    assert_eq!(log(&rt, root), "-120 -120 ");
    assert_eq!(rt.get(root, "rot").to_number(), -30.0);
}

/// QQuickWheelHandler::wantsPointerEvent: a handler for the default devices (Mouse) takes no
/// event of a touchpad, known by the axis source. A slow finger frame moves less than half a
/// pixel, so its pixel delta rounds to zero like a wheel's.
#[test]
fn slow_touchpad_frame_is_not_a_mouse_wheel() {
    let (mut rt, root) = load(
        "wheel-source",
        r#"import QtQuick
        Item {
            id: root
            width: 200; height: 200
            property string log: ""
            Rectangle {
                width: 100; height: 100
                WheelHandler { onWheel: event => root.log += "mouse:" + event.angleDelta.y + " " }
                WheelHandler {
                    acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                    onWheel: event => root.log += "any:" + event.angleDelta.y + " "
                }
            }
        }"#,
    );
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 50.0, 50.0);
    p.motion(&mut rt, root, 50.0, 50.0);
    // wl_pointer.axis 0.3 from a finger: angleDelta qRound(-0.3 * 12), pixelDelta qRound(0.3).
    let mut deltas = WheelDeltas::default();
    let (angle, pixel) = deltas.frame((0, 0), (0.0, 0.3), false).unwrap();
    assert_eq!((angle, pixel), ((0.0, -4.0), (0.0, 0.0)));
    let finger = Axis {
        angle,
        pixel,
        time: (0.0, 0.0),
        synthesized: true,
        inverted: false,
    };
    p.wheel_axis(&mut rt, &finger);
    assert_eq!(log(&rt, root), "any:-4 ");
    // The same deltas from a wheel reach both handlers.
    rt.set(root, "log", Value::Str("".into()));
    p.wheel(&mut rt, angle, pixel, (60.0, 60.0));
    let mut got: Vec<String> = (log(&rt, root).split_whitespace().map(str::to_owned)).collect();
    got.sort();
    assert_eq!(got, ["any:-4", "mouse:-4"]);
}

/// QWheelEvent::inverted() comes from `wl_pointer.axis_relative_direction`; a WheelHandler
/// with `invertible: false` turns its rotation back (QQuickWheelHandler::handleEventPoint).
#[test]
fn inverted_axis_reaches_wheel_events() {
    let (mut rt, root) = load(
        "wheel-inverted",
        r#"import QtQuick
        Item {
            id: root
            width: 200; height: 200
            property string log: ""
            property real rot: wh.rotation
            property real fixedRot: fixed.rotation
            MouseArea {
                width: 100; height: 100
                onWheel: wheel => { root.log += "area:" + wheel.inverted + " "; wheel.accepted = false }
                WheelHandler {
                    id: wh
                    blocking: false
                    onWheel: event => root.log += "handler:" + event.inverted + " "
                }
                WheelHandler { id: fixed; blocking: false; invertible: false }
            }
        }"#,
    );
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 50.0, 50.0);
    p.motion(&mut rt, root, 50.0, 50.0);
    let mut a = Axis {
        angle: (0.0, -120.0),
        pixel: (0.0, 0.0),
        time: (0.0, 0.0),
        synthesized: false,
        inverted: true,
    };
    p.wheel_axis(&mut rt, &a);
    assert_eq!(log(&rt, root), "handler:true area:true ");
    assert_eq!(rt.get(root, "rot").to_number(), -15.0);
    assert_eq!(rt.get(root, "fixedRot").to_number(), 15.0);
    a.inverted = false;
    a.time = (60.0, 60.0);
    p.wheel_axis(&mut rt, &a);
    assert_eq!(
        log(&rt, root),
        "handler:true area:true handler:false area:false "
    );
    assert_eq!(rt.get(root, "fixedRot").to_number(), 0.0);
}

/// QQuickWheelHandler stays `active` for `activeTimeout` after its last wheel event: the end
/// of a scroll is the absence of events, so only the clock can tell it. A handler whose time
/// ran out goes inactive at the next tick; one with time left stays active and is the next
/// deadline.
#[test]
fn wheel_handler_goes_inactive_when_its_timeout_runs_out() {
    let (mut rt, root) = load(
        "wheelhandler_timeout",
        r#"import QtQuick
        Item {
            id: root
            width: 200; height: 200
            Rectangle {
                width: 100; height: 100
                WheelHandler { id: brief; activeTimeout: 0 }
            }
            Rectangle {
                y: 100; width: 100; height: 100
                WheelHandler { id: long; activeTimeout: 3600 }
            }
            property bool briefActive: brief.active
            property bool longActive: long.active
        }"#,
    );
    let mut p = Pointer::default();
    assert_eq!(p.hold_due(&rt), None, "no wheel event: no deadline");
    p.enter(&mut rt, root, 50.0, 50.0);
    p.motion(&mut rt, root, 50.0, 50.0);
    p.wheel(&mut rt, (0.0, -120.0), (0.0, 0.0), (0.0, 0.0));
    p.motion(&mut rt, root, 50.0, 150.0);
    p.wheel(&mut rt, (0.0, -120.0), (0.0, 0.0), (0.0, 0.0));
    assert!(rt.get(root, "briefActive").truthy());
    assert!(rt.get(root, "longActive").truthy());
    assert_eq!(p.hold_due(&rt), Some(0.0), "the brief one is due now");
    p.tick(&mut rt);
    assert!(!rt.get(root, "briefActive").truthy());
    assert!(rt.get(root, "longActive").truthy());
    let left = p.hold_due(&rt).expect("the long one is still waited for");
    assert!(left > 3_000_000.0 && left <= 3_600_000.0, "{left}");
}

/// A press at `from`, 12 moves 16 ms apart to `to`, release; the clock drives running flicks.
fn timed_drag(p: &mut Pointer, rt: &mut Runtime, root: usize, from: (f64, f64), to: (f64, f64)) {
    p.time = 1000.0;
    rt.tick(p.time);
    p.enter(rt, root, from.0, from.1);
    p.motion(rt, root, from.0, from.1);
    p.press(rt, BTN_LEFT);
    for i in 1..=12 {
        let f = i as f64 / 12.0;
        p.time += 16.0;
        rt.tick(p.time);
        p.motion(
            rt,
            root,
            from.0 + (to.0 - from.0) * f,
            from.1 + (to.1 - from.1) * f,
        );
    }
    p.release(rt, BTN_LEFT);
    while rt.tick(p.time) {
        p.time += 16.0;
    }
    rt.settle();
}

fn hlist(name: &str, list: &str, area: &str) -> (Runtime, usize, usize) {
    let (rt, root) = load(
        name,
        &format!(
            r#"import QtQuick
        Item {{
            id: win
            width: 400; height: 200
            property string log: ""
            ListView {{
                width: 300; height: 100
                orientation: ListView.Horizontal
                {list}
                model: 10
                delegate: Rectangle {{
                    width: 100; height: 100
                    MouseArea {{
                        anchors.fill: parent
                        {area}
                        onPressed: win.log += "p"
                        onCanceled: win.log += "x"
                        onReleased: win.log += "r"
                        onClicked: win.log += "c"
                    }}
                }}
            }}
        }}"#
        ),
    );
    let mut rt = rt;
    crate::scene::layout_window(
        &mut rt,
        &mut crate::scene::Fonts::default(),
        root,
        [400.0, 200.0],
    );
    let lv = rt.world.borrow().items[root].children[0];
    (rt, root, lv)
}

/// QQuickFlickable takes the grab from a delegate MouseArea once the drag passes the threshold
/// along the view's (horizontal) axis: the MouseArea is canceled, never clicked, and the release
/// flicks contentX on.
#[test]
fn horizontal_list_drag_steals_from_delegate_and_flicks() {
    let (mut rt, root, lv) = hlist("hsteal", "", "");
    let mut p = Pointer::default();
    timed_drag(&mut p, &mut rt, root, (250.0, 50.0), (100.0, 50.0));
    assert_eq!(log(&rt, root), "px");
    assert!(rt.get(lv, "contentX").to_number() > 150.0);
    assert_eq!(rt.get(lv, "contentY").to_number(), 0.0);
    assert!(!rt.get(lv, "moving").truthy());
}

/// A move within the drag threshold leaves the press with the delegate, which clicks.
#[test]
fn list_drag_below_threshold_clicks_delegate() {
    let (mut rt, root, lv) = hlist("hsmall", "", "");
    let mut p = Pointer::default();
    timed_drag(&mut p, &mut rt, root, (250.0, 50.0), (242.0, 50.0));
    assert_eq!(log(&rt, root), "prc");
    assert_eq!(rt.get(lv, "contentX").to_number(), 0.0);
}

/// A drag across the view's axis does not move a horizontal ListView.
#[test]
fn horizontal_list_ignores_vertical_drag() {
    let (mut rt, root, lv) = hlist("hcross", "", "");
    let mut p = Pointer::default();
    timed_drag(&mut p, &mut rt, root, (250.0, 90.0), (250.0, 10.0));
    assert_eq!(rt.get(lv, "contentX").to_number(), 0.0);
    assert_eq!(rt.get(lv, "contentY").to_number(), 0.0);
}

/// interactive: false views ignore the drag; the delegate keeps the press and clicks.
#[test]
fn non_interactive_list_does_not_drag() {
    let (mut rt, root, lv) = hlist("hnonint", "interactive: false", "");
    let mut p = Pointer::default();
    timed_drag(&mut p, &mut rt, root, (250.0, 50.0), (200.0, 50.0));
    assert_eq!(log(&rt, root), "prc");
    assert_eq!(rt.get(lv, "contentX").to_number(), 0.0);
}

/// MouseArea.preventStealing keeps the grab: the view does not move.
#[test]
fn prevent_stealing_keeps_the_grab_from_the_list() {
    let (mut rt, root, lv) = hlist("hprevent", "", "preventStealing: true");
    let mut p = Pointer::default();
    timed_drag(&mut p, &mut rt, root, (250.0, 50.0), (100.0, 50.0));
    assert_eq!(rt.get(lv, "contentX").to_number(), 0.0);
    assert!(log(&rt, root).starts_with("pr"));
}

/// StopAtBounds: dragging past the start does not overshoot.
#[test]
fn stop_at_bounds_list_does_not_overshoot() {
    let (mut rt, root, lv) = hlist("hbounds", "boundsBehavior: Flickable.StopAtBounds", "");
    let mut p = Pointer {
        time: 1000.0,
        ..Default::default()
    };
    p.enter(&mut rt, root, 50.0, 50.0);
    p.motion(&mut rt, root, 50.0, 50.0);
    p.press(&mut rt, BTN_LEFT);
    for i in 1..=10 {
        p.time += 16.0;
        rt.tick(p.time);
        p.motion(&mut rt, root, 50.0 + 15.0 * i as f64, 50.0);
        assert_eq!(rt.get(lv, "contentX").to_number(), 0.0);
    }
    p.release(&mut rt, BTN_LEFT);
}

/// Steps the released swipe's animation one 16 ms frame of world time at a time.
fn run_swipe_anims(p: &mut Pointer, rt: &mut Runtime) {
    for _ in 0..200 {
        if p.swipe_anims.is_empty() {
            break;
        }
        p.time += 16.0;
        rt.tick(p.time);
        p.tick(rt);
        rt.settle();
    }
    assert!(p.swipe_anims.is_empty());
}

fn swipe_view(name: &str, extra: &str) -> (Runtime, usize, usize) {
    let (mut rt, root) = load(
        name,
        &format!(
            r#"import QtQuick
        import QtQuick.Controls
        Item {{
            id: win
            width: 300; height: 200
            property string log: ""
            SwipeView {{
                width: 300; height: 200
                {extra}
                onCurrentIndexChanged: win.log += currentIndex
                Repeater {{
                    model: 3
                    Item {{ }}
                }}
            }}
        }}"#
        ),
    );
    crate::scene::layout_window(
        &mut rt,
        &mut crate::scene::Fonts::default(),
        root,
        [300.0, 200.0],
    );
    let sv = rt.world.borrow().items[root].children[0];
    (rt, root, sv)
}

/// A quick swipe past the drag threshold snaps to the next page (SnapOneItem +
/// StrictlyEnforceRange), never further.
#[test]
fn swipe_view_drag_snaps_one_page() {
    let (mut rt, root, sv) = swipe_view("swipe1", "");
    let mut p = Pointer::default();
    timed_drag(&mut p, &mut rt, root, (250.0, 100.0), (20.0, 100.0));
    run_swipe_anims(&mut p, &mut rt);
    assert_eq!(rt.get(sv, "currentIndex").to_number(), 1.0);
    // The first page became current when the Repeater made it.
    assert_eq!(log(&rt, root), "01");
}

/// The pages settling after a release repaint their window at every step, also when nothing
/// else in it changes (`touched` is what the host's dirty pass reads). They used to stay where
/// the release left them until something else drew a frame.
#[test]
fn released_swipe_repaints_at_every_step() {
    let (mut rt, root, sv) = swipe_view("swipe_repaint", "");
    let mut p = Pointer::default();
    timed_drag(&mut p, &mut rt, root, (250.0, 100.0), (20.0, 100.0));
    let mut steps = 0;
    while !p.swipe_anims.is_empty() {
        rt.world.borrow_mut().touched.clear();
        p.time += 16.0;
        rt.tick(p.time);
        p.tick(&mut rt);
        rt.settle();
        assert!(rt.world.borrow().touched.contains(&sv), "step {steps}");
        steps += 1;
    }
    assert!(steps > 1, "{steps}");
    assert_eq!(rt.get(sv, "currentIndex").to_number(), 1.0);
}

/// interactive: false SwipeViews stay on their page.
#[test]
fn non_interactive_swipe_view_does_not_swipe() {
    let (mut rt, root, sv) = swipe_view("swipe2", "interactive: false");
    let mut p = Pointer::default();
    timed_drag(&mut p, &mut rt, root, (250.0, 100.0), (20.0, 100.0));
    run_swipe_anims(&mut p, &mut rt);
    assert_eq!(rt.get(sv, "currentIndex").to_number(), 0.0);
    assert_eq!(log(&rt, root), "0");
}

/// A horizontal drag of `dx` in `steps` motions `dt` ms apart, held still `pause` ms before release.
fn paced_swipe(rt: &mut Runtime, root: usize, dx: f64, steps: usize, dt: f64, pause: f64) {
    let mut p = Pointer {
        time: 1000.0,
        ..Default::default()
    };
    rt.tick(p.time);
    p.enter(rt, root, 250.0, 100.0);
    p.motion(rt, root, 250.0, 100.0);
    p.press(rt, BTN_LEFT);
    for i in 1..=steps {
        p.time += dt;
        rt.tick(p.time);
        p.motion(rt, root, 250.0 + dx * i as f64 / steps as f64, 100.0);
    }
    p.time += pause;
    rt.tick(p.time);
    p.release(rt, BTN_LEFT);
    while rt.tick(p.time) {
        p.time += 16.0;
    }
    rt.settle();
    run_swipe_anims(&mut p, rt);
}

/// A slow 60 px drag still moving at release flicks to the next page (100 px/s is above
/// minimumFlickVelocity), as Qt does in the left sidebar; held still 200 ms first it snaps back.
#[test]
fn slow_swipe_flicks_unless_held() {
    let (mut rt, root, sv) = swipe_view("swipe3", "");
    paced_swipe(&mut rt, root, -60.0, 36, 1000.0 / 60.0, 0.0);
    assert_eq!(rt.get(sv, "currentIndex").to_number(), 1.0);
    let (mut rt, root, sv) = swipe_view("swipe4", "");
    paced_swipe(&mut rt, root, -60.0, 36, 1000.0 / 60.0, 200.0);
    assert_eq!(rt.get(sv, "currentIndex").to_number(), 0.0);
}

/// DragOverBounds (StyledListView): pulling down at the top drags the content past its start
/// at half the pointer's speed, and the release returns it to the bounds.
#[test]
fn drag_over_bounds_list_follows_past_the_top() {
    let (mut rt, root) = load(
        "vover",
        r#"import QtQuick
        Item {
            id: win
            width: 200; height: 300
            ListView {
                width: 200; height: 200
                boundsBehavior: Flickable.DragOverBounds
                property real scrollTargetY: 0
                Behavior on contentY { NumberAnimation { id: anim; duration: 200 } }
                onContentYChanged: if (!anim.running) scrollTargetY = contentY
                model: 20
                delegate: Rectangle { width: 200; height: 40; MouseArea { anchors.fill: parent } }
            }
        }"#,
    );
    crate::scene::layout_window(
        &mut rt,
        &mut crate::scene::Fonts::default(),
        root,
        [200.0, 300.0],
    );
    let lv = rt.world.borrow().items[root].children[0];
    let mut p = Pointer {
        time: 1000.0,
        ..Default::default()
    };
    rt.tick(p.time);
    p.enter(&mut rt, root, 100.0, 20.0);
    p.motion(&mut rt, root, 100.0, 20.0);
    p.press(&mut rt, BTN_LEFT);
    for i in 1..=20 {
        p.time += 16.0;
        rt.tick(p.time);
        p.motion(&mut rt, root, 100.0, 20.0 + 5.0 * i as f64);
    }
    let held = rt.get(lv, "contentY").to_number();
    assert!(held < -30.0 && held > -60.0, "{held}");
    p.time += 300.0;
    rt.tick(p.time);
    p.release(&mut rt, BTN_LEFT);
    for _ in 0..200 {
        p.time += 16.0;
        rt.tick(p.time);
    }
    rt.settle();
    assert_eq!(rt.get(lv, "contentY").to_number(), 0.0);
}

#[test]
fn drag_over_bounds_radio_list_follows_past_the_top() {
    let (mut rt, root) = load(
        "vover2",
        r#"import QtQuick
        import QtQuick.Controls
        Item {
            id: win
            width: 200; height: 300
            ListView {
                width: 200; height: 200
                boundsBehavior: Flickable.DragOverBounds
                property real scrollTargetY: 0
                Behavior on contentY { NumberAnimation { id: anim; duration: 200 } }
                onContentYChanged: if (!anim.running) scrollTargetY = contentY
                model: 20
                spacing: 6
                delegate: RadioButton { anchors.left: parent?.left; anchors.right: parent?.right; text: "r" + index; checked: index === 0 }
            }
        }"#,
    );
    crate::scene::layout_window(
        &mut rt,
        &mut crate::scene::Fonts::default(),
        root,
        [200.0, 300.0],
    );
    let lv = rt.world.borrow().items[root].children[0];
    let mut p = Pointer {
        time: 1000.0,
        ..Default::default()
    };
    rt.tick(p.time);
    p.enter(&mut rt, root, 100.0, 20.0);
    p.motion(&mut rt, root, 100.0, 20.0);
    p.press(&mut rt, BTN_LEFT);
    for i in 1..=20 {
        p.time += 16.0;
        rt.tick(p.time);
        p.motion(&mut rt, root, 100.0, 20.0 + 5.0 * i as f64);
    }
    let held = rt.get(lv, "contentY").to_number();
    assert!(held < -30.0 && held > -60.0, "{held}");
    p.time += 300.0;
    rt.tick(p.time);
    p.release(&mut rt, BTN_LEFT);
    for _ in 0..200 {
        p.time += 16.0;
        rt.tick(p.time);
    }
    rt.settle();
    assert_eq!(rt.get(lv, "contentY").to_number(), 0.0);
}

#[test]
fn drag_over_bounds_pointing_list_follows_past_the_top() {
    let (mut rt, root) = load(
        "vover3",
        r#"import QtQuick
        import QtQuick.Controls
        Item {
            id: win
            width: 200; height: 300
            ListView {
                width: 200; height: 200
                boundsBehavior: Flickable.DragOverBounds
                property real scrollTargetY: 0
                Behavior on contentY { NumberAnimation { id: anim; duration: 200 } }
                onContentYChanged: if (!anim.running) scrollTargetY = contentY
                spacing: 6
                model: ["a","b","c","d","e","f","g","h","i","j","k","l","m","n","o","p"]
                delegate: RadioButton { required property var modelData; required property int index; padding: 4; anchors.left: parent?.left; anchors.right: parent?.right; text: modelData; checked: index === 0
                    MouseArea { anchors.fill: parent; onPressed: (mouse) => mouse.accepted = false; cursorShape: Qt.PointingHandCursor }
                    indicator: Item {} }
            }
        }"#,
    );
    crate::scene::layout_window(
        &mut rt,
        &mut crate::scene::Fonts::default(),
        root,
        [200.0, 300.0],
    );
    let lv = rt.world.borrow().items[root].children[0];
    let mut p = Pointer {
        time: 1000.0,
        ..Default::default()
    };
    rt.tick(p.time);
    p.enter(&mut rt, root, 100.0, 20.0);
    p.motion(&mut rt, root, 100.0, 20.0);
    p.press(&mut rt, BTN_LEFT);
    for i in 1..=20 {
        p.time += 16.0;
        rt.tick(p.time);
        p.motion(&mut rt, root, 100.0, 20.0 + 5.0 * i as f64);
    }
    let held = rt.get(lv, "contentY").to_number();
    assert!(held < -30.0 && held > -60.0, "{held}");
    p.time += 300.0;
    rt.tick(p.time);
    p.release(&mut rt, BTN_LEFT);
    for _ in 0..200 {
        p.time += 16.0;
        rt.tick(p.time);
    }
    rt.settle();
    assert_eq!(rt.get(lv, "contentY").to_number(), 0.0);
}

/// A press in the spacing between delegates goes to the ListView, not to a MouseArea behind
/// it (SelectionDialog background), so pulling at the top still overshoots.
#[test]
fn press_between_delegates_stays_with_the_list() {
    let (mut rt, root) = load(
        "vgap",
        r#"import QtQuick
        import QtQuick.Controls
        Item {
            id: win
            width: 200; height: 300
            property int bg: 0
            MouseArea { anchors.fill: parent; onPressed: win.bg++ }
            ListView {
                width: 200; height: 200
                boundsBehavior: Flickable.DragOverBounds
                spacing: 20
                model: ["a","b","c","d","e","f","g","h","i","j","k","l","m","n","o","p"]
                delegate: RadioButton { required property var modelData; padding: 4; anchors.left: parent?.left; anchors.right: parent?.right; text: modelData; height: 20 }
            }
        }"#,
    );
    crate::scene::layout_window(
        &mut rt,
        &mut crate::scene::Fonts::default(),
        root,
        [200.0, 300.0],
    );
    let lv = rt.world.borrow().items[root].children[1];
    let mut p = Pointer {
        time: 1000.0,
        ..Default::default()
    };
    rt.tick(p.time);
    p.enter(&mut rt, root, 100.0, 30.0);
    p.motion(&mut rt, root, 100.0, 30.0);
    p.press(&mut rt, BTN_LEFT);
    for i in 1..=20 {
        p.time += 16.0;
        rt.tick(p.time);
        p.motion(&mut rt, root, 100.0, 30.0 + 5.0 * i as f64);
    }
    let held = rt.get(lv, "contentY").to_number();
    assert!(held < -30.0 && held > -60.0, "{held}");
    assert_eq!(rt.get(root, "bg").to_number(), 0.0);
    p.release(&mut rt, BTN_LEFT);
}

/// Pixel of the window `root` painted at `(x, y)`.
fn painted(rt: &mut Runtime, root: usize, x: usize, y: usize) -> [u8; 4] {
    let mut fonts = crate::scene::Fonts::default();
    crate::scene::layout_window(rt, &mut fonts, root, [300.0, 200.0]);
    let mut cv = qsg::raster::Canvas::new(300, 200, [0, 0, 0, 0]);
    crate::scene::paint_window(
        rt,
        &mut fonts,
        root,
        &mut crate::scene::Target::Cpu(&mut cv),
    );
    cv.px[y * 300 + x]
}

/// A layered (OpacityMask) SwipeView repaints its pages while a held drag moves them and
/// after they settle (SidebarLeftContent).
#[test]
fn layered_swipe_view_repaints_while_held() {
    let (mut rt, root) = load(
        "swipelayer",
        r##"import QtQuick
        import QtQuick.Controls
        import Qt5Compat.GraphicalEffects
        Item {
            id: win
            width: 300; height: 200
            SwipeView {
                id: sv
                width: 300; height: 200
                layer.enabled: true
                layer.effect: OpacityMask { maskSource: Rectangle { width: sv.width; height: sv.height; radius: 4 } }
                Rectangle { color: "#ff0000" }
                Rectangle { color: "#0000ff" }
            }
        }"##,
    );
    assert_eq!(painted(&mut rt, root, 280, 100)[0], 255);
    let mut p = Pointer {
        time: 1000.0,
        ..Default::default()
    };
    rt.tick(p.time);
    p.enter(&mut rt, root, 250.0, 100.0);
    p.motion(&mut rt, root, 250.0, 100.0);
    p.press(&mut rt, BTN_LEFT);
    for i in 1..=10 {
        p.time += 16.0;
        rt.tick(p.time);
        p.motion(&mut rt, root, 250.0 - 10.0 * i as f64, 100.0);
        painted(&mut rt, root, 0, 0);
    }
    let held = painted(&mut rt, root, 280, 100);
    assert_eq!(held[2], 255, "{held:?}");
    p.time += 16.0;
    rt.tick(p.time);
    p.release(&mut rt, BTN_LEFT);
    for _ in 0..100 {
        p.time += 16.0;
        rt.tick(p.time);
        run_swipe_anims(&mut p, &mut rt);
        painted(&mut rt, root, 0, 0);
    }
    let sv = rt.world.borrow().items[root].children[0];
    assert_eq!(rt.get(sv, "currentIndex").to_number(), 1.0);
    let settled = [
        painted(&mut rt, root, 5, 100),
        painted(&mut rt, root, 295, 100),
    ];
    assert!(
        settled.iter().all(|c| c[2] == 255 && c[0] == 0),
        "{settled:?}"
    );
}

/// SidebarLeftContent: pages from `contentChildren: [c.createObject(), ...]`, currentIndex bound
/// both ways to a tab bar; a held drag moves the pages and a release settles them on a page.
#[test]
fn content_children_swipe_view_follows_held_drag() {
    let (mut rt, root) = load(
        "swipecc",
        r#"import QtQuick
        import QtQuick.Controls
        Item {
            id: win
            width: 300; height: 200
            Item {
                id: tabBar
                property int currentIndex: swipeView.currentIndex
            }
            Rectangle {
                anchors.fill: parent
                SwipeView {
                    id: swipeView
                    anchors.fill: parent
                    spacing: 10
                    currentIndex: tabBar.currentIndex
                    clip: true
                    contentChildren: [page.createObject(), page.createObject(), page.createObject()]
                }
            }
            Component { id: page; Item { } }
        }"#,
    );
    let mut fonts = crate::scene::Fonts::default();
    crate::scene::layout_window(&mut rt, &mut fonts, root, [300.0, 200.0]);
    let sv = rt.world.borrow().items[root].children[1];
    let sv = rt.world.borrow().items[sv].children[0];
    let pages = |rt: &mut Runtime| -> Vec<f64> {
        let ps = crate::scene::kids(&mut rt.world.borrow_mut(), sv);
        ps.iter().map(|&c| rt.get(c, "x").to_number()).collect()
    };
    assert_eq!(pages(&mut rt), vec![0.0, 310.0, 620.0]);
    let mut p = Pointer {
        time: 1000.0,
        ..Default::default()
    };
    rt.tick(p.time);
    p.enter(&mut rt, root, 250.0, 100.0);
    p.motion(&mut rt, root, 250.0, 100.0);
    p.press(&mut rt, BTN_LEFT);
    for i in 1..=10 {
        p.time += 16.0;
        rt.tick(p.time);
        p.motion(&mut rt, root, 250.0 - 10.0 * i as f64, 100.0);
        local_pass(&mut rt, &mut fonts);
    }
    // The second move (20 px) crosses the threshold and steals, the third (30 px) is where
    // the pages start from (QQuickFlickablePrivate::drag), the tenth is 70 px past it.
    let held = pages(&mut rt);
    assert_eq!(held, vec![-70.0, 240.0, 550.0]);
    p.time += 16.0;
    rt.tick(p.time);
    p.release(&mut rt, BTN_LEFT);
    for _ in 0..100 {
        p.time += 16.0;
        rt.tick(p.time);
        run_swipe_anims(&mut p, &mut rt);
        local_pass(&mut rt, &mut fonts);
    }
    assert_eq!(rt.get(sv, "currentIndex").to_number(), 1.0);
    assert_eq!(pages(&mut rt), vec![-310.0, 0.0, 310.0]);
}

/// A swipe started over an empty DragOverBounds list leaves the list untouched: the SwipeView
/// steals the press, so the list stays at its top and atYEnd (AssistantChat ScrollToBottomButton).
#[test]
fn swipe_over_inner_list_leaves_it_at_rest() {
    let (mut rt, root) = load(
        "swipeinner",
        r#"import QtQuick
        import QtQuick.Controls
        Item {
            id: win
            width: 300; height: 200
            property bool moved: false
            SwipeView {
                id: sv
                anchors.fill: parent
                currentIndex: 1
                Item { }
                Item {
                    ListView {
                        id: lv
                        objectName: "lv"
                        anchors.fill: parent
                        boundsBehavior: Flickable.DragOverBounds
                        topMargin: 40
                        model: 0
                        onMovingChanged: if (moving) win.moved = true
                    }
                }
            }
        }"#,
    );
    let mut fonts = crate::scene::Fonts::default();
    crate::scene::layout_window(&mut rt, &mut fonts, root, [300.0, 200.0]);
    let sv = rt.world.borrow().items[root].children[0];
    let page = crate::scene::kids(&mut rt.world.borrow_mut(), sv)[1];
    let lv = rt.world.borrow().items[page].children[0];
    let mut p = Pointer {
        time: 1000.0,
        ..Default::default()
    };
    rt.tick(p.time);
    p.enter(&mut rt, root, 50.0, 100.0);
    p.motion(&mut rt, root, 50.0, 100.0);
    p.press(&mut rt, BTN_LEFT);
    for i in 1..=13 {
        p.time += 50.0;
        rt.tick(p.time);
        p.motion(
            &mut rt,
            root,
            50.0 + 15.0 * i as f64,
            100.0 + (i % 2) as f64,
        );
        local_pass(&mut rt, &mut fonts);
    }
    p.time += 1000.0;
    rt.tick(p.time);
    p.release(&mut rt, BTN_LEFT);
    for _ in 0..100 {
        p.time += 16.0;
        rt.tick(p.time);
        run_swipe_anims(&mut p, &mut rt);
        local_pass(&mut rt, &mut fonts);
    }
    assert_eq!(rt.get(sv, "currentIndex").to_number(), 0.0);
    // An empty view rests at -topMargin, as in Qt.
    assert_eq!(rt.get(lv, "contentY").to_number(), -40.0);
    assert!(!rt.get(root, "moved").truthy());
    crate::scene::flick_flags(&mut rt.world.borrow_mut(), lv);
    assert!(rt.get(lv, "atYBeginning").truthy() && rt.get(lv, "atYEnd").truthy());
}

/// The frame loop's local pass over the items queued as resized (App::layout without a
/// layout_rev change).
fn local_pass(rt: &mut Runtime, fonts: &mut crate::scene::Fonts) {
    let rs = std::mem::take(&mut rt.world.borrow_mut().resized);
    crate::scene::relayout_targets(&mut rt.world.borrow_mut(), fonts, &rs, 0);
    rt.settle();
}

#[test]
fn button_double_click_replaces_second_clicked_only_when_connected() {
    let qml = |handler: &str| {
        format!(
            r#"import QtQuick
            import QtQuick.Controls
            Item {{
                width: 200; height: 200
                property string log: ""
                Button {{
                    width: 100; height: 40
                    onPressed: parent.log += "p"
                    onReleased: parent.log += "r"
                    onClicked: parent.log += "c"
                    {handler}
                }}
            }}"#
        )
    };
    for (name, handler, want) in [
        (
            "btn-dclick",
            "onDoubleClicked: parent.log += \"d\"",
            "prcpdr",
        ),
        ("btn-nodclick", "", "prcprc"),
    ] {
        let (mut rt, root) = load(name, &qml(handler));
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 50.0, 20.0);
        p.motion(&mut rt, root, 50.0, 20.0);
        click(&mut p, &mut rt);
        p.time += 5.0;
        click(&mut p, &mut rt);
        assert_eq!(log(&rt, root), want, "{name}");
    }
}

#[test]
fn press_on_scrim_leaves_focus_with_the_dialog_field() {
    // To Do dialog: a click on the FAB's spot while the dialog is open lands on the scrim
    // MouseArea, so the FAB below never takes focus and typing still reaches the TextField.
    let (mut rt, root) = load(
        "scrim-focus",
        r#"import QtQuick
        import QtQuick.Controls
        Item {
            id: root
            width: 200; height: 200
            property bool show: false
            property bool fabFocus: fab.focus
            property bool inputFocus: input.focus
            Button { id: fab; x: 150; y: 150; width: 40; height: 40; onClicked: root.show = true }
            Item {
                width: 200; height: 200
                z: 9999
                visible: root.show
                MouseArea { width: 200; height: 200; preventStealing: true }
                TextField { id: input; width: 100; height: 30; focus: root.show }
            }
        }"#,
    );
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 170.0, 170.0);
    p.motion(&mut rt, root, 170.0, 170.0);
    click(&mut p, &mut rt);
    rt.settle();
    assert!(rt.get(root, "show").truthy());
    assert!(rt.get(root, "inputFocus").truthy());
    click(&mut p, &mut rt);
    rt.settle();
    assert!(!rt.get(root, "fabFocus").truthy());
    assert!(rt.get(root, "inputFocus").truthy());
}

/// Items moving under a pointer that has not moved since it entered get (un)hovered on the next
/// frame, as QQuickDeliveryAgent::flushFrameSynchronousEvents does once Enter set the position.
#[test]
fn hover_follows_items_moving_under_a_pointer_still_since_enter() {
    let (mut rt, root) = load(
        "still-rehover",
        r#"import QtQuick
        Item {
            width: 200; height: 200
            property alias a: a
            property alias b: b
            MouseArea { id: a; width: 100; height: 100; hoverEnabled: true }
            MouseArea { id: b; x: 100; width: 100; height: 100; hoverEnabled: true }
        }"#,
    );
    let (a, b) = match (rt.get(root, "a"), rt.get(root, "b")) {
        (Value::Item(a), Value::Item(b)) => (a, b),
        _ => panic!("no areas"),
    };
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 50.0, 50.0);
    assert!(rt.get(a, "containsMouse").truthy());
    // `a` slides away and `b` slides in under the pointer without any motion.
    rt.set(a, "x", Value::Num(150.0));
    rt.set(b, "x", Value::Num(0.0));
    rt.settle();
    frame(&mut p, root);
    assert!(p.rehover(&mut rt));
    assert!(!rt.get(a, "containsMouse").truthy());
    assert!(rt.get(b, "containsMouse").truthy());
}

/// A drag that starts on a TextEdit selects its text and leaves the DragHandler of an item
/// around it alone: QQuickTextEdit::mousePressEvent sets keepMouseGrab from selectByMouse, and
/// QQuickPointerHandler::approveGrabTransition does not take a grab that is kept. With
/// `selectByMouse: false` the handler takes the grab and moves its item. The numbers are Qt
/// 6.12's for the same drag.
#[test]
fn drag_on_a_text_edit_selects_and_leaves_the_drag_handler_around_it() {
    for (select, x, selected) in [("true", 20.0, "uick brown fo"), ("false", 120.0, "")] {
        let (mut rt, root) = load(
            "drag-text-edit",
            &format!(
                r#"import QtQuick
                Item {{
                    width: 400; height: 300
                    property alias card: card
                    property alias edit: edit
                    Item {{
                        id: card
                        x: 20; y: 20; width: 300; height: 100
                        DragHandler {{}}
                        TextEdit {{
                            id: edit
                            width: 300; height: 100
                            selectByMouse: {select}
                            font.family: "Noto Sans"; font.pixelSize: 16
                            text: "The quick brown fox jumps"
                        }}
                    }}
                }}"#
            ),
        );
        let Value::Item(card) = rt.get(root, "card") else {
            panic!("no card")
        };
        let Value::Item(edit) = rt.get(root, "edit") else {
            panic!("no editor")
        };
        let mut p = Pointer::default();
        drag(&mut p, &mut rt, root, (60.0, 30.0), (160.0, 34.0));
        assert_eq!(rt.get(card, "x").to_number(), x, "selectByMouse: {select}");
        let text = rt.get(edit, "selectedText").to_js_string();
        assert_eq!(text, selected, "selectByMouse: {select}");
    }
}

/// A hover MouseArea that its own DragHandler moves keeps the pointer position of the last move
/// it saw before the handler took the grab. The frame after the release delivers hover at the
/// still pointer (QQuickDeliveryAgentPrivate::flushFrameSynchronousEvents), and
/// QQuickMouseArea::hoverMoveEvent takes the new position, so the cursor follows it.
#[test]
fn frame_hover_moves_a_mouse_area_dragged_under_a_still_pointer() {
    let (mut rt, root) = load(
        "drag-rehover",
        r#"import QtQuick
        Item {
            width: 400; height: 300
            property alias a: a
            MouseArea {
                id: a
                x: 100; y: 100; width: 100; height: 60
                hoverEnabled: true
                readonly property bool edge: !(8 < mouseX && mouseX < width - 8 && 8 < mouseY && mouseY < height - 8)
                cursorShape: drag.active ? Qt.ArrowCursor : edge ? Qt.SizeFDiagCursor : Qt.ArrowCursor
                DragHandler { id: drag }
            }
        }"#,
    );
    let Value::Item(a) = rt.get(root, "a") else {
        panic!("no area")
    };
    let mut p = Pointer::default();
    // Pressed 5 px inside the resize edge; the first moves cross it before the handler drags.
    drag(&mut p, &mut rt, root, (187.0, 147.0), (223.0, 171.0));
    assert_eq!(rt.get(a, "x").to_number(), 136.0);
    frame(&mut p, root);
    assert!(p.rehover(&mut rt));
    assert_eq!(rt.get(a, "mouseX").to_number(), 87.0);
    assert_eq!(rt.get(a, "mouseY").to_number(), 47.0);
    assert_eq!(p.cursor_shape(&rt), 0);
    // QQuickMouseArea::hoverMoveEvent drops a position it already has.
    rt.set(root, "width", Value::Num(401.0));
    rt.settle();
    frame(&mut p, root);
    assert!(!p.rehover(&mut rt));
}

/// QQuickSpinBox without style items: a click on the default (Fusion) down/up indicator steps
/// the value and emits valueModified.
#[test]
fn plain_spin_box_indicator_click_steps_value() {
    let (mut rt, root) = load(
        "plainspin",
        r#"import QtQuick
        import QtQuick.Controls
        Item {
            id: win
            width: 300; height: 100
            property string log: ""
            SpinBox {
                id: spin
                x: 10; y: 10; width: 140; height: 40
                editable: true
                from: 0; to: 10; value: 5
                onValueModified: win.log += "m" + value + " "
            }
            property real v: spin.value
        }"#,
    );
    // Default (Fusion) indicators: a 16px column at the trailing edge, up over down.
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 141.0, 40.0);
    p.motion(&mut rt, root, 141.0, 40.0);
    click(&mut p, &mut rt);
    assert_eq!(log(&rt, root), "m4 ");
    assert_eq!(rt.get(root, "v").to_number(), 4.0);
    p.motion(&mut rt, root, 141.0, 20.0);
    click(&mut p, &mut rt);
    click(&mut p, &mut rt);
    assert_eq!(log(&rt, root), "m4 m5 m6 ");
    // The text field left of the indicators does not step.
    p.motion(&mut rt, root, 20.0, 30.0);
    click(&mut p, &mut rt);
    assert_eq!(rt.get(root, "v").to_number(), 6.0);
}

/// QGuiApplicationPrivate::processMouseEvent drops a mouse move that keeps the position (a
/// compositor sends one after a press): the pressed Flickable gets no velocity sample from it,
/// and the next wheel notch flicks as far as it does after a plain click.
#[test]
fn a_move_that_keeps_the_position_gives_a_pressed_flickable_no_velocity_sample() {
    fn scrolled(still_move: bool) -> f64 {
        let (mut rt, root) = load(
            "stillmove",
            r#"import QtQuick
            Item {
                width: 300; height: 300
                ListView {
                    id: lv
                    width: 200; height: 200; model: 50
                    delegate: Rectangle { width: 200; height: 40 }
                }
                property real cy: lv.contentY
            }"#,
        );
        crate::scene::layout_window(
            &mut rt,
            &mut crate::scene::Fonts::default(),
            root,
            [300.0, 300.0],
        );
        // The shell's QT_QUICK_FLICKABLE_WHEEL_DECELERATION=10000: a notch flicks at the
        // velocity the samples average to.
        let lv = rt.world.borrow_mut().of_type("ListView")[0];
        rt.world
            .borrow_mut()
            .flicks
            .entry(lv)
            .or_default()
            .wheel_decel = 10000.0;
        let mut p = Pointer {
            time: 1000.0,
            ..Default::default()
        };
        rt.tick(p.time);
        p.enter(&mut rt, root, 100.0, 160.0);
        // The pointer comes to rest from below: the point keeps a velocity that decays, never
        // zero, the way the wheel is about to flick.
        for y in [160.0, 130.0, 101.0, 100.0] {
            p.time += 16.0;
            p.motion(&mut rt, root, 100.0, y);
        }
        p.time += 50.0;
        p.press(&mut rt, BTN_LEFT);
        if still_move {
            p.time += 1.0;
            p.motion(&mut rt, root, 100.0, 100.0);
        }
        p.time += 60.0;
        p.release(&mut rt, BTN_LEFT);
        p.time += 1500.0;
        rt.tick(p.time);
        p.wheel(&mut rt, (0.0, -120.0), (0.0, 0.0), (p.time, p.time));
        while rt.tick(p.time) {
            p.time += 16.0;
        }
        rt.settle();
        rt.get(root, "cy").to_number()
    }
    let run = |still| std::thread::spawn(move || scrolled(still)).join().unwrap();
    let plain = run(false);
    assert!(
        (plain - 72.0).abs() < 0.5,
        "a notch after a plain click: {plain}"
    );
    assert_eq!(run(true), plain);
}

/// QQuickScrollView makes a Flickable child its contentItem, sized to the view: wheel notches
/// over a ScrollView's ListView scroll it like a bare ListView.
#[test]
fn wheel_scrolls_list_view_inside_scroll_view() {
    // Each layout pass memoizes per thread by item index: one fresh thread per scene.
    fn scrolled(wrap: bool) -> f64 {
        let list = r#"ListView {
                id: lv
                model: 50
                delegate: Rectangle { width: 200; height: 40 }
            }"#;
        let body = if wrap {
            format!("ScrollView {{ width: 200; height: 200; {list} }}")
        } else {
            list.replace("model: 50", "width: 200; height: 200; model: 50")
        };
        let (mut rt, root) = load(
            if wrap { "svwheel" } else { "lvwheel" },
            &format!(
                r#"import QtQuick
            import QtQuick.Controls
            Item {{
                width: 300; height: 300
                {body}
                property real cy: lv.contentY
            }}"#
            ),
        );
        crate::scene::layout_window(
            &mut rt,
            &mut crate::scene::Fonts::default(),
            root,
            [300.0, 300.0],
        );
        let mut p = Pointer {
            time: 1000.0,
            ..Default::default()
        };
        rt.tick(p.time);
        p.enter(&mut rt, root, 100.0, 100.0);
        p.motion(&mut rt, root, 100.0, 100.0);
        for _ in 0..5 {
            p.time += 50.0;
            rt.tick(p.time);
            p.wheel(&mut rt, (0.0, -120.0), (0.0, 0.0), (p.time, p.time));
        }
        while rt.tick(p.time) {
            p.time += 16.0;
        }
        rt.settle();
        rt.get(root, "cy").to_number()
    }
    let run = |wrap| std::thread::spawn(move || scrolled(wrap)).join().unwrap();
    let bare = run(false);
    assert!(bare > 0.0, "bare ListView did not scroll");
    assert_eq!(run(true), bare);
}

/// QQuickPopupPositioner with QQuickComboBox's allowVerticalFlip: a popup that would leave the
/// window below the combo opens above it; one that fits stays below.
#[test]
fn combo_box_popup_flips_above_when_clipped_below() {
    fn hit(wh: f64, combo_y: f64, py: f64) -> String {
        let (mut rt, root) = load(
            "comboflip",
            &format!(
                r#"import QtQuick
            import QtQuick.Controls
            Item {{
                id: win
                width: 300; height: {wh}
                property string log: ""
                ComboBox {{
                    id: cb
                    y: {combo_y}; width: 200; height: 40
                    popup: Popup {{
                        y: cb.height + 4
                        width: cb.width; height: 100; padding: 0
                        contentItem: MouseArea {{ onClicked: win.log += "in" }}
                    }}
                }}
                Component.onCompleted: cb.popup.open()
            }}"#
            ),
        );
        crate::scene::layout_window(
            &mut rt,
            &mut crate::scene::Fonts::default(),
            root,
            [300.0, wh],
        );
        let mut p = Pointer::default();
        p.enter(&mut rt, root, 100.0, py);
        p.motion(&mut rt, root, 100.0, py);
        click(&mut p, &mut rt);
        log(&rt, root)
    }
    let hit_in = |wh, y, py| std::thread::spawn(move || hit(wh, y, py)).join().unwrap();
    let hit = |y, py| hit_in(300.0, y, py);
    // Below 250 + 44 the popup would end at 394: mirrored to 250 + 40 - 44 - 100 = 146.
    assert_eq!(hit(250.0, 200.0), "in");
    // Fits below (54..154): no flip.
    assert_eq!(hit(10.0, 100.0), "in");
    assert_eq!(hit(10.0, 40.0), "");
    // Fits neither way (popup 100 in a 120 window at 60): moved up to end at the bottom edge.
    assert_eq!(hit_in(60.0, 120.0, 25.0), "in");
}

/// A pointer entered at (0, 0) (Hyprland's enter for a surface it is not over) gets the enter
/// hover only: items sliding in under that point stay unhovered until real motion.
#[test]
fn no_rehover_after_enter_at_origin() {
    let (mut rt, root) = load(
        "origin-rehover",
        r#"import QtQuick
        Item {
            width: 200; height: 200
            property alias a: a
            MouseArea { id: a; x: 100; width: 100; height: 100; hoverEnabled: true }
        }"#,
    );
    let Value::Item(a) = rt.get(root, "a") else {
        panic!("no area")
    };
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 0.0, 0.0);
    rt.set(a, "x", Value::Num(0.0));
    rt.settle();
    frame(&mut p, root);
    assert!(!p.rehover(&mut rt));
    assert!(!rt.get(a, "containsMouse").truthy());
    p.motion(&mut rt, root, 1.0, 1.0);
    assert!(rt.get(a, "containsMouse").truthy());
}

/// QQuickItem::mapFromScene: the pointer hits an item where its ancestors' `scale` and
/// `rotation` show it, and the event position is in the item's own coordinates.
#[test]
fn hit_testing_follows_ancestor_scale_and_rotation() {
    let (mut rt, root) = load(
        "xf-hit",
        r#"import QtQuick
        Item {
            id: root
            width: 400; height: 200
            property string log: ""
            // Shows at (50, 50) 100x100: twice the size about its center.
            Item {
                x: 75; y: 75; width: 50; height: 50
                scale: 2
                MouseArea {
                    width: 10; height: 10
                    onPressed: mouse => root.log += "scaled:" + mouse.x + "," + mouse.y + " "
                    onPositionChanged: mouse => root.log += "moved:" + mouse.x + "," + mouse.y + " "
                }
            }
            // Shows at (230, 70) 40x60, its top left corner at the top right.
            Item {
                x: 220; y: 80; width: 60; height: 40
                rotation: 90
                MouseArea {
                    width: 10; height: 10
                    onPressed: mouse => root.log += "rotated:" + mouse.x + "," + mouse.y + " "
                }
            }
        }"#,
    );
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 0.0, 0.0);
    // Where the area would be without the scale: its item now starts at (50, 50).
    p.motion(&mut rt, root, 80.0, 80.0);
    click(&mut p, &mut rt);
    assert_eq!(log(&rt, root), "");
    p.motion(&mut rt, root, 60.0, 64.0);
    p.press(&mut rt, BTN_LEFT);
    p.motion(&mut rt, root, 62.0, 68.0);
    p.release(&mut rt, BTN_LEFT);
    assert_eq!(log(&rt, root), "scaled:5,7 moved:6,9 ");
    rt.set(root, "log", Value::Str("".into()));
    // The rotated item's corner (0, 0) shows at (270, 70); 4 right and 6 down in the item
    // is 6 left and 4 down on the screen.
    p.motion(&mut rt, root, 264.0, 74.0);
    click(&mut p, &mut rt);
    assert_eq!(log(&rt, root), "rotated:4,6 ");
    // Its place without the rotation.
    rt.set(root, "log", Value::Str("".into()));
    p.motion(&mut rt, root, 224.0, 86.0);
    click(&mut p, &mut rt);
    assert_eq!(log(&rt, root), "");
}

/// QQuickControl::hovered and QQuickTextArea/QQuickTextField::hovered: true under the pointer
/// without an explicit hoverEnabled, as for buttons.
#[test]
fn combo_box_and_text_area_are_hovered() {
    let (mut rt, root) = load(
        "control-hover",
        r#"import QtQuick
        import QtQuick.Controls
        Item {
            width: 300; height: 100
            property alias combo: combo
            property alias area: area
            property alias field: field
            property alias off: off
            ComboBox { id: combo; width: 100; height: 40; model: ["a", "b"] }
            TextArea { id: area; x: 100; width: 100; height: 40 }
            TextField { id: field; x: 200; width: 100; height: 40 }
            TextArea { id: off; y: 50; width: 100; height: 40; hoverEnabled: false }
        }"#,
    );
    let item = |rt: &Runtime, name: &str| match rt.get(root, name) {
        Value::Item(i) => i,
        _ => panic!("no {name}"),
    };
    let ids = ["combo", "area", "field", "off"].map(|n| item(&rt, n));
    let hovered = |rt: &Runtime| ids.map(|i| rt.get(i, "hovered").truthy());
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 50.0, 20.0);
    assert_eq!(hovered(&rt), [true, false, false, false]);
    p.motion(&mut rt, root, 150.0, 20.0);
    assert_eq!(hovered(&rt), [false, true, false, false]);
    p.motion(&mut rt, root, 250.0, 20.0);
    assert_eq!(hovered(&rt), [false, false, true, false]);
    p.motion(&mut rt, root, 50.0, 70.0);
    assert_eq!(hovered(&rt), [false, false, false, false]);
}

/// A tap with the first finger clicks like the left button (Qt Quick's touch-to-mouse
/// synthesis); a second finger down meanwhile does nothing; lifting clears hover.
#[test]
fn first_touch_point_clicks_and_unhovers() {
    let (mut rt, root) = load(
        "touch-tap",
        r#"import QtQuick
        Item {
            width: 100; height: 100
            property string log: ""
            MouseArea {
                width: 100; height: 100
                hoverEnabled: true
                onPressed: parent.log += "p"
                onClicked: parent.log += "c"
                onContainsMouseChanged: parent.log += containsMouse ? "h" : "u"
            }
        }"#,
    );
    let mut p = Pointer::default();
    assert!(p.touch_down(&mut rt, root, 3, 10.0, 10.0));
    assert!(!p.touch_down(&mut rt, root, 4, 50.0, 50.0));
    assert!(!p.touch_up(&mut rt, 4));
    p.touch_motion(&mut rt, 3, 12.0, 11.0);
    assert!(p.touch_up(&mut rt, 3));
    rt.settle();
    assert_eq!(log(&rt, root), "hpcu");
}

/// wl_touch.cancel (a compositor gesture) cancels the press: no clicked().
#[test]
fn touch_cancel_does_not_click() {
    let (mut rt, root) = load(
        "touch-cancel",
        r#"import QtQuick
        Item {
            width: 100; height: 100
            property string log: ""
            MouseArea {
                width: 100; height: 100
                onPressed: parent.log += "p"
                onClicked: parent.log += "c"
            }
        }"#,
    );
    let mut p = Pointer::default();
    p.touch_down(&mut rt, root, 0, 10.0, 10.0);
    assert!(p.touch_cancel(&mut rt));
    assert!(!p.touch_cancel(&mut rt));
    rt.settle();
    assert_eq!(log(&rt, root), "p");
    // The next touch starts a fresh sequence.
    assert!(p.touch_down(&mut rt, root, 1, 10.0, 10.0));
    assert!(p.touch_up(&mut rt, 1));
    rt.settle();
    assert_eq!(log(&rt, root), "ppc");
}

/// The seat drops its pointer capability while a button is held (mouse unplugged): the press
/// is cancelled without a click and hover ends, as no wl_pointer.button or leave follows.
#[test]
fn pointer_capability_loss_cancels_press_and_hover() {
    let (mut rt, root) = load(
        "ptr-lost",
        r#"import QtQuick
        Item {
            width: 100; height: 100
            property string log: ""
            property alias area: ma
            MouseArea {
                id: ma
                width: 100; height: 100
                hoverEnabled: true
                onPressed: parent.log += "p"
                onClicked: parent.log += "c"
                onCanceled: parent.log += "x"
                onContainsMouseChanged: parent.log += containsMouse ? "h" : "u"
            }
        }"#,
    );
    let Value::Item(area) = rt.get(root, "area") else {
        panic!("no area")
    };
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 10.0, 10.0);
    p.press(&mut rt, BTN_LEFT);
    assert!(rt.get(area, "pressed").truthy());
    p.pointer_lost(&mut rt);
    rt.settle();
    assert!(!rt.get(area, "pressed").truthy());
    assert!(!rt.get(area, "containsMouse").truthy());
    assert!(!rt.get(area, "containsPress").truthy());
    assert_eq!(log(&rt, root), "hpxu");
    // A new pointer starts clean: its first press and release click.
    p.enter(&mut rt, root, 10.0, 10.0);
    click(&mut p, &mut rt);
    rt.settle();
    assert_eq!(log(&rt, root), "hpxuhpc");
}

/// Hover is delivered again under a still pointer at most once in 100 ms
/// (QQuickDeliveryAgentPrivate::frameSynchronousHoverInterval): a frame that comes sooner
/// leaves it to the delay timer, which delivers it when the 100 ms are over; the first frame
/// after them delivers it itself. An interval below 0 delivers none.
#[test]
fn hover_under_a_still_pointer_comes_once_in_its_interval() {
    use std::time::{Duration, Instant};
    let (mut rt, root) = load(
        "throttled-rehover",
        r#"import QtQuick
        Item {
            width: 200; height: 200
            property alias a: a
            MouseArea { id: a; x: 100; width: 100; height: 100; hoverEnabled: true }
        }"#,
    );
    let Value::Item(a) = rt.get(root, "a") else {
        panic!("no area")
    };
    let ms = Duration::from_millis;
    let t0 = Instant::now();
    let slide = |rt: &mut Runtime, x: f64| {
        rt.set(a, "x", Value::Num(x));
        rt.settle();
    };
    let over = |rt: &Runtime| rt.get(a, "containsMouse").truthy();
    let mut p = Pointer::default();
    // The window's first frame starts its interval.
    p.frame(root, t0);
    p.enter(&mut rt, root, 50.0, 50.0);
    assert_eq!(p.hover_due(t0), None);
    // 150 ms on the interval is over: the frame delivers the hover and starts it again.
    slide(&mut rt, 0.0);
    p.frame(root, t0 + ms(150));
    assert!(p.rehover(&mut rt) && over(&rt));
    assert_eq!(p.hover_due(t0 + ms(150)), None);
    // Frames inside the interval deliver nothing; the delay timer is set for its end.
    let before = p.hit_tests;
    for k in 1..=5 {
        slide(&mut rt, 100.0 + k as f64);
        p.frame(root, t0 + ms(150 + 16 * k));
        assert!(!p.rehover(&mut rt) && over(&rt), "frame {k}");
    }
    assert_eq!(p.hit_tests, before, "hit tests inside the interval");
    assert_eq!(p.hover_due(t0 + ms(230)), Some(20.0));
    // Not due yet: nothing.
    p.hover_timer(t0 + ms(249));
    assert!(!p.rehover(&mut rt) && over(&rt));
    // Due: delivered without a frame, and the interval starts again from here.
    p.hover_timer(t0 + ms(251));
    assert!(p.rehover(&mut rt) && !over(&rt));
    assert_eq!(p.hit_tests, before + 1);
    assert_eq!(p.hover_due(t0 + ms(251)), None);
    slide(&mut rt, 0.0);
    p.frame(root, t0 + ms(300));
    assert!(!p.rehover(&mut rt) && !over(&rt));
    assert_eq!(p.hover_due(t0 + ms(300)), Some(51.0));
    // A held button delivers none.
    p.press(&mut rt, BTN_LEFT);
    let held = over(&rt);
    slide(&mut rt, 100.0);
    p.hover_timer(t0 + ms(351));
    assert!(!p.rehover(&mut rt));
    assert_eq!(over(&rt), held);
    p.release(&mut rt, BTN_LEFT);
    // Below 0 no frame delivers it, however late.
    p.hover_every = Some(-1);
    let was = over(&rt);
    slide(&mut rt, if was { 100.0 } else { 0.0 });
    p.frame(root, t0 + ms(5000));
    assert!(!p.rehover(&mut rt));
    assert_eq!(over(&rt), was);
    assert_eq!(p.hover_due(t0 + ms(5000)), None);
}

/// Hover is delivered again only with a frame of the window under the pointer
/// (QQuickWindowPrivate::flushFrameSynchronousEvents runs that window's delivery agent): frames
/// of other windows, however many, hit-test nothing, and the next frame of the pointer's window
/// hit-tests once.
#[test]
fn frames_of_other_windows_do_not_hit_test_under_a_still_pointer() {
    let (mut rt, root) = load(
        "other-window-rehover",
        r#"import QtQuick
        Item {
            width: 200; height: 200
            property alias a: a
            property alias other: other
            MouseArea { id: a; x: 100; width: 100; height: 100; hoverEnabled: true }
            Item { id: other; visible: false; width: 10; height: 10 }
        }"#,
    );
    let (a, other) = match (rt.get(root, "a"), rt.get(root, "other")) {
        (Value::Item(a), Value::Item(o)) => (a, o),
        _ => panic!("no items"),
    };
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 50.0, 50.0);
    let before = p.hit_tests;
    // 20 frames of another window, each after a change of the scene.
    for k in 0..20 {
        rt.set(other, "width", Value::Num(11.0 + k as f64));
        rt.settle();
        frame(&mut p, other);
        assert!(!p.rehover(&mut rt));
    }
    assert_eq!(
        p.hit_tests - before,
        0,
        "hit tests for frames of another window"
    );
    // The area slides in under the pointer: the window's own frame delivers the hover.
    rt.set(a, "x", Value::Num(0.0));
    rt.settle();
    assert!(!p.rehover(&mut rt));
    assert!(!rt.get(a, "containsMouse").truthy());
    frame(&mut p, root);
    assert!(p.rehover(&mut rt));
    assert!(rt.get(a, "containsMouse").truthy());
    assert_eq!(
        p.hit_tests - before,
        1,
        "hit tests for the window's own frame"
    );
}

/// `enabled` of a MouseArea is QQuickMouseArea's own property: QQuickItem::isEnabled stays
/// true, so QQuickWindowPrivate::findCursorItemAndHandler still takes the area's cursor. The
/// settings rows have such an area (`enabled: root.clickable`, PointingHandCursor): Quickshell
/// shows the hand over every row. An area inside a disabled item gives no cursor.
#[test]
fn disabled_mouse_area_keeps_its_cursor() {
    let (mut rt, root) = load(
        "cursor-disabled-area",
        r#"import QtQuick
        Item {
            width: 400; height: 300
            property alias row: row
            property int clicks: 0
            MouseArea {
                id: row
                width: 400; height: 100
                enabled: false
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: parent.clicks++
            }
            Item {
                y: 100; width: 400; height: 100
                enabled: false
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor }
            }
        }"#,
    );
    let Value::Item(row) = rt.get(root, "row") else {
        panic!("no area")
    };
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 200.0, 50.0);
    p.motion(&mut rt, root, 200.0, 50.0);
    assert_eq!(p.cursor_shape(&rt), 13);
    assert!(!rt.get(row, "containsMouse").truthy());
    click(&mut p, &mut rt);
    assert_eq!(rt.get(root, "clicks").to_number(), 0.0);
    p.motion(&mut rt, root, 200.0, 150.0);
    assert_eq!(p.cursor_shape(&rt), 0);
    p.motion(&mut rt, root, 200.0, 250.0);
    assert_eq!(p.cursor_shape(&rt), 0);
}

/// QQuickSlider::mousePressEvent and QQuickSliderPrivate::handleMove: the press already moves
/// the slider; the value is the step under the pointer, the position is the pointer's own
/// (Slider.NoSnap), and stays there after the release until the value changes.
#[test]
fn pressed_slider_shows_the_cursor_of_the_area_over_it() {
    let (mut rt, root) = load(
        "slider-press-cursor",
        r#"import QtQuick
        import QtQuick.Controls
        Item {
            width: 300; height: 100
            Slider {
                id: s
                width: 200; height: 40
                MouseArea {
                    anchors.fill: parent
                    onPressed: (mouse) => mouse.accepted = false
                    cursorShape: s.pressed ? Qt.ClosedHandCursor : Qt.PointingHandCursor
                }
            }
        }"#,
    );
    rt.settle();
    crate::scene::layout_window(
        &mut rt,
        &mut crate::scene::Fonts::default(),
        root,
        [300.0, 100.0],
    );
    rt.settle();
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 50.0, 20.0);
    p.motion(&mut rt, root, 50.0, 20.0);
    assert_eq!(p.cursor_shape(&rt), 13);
    p.press(&mut rt, BTN_LEFT);
    assert_eq!(p.cursor_shape(&rt), 18);
    p.motion(&mut rt, root, 80.0, 20.0);
    assert_eq!(p.cursor_shape(&rt), 18);
    p.release(&mut rt, BTN_LEFT);
    assert_eq!(p.cursor_shape(&rt), 13);
}

#[test]
fn stepped_slider_handle_stays_under_the_pointer() {
    let (mut rt, root) = load(
        "slider-step-position",
        r#"import QtQuick
        import QtQuick.Controls
        Item {
            width: 300; height: 100
            property alias s: s
            property real v: 60
            property int moves: 0
            property string log: ""
            Slider {
                id: s
                width: 200; height: 40
                padding: 0
                handle: Item { width: 0; height: 40 }
                from: 0; to: 100; stepSize: 5
                value: parent.v
                onMoved: parent.moves++
                onValueChanged: {
                    parent.log += (pressed ? "p" : "r") + value + " "
                    if (pressed) parent.v = value
                }
            }
        }"#,
    );
    let Value::Item(s) = rt.get(root, "s") else {
        panic!("no slider")
    };
    // A frame after every event, as the shell lays a window out.
    let frame = |rt: &mut Runtime| {
        rt.settle();
        crate::scene::layout_window(
            rt,
            &mut crate::scene::Fonts::default(),
            root,
            [300.0, 100.0],
        );
        rt.settle();
    };
    frame(&mut rt);
    rt.set(root, "log", Value::Str("".into()));
    let at = |rt: &Runtime| {
        (
            rt.get(s, "value").to_number(),
            rt.get(s, "position").to_number(),
            rt.get(s, "visualPosition").to_number(),
        )
    };
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 48.0, 20.0);
    p.motion(&mut rt, root, 48.0, 20.0);
    // A press without a move.
    p.press(&mut rt, BTN_LEFT);
    frame(&mut rt);
    assert_eq!(log(&rt, root), "p25 ");
    assert_eq!(at(&rt), (25.0, 0.24, 0.24));
    assert_eq!(rt.get(root, "moves").to_number(), 1.0);
    p.release(&mut rt, BTN_LEFT);
    frame(&mut rt);
    assert_eq!(at(&rt), (25.0, 0.24, 0.24));
    assert_eq!(rt.get(root, "moves").to_number(), 1.0);
    // Within the same step the handle still follows the pointer.
    p.motion(&mut rt, root, 50.0, 20.0);
    p.press(&mut rt, BTN_LEFT);
    frame(&mut rt);
    assert_eq!(at(&rt), (25.0, 0.25, 0.25));
    p.motion(&mut rt, root, 52.0, 20.0);
    frame(&mut rt);
    assert_eq!(at(&rt), (25.0, 0.26, 0.26));
    assert_eq!(rt.get(root, "moves").to_number(), 3.0);
    p.motion(&mut rt, root, 59.0, 20.0);
    frame(&mut rt);
    assert_eq!(at(&rt), (30.0, 0.295, 0.295));
    p.release(&mut rt, BTN_LEFT);
    frame(&mut rt);
    assert_eq!(at(&rt), (30.0, 0.295, 0.295));
    assert_eq!(log(&rt, root), "p25 p30 ");
    // A value from outside puts the handle on it.
    rt.set(root, "v", Value::Num(60.0));
    frame(&mut rt);
    assert_eq!(at(&rt), (60.0, 0.6, 0.6));
    rt.set(root, "v", Value::Num(30.0));
    frame(&mut rt);
    assert_eq!(at(&rt), (30.0, 0.3, 0.3));
}

/// QQuickMouseArea::hoverEnterEvent emits positionChanged after mouseX and mouseY: an area that
/// maps under a pointer that then stays still is told where the pointer is (the region
/// selector marks the window under it from that handler).
#[test]
fn entering_pointer_tells_a_mouse_area_its_position() {
    let (mut rt, root) = load(
        "enter-position",
        r#"import QtQuick
        Item {
            width: 200; height: 200
            property string log: ""
            MouseArea {
                x: 20; y: 10; width: 100; height: 100
                hoverEnabled: true
                onPositionChanged: mouse => root.log += `${mouse.x},${mouse.y} at ${mouseX},${mouseY};`
            }
            id: root
        }"#,
    );
    let mut p = Pointer::default();
    p.enter(&mut rt, root, 70.5, 50.5);
    assert_eq!(log(&rt, root), "50.5,40.5 at 50.5,40.5;");
    // Hyprland follows the enter with a motion to the same place: no second signal.
    p.motion(&mut rt, root, 70.5, 50.5);
    assert_eq!(log(&rt, root), "50.5,40.5 at 50.5,40.5;");
}
