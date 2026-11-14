//! A binding that calls `createObject` is an ordinary binding in Qt: it is evaluated again
//! whenever something it read changes, also when the objects it makes have a parent. The
//! objects of an earlier evaluation that have a parent live on until the parent goes
//! (`qml/services/Brightness.qml` `monitors`, `qml/modules/rusty/overlay/Overlay.qml` `regions`).
//! Each fixture is run in Qt 6.12 and here, and the `console.log` lines must be the same.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

use qml::runtime::Runtime;

/// `Brightness.qml:23`'s shape: a typed list and a `var`, both mapped over a list that grows
/// and shrinks.
const SCREENS: &str = r#"import QtQuick
Item {
    id: root
    property var screens: ["s1", "s2"]
    Component { id: comp; QtObject { property string screen } }
    readonly property list<QtObject> monitors: screens.map(s => comp.createObject(root, { screen: s }))
    property var regions: screens.map(s => comp.createObject(root, { screen: "r-" + s }))
    onMonitorsChanged: console.log("monitorsChanged", monitors.length)
    function names() { var a = []; for (var i = 0; i < monitors.length; i++) a.push(monitors[i].screen); return a.join(",") }
    Component.onCompleted: {
        console.log("start", monitors.length, names(), regions.length);
        screens = ["s1", "s2", "s3"];
        console.log("hotplug", monitors.length, names(), regions.length);
        screens = ["s3"];
        console.log("unplug", monitors.length, names(), regions.length);
        Qt.quit();
    }
}
"#;

/// `Overlay.qml:35`'s shape: the list starts empty.
const WIDGETS: &str = r#"import QtQuick
Item {
    id: root
    property var widgets: []
    Component { id: comp; QtObject { property string item } }
    property var regions: widgets.map(w => comp.createObject(root, { item: w }))
    function names() { return regions.map(r => r.item).join(",") }
    Component.onCompleted: {
        console.log("start", regions.length);
        widgets = ["w1"]; console.log("pin 1:", regions.length, names());
        widgets = ["w1", "w2"]; console.log("pin 2:", regions.length, names());
        widgets = []; console.log("unpin all:", regions.length, names());
        Qt.quit();
    }
}
"#;

/// The same on a `QtObject` root with the component held in a property.
const QTOBJECT_ROOT: &str = r#"import QtQuick
QtObject {
    id: root
    property var screens: ["a", "b"]
    property Component comp: Component { QtObject { property string screen } }
    property var monitors: screens.map(s => comp.createObject(root, {screen: s}))
    Component.onCompleted: {
        console.log("1 =>", monitors.length, monitors.map(m => m.screen).join(","))
        screens = ["a", "b", "c"]
        console.log("2 =>", monitors.length, monitors.map(m => m.screen).join(","))
        screens = ["c"]
        console.log("3 =>", monitors.length, monitors.map(m => m.screen).join(","))
        Qt.quit()
    }
}
"#;

/// One object per evaluation (`animation: x.createObject(this)` with an input that changes):
/// the property holds the new object, the earlier ones stay alive under their parent, and
/// the binding runs once per change of an input.
const ONE_OBJECT: &str = r#"import QtQuick
Item {
    id: root
    property int duration: 100
    property int other: 0
    property int created: 0
    property int destroyed: 0
    Component {
        id: comp
        QtObject {
            property int duration
            Component.onCompleted: root.created++
            Component.onDestruction: root.destroyed++
        }
    }
    Item {
        id: holder
        property QtObject made: comp.createObject(holder, { duration: root.duration })
        onMadeChanged: console.log("madeChanged", made.duration)
    }
    Component.onCompleted: {
        var first = holder.made;
        console.log("start", holder.made.duration, "created", created);
        other = 1;
        console.log("unrelated write", holder.made === first, "created", created);
        duration = 200;
        console.log("input changed", holder.made.duration, holder.made === first, "created", created);
        console.log("first object", first.duration, "destroyed", destroyed);
        Qt.quit();
    }
}
"#;

/// A list of objects with a parent under a list property that takes them (`animations` of a
/// ParallelAnimation) and the page list of a container.
const LIST_PROPERTIES: &str = r#"import QtQuick
import QtQuick.Controls
Item {
    id: root
    property int to: 10
    property int pages: 1
    property int pagesMade: 0
    Component { id: anim; NumberAnimation {} }
    Component { id: page; Item { Component.onCompleted: root.pagesMade++ } }
    ParallelAnimation { id: par; animations: [anim.createObject(par, { to: root.to })] }
    SwipeView {
        id: view
        contentChildren: { var a = []; for (var i = 0; i < root.pages; i++) a.push(page.createObject(view)); return a }
    }
    Component.onCompleted: {
        console.log("start", par.animations.length, par.animations[0].to, "pages made", pagesMade);
        to = 20; pages = 3;
        console.log("changed", par.animations.length, par.animations[0].to, "pages made", pagesMade);
        Qt.quit();
    }
}
"#;

#[test]
fn bindings_that_create_objects_follow_their_inputs_as_in_qt() {
    if !qt_log::qt_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        ("screens.qml", SCREENS),
        ("widgets.qml", WIDGETS),
        ("qtobject_root.qml", QTOBJECT_ROOT),
        ("one_object.qml", ONE_OBJECT),
        ("list_properties.qml", LIST_PROPERTIES),
    ];
    let mut differing = Vec::new();
    for (name, src) in cases {
        let (qt, ours) = qt_log::both(dir.path(), name, src);
        assert!(!qt.is_empty(), "{name}: Qt printed nothing");
        if qt != ours {
            differing.push(format!("{name}\n  Qt:   {qt:#?}\n  ours: {ours:#?}"));
        }
    }
    println!(
        "compared with Qt 6.12: {} fixtures, {} differ",
        cases.len(),
        differing.len()
    );
    assert!(differing.is_empty(), "{}", differing.join("\n"));
}

fn screen_names(rt: &Runtime, root: usize) -> String {
    rt.get(root, "names").to_js_string()
}

/// `Brightness.qml:23` itself, on the engine's own `Rustshell.screens`: a monitor plugged in
/// gets its object, an unplugged one loses it.
#[test]
fn monitors_follow_quickshell_screens() {
    let mut rt = Runtime::new(None);
    rt.set_screens(&[("DP-1".into(), 0, 0, 1920, 1080)]);
    let root = rt
        .load_str(
            r#"import QtQuick
import Rustshell
Item {
    id: root
    Component { id: monitorComp; QtObject { property var screen } }
    readonly property list<QtObject> monitors: Rustshell.screens.map(screen => monitorComp.createObject(root, { screen }))
    property string names: monitors.map(m => m.screen.name).join(",")
}"#,
        )
        .unwrap();
    rt.settle();
    assert_eq!(screen_names(&rt, root), "DP-1");
    rt.set_screens(&[
        ("DP-1".into(), 0, 0, 1920, 1080),
        ("HDMI-A-1".into(), 1920, 0, 1920, 1080),
    ]);
    rt.settle();
    assert_eq!(screen_names(&rt, root), "DP-1,HDMI-A-1");
    rt.set_screens(&[("HDMI-A-1".into(), 0, 0, 1920, 1080)]);
    rt.settle();
    assert_eq!(screen_names(&rt, root), "HDMI-A-1");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

/// `Overlay.qml:35` itself, on the engine's own `Region`: every pinned widget gets a region.
#[test]
fn overlay_regions_follow_the_pinned_widgets() {
    let mut rt = Runtime::new(None);
    let root = rt
        .load_str(
            r#"import QtQuick
import Rustshell
Item {
    id: root
    property list<Item> widgets: []
    property string log: ""
    Item { id: w1 } Item { id: w2 }
    Component { id: regionComponent; Region {} }
    property Region mask: Region {
        regions: root.widgets.map((widget) => regionComponent.createObject(this, { item: widget }))
    }
    function items() { return mask.regions.map(r => r.item === w1 ? "w1" : r.item === w2 ? "w2" : "?").join(",") }
    Component.onCompleted: {
        log += "[" + items() + "]"
        widgets = [...widgets, w1]
        log += "[" + items() + "]"
        widgets = [...widgets, w2]
        log += "[" + items() + "]"
        widgets = widgets.filter(w => w !== w1)
        log += "[" + items() + "]"
    }
}"#,
        )
        .unwrap();
    rt.settle();
    assert_eq!(rt.get(root, "log").to_js_string(), "[][w1][w1,w2][w2]");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
