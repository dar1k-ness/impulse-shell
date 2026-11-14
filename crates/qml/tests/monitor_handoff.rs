//! A monitor unplugged while the shell runs: its desktop widgets and panel widgets show on a
//! remaining monitor meanwhile, `layout.toml` stays as it was, and when the same monitor (by
//! EDID) comes back, on any connector, everything is where it was again.
use qml::layout_model::{Edid, MonitorId};
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::PathBuf;

const MANIFEST: &str = r#"
[[widget]]
type = "clock"
name = "Clock"
placements = ["desktop", "panel"]
component = "W.qml"
size = { min = [10, 10], max = [2000, 2000], default = [100, 100] }
"#;

const LAYOUT: &str = r#"version = 2

[[monitor]]
connector = "DP-1"
edid = { make = "Dell", model = "U2720Q", serial = "AAA" }
primary = true

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "clock"
x = 100
y = 100
w = 300
h = 150

[[monitor.container]]
id = "bar"
kind = "panel"
edge = "top"
thickness = 40

[[monitor.container.widget]]
id = "time"
type = "clock"
zone = "start"
width = "auto"

[[monitor]]
connector = "HDMI-A-1"
edid = { make = "LG", model = "27GL850", serial = "BBB" }

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "notes"
type = "clock"
x = 100
y = 100
w = 200
h = 100
settings = { style = "kept" }

[[monitor.container]]
id = "bar"
kind = "panel"
edge = "bottom"
thickness = 30

[[monitor.container.widget]]
id = "date"
type = "clock"
zone = "end"
width = "auto"
"#;

const MAIN: &str = r#"import QtQuick
import Impulse
Item {
    readonly property var dpDesktop: dpD.widgets
    readonly property var hdmiDesktop: hdmiD.widgets
    readonly property var dpShown: dpD.shown
    readonly property var hdmiShown: hdmiD.shown
    readonly property var dpPanel: dpP.start.concat(dpP.center).concat(dpP.end)
    readonly property string hdmiEdge: hdmiP.edge
    readonly property var hdmiPanel: hdmiP.start.concat(hdmiP.center).concat(hdmiP.end)
    DesktopContainer { id: dpD; width: 1920; height: 1080; connector: "DP-1" }
    DesktopContainer { id: hdmiD; width: 1920; height: 1080; connector: "HDMI-A-1" }
    PanelContainer { id: dpP; connector: "DP-1" }
    PanelContainer { id: hdmiP; connector: "HDMI-A-1" }
    function natural(key: string, w: real, h: real) { dpD.setNaturalSize(key, w, h) }
}
"#;

fn start(tag: &str) -> (Runtime, usize, PathBuf) {
    let d = std::env::temp_dir().join(format!("monitor-handoff-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(d.join("W.qml"), "import QtQuick\nItem {}\n").unwrap();
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    let path = d.join("layout.toml");
    std::fs::write(&path, LAYOUT).unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    rt.world.borrow_mut().open_layout(&path);
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    (rt, id, path)
}

fn monitor(connector: &str, make: &str, model: &str, serial: &str) -> MonitorId {
    MonitorId {
        connector: connector.into(),
        edid: Some(Edid {
            make: make.into(),
            model: model.into(),
            serial: serial.into(),
        }),
    }
}

fn dell(connector: &str) -> MonitorId {
    monitor(connector, "Dell", "U2720Q", "AAA")
}

fn lg(connector: &str) -> MonitorId {
    monitor(connector, "LG", "27GL850", "BBB")
}

fn plug(rt: &mut Runtime, outputs: &[MonitorId]) {
    rt.set_outputs(outputs);
    rt.settle();
}

fn list(v: &Value) -> Vec<Value> {
    match v {
        Value::Obj(o) => o.borrow().array.clone().unwrap_or_default(),
        _ => vec![],
    }
}

fn get(v: &Value, key: &str) -> Value {
    match v {
        Value::Obj(o) => o.borrow().props.get(key).cloned().unwrap_or_default(),
        _ => Value::Undefined,
    }
}

/// A desktop's widgets (`<name>Desktop`) as (id, x, y, width, height, guest), where they
/// show (`<name>Shown`, by each entry's `key`).
fn desktop(rt: &Runtime, id: usize, prop: &str) -> Vec<(String, i64, i64, i64, i64, String)> {
    let (v, shown) = {
        let mut w = rt.world.borrow_mut();
        (
            w.read(id, prop).unwrap_or_default(),
            w.read(id, &prop.replace("Desktop", "Shown"))
                .unwrap_or_default(),
        )
    };
    list(&v)
        .iter()
        .map(|w| {
            let s = |k: &str| get(w, k).to_js_string();
            let at = get(&shown, &s("key"));
            let n = |k: &str| get(&at, k).to_number() as i64;
            (s("id"), n("x"), n("y"), n("width"), n("height"), s("guest"))
        })
        .collect()
}

/// A panel's entries in zone order as (id, guest).
fn panel(rt: &Runtime, id: usize, prop: &str) -> Vec<(String, String)> {
    let v = rt.world.borrow_mut().read(id, prop).unwrap_or_default();
    list(&v)
        .iter()
        .flat_map(|run| list(&get(run, "entries")))
        .map(|e| {
            (
                get(&e, "id").to_js_string(),
                get(&e, "guest").to_js_string(),
            )
        })
        .collect()
}

fn own(id: &str, x: i64, y: i64, w: i64, h: i64) -> (String, i64, i64, i64, i64, String) {
    (id.into(), x, y, w, h, String::new())
}

#[test]
fn unplugged_monitor_visits_the_primary_and_returns_exactly() {
    let (mut rt, id, path) = start("visit");
    plug(&mut rt, &[dell("DP-1"), lg("HDMI-A-1")]);
    let before = (
        desktop(&rt, id, "dpDesktop"),
        desktop(&rt, id, "hdmiDesktop"),
        panel(&rt, id, "dpPanel"),
        panel(&rt, id, "hdmiPanel"),
    );
    assert_eq!(before.0, vec![own("clock", 100, 100, 300, 150)]);
    assert_eq!(before.1, vec![own("notes", 100, 100, 200, 100)]);
    assert_eq!(before.2, vec![("time".into(), String::new())]);
    assert_eq!(before.3, vec![("date".into(), String::new())]);

    plug(&mut rt, &[dell("DP-1")]);
    let d = desktop(&rt, id, "dpDesktop");
    assert_eq!(d[0], own("clock", 100, 100, 300, 150));
    assert_eq!(d.len(), 2, "{d:?}");
    let notes = &d[1];
    assert_eq!(
        (notes.0.as_str(), notes.3, notes.4, notes.5.as_str()),
        ("notes", 200, 100, "HDMI-A-1")
    );
    let clock = (100, 100, 300, 150);
    assert!(
        notes.1 >= clock.0 + clock.2
            || notes.1 + notes.3 <= clock.0
            || notes.2 >= clock.1 + clock.3
            || notes.2 + notes.4 <= clock.1,
        "the visiting widget overlaps the host's: {d:?}"
    );
    assert_eq!(
        panel(&rt, id, "dpPanel"),
        vec![
            ("time".into(), String::new()),
            ("date".into(), "HDMI-A-1".into())
        ]
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        LAYOUT,
        "the handoff rewrote layout.toml"
    );

    plug(&mut rt, &[dell("DP-1"), lg("HDMI-A-1")]);
    let after = (
        desktop(&rt, id, "dpDesktop"),
        desktop(&rt, id, "hdmiDesktop"),
        panel(&rt, id, "dpPanel"),
        panel(&rt, id, "hdmiPanel"),
    );
    assert_eq!(after, before);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LAYOUT);
}

#[test]
fn the_same_monitor_on_another_connector_gets_its_layout_back() {
    let (mut rt, id, path) = start("port");
    plug(&mut rt, &[dell("DP-1"), lg("HDMI-A-1")]);
    plug(&mut rt, &[dell("DP-1")]);
    plug(&mut rt, &[dell("DP-1"), lg("HDMI-A-1")]);
    let hdmi = desktop(&rt, id, "hdmiDesktop");
    // Plugged back into DP-2: matched by EDID, so it shows its own layout there, the guests
    // leave the primary, and the stored connector is not rewritten for it.
    plug(&mut rt, &[dell("DP-1")]);
    plug(&mut rt, &[dell("DP-1"), lg("DP-2")]);
    assert_eq!(
        desktop(&rt, id, "dpDesktop"),
        vec![own("clock", 100, 100, 300, 150)]
    );
    let v = rt
        .world
        .borrow()
        .layout
        .as_ref()
        .unwrap()
        .container("DP-2", "desktop")
        .map(|c| c.widgets[0].id.clone());
    assert_eq!(v.as_deref(), Some("notes"));
    assert_eq!(hdmi, vec![own("notes", 100, 100, 200, 100)]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LAYOUT);
}

#[test]
fn a_different_monitor_on_the_freed_connector_does_not_take_the_layout() {
    let (mut rt, id, path) = start("other");
    plug(&mut rt, &[dell("DP-1"), lg("HDMI-A-1")]);
    plug(
        &mut rt,
        &[dell("DP-1"), monitor("HDMI-A-1", "Acer", "XB271", "CCC")],
    );
    // The newcomer starts from the primary layout; the LG's widgets keep visiting the primary.
    assert_eq!(
        desktop(&rt, id, "hdmiDesktop"),
        vec![own("clock", 100, 100, 300, 150)]
    );
    let d = desktop(&rt, id, "dpDesktop");
    assert_eq!(
        d.iter()
            .map(|w| (w.0.as_str(), w.5.as_str()))
            .collect::<Vec<_>>(),
        [("clock", ""), ("notes", "HDMI-A-1")]
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LAYOUT);
}

#[test]
fn a_monitor_never_connected_does_not_visit() {
    let (mut rt, id, _) = start("never");
    plug(&mut rt, &[dell("DP-1")]);
    assert_eq!(
        desktop(&rt, id, "dpDesktop"),
        vec![own("clock", 100, 100, 300, 150)]
    );
    assert_eq!(
        panel(&rt, id, "dpPanel"),
        vec![("time".into(), String::new())]
    );
}

#[test]
fn unplugging_the_primary_moves_its_widgets_to_the_other_monitor() {
    let (mut rt, id, path) = start("primary");
    plug(&mut rt, &[dell("DP-1"), lg("HDMI-A-1")]);
    plug(&mut rt, &[lg("HDMI-A-1")]);
    let d = desktop(&rt, id, "hdmiDesktop");
    assert_eq!(
        d.iter()
            .map(|w| (w.0.as_str(), w.5.as_str()))
            .collect::<Vec<_>>(),
        [("notes", ""), ("clock", "DP-1")]
    );
    assert_eq!(
        rt.world
            .borrow_mut()
            .read(id, "hdmiEdge")
            .unwrap_or_default()
            .to_js_string(),
        "bottom"
    );
    assert_eq!(
        panel(&rt, id, "hdmiPanel"),
        vec![
            ("time".into(), "DP-1".into()),
            ("date".into(), String::new())
        ]
    );
    plug(&mut rt, &[dell("DP-1"), lg("HDMI-A-1")]);
    assert_eq!(
        desktop(&rt, id, "hdmiDesktop"),
        vec![own("notes", 100, 100, 200, 100)]
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LAYOUT);
}

/// Two unplugged monitors visiting with a widget of the same id, each without a size in the
/// layout: when one comes back, the other keeps its own key and its own measured size, never
/// the one that left.
#[test]
fn a_visiting_widget_keeps_its_measured_size_when_another_guest_leaves() {
    let layout = LAYOUT
        .replace("id = \"notes\"", "id = \"memo\"")
        .replace("w = 200\nh = 100\n", "")
        + r#"
[[monitor]]
connector = "DP-2"
edid = { make = "Acer", model = "XB271", serial = "CCC" }

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "memo"
type = "clock"
x = 900
y = 600
"#;
    let (mut rt, id, path) = start("guest-keys");
    std::fs::write(&path, &layout).unwrap();
    rt.world.borrow_mut().open_layout(&path);
    rt.settle();
    let acer = || monitor("DP-2", "Acer", "XB271", "CCC");
    plug(&mut rt, &[dell("DP-1"), lg("HDMI-A-1"), acer()]);
    plug(&mut rt, &[dell("DP-1")]);
    let keys = |rt: &Runtime| -> Vec<(String, String)> {
        let v = rt
            .world
            .borrow_mut()
            .read(id, "dpDesktop")
            .unwrap_or_default();
        list(&v)
            .iter()
            .map(|w| (get(w, "key").to_js_string(), get(w, "guest").to_js_string()))
            .collect()
    };
    let visiting = keys(&rt);
    assert_eq!(visiting.len(), 3, "{visiting:?}");
    let (lg_memo, acer_memo) = (visiting[1].0.clone(), visiting[2].0.clone());
    assert_eq!(
        (visiting[1].1.as_str(), visiting[2].1.as_str()),
        ("HDMI-A-1", "DP-2")
    );
    assert_ne!(lg_memo, acer_memo);
    rt.call_method(
        id,
        "natural",
        vec![Value::str(&lg_memo), Value::Num(500.0), Value::Num(300.0)],
    )
    .unwrap()
    .unwrap();
    rt.call_method(
        id,
        "natural",
        vec![Value::str(&acer_memo), Value::Num(120.0), Value::Num(80.0)],
    )
    .unwrap()
    .unwrap();
    rt.settle();
    let size = |rt: &Runtime| {
        desktop(rt, id, "dpDesktop")
            .into_iter()
            .find(|w| w.5 == "DP-2")
            .map(|w| (w.3, w.4))
    };
    assert_eq!(size(&rt), Some((120, 80)));
    plug(&mut rt, &[dell("DP-1"), lg("HDMI-A-1")]);
    assert_eq!(keys(&rt)[1], (acer_memo, "DP-2".to_string()));
    assert_eq!(size(&rt), Some((120, 80)));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
