//! `SidebarContainer` reads the layout's sidebar container and packs its widgets into
//! sections; `WidgetHost.placement` hands each widget its cell span and metrics.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

const MANIFEST: &str = r##"
[[widget]]
type = "toggle"
name = "Toggle"
icon = "toggle_on"
placements = ["sidebar"]
component = "widgets/Toggle.qml"
size = { min = [1, 1], max = [2, 1], default = [1, 1] }

[[widget.setting]]
key = "label"
kind = "string"
default = "toggle"

[[widget]]
type = "block"
name = "Block"
icon = "calendar_month"
placements = ["sidebar"]
component = "widgets/Block.qml"
size = { min = [1, 1], default = [4, 1] }
"##;

const TOGGLE: &str = r#"import QtQuick
Item {
    property string label: ""
    property int cellSize: 1
    property real baseCellWidth: 0
    property real cellSpacing: 0
    implicitWidth: baseCellWidth * cellSize + cellSpacing * (cellSize - 1)
    implicitHeight: 56
}
"#;

const BLOCK: &str = r#"import QtQuick
Item { implicitHeight: 120 }
"#;

const LAYOUT: &str = r##"
version = 4

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "right"
kind = "sidebar"
side = "right"
columns = 4

[[monitor.container.widget]]
id = "wifi"
type = "toggle"
size = 2
settings = { label = "wifi" }

[[monitor.container.widget]]
id = "mic"
type = "toggle"
size = 1

[[monitor.container.widget]]
id = "bt"
type = "toggle"
size = 2

[[monitor.container.widget]]
id = "cal"
type = "block"
size = 4
"##;

const MAIN: &str = r#"import QtQuick
import Impulse
Item {
    id: root
    property alias container: container
    SidebarContainer { id: container; connector: "DP-1" }
}
"#;

fn root(tag: &str, layout: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sidebar-container-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("widgets")).unwrap();
    std::fs::write(d.join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(d.join("widgets/Toggle.qml"), TOGGLE).unwrap();
    std::fs::write(d.join("widgets/Block.qml"), BLOCK).unwrap();
    std::fs::write(d.join("layout.toml"), layout).unwrap();
    d
}

fn start(d: &Path, main: &str) -> (Runtime, usize) {
    std::fs::write(d.join("main.qml"), main).unwrap();
    let mut rt = Runtime::new(Some(d.to_path_buf()));
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    (rt, id)
}

fn read(rt: &Runtime, id: usize, prop: &str) -> Value {
    rt.world.borrow_mut().read(id, prop).unwrap_or_default()
}

fn container(rt: &Runtime, main: usize) -> usize {
    match read(rt, main, "container") {
        Value::Item(i) => i,
        v => panic!("no container: {v:?}"),
    }
}

/// Sections as `grid|block: type/size,type/size | ...` per row.
fn shape(rt: &Runtime, c: usize) -> String {
    let w = rt.world.borrow();
    let get = |v: &Value, k: &str| -> Value {
        match v {
            Value::Obj(o) => o.borrow().props.get(k).cloned().unwrap_or_default(),
            _ => Value::Null,
        }
    };
    let items = |v: &Value| -> Vec<Value> {
        match v {
            Value::Obj(o) => o.borrow().array.clone().unwrap_or_default(),
            _ => vec![],
        }
    };
    let sections = w.items[c]
        .props
        .get("sections")
        .map(|p| p.value.clone())
        .unwrap_or_default();
    items(&sections)
        .iter()
        .map(|s| {
            let kind = if get(s, "grid").to_js_string() == "true" {
                "grid"
            } else {
                "block"
            };
            let rows: Vec<String> = items(&get(s, "rows"))
                .iter()
                .map(|r| {
                    items(r)
                        .iter()
                        .map(|c| {
                            format!(
                                "{}/{}",
                                get(c, "id").to_js_string(),
                                get(c, "size").to_js_string()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect();
            format!("{kind}: {}", rows.join(" | "))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[test]
fn places_the_layouts_sidebar_widgets_in_sections() {
    let d = root("place", LAYOUT);
    let (rt, main) = start(&d, MAIN);
    let c = container(&rt, main);
    assert_eq!(read(&rt, c, "columns").to_js_string(), "4");
    assert_eq!(shape(&rt, c), "grid: wifi/2,mic/1 | bt/2; block: cal/4");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

#[test]
fn follows_layout_edits_on_disk() {
    let d = root("live", LAYOUT);
    let (mut rt, main) = start(&d, MAIN);
    let c = container(&rt, main);
    let mut lp = Loop::new(&mut rt);
    lp.drain(&mut rt);
    std::fs::write(
        d.join("layout.toml"),
        LAYOUT
            .replace("columns = 4", "columns = 5")
            .replace("size = 4", "size = 5"),
    )
    .unwrap();
    assert!(
        lp.until(&mut rt, |rt| read(rt, c, "columns").to_js_string() == "5"),
        "the edit did not reach the container"
    );
    assert_eq!(read(&rt, c, "columns").to_js_string(), "5");
    assert_eq!(shape(&rt, c), "grid: wifi/2,mic/1,bt/2; block: cal/5");
}

#[test]
fn a_monitor_without_its_own_layout_uses_the_primarys() {
    let d = root("primary", LAYOUT);
    let (rt, main) = start(&d, &MAIN.replace("\"DP-1\"", "\"HDMI-A-1\""));
    let c = container(&rt, main);
    assert_eq!(shape(&rt, c), "grid: wifi/2,mic/1 | bt/2; block: cal/4");
}

#[test]
fn no_sidebar_container_means_no_sections() {
    let d = root("empty", "version = 4\n");
    let (rt, main) = start(&d, MAIN);
    let c = container(&rt, main);
    assert_eq!(read(&rt, c, "columns").to_js_string(), "0");
    assert_eq!(shape(&rt, c), "");
}

const HOSTED: &str = r#"import QtQuick
import Impulse
Item {
    property alias host: host
    WidgetHost {
        id: host
        widgetType: "toggle"
        settings: ({ label: "wifi" })
        placement: ({ cellSize: 2, baseCellWidth: 50, cellSpacing: 6, label: "ignored" })
    }
}
"#;

#[test]
fn placement_reaches_the_widget_and_settings_win() {
    let d = root("placement", LAYOUT);
    let (rt, main) = start(&d, HOSTED);
    let Value::Item(host) = read(&rt, main, "host") else {
        panic!("no host")
    };
    let Value::Item(item) = read(&rt, host, "item") else {
        panic!("no item")
    };
    assert_eq!(read(&rt, item, "cellSize").to_js_string(), "2");
    assert_eq!(read(&rt, item, "implicitWidth").to_js_string(), "106");
    assert_eq!(read(&rt, item, "label").to_js_string(), "wifi");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

#[test]
fn the_original_registers_every_sidebar_widget_with_its_component() {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let mut reg = qml::widgets::Registry::default();
    let errors = reg.load_manifest_file(&ri.join("widgets.toml"));
    assert!(errors.is_empty(), "{errors:?}");
    let sidebar: Vec<_> = reg
        .for_placement(qml::widgets::Placement::Sidebar)
        .collect();
    let types: Vec<&str> = sidebar.iter().map(|t| t.id.as_str()).collect();
    for t in [
        "network",
        "bluetooth",
        "audio",
        "mic",
        "nightLight",
        "idleInhibitor",
        "sliders",
        "quickToggles",
        "calendar",
        "notes",
        "calendarTasks",
        "notificationCenter",
    ] {
        assert!(types.contains(&t), "{t} missing from {types:?}");
    }
    for t in sidebar {
        assert!(
            t.component.is_file(),
            "{}: {} missing",
            t.id,
            t.component.display()
        );
    }
}

/// Each cell's `id: settings.label (settingErrors count)`, in order.
fn cell_settings(rt: &Runtime, c: usize) -> Vec<String> {
    let get = |v: &Value, k: &str| -> Value {
        match v {
            Value::Obj(o) => o.borrow().props.get(k).cloned().unwrap_or_default(),
            _ => Value::Null,
        }
    };
    let items = |v: &Value| -> Vec<Value> {
        match v {
            Value::Obj(o) => o.borrow().array.clone().unwrap_or_default(),
            _ => vec![],
        }
    };
    let sections = read(rt, c, "sections");
    let mut out = vec![];
    for s in items(&sections) {
        for r in items(&get(&s, "rows")) {
            for cell in items(&r) {
                out.push(format!(
                    "{}: {} ({})",
                    get(&cell, "id").to_js_string(),
                    get(&get(&cell, "settings"), "label").to_js_string(),
                    items(&get(&cell, "settingErrors")).len()
                ));
            }
        }
    }
    out
}

#[test]
fn cells_carry_resolved_settings_and_their_errors() {
    let src = LAYOUT.replace(
        "settings = { label = \"wifi\" }",
        "settings = { label = \"wifi\" }\n\n[[monitor.container.widget]]\nid = \"bad\"\ntype = \"toggle\"\nsize = 1\nsettings = { label = [\"x\"] }",
    );
    assert_ne!(src, LAYOUT);
    let d = root("resolved", &src);
    let (rt, main) = start(&d, MAIN);
    let c = container(&rt, main);
    assert_eq!(
        cell_settings(&rt, c),
        [
            "wifi: wifi (0)",
            "bad: toggle (1)",
            "mic: toggle (0)",
            "bt: toggle (0)",
            "cal: undefined (0)",
        ]
    );
    assert!(
        rt.errors().iter().any(|e| e.contains("\"bad\"")),
        "{:?}",
        rt.errors()
    );
}
