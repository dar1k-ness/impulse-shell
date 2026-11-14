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

/// SidebarLayoutContent's shape: sections, rows and cells from nested Repeaters, each cell an
/// inline component with a WidgetHost and a Connections that declares functions.
const NESTED: &str = r#"import QtQuick
import QtQuick.Layouts
import Impulse
ColumnLayout {
    id: root
    property alias container: container
    property int requests: 0
    signal forwarded()
    SidebarContainer { id: container; connector: "DP-1" }
    Repeater {
        model: container.sections
        delegate: Rectangle {
            id: section
            required property var modelData
            readonly property bool grid: section.modelData.grid
            Layout.fillWidth: true
            implicitHeight: section.grid ? rows.implicitHeight : block.implicitHeight
            Column {
                id: rows
                visible: section.grid
                Repeater {
                    model: section.grid ? section.modelData.rows : []
                    delegate: RowLayout {
                        id: row
                        required property var modelData
                        Repeater {
                            model: row.modelData
                            delegate: Cell {
                                required property var modelData
                                required property int index
                                cellData: modelData
                                placement: ({ "cellSize": modelData.size, "baseCellWidth": 50, "cellSpacing": 6 })
                            }
                        }
                    }
                }
            }
            Cell {
                id: block
                visible: !section.grid
                cellData: section.grid ? ({}) : section.modelData.rows[0][0]
            }
        }
    }
    component Cell: Item {
        id: cell
        required property var cellData
        property var placement: ({})
        implicitWidth: host.implicitWidth
        implicitHeight: host.implicitHeight
        WidgetHost {
            id: host
            widgetType: cell.cellData.type ?? ""
            settings: cell.cellData.settings ?? ({})
            placement: cell.placement
        }
        Connections {
            target: host.item
            ignoreUnknownSignals: true
            function onLabelChanged() {
                root.forwarded();
            }
            function onImplicitHeightChanged() {
                root.requests += 1;
            }
        }
    }
}
"#;

/// Writes `layout` once everything pending has run (the watch is armed), then runs the loop
/// on the events that write causes until the container shows `want`.
fn edit(rt: &mut Runtime, lp: &mut Loop, d: &Path, c: usize, layout: &str, want: &str) {
    lp.drain(rt);
    std::fs::write(d.join("layout.toml"), layout).unwrap();
    assert!(
        lp.until(rt, |rt| shape(rt, c) == want),
        "the edit did not reach the container"
    );
    rt.world.borrow_mut().purge_dead();
    rt.settle();
    assert_eq!(shape(rt, c), want);
}

#[test]
fn live_edits_rebuild_an_active_sidebar() {
    let d = root("nested", LAYOUT);
    let (mut rt, main) = start(&d, NESTED);
    let c = container(&rt, main);
    let mut lp = Loop::new(&mut rt);
    assert_eq!(shape(&rt, c), "grid: wifi/2,mic/1 | bt/2; block: cal/4");
    for round in 0..3 {
        let _ = round;
        edit(
            &mut rt,
            &mut lp,
            &d,
            c,
            &LAYOUT.replace("id = \"mic\"", "id = \"dark\""),
            "grid: wifi/2,dark/1 | bt/2; block: cal/4",
        );
        edit(
            &mut rt,
            &mut lp,
            &d,
            c,
            &LAYOUT
                .replace("columns = 4", "columns = 3")
                .replace("size = 4", "size = 3"),
            "grid: wifi/2,mic/1 | bt/2; block: cal/3",
        );
        edit(
            &mut rt,
            &mut lp,
            &d,
            c,
            &LAYOUT.replace(
                "[[monitor.container.widget]]\nid = \"mic\"\ntype = \"toggle\"\nsize = 1\n",
                "",
            ),
            "grid: wifi/2,bt/2; block: cal/4",
        );
        let reordered = LAYOUT
            .replace("id = \"wifi\"", "id = \"tmp\"")
            .replace("id = \"bt\"", "id = \"wifi\"")
            .replace("id = \"tmp\"", "id = \"bt\"");
        edit(
            &mut rt,
            &mut lp,
            &d,
            c,
            &reordered,
            "grid: bt/2,mic/1 | wifi/2; block: cal/4",
        );
        edit(
            &mut rt,
            &mut lp,
            &d,
            c,
            LAYOUT,
            "grid: wifi/2,mic/1 | bt/2; block: cal/4",
        );
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
