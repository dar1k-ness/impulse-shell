//! The tree's `PanelZone`: a stretching run takes the zone's free space only while it shows. The original
//! hides the active window title on a vertical or shortened bar; the start zone then keeps its
//! free space after its runs, so the left sidebar button stays at the panel's start.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::Path;

const RUN: &str = r#"import QtQuick
import QtQuick.Layouts
Item {
    required property var modelData
    property bool vertical
    property string edge
    property bool zoneHovered
    property int shortened
    property bool center
    // Whether a widget of it stretches now, as PanelRun has it.
    readonly property bool stretch: modelData.stretch
    objectName: modelData.name
    visible: modelData.shown
    implicitWidth: 30
    implicitHeight: 30
    Layout.fillHeight: vertical && modelData.stretch
}
"#;

const MAIN: &str = r#"import QtQuick
Item {
    id: main
    width: 46
    height: 300
    property bool titleShown: false
    QtObject {
        id: title
        property string name: "title"
        property bool stretch: true
        property bool shown: main.titleShown
    }
    function run(name) {
        for (let i = 0; i < zone.children.length; i++)
            if (zone.children[i].objectName === name)
                return zone.children[i];
        return null;
    }
    readonly property real buttonY: run("button")?.y ?? -1
    readonly property real titleHeight: run("title")?.height ?? -1
    PanelZone {
        id: zone
        anchors.fill: parent
        vertical: true
        fillAfter: true
        runs: [{ name: "button", stretch: false, shown: true }, title]
    }
}
"#;

#[test]
fn a_hidden_stretching_run_leaves_the_zone_filled_after_its_runs() {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml/modules/rusty/bar/panel");
    let d = std::env::temp_dir().join(format!("panel-zone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::copy(ri.join("PanelZone.qml"), d.join("PanelZone.qml")).unwrap();
    std::fs::write(d.join("PanelRun.qml"), RUN).unwrap();
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    let mut fonts = scene::Fonts::default();
    let mut frame = |rt: &mut Runtime| {
        rt.settle();
        scene::layout_window(rt, &mut fonts, main, [46.0, 300.0]);
        rt.settle();
        (
            rt.get(main, "buttonY").to_number(),
            rt.get(main, "titleHeight").to_number(),
        )
    };
    assert_eq!(frame(&mut rt).0, 0.0);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    // Shown, the title takes all the free space itself.
    rt.set(main, "titleShown", Value::Bool(true));
    assert_eq!(frame(&mut rt), (0.0, 300.0 - 30.0 - 4.0));
}
