//! AssistantChat's `onCountChanged: if (atYEnd) Qt.callLater(positionViewAtEnd)`: QQuickItemView
//! lays out the inserted delegates before positioning, so the view ends at the new last item's
//! bottom edge instead of the content height before the insertion, and moves there directly
//! (QQuickFlickable::setContentY), not through `Behavior on contentY`. A row that grows later
//! does not take the view along. The numbers are what Quickshell 0.3.2 printed for
//! `views_like_qt/pae2w.qml`, the same list.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r##"
import QtQuick
import Rustshell
FloatingWindow {
    width: 300; height: 200
    property var view: lv
    property var names: ["a", "b"]
    property real extra: 0
    ListView {
        id: lv
        anchors.fill: parent
        spacing: 10
        topMargin: 30
        model: ScriptModel { values: names }
        onCountChanged: if (atYEnd) Qt.callLater(positionViewAtEnd)
        Behavior on contentY { NumberAnimation { duration: 400 } }
        delegate: Rectangle { required property var modelData; width: 300; height: modelData === "f" ? 50 + extra : 50 }
    }
}
"##;

#[test]
fn position_view_at_end_after_insert_reaches_the_new_last_item() {
    let mut rt = Runtime::new(None);
    let win = rt.load_str(QML).unwrap();
    let mut fonts = scene::Fonts::default();
    let mut now = 0.0;
    let mut frame = |rt: &mut Runtime| {
        now += 16.0;
        rt.tick(now);
        scene::layout_window(rt, &mut fonts, win, [300.0, 200.0]);
    };
    for _ in 0..3 {
        frame(&mut rt);
    }
    let Some(Value::Item(lv)) = rt.world.borrow_mut().read(win, "view") else {
        panic!("no view");
    };
    let names = ["a", "b", "c", "d", "e", "f"];
    for n in 3..=names.len() {
        let v = Value::array(names[..n].iter().map(|s| Value::str(s)).collect());
        rt.set(win, "names", v);
        for _ in 0..3 {
            frame(&mut rt);
        }
        let ch = rt.get(lv, "contentHeight").to_number();
        assert_eq!(ch, n as f64 * 60.0 - 10.0);
        // Qt contentY at the end: contentHeight - height, not above -topMargin.
        let want = (ch - 200.0).max(-30.0);
        assert_eq!(rt.get(lv, "contentY").to_number(), want, "{n} items");
        assert!(rt.get(lv, "atYEnd").truthy(), "{n} items");
    }
    rt.set(win, "extra", Value::Num(40.0));
    for _ in 0..3 {
        frame(&mut rt);
    }
    assert_eq!(rt.get(lv, "contentHeight").to_number(), 390.0);
    assert_eq!(
        rt.get(lv, "contentY").to_number(),
        150.0,
        "the last row grew"
    );
    assert!(!rt.get(lv, "atYEnd").truthy());
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
