//! A layout change re-syncs only the views whose content it changed: moving a desktop widget
//! writes none of the panel's, the sidebar's or LayoutState's outputs (their Repeaters keep
//! their delegates), while a panel change still reaches the panel.
use qml::layout_model::{Edit, Place};
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

const MANIFEST: &str = r#"
[[widget]]
type = "clock"
name = "Clock"
icon = "schedule"
placements = ["panel", "desktop", "sidebar"]
component = "W.qml"
size = { min = [40, 20], max = [800, 400], default = [200, 100] }
"#;

const LAYOUT: &str = r#"version = 4

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "bar"
kind = "panel"
edge = "top"
thickness = 40

[[monitor.container.widget]]
id = "a"
type = "clock"
zone = "start"

[[monitor.container.widget]]
id = "c"
type = "clock"
zone = "center"

[[monitor.container]]
id = "desk"
kind = "desktop"

[[monitor.container.widget]]
id = "d1"
type = "clock"
x = 100
y = 100
w = 200
h = 100

[[monitor.container]]
id = "side"
kind = "sidebar"
side = "right"
columns = 2

[[monitor.container.widget]]
id = "t1"
type = "clock"
size = 1
"#;

// Each container's views are Repeaters over its outputs, as in the original's bar and sidebar; every
// delegate made counts itself.
const MAIN: &str = r#"import QtQuick
import Rustshell
import Impulse
Item {
    id: root
    property var panelView: panel
    property var sidebarView: sidebar
    property var deskView: desk
    property int panelMade: 0
    property int sidebarMade: 0
    property int desktopMade: 0
    PanelWindow {
        objectName: "bar"
        implicitWidth: 1920
        implicitHeight: 40
        PanelContainer { id: panel; connector: "DP-1" }
        Repeater {
            model: panel.start.concat(panel.center)
            Item {
                required property var modelData
                Component.onCompleted: root.panelMade++
            }
        }
        WidgetHost { widgetType: "clock"; x: 10; width: 120; height: 40 }
        WidgetHost { widgetType: "clock"; x: 900; width: 120; height: 40 }
    }
    PanelWindow {
        objectName: "background"
        implicitWidth: 1920
        implicitHeight: 1080
        DesktopContainer {
            id: desk
            connector: "DP-1"
            width: 1920
            height: 1080
            Repeater {
                model: desk.widgets
                WidgetHost {
                    required property var modelData
                    widgetType: modelData.type
                    x: modelData.x
                    y: modelData.y
                    width: modelData.width
                    height: modelData.height
                    Component.onCompleted: root.desktopMade++
                }
            }
        }
    }
    PanelWindow {
        objectName: "sidebar"
        implicitWidth: 440
        implicitHeight: 1040
        SidebarContainer { id: sidebar; connector: "DP-1" }
        Repeater {
            model: sidebar.sections
            Item {
                required property var modelData
                Component.onCompleted: root.sidebarMade++
            }
        }
        WidgetHost { widgetType: "clock"; x: 20; y: 60; width: 56; height: 56 }
    }
}
"#;

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("layout-view-sync-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("modules/common")).unwrap();
    std::fs::write(d.join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(d.join("W.qml"), "import QtQuick\nItem {}\n").unwrap();
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    std::fs::write(d.join("layout.toml"), LAYOUT).unwrap();
    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    std::fs::copy(
        ri.join("modules/common/LayoutState.qml"),
        d.join("modules/common/LayoutState.qml"),
    )
    .unwrap();
    d
}

fn read(rt: &Runtime, id: usize, k: &str) -> Value {
    rt.world
        .borrow_mut()
        .read(id, k)
        .unwrap_or(Value::Undefined)
}

fn item(rt: &Runtime, id: usize, k: &str) -> usize {
    match read(rt, id, k) {
        Value::Item(i) => i,
        v => panic!("{k}: {v:?}"),
    }
}

fn num(rt: &Runtime, id: usize, k: &str) -> f64 {
    read(rt, id, k).to_number()
}

/// Applies `edit` to the shell's layout (written to `layout.toml`) and lets the views follow.
fn apply(rt: &mut Runtime, edit: Edit) {
    let r = rt
        .world
        .borrow_mut()
        .layout
        .as_mut()
        .unwrap()
        .apply(&[edit]);
    assert_eq!(r, Ok(Ok(true)));
    rt.settle();
}

/// Property `p` of entry `n` of the array `k` of item `id`.
fn entry(rt: &Runtime, id: usize, k: &str, n: usize, p: &str) -> f64 {
    let Value::Obj(o) = read(rt, id, k) else {
        panic!("{k}")
    };
    let Value::Obj(e) = o.borrow().array.as_ref().unwrap()[n].clone() else {
        panic!()
    };
    let v = e.borrow().props.get(p).map(|v| v.to_number());
    v.unwrap()
}

/// The outputs of each view, to tell a rewrite (a fresh array) from an untouched value.
fn outputs(rt: &Runtime, panel: usize, sidebar: usize, state: usize) -> Vec<(String, Value)> {
    let mut out = vec![];
    for (id, keys) in [
        (panel, &["start", "center", "end"][..]),
        (sidebar, &["sections"][..]),
        (state, &["containerKinds", "panelEdges"][..]),
    ] {
        for k in keys {
            out.push((k.to_string(), read(rt, id, k)));
        }
    }
    out
}

fn unchanged(a: &[(String, Value)], b: &[(String, Value)]) -> Vec<String> {
    a.iter()
        .zip(b)
        .filter(|((_, x), (_, y))| !x.strict_eq(y))
        .map(|((k, _), _)| k.clone())
        .collect()
}

#[test]
fn a_desktop_move_leaves_panel_and_sidebar_views_while_a_panel_edit_updates_the_panel() {
    let d = dir();
    let mut rt = Runtime::new(Some(d.clone()));
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.load_singletons(&[d.join("modules/common")]);
    rt.settle();
    let root = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    let (panel, sidebar) = (item(&rt, root, "panelView"), item(&rt, root, "sidebarView"));
    let state = *rt.world.borrow().singletons.get("LayoutState").unwrap();
    assert_eq!(
        read(&rt, state, "containerKinds").to_js_string(),
        "panel,desktop,sidebar"
    );
    assert_eq!(
        (num(&rt, root, "panelMade"), num(&rt, root, "sidebarMade")),
        (2.0, 1.0)
    );
    assert_eq!(num(&rt, root, "desktopMade"), 1.0);

    let before = outputs(&rt, panel, sidebar, state);
    let generation = rt.world.borrow().layout.as_ref().unwrap().generation();

    // Moving the desktop clock touches only the desktop container.
    let place = Place::Desktop {
        x: 600,
        y: 400,
        size: Some((200, 100)),
    };
    let moved = Edit::MoveWidget {
        monitor: 0,
        id: "d1".into(),
        container: "desk".into(),
        place,
        index: None,
    };
    apply(&mut rt, moved);
    let saved = std::fs::read_to_string(d.join("layout.toml")).unwrap();
    assert!(saved.contains("x = 600"), "{saved}");
    assert!(rt.world.borrow().layout.as_ref().unwrap().generation() > generation);
    let desk = item(&rt, root, "deskView");
    assert_eq!(entry(&rt, desk, "widgets", 0, "x"), 600.0);
    let after = outputs(&rt, panel, sidebar, state);
    assert_eq!(unchanged(&before, &after), Vec::<String>::new());
    assert_eq!(
        (num(&rt, root, "panelMade"), num(&rt, root, "sidebarMade")),
        (2.0, 1.0)
    );

    // Removing the panel's center clock is a panel edit: the panel's zones follow, the
    // sidebar and LayoutState stay as they were.
    apply(
        &mut rt,
        Edit::RemoveWidget {
            monitor: 0,
            id: "c".into(),
        },
    );
    let edited = outputs(&rt, panel, sidebar, state);
    assert_eq!(unchanged(&after, &edited), ["center"]);
    let Value::Obj(center) = read(&rt, panel, "center") else {
        panic!()
    };
    assert!(center.borrow().array.as_ref().is_some_and(|a| a.is_empty()));
    // The Repeater's model changed, so it made its delegate (the start run) again.
    assert_eq!(
        (num(&rt, root, "panelMade"), num(&rt, root, "sidebarMade")),
        (3.0, 1.0)
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
