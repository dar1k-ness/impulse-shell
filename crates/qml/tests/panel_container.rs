//! `PanelContainer` turns a panel of `layout.toml` into the zones its QML lays out: each zone
//! a list of runs in layout order, adjacent `pill` widgets sharing one run, plus the panel's
//! edge, orientation and size, and it follows the layout live.
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

const MANIFEST: &str = r#"
[[widget]]
type = "button"
name = "Button"
placements = ["panel"]
component = "W.qml"
size = { min = [24, 24], default = [24, 24] }

[[widget]]
type = "clock"
name = "Clock"
placements = ["panel"]
component = "W.qml"
size = { min = [24, 24], default = [24, 24] }
pill = true

[[widget.setting]]
key = "showDate"
kind = "bool"
default = true

[[widget]]
type = "battery"
name = "Battery"
placements = ["panel"]
component = "W.qml"
size = { min = [24, 24], default = [24, 24] }
pill = true

[[widget]]
type = "spacer"
name = "Spacer"
placements = ["panel"]
component = "W.qml"
size = { min = [0, 0], default = [24, 24] }
"#;

fn layout(edge: &str, center: &str) -> String {
    format!(
        r#"version = 2

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "bar"
kind = "panel"
edge = "{edge}"
thickness = 40
length = "60%"
align = "center"
floating = false

[[monitor.container.widget]]
id = "left"
type = "button"
zone = "start"
width = "auto"

[[monitor.container.widget]]
id = "gap"
type = "spacer"
zone = "start"
width = "stretch"
{center}
[[monitor.container.widget]]
id = "right"
type = "button"
zone = "end"
width = 120
"#
    )
}

const CENTER: &str = r#"
[[monitor.container.widget]]
id = "clock"
type = "clock"
zone = "center"
width = "auto"
settings = { showDate = false }

[[monitor.container.widget]]
id = "battery"
type = "battery"
zone = "center"
width = "auto"
"#;

const MAIN: &str = r#"import QtQuick
import Impulse
PanelContainer { connector: "DP-1" }
"#;

fn start(tag: &str, layout_src: &str) -> (Runtime, usize, PathBuf) {
    let d = std::env::temp_dir().join(format!("panel-container-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(d.join("W.qml"), "import QtQuick\nItem {}\n").unwrap();
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    let path = d.join("layout.toml");
    std::fs::write(&path, layout_src).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    rt.world.borrow_mut().open_layout(&path);
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    (rt, id, path)
}

fn read(rt: &Runtime, id: usize, prop: &str) -> Value {
    rt.world.borrow_mut().read(id, prop).unwrap_or_default()
}

fn get(v: &Value, key: &str) -> Value {
    match v {
        Value::Obj(o) => o.borrow().props.get(key).cloned().unwrap_or_default(),
        _ => Value::Undefined,
    }
}

fn list(v: &Value) -> Vec<Value> {
    match v {
        Value::Obj(o) => o.borrow().array.clone().unwrap_or_default(),
        _ => vec![],
    }
}

/// A zone as `[(pill, stretch, ["id:width", ...]), ...]`.
fn runs(rt: &Runtime, id: usize, zone: &str) -> Vec<(bool, bool, Vec<String>)> {
    list(&read(rt, id, zone))
        .iter()
        .map(|r| {
            let entries = list(&get(r, "entries"))
                .iter()
                .map(|e| {
                    let w = get(e, "width").to_js_string();
                    match w.as_str() {
                        "px" => format!(
                            "{}:{}",
                            get(e, "id").to_js_string(),
                            get(e, "px").to_js_string()
                        ),
                        _ => format!("{}:{w}", get(e, "id").to_js_string()),
                    }
                })
                .collect();
            (get(r, "pill").truthy(), get(r, "stretch").truthy(), entries)
        })
        .collect()
}

fn s(rt: &Runtime, id: usize, prop: &str) -> String {
    read(rt, id, prop).to_js_string()
}

fn rewrite(rt: &mut Runtime, path: &Path, src: &str) {
    std::fs::write(path, src).unwrap();
    rt.world.borrow_mut().layout.as_mut().unwrap().reload();
    rt.settle();
}

#[test]
fn zones_hold_the_layout_entries_in_order_with_their_width_modes() {
    let (rt, p, _) = start("zones", &layout("top", CENTER));
    assert_eq!(s(&rt, p, "present"), "true");
    assert_eq!(s(&rt, p, "panelId"), "bar");
    assert_eq!(
        runs(&rt, p, "start"),
        vec![
            (false, false, vec!["left:auto".to_string()]),
            (false, true, vec!["gap:stretch".to_string()]),
        ]
    );
    assert_eq!(s(&rt, p, "startStretch"), "true");
    assert_eq!(
        runs(&rt, p, "end"),
        vec![(false, false, vec!["right:120".to_string()])]
    );
    assert_eq!(s(&rt, p, "endStretch"), "false");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

#[test]
fn adjacent_pill_widgets_share_one_run_and_keep_their_settings() {
    let (rt, p, _) = start("pill", &layout("top", CENTER));
    assert_eq!(
        runs(&rt, p, "center"),
        vec![(
            true,
            false,
            vec!["clock:auto".to_string(), "battery:auto".to_string()]
        )]
    );
    let clock = list(&get(&list(&read(&rt, p, "center"))[0], "entries"))[0].clone();
    assert_eq!(get(&clock, "type").to_js_string(), "clock");
    assert_eq!(
        get(&get(&clock, "settings"), "showDate").to_js_string(),
        "false"
    );
}

#[test]
fn zone_entries_carry_resolved_settings_and_their_errors() {
    let src = layout("top", CENTER).replace(
        "settings = { showDate = false }",
        "settings = { showDate = \"no\", bogus = [1] }",
    );
    assert_ne!(src, layout("top", CENTER));
    let (rt, p, _) = start("resolved", &src);
    let entries = list(&get(&list(&read(&rt, p, "center"))[0], "entries"));
    let (clock, battery) = (&entries[0], &entries[1]);
    // The bad override falls back to the schema default; the unknown key is not published.
    let settings = get(clock, "settings");
    assert_eq!(get(&settings, "showDate").to_js_string(), "true");
    assert!(matches!(
        get(&settings, "bogus"),
        Value::Undefined | Value::Null
    ));
    let errors = list(&get(clock, "settingErrors"));
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(
        rt.errors().iter().any(|e| e.contains("\"clock\"")),
        "{:?}",
        rt.errors()
    );
    // A widget with no overrides gets its type's defaults (none here) and no errors.
    assert!(list(&get(battery, "settingErrors")).is_empty());
}

#[test]
fn a_non_pill_widget_splits_pill_runs() {
    let center = format!(
        "{CENTER}\n[[monitor.container.widget]]\nid = \"mid\"\ntype = \"button\"\nzone = \"center\"\nwidth = \"auto\"\n\n[[monitor.container.widget]]\nid = \"clock2\"\ntype = \"clock\"\nzone = \"center\"\nwidth = \"auto\"\n"
    );
    let (rt, p, _) = start("split", &layout("top", &center));
    let pills: Vec<(bool, usize)> = runs(&rt, p, "center")
        .iter()
        .map(|r| (r.0, r.2.len()))
        .collect();
    assert_eq!(pills, vec![(true, 2), (false, 1), (true, 1)]);
}

#[test]
fn a_left_or_right_edge_makes_the_panel_vertical() {
    let (rt, p, _) = start("vertical", &layout("left", CENTER));
    assert_eq!(s(&rt, p, "edge"), "left");
    assert_eq!(s(&rt, p, "vertical"), "true");
    assert_eq!(s(&rt, p, "thickness"), "40");
    assert_eq!(s(&rt, p, "lengthMode"), "percent");
    assert_eq!(s(&rt, p, "lengthValue"), "60");
    let (rt, p, _) = start("horizontal", &layout("bottom", CENTER));
    assert_eq!(s(&rt, p, "edge"), "bottom");
    assert_eq!(s(&rt, p, "vertical"), "false");
}

#[test]
fn layout_changes_on_disk_reach_the_panel_live() {
    let (mut rt, p, path) = start("live", &layout("top", CENTER));
    rewrite(&mut rt, &path, &layout("right", ""));
    assert_eq!(s(&rt, p, "vertical"), "true");
    assert!(runs(&rt, p, "center").is_empty());
    assert_eq!(s(&rt, p, "centerStretch"), "false");
}

#[test]
fn a_monitor_without_a_panel_has_no_zones() {
    let src = layout("top", CENTER).replace("kind = \"panel\"", "kind = \"desktop\"");
    let src = src
        .lines()
        .filter(|l| {
            ![
                "edge",
                "thickness",
                "length",
                "align",
                "floating",
                "zone",
                "width",
            ]
            .iter()
            .any(|k| l.starts_with(k))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (rt, p, _) = start("absent", &src);
    assert_eq!(s(&rt, p, "present"), "false");
    assert!(runs(&rt, p, "start").is_empty());
}

#[test]
fn panel_widgets_render_their_widget_and_honour_its_shown_flag() {
    let d = std::env::temp_dir().join(format!("panel-widget-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let manifest = MANIFEST.to_string()
        + "\n[[widget]]\ntype = \"ghost\"\nname = \"Ghost\"\nplacements = [\"panel\"]\ncomponent = \"H.qml\"\nsize = { min = [24, 24], default = [24, 24] }\npill = true\n";
    std::fs::write(d.join("widgets.toml"), manifest).unwrap();
    std::fs::write(
        d.join("W.qml"),
        "import QtQuick\nItem { implicitWidth: 30; implicitHeight: 20 }\n",
    )
    .unwrap();
    std::fs::write(
        d.join("H.qml"),
        "import QtQuick\nItem { readonly property bool shown: false; visible: shown; implicitWidth: 40 }\n",
    )
    .unwrap();
    std::fs::write(
        d.join("PanelWidget.qml"),
        include_str!("../../../qml/modules/rusty/bar/panel/PanelWidget.qml"),
    )
    .unwrap();
    std::fs::write(
        d.join("main.qml"),
        r#"import QtQuick
import Impulse
Item {
    // Each host as "type:visible:implicitWidth", in zone order.
    property string hosts: {
        const out = [];
        for (let i = 0; i < row.children.length; i++) {
            const c = row.children[i];
            if (c.widgetType !== undefined)
                out.push(c.widgetType + ":" + c.visible + ":" + c.implicitWidth);
        }
        return out.join(",");
    }
    PanelContainer { id: panel; connector: "DP-1" }
    Row {
        id: row
        Repeater {
            model: panel.center.length > 0 ? panel.center[0].entries : []
            delegate: PanelWidget {}
        }
    }
}
"#,
    )
    .unwrap();
    let center = "\n[[monitor.container.widget]]\nid = \"clock\"\ntype = \"clock\"\nzone = \"center\"\nwidth = \"auto\"\n\n[[monitor.container.widget]]\nid = \"ghost\"\ntype = \"ghost\"\nzone = \"center\"\nwidth = \"auto\"\n";
    let path = d.join("layout.toml");
    std::fs::write(&path, layout("top", center)).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    rt.world.borrow_mut().open_layout(&path);
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    // The clock's 30px shows; the ghost hides itself (and the row skips it).
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(s(&rt, id, "hosts"), "clock:true:30,ghost:false:40");
}

/// The original's bars pick their family from `LayoutState.panelEdge(screen)`: the edge of that monitor's
/// panel, the primary monitor's for a monitor the layout does not name, following the layout.
#[test]
fn layout_state_reports_each_monitors_panel_edge() {
    let (mut rt, _, path) = start("edge", &layout("top", CENTER));
    let d = path.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(d.join("modules/common")).unwrap();
    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    std::fs::copy(
        ri.join("modules/common/LayoutState.qml"),
        d.join("modules/common/LayoutState.qml"),
    )
    .unwrap();
    std::fs::write(
        d.join("state.qml"),
        "import QtQuick\nimport ri.modules.common\nItem { property string named: LayoutState.panelEdge(\"DP-1\")\n property string other: LayoutState.panelEdge(\"HDMI-A-1\") }\n",
    )
    .unwrap();
    rt.load_singletons(&[d.join("modules/common")]);
    rt.settle();
    let id = rt.load_file(&d.join("state.qml")).unwrap();
    rt.settle();
    assert_eq!(
        (s(&rt, id, "named"), s(&rt, id, "other")),
        ("top".into(), "top".into())
    );
    rewrite(&mut rt, &path, &layout("left", CENTER));
    assert_eq!(
        (s(&rt, id, "named"), s(&rt, id, "other")),
        ("left".into(), "left".into())
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

/// The original's Settings write `bar.vertical` / `bar.bottom`; PanelContent passes the edge they choose
/// on as `rustyBarEdge` (here from two root properties standing in for `Config.options.bar`).
const RI_BAR_MAIN: &str = r#"import QtQuick
import Impulse
Item {
    id: root
    property bool barVertical: false
    property bool barBottom: false
    readonly property string edge: panel.edge
    readonly property real thickness: panel.thickness
    PanelContainer {
        id: panel
        connector: "DP-1"
        rustyBarEdge: root.barVertical ? (root.barBottom ? "right" : "left")
            : (root.barBottom ? "bottom" : "top")
    }
}
"#;

/// The layout made from the original's options keeps the bar where the original's settings put it: toggling
/// `bar.vertical` and `bar.bottom` moves the panel live, after a restart, and after the layout
/// was edited (a layout change, saved to `layout.toml`).
#[test]
fn a_layout_made_from_the_original_keeps_following_the_original_s_bar_edge() {
    use qml::layout_model::{Edit, rusty};
    let d = std::env::temp_dir().join(format!("panel-container-iibar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(d.join("W.qml"), "import QtQuick\nItem {}\n").unwrap();
    std::fs::write(d.join("main.qml"), RI_BAR_MAIN).unwrap();
    let path = d.join("layout.toml");
    let monitor = rusty::Monitor {
        connector: "DP-1".into(),
    };
    let options = serde_json::json!({"bar": {"vertical": false, "bottom": false}});
    std::fs::write(&path, rusty::from_options(&options, &monitor).to_toml()).unwrap();

    let set = |rt: &mut Runtime, id: usize, vertical: bool, bottom: bool| {
        rt.set(id, "barVertical", Value::Bool(vertical));
        rt.set(id, "barBottom", Value::Bool(bottom));
        rt.settle();
    };
    let start = |vertical: bool, bottom: bool| {
        let mut rt = Runtime::new(Some(d.clone()));
        rt.world.borrow_mut().open_layout(&path);
        let id = rt.load_file(&d.join("main.qml")).unwrap();
        set(&mut rt, id, vertical, bottom);
        (rt, id)
    };
    let placed = |rt: &Runtime, id: usize| (s(rt, id, "edge"), s(rt, id, "thickness"));
    let at = |edge: &str, thickness: &str| (edge.to_string(), thickness.to_string());
    let toggles = [
        (true, false, at("left", "46")),
        (true, true, at("right", "46")),
        (false, true, at("bottom", "40")),
        (false, false, at("top", "40")),
    ];

    let (mut rt, id) = start(false, false);
    assert_eq!(placed(&rt, id), at("top", "40"));
    for (vertical, bottom, want) in &toggles {
        set(&mut rt, id, *vertical, *bottom);
        assert_eq!(
            &placed(&rt, id),
            want,
            "live: vertical {vertical}, bottom {bottom}"
        );
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    drop(rt);

    for (vertical, bottom, want) in &toggles {
        let (rt, id) = start(*vertical, *bottom);
        assert_eq!(
            &placed(&rt, id),
            want,
            "restart: vertical {vertical}, bottom {bottom}"
        );
    }

    let (mut rt, id) = start(true, false);
    let saved = rt
        .world
        .borrow_mut()
        .layout
        .as_mut()
        .unwrap()
        .apply(&[Edit::RemoveWidget {
            monitor: 0,
            id: "battery".into(),
        }])
        .unwrap();
    assert_eq!(saved, Ok(true));
    rt.settle();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains("\"battery\"") && text.contains("edge = \"rusty\""),
        "{text}"
    );
    assert_eq!(placed(&rt, id), at("left", "46"));
    for (vertical, bottom, want) in &toggles {
        set(&mut rt, id, *vertical, *bottom);
        assert_eq!(
            &placed(&rt, id),
            want,
            "edited: vertical {vertical}, bottom {bottom}"
        );
    }
    drop(rt);
    for (vertical, bottom, want) in &toggles {
        let (rt, id) = start(*vertical, *bottom);
        assert_eq!(
            &placed(&rt, id),
            want,
            "edited, restart: vertical {vertical}, bottom {bottom}"
        );
    }
    std::fs::remove_dir_all(&d).unwrap();
}

/// The original's bars pick their window by `LayoutState.panelEdge`: a panel on the original's bar edge reports
/// none, so the bars fall back to the original's settings, as they do without a layout.
#[test]
fn layout_state_leaves_the_original_s_bar_edge_to_its_settings() {
    let ri_panel =
        layout("top", CENTER).replace("edge = \"top\"\nthickness = 40\n", "edge = \"rusty\"\n");
    let (mut rt, _, path) = start("iiedge", &ri_panel);
    let d = path.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(d.join("modules/common")).unwrap();
    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    std::fs::copy(
        ri.join("modules/common/LayoutState.qml"),
        d.join("modules/common/LayoutState.qml"),
    )
    .unwrap();
    std::fs::write(
        d.join("state.qml"),
        "import QtQuick\nimport ri.modules.common\nItem { property string named: LayoutState.panelEdge(\"DP-1\")\n property string other: LayoutState.panelEdge(\"HDMI-A-1\") }\n",
    )
    .unwrap();
    rt.load_singletons(&[d.join("modules/common")]);
    rt.settle();
    let id = rt.load_file(&d.join("state.qml")).unwrap();
    rt.settle();
    assert_eq!(
        (s(&rt, id, "named"), s(&rt, id, "other")),
        (String::new(), String::new())
    );
    rewrite(&mut rt, &path, &layout("right", CENTER));
    assert_eq!(
        (s(&rt, id, "named"), s(&rt, id, "other")),
        ("right".into(), "right".into())
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}
