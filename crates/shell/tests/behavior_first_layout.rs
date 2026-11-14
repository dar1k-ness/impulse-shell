//! waffle NotificationCenterContent: `Behavior on implicitHeight` of the notification pane, whose
//! height follows the panel's ColumnLayout. The original kept the Behavior disabled for
//! `arbitraryRaceConditionDelay` ms after creation so the window's first sizing did not play as
//! a grow animation. The native runtime arms Behaviors only once an item went through its first
//! layout (shown, or laid out hidden at the predicted size), so the pane opens at its height and
//! later changes (notifications cleared or arriving) still animate, with no timer.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r##"
import QtQuick
import QtQuick.Layouts
import Rustshell
FloatingWindow {
    width: 400; height: 1000
    property var pane: p
    property int count: 3
    property bool shown: false
    ColumnLayout {
        id: col
        anchors { top: parent.top; bottom: parent.bottom; horizontalCenter: parent.horizontalCenter }
        spacing: 12
        Item {
            Layout.fillHeight: true
            implicitWidth: 300
            Rectangle {
                id: p
                anchors { bottom: parent.bottom; left: parent.left; right: parent.right }
                implicitHeight: count > 0 ? col.height - cal.height - col.spacing - 2 : 230
                Behavior on implicitHeight { NumberAnimation { duration: 200 } }
                opacity: shown ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 200 } }
            }
        }
        Rectangle { id: cal; implicitWidth: 300; implicitHeight: 400 }
    }
}
"##;

const FULL: f64 = 1000.0 - 400.0 - 12.0 - 2.0;

fn run(hidden_first: bool) {
    let mut rt = Runtime::new(None);
    rt.world.borrow_mut().track_fresh = true;
    let win = rt.load_str(QML).unwrap();
    let mut fonts = scene::Fonts::default();
    let Some(Value::Item(p)) = rt.world.borrow_mut().read(win, "pane") else {
        panic!("no pane");
    };
    let mut now = 1000.0;
    rt.tick(now);
    if hidden_first {
        scene::layout_hidden_window(&mut rt, &mut fonts, win, [400.0, 1000.0]);
        // Shown after the prelayout: a show animation still plays, only geometry waits.
        rt.set(win, "shown", Value::Bool(true));
    }
    let mut frame = |rt: &mut Runtime| {
        now += 16.0;
        rt.tick(now);
        scene::layout_window(rt, &mut fonts, win, [400.0, 1000.0]);
        rt.get(p, "implicitHeight").to_number()
    };
    // Opens at its height: no frame of a grow animation.
    let mut fades = vec![];
    for i in 0..20 {
        assert_eq!(frame(&mut rt), FULL, "frame {i}");
        fades.push(rt.get(p, "opacity").to_number());
    }
    if hidden_first {
        assert!(fades[0] < 1.0 && fades[19] == 1.0, "{fades:?}");
    }
    // Later changes animate both ways.
    for (count, to) in [(0, 230.0), (3, FULL)] {
        rt.set(win, "count", Value::Num(count as f64));
        let from = if to == FULL { 230.0 } else { FULL };
        let mid = frame(&mut rt);
        assert!(
            mid != to && (mid - from).abs() < (to - from).abs(),
            "{from} -> {to}: {mid}"
        );
        let mut seen = vec![mid];
        for _ in 0..30 {
            seen.push(frame(&mut rt));
        }
        assert_eq!(*seen.last().unwrap(), to, "{seen:?}");
        assert!(
            seen.iter().filter(|&&v| v != from && v != to).count() >= 5,
            "{seen:?}"
        );
    }
}

#[test]
fn height_behavior_does_not_animate_the_first_layout() {
    run(false);
}

#[test]
fn height_behavior_does_not_animate_after_a_hidden_prelayout() {
    run(true);
}
