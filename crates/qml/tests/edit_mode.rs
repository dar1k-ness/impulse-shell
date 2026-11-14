//! `Impulse/EditController` driven like the edit overlay drives it: presses, drags and keys in
//! output coordinates select, move, resize and remove widgets across the panel, desktop and
//! sidebar of a real `layout.toml`; every change is one undo step written to the file, Esc
//! drops a drag without a change and then leaves edit mode.
use qml::edit_mode::WinRect;
use qml::layout_model::Layout;
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

const MANIFEST: &str = r##"
[[widget]]
type = "clock"
name = "Clock"
icon = "schedule"
placements = ["panel", "desktop", "sidebar"]
component = "W.qml"
size = { min = [40, 20], max = [800, 400], default = [200, 100] }

[[widget]]
type = "bar-only"
name = "Bar only"
icon = "x"
placements = ["panel"]
component = "W.qml"
size = { min = [10, 10], default = [20, 20] }

[[widget]]
type = "toggle"
name = "Toggle"
icon = "x"
placements = ["sidebar"]
component = "W.qml"
size = { min = [10, 10], default = [56, 56] }
"##;

const LAYOUT: &str = r#"version = 5

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
type = "bar-only"
zone = "start"

[[monitor.container.widget]]
id = "b"
type = "bar-only"
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
type = "toggle"
size = 1
"#;

// Hosts sit where the containers would lay them out; the overlay's controller is the one
// with a connector.
const MAIN: &str = r#"import QtQuick
import Rustshell
import Impulse
Item {
    id: root
    property var ctl: overlay
    EditController { id: overlay; connector: "DP-1" }
    function start(): bool { return overlay.start() }
    function finish(): bool { return overlay.finish() }
    function press(x: real, y: real): bool { return overlay.press(x, y) }
    function move(x: real, y: real): bool { return overlay.move(x, y) }
    function release(x: real, y: real): bool { return overlay.release(x, y) }
    function key(k: int, m: int): bool { return overlay.key(k, m) }
    function undo(): bool { return overlay.undo() }
    PanelWindow {
        objectName: "bar"
        implicitWidth: 1920
        implicitHeight: 40
        PanelContainer { connector: "DP-1" }
        WidgetHost { layoutId: "a"; widgetType: "bar-only"; x: 10; width: 30; height: 40 }
        WidgetHost { layoutId: "b"; widgetType: "bar-only"; x: 50; width: 30; height: 40 }
        WidgetHost { layoutId: "c"; widgetType: "clock"; x: 900; width: 120; height: 40 }
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
                    layoutId: modelData.id
                    widgetType: modelData.type
                    x: modelData.x
                    y: modelData.y
                    width: modelData.width
                    height: modelData.height
                }
            }
        }
    }
    PanelWindow {
        objectName: "sidebar"
        implicitWidth: 440
        implicitHeight: 1040
        // No connector yet, as before SidebarRightContent writes it on load.
        SidebarContainer {}
        WidgetHost { layoutId: "t1"; widgetType: "toggle"; x: 20; y: 60; width: 56; height: 56 }
    }
}
"#;

const ESC: f64 = 0x0100_0000 as f64;
const DEL: f64 = 0x0100_0007 as f64;
const KEY_Z: f64 = 0x5a as f64;
const CTRL: f64 = 0x0400_0000 as f64;
const SHIFT: f64 = 0x0200_0000 as f64;

fn root(tag: &str, manifest: &str, layout: &str, main: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("edit-mode-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("widgets.toml"), manifest).unwrap();
    std::fs::write(d.join("W.qml"), "import QtQuick\nItem {}\n").unwrap();
    std::fs::write(d.join("main.qml"), main).unwrap();
    std::fs::write(d.join("layout.toml"), layout).unwrap();
    d
}

struct T {
    rt: Runtime,
    root: usize,
    ctl: usize,
    dir: PathBuf,
}

fn start(tag: &str) -> T {
    start_with(tag, MANIFEST, LAYOUT, MAIN)
}

fn start_with(tag: &str, manifest: &str, layout: &str, main: &str) -> T {
    let dir = root(tag, manifest, layout, main);
    let mut rt = Runtime::new(Some(dir.clone()));
    rt.world.borrow_mut().open_layout(dir.join("layout.toml"));
    let id = rt.load_file(&dir.join("main.qml")).unwrap();
    rt.settle();
    {
        let mut w = rt.world.borrow_mut();
        // Where the shell would place them: the bar on top, the sidebar against the right edge
        // below the bar, the background across the output.
        for (name, x, y, ww, hh) in [
            ("bar", 0.0, 0.0, 1920.0, 40.0),
            ("background", 0.0, 0.0, 1920.0, 1080.0),
            ("sidebar", 1480.0, 40.0, 440.0, 1040.0),
        ] {
            let win = w
                .of_type("PanelWindow")
                .into_iter()
                .find(|&i| matches!(w.read(i, "objectName"), Some(Value::Str(s)) if s.as_ref() == name))
                .unwrap();
            w.window_rects.insert(
                win,
                WinRect {
                    connector: "DP-1".into(),
                    x,
                    y,
                    w: ww,
                    h: hh,
                },
            );
        }
    }
    let ctl = match rt.world.borrow_mut().read(id, "ctl") {
        Some(Value::Item(i)) => i,
        v => panic!("ctl {v:?}"),
    };
    T {
        rt,
        root: id,
        ctl,
        dir,
    }
}

impl T {
    fn call(&mut self, f: &str, args: &[f64]) -> Value {
        let v = self
            .rt
            .call_method(self.root, f, args.iter().map(|&a| Value::Num(a)).collect())
            .unwrap()
            .unwrap();
        self.rt.settle();
        v
    }

    fn drag(&mut self, from: (f64, f64), to: (f64, f64)) -> Value {
        self.call("press", &[from.0, from.1]);
        // Several motion events, as a pointer sends them.
        for k in 1..=5 {
            let t = k as f64 / 5.0;
            self.call(
                "move",
                &[from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t],
            );
        }
        self.call("release", &[to.0, to.1])
    }

    fn read(&self, k: &str) -> Value {
        self.rt
            .world
            .borrow_mut()
            .read(self.ctl, k)
            .unwrap_or(Value::Undefined)
    }

    fn file(&self) -> String {
        std::fs::read_to_string(self.dir.join("layout.toml")).unwrap()
    }

    fn layout(&self) -> Layout {
        let w = self.rt.world.borrow();
        w.layout.as_ref().unwrap().editor().layout().clone()
    }

    fn container_of(&self, id: &str) -> String {
        let w = self.rt.world.borrow();
        let l = w.layout.as_ref().unwrap().editor().layout();
        l.monitors[0]
            .containers
            .iter()
            .find(|c| c.widgets.iter().any(|wd| wd.id == id))
            .map_or(String::new(), |c| c.id.clone())
    }

    fn frame_ids(&self) -> Vec<String> {
        let Value::Obj(a) = self.read("frames") else {
            return vec![];
        };
        let a = a.borrow();
        a.array
            .iter()
            .flatten()
            .filter_map(|f| match f {
                Value::Obj(o) => o.borrow().props.get("id").map(|v| v.to_js_string()),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn frames_show_every_widget_while_editing() {
    let mut t = start("frames");
    assert!(!t.read("active").truthy());
    assert!(t.frame_ids().is_empty());
    t.call("start", &[]);
    assert!(t.read("active").truthy());
    let mut ids = t.frame_ids();
    ids.sort();
    assert_eq!(ids, ["a", "b", "c", "d1", "t1"]);
    t.call("finish", &[]);
    assert!(!t.read("active").truthy());
    assert!(t.frame_ids().is_empty());
}

#[test]
fn click_selects_and_empty_click_deselects() {
    let mut t = start("select");
    t.call("start", &[]);
    t.call("press", &[150.0, 150.0]);
    t.call("release", &[150.0, 150.0]);
    assert_eq!(t.read("selectedId").to_js_string(), "d1");
    assert_eq!(t.read("selectedKind").to_js_string(), "desktop");
    // A click is no edit.
    assert!(!t.read("canUndo").truthy());
    t.call("press", &[700.0, 700.0]);
    t.call("release", &[700.0, 700.0]);
    assert_eq!(t.read("selectedId").to_js_string(), "");
}

#[test]
fn drag_on_the_desktop_moves_and_undo_redo_restore() {
    let mut t = start("move");
    t.call("start", &[]);
    let before = t.file();
    assert!(t.drag((150.0, 150.0), (650.0, 450.0)).truthy());
    assert!(
        t.file().contains("x = 600") && t.file().contains("y = 400"),
        "{}",
        t.file()
    );
    assert!(t.read("canUndo").truthy());
    assert!(t.call("key", &[KEY_Z, CTRL]).truthy());
    assert_eq!(Layout::parse(&t.file()), Layout::parse(&before));
    assert!(t.call("key", &[KEY_Z, CTRL + SHIFT]).truthy());
    assert!(t.file().contains("x = 600"));
}

#[test]
fn ghost_and_guides_follow_the_drag_and_esc_cancels_it() {
    let mut t = start("cancel");
    t.call("start", &[]);
    let before = t.file();
    t.call("press", &[150.0, 150.0]);
    // Below the drag distance nothing moves yet.
    t.call("move", &[154.0, 150.0]);
    assert!(!t.read("dragging").truthy());
    // Centre of the widget 3 px off the screen's vertical centre line: snaps, with a guide.
    t.call("move", &[913.0, 300.0]);
    assert!(t.read("dragging").truthy());
    let Value::Obj(g) = t.read("ghost") else {
        panic!("no ghost")
    };
    assert_eq!(
        g.borrow().props.get("x").map(|v| v.to_number()),
        Some(860.0)
    );
    let Value::Obj(guides) = t.read("guides") else {
        panic!()
    };
    assert!(!guides.borrow().array.as_ref().unwrap().is_empty());
    assert!(t.call("key", &[ESC, 0.0]).truthy());
    assert!(!t.read("dragging").truthy());
    // The release after Esc changes nothing; edit mode stays on until the next Esc.
    assert!(!t.call("release", &[913.0, 300.0]).truthy());
    assert_eq!(t.file(), before);
    assert!(t.read("active").truthy());
    t.call("key", &[ESC, 0.0]);
    assert!(!t.read("active").truthy());
}

#[test]
fn widgets_move_between_panel_desktop_and_sidebar() {
    let mut t = start("across");
    t.call("start", &[]);
    // The panel clock onto the desktop.
    assert!(t.drag((960.0, 20.0), (600.0, 600.0)).truthy());
    assert_eq!(t.container_of("c"), "desk");
    t.rt.settle();
    assert!(t.frame_ids().contains(&"c".to_string()));
    // The desktop clock into the sidebar, after the toggle.
    assert!(t.drag((150.0, 150.0), (1600.0, 120.0)).truthy());
    assert_eq!(t.container_of("d1"), "side");
    // A panel-only widget cannot leave the panel.
    assert!(!t.drag((20.0, 20.0), (700.0, 700.0)).truthy());
    assert_eq!(t.container_of("a"), "bar");
    // Back in two undo steps.
    t.call("undo", &[]);
    t.call("undo", &[]);
    assert_eq!(
        (t.container_of("c"), t.container_of("d1")),
        ("bar".into(), "desk".into())
    );
}

// The original's own widget types (qml/widgets.toml) on a bar, the desktop and the right sidebar.
const SHIPPED_LAYOUT: &str = r#"version = 5

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "bar"
kind = "panel"
edge = "top"
thickness = 40

[[monitor.container.widget]]
id = "battery"
type = "battery"
zone = "start"

[[monitor.container.widget]]
id = "clock"
type = "clock"
zone = "center"
settings = { showDate = false }

[[monitor.container]]
id = "desk"
kind = "desktop"

[[monitor.container.widget]]
id = "desktop-weather"
type = "weather"
x = 100
y = 100
w = 200
h = 100
settings = { placementStrategy = "mostBusy" }

[[monitor.container]]
id = "side"
kind = "sidebar"
side = "right"
columns = 2

[[monitor.container.widget]]
id = "darkMode"
type = "darkMode"
size = 1
"#;

const SHIPPED_MAIN: &str = r#"import QtQuick
import Rustshell
import Impulse
Item {
    id: root
    property var ctl: overlay
    EditController { id: overlay; connector: "DP-1" }
    function start(): bool { return overlay.start() }
    function press(x: real, y: real): bool { return overlay.press(x, y) }
    function move(x: real, y: real): bool { return overlay.move(x, y) }
    function release(x: real, y: real): bool { return overlay.release(x, y) }
    function undo(): bool { return overlay.undo() }
    function redo(): bool { return overlay.redo() }
    PanelWindow {
        objectName: "bar"
        implicitWidth: 1920
        implicitHeight: 40
        PanelContainer { connector: "DP-1" }
        WidgetHost { layoutId: "battery"; widgetType: "battery"; surface: "panel"; x: 10; width: 30; height: 40 }
        WidgetHost { layoutId: "clock"; widgetType: "clock"; surface: "panel"; x: 900; width: 120; height: 40 }
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
                    layoutId: modelData.id
                    widgetType: modelData.type
                    surface: "desktop"
                    x: modelData.x
                    y: modelData.y
                    width: modelData.width
                    height: modelData.height
                }
            }
        }
    }
    PanelWindow {
        objectName: "sidebar"
        implicitWidth: 440
        implicitHeight: 1040
        SidebarContainer {}
        WidgetHost { layoutId: "darkMode"; widgetType: "darkMode"; surface: "sidebar"; x: 20; y: 60; width: 56; height: 56 }
    }
}
"#;

#[test]
fn the_original_widgets_cross_surfaces_with_their_settings_and_the_file_survives_restart() {
    let manifest = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml/widgets.toml"),
    )
    .unwrap();
    let mut t = start_with("ri-across", &manifest, SHIPPED_LAYOUT, SHIPPED_MAIN);
    t.call("start", &[]);
    let before = t.layout();
    // The desktop weather onto the bar, left of the clock.
    assert!(t.drag((150.0, 150.0), (400.0, 20.0)).truthy());
    assert_eq!(t.container_of("desktop-weather"), "bar");
    // The bar clock into the sidebar, after the toggle.
    assert!(t.drag((960.0, 20.0), (1600.0, 120.0)).truthy());
    assert_eq!(t.container_of("clock"), "side");
    // The sidebar's dark mode toggle onto the bar's right end.
    assert!(t.drag((1528.0, 128.0), (1800.0, 20.0)).truthy());
    assert_eq!(t.container_of("darkMode"), "bar");
    // The battery has no sidebar form: it stays on the bar.
    assert!(!t.drag((25.0, 20.0), (1600.0, 300.0)).truthy());
    assert_eq!(t.container_of("battery"), "bar");

    let after = t.layout();
    let widget = |l: &Layout, id: &str| {
        l.monitors[0]
            .containers
            .iter()
            .flat_map(|c| &c.widgets)
            .find(|w| w.id == id)
            .unwrap()
            .clone()
    };
    for id in ["clock", "desktop-weather", "darkMode"] {
        assert_eq!(
            widget(&after, id).settings,
            widget(&before, id).settings,
            "{id}"
        );
    }
    // A widget arriving in the sidebar spans its sidebar variant's default, within the grid.
    assert!(matches!(
        widget(&after, "clock").place,
        qml::layout_model::Place::Sidebar { size: 2 }
    ));
    // Every move is in layout.toml, and a restart reads the same layout back.
    assert_eq!(Layout::parse(&t.file()).unwrap(), after);
    let restarted = qml::layout_model::Live::open(t.dir.join("layout.toml"));
    assert_eq!(restarted.editor().layout(), &after);
    // Three undo steps restore the original file, three redo steps the moves.
    for _ in 0..3 {
        assert!(t.call("undo", &[]).truthy());
    }
    assert_eq!(Layout::parse(&t.file()).unwrap(), before);
    for _ in 0..3 {
        assert!(t.call("redo", &[]).truthy());
    }
    assert_eq!(Layout::parse(&t.file()).unwrap(), after);
}

#[test]
fn resize_from_the_handle_and_remove_with_delete() {
    let mut t = start("resize");
    t.call("start", &[]);
    t.call("press", &[150.0, 150.0]);
    t.call("release", &[150.0, 150.0]);
    assert!(t.drag((298.0, 198.0), (398.0, 248.0)).truthy());
    assert!(
        t.file().contains("w = 300") && t.file().contains("h = 150"),
        "{}",
        t.file()
    );
    assert_eq!(t.read("selectedId").to_js_string(), "d1");
    assert!(t.call("key", &[DEL, 0.0]).truthy());
    assert!(!t.file().contains("id = \"d1\""));
    t.call("key", &[KEY_Z, CTRL]);
    assert!(t.file().contains("id = \"d1\""));
}

#[test]
fn input_outside_edit_mode_does_nothing() {
    let mut t = start("inactive");
    let before = t.file();
    assert!(!t.drag((150.0, 150.0), (650.0, 450.0)).truthy());
    assert!(!t.call("key", &[DEL, 0.0]).truthy());
    assert_eq!(t.file(), before);
}

#[allow(dead_code)]
fn unused(_: &Path) {}

#[test]
fn a_widget_scrolled_out_of_its_clip_is_framed_only_where_it_shows() {
    // The sidebar's toggle half under a clipping viewport's bottom edge (60 + 56 against 90).
    let main = MAIN.replace(
        r#"WidgetHost { layoutId: "t1"; widgetType: "toggle"; x: 20; y: 60; width: 56; height: 56 }"#,
        r#"Item {
            clip: true
            width: 440
            height: 90
            WidgetHost { layoutId: "t1"; widgetType: "toggle"; x: 20; y: 60; width: 56; height: 56 }
        }"#,
    );
    assert_ne!(main, MAIN);
    let mut t = start_with("clip", MANIFEST, LAYOUT, &main);
    t.call("start", &[]);
    let Value::Obj(a) = t.read("frames") else {
        panic!("no frames")
    };
    let t1 = a
        .borrow()
        .array
        .iter()
        .flatten()
        .find_map(|f| match f {
            Value::Obj(o)
                if o.borrow().props.get("id").map(|v| v.to_js_string()) == Some("t1".into()) =>
            {
                let o = o.borrow();
                let n = |k: &str| o.props.get(k).map_or(f64::NAN, |v| v.to_number());
                Some((n("x"), n("y"), n("width"), n("height")))
            }
            _ => None,
        })
        .expect("t1 framed");
    // The sidebar window is at (1480, 40): the frame stops at the viewport's bottom, y 40 + 90.
    assert_eq!(t1, (1500.0, 100.0, 56.0, 30.0));
}

/// The panel reports no room left in its start and end zones, and room for 300 px in the
/// center, as PanelContent measures its zones.
fn full_panel_main() -> String {
    let main = MAIN.replace(
        r#"PanelContainer { connector: "DP-1" }"#,
        r#"PanelContainer { connector: "DP-1"; startRoom: 0; centerRoom: 300; endRoom: 10 }"#,
    );
    assert_ne!(main, MAIN);
    main
}

#[test]
fn a_drop_into_a_full_panel_zone_is_refused_without_a_write() {
    let mut t = start_with("full-panel", MANIFEST, LAYOUT, &full_panel_main());
    t.call("start", &[]);
    let before = t.file();
    // `a` (30 px) into the end zone, which has 10 px left: refused while dragging and on release.
    t.call("press", &[20.0, 20.0]);
    t.call("move", &[1000.0, 20.0]);
    t.call("move", &[1800.0, 20.0]);
    let valid = |t: &T| match t.read("ghost") {
        Value::Obj(o) => o.borrow().props.get("valid").is_some_and(|v| v.truthy()),
        v => panic!("ghost {v:?}"),
    };
    assert!(!valid(&t));
    assert!(
        matches!(t.read("refusal"), Value::Obj(_)),
        "the refusal badge shows"
    );
    assert!(!t.call("release", &[1800.0, 20.0]).truthy());
    assert_eq!(t.container_of("a"), "bar");
    // The desktop clock's panel variant (200 px) has no room in the start zone either.
    assert!(!t.drag((150.0, 150.0), (20.0, 20.0)).truthy());
    assert_eq!(t.container_of("d1"), "desk");
    assert_eq!(t.file(), before, "a refused drop writes nothing");
    // Moving within its own zone needs no room: `a` goes after `b`.
    assert!(t.drag((20.0, 20.0), (75.0, 20.0)).truthy());
    let order: Vec<String> = t.layout().monitors[0].containers[0]
        .widgets
        .iter()
        .map(|w| w.id.clone())
        .collect();
    assert_eq!(order, ["b", "a", "c"]);
    // The center zone has room for it.
    assert!(t.drag((150.0, 150.0), (960.0, 20.0)).truthy());
    assert_eq!(t.container_of("d1"), "bar");
}

#[test]
fn a_drop_from_another_container_charges_the_length_the_panel_measured() {
    // Room for the clock's panel variant default (200 px) in the start zone, not for the
    // 260 px the panel measured of the desktop clock in its panel form: refused, no write.
    let main = MAIN.replace(
        r#"PanelContainer { connector: "DP-1" }"#,
        r#"PanelContainer { connector: "DP-1"; startRoom: 250; centerRoom: 300; endRoom: 0; panelLengths: ({ d1: 260 }) }"#,
    );
    assert_ne!(main, MAIN);
    let mut t = start_with("measured-length", MANIFEST, LAYOUT, &main);
    t.call("start", &[]);
    let before = t.file();
    t.call("press", &[150.0, 150.0]);
    t.call("move", &[100.0, 60.0]);
    t.call("move", &[20.0, 20.0]);
    assert!(
        matches!(t.read("refusal"), Value::Obj(_)),
        "the refusal badge shows"
    );
    assert!(!t.call("release", &[20.0, 20.0]).truthy());
    assert_eq!(t.container_of("d1"), "desk");
    assert_eq!(t.file(), before, "a refused drop writes nothing");
    // The center zone has room for the 260 px.
    assert!(t.drag((150.0, 150.0), (960.0, 20.0)).truthy());
    assert_eq!(t.container_of("d1"), "bar");
}

#[test]
fn a_quick_toggle_goes_sidebar_panel_sidebar_at_its_natural_size() {
    let manifest = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml/widgets.toml"),
    )
    .unwrap();
    let layout = SHIPPED_LAYOUT.replace(
        r#"settings = { showDate = false }
"#,
        r#"settings = { showDate = false }

[[monitor.container.widget]]
id = "nightLight"
type = "nightLight"
zone = "end"
"#,
    );
    let main = SHIPPED_MAIN.replace(
        r#"surface: "panel"; x: 900; width: 120; height: 40 }"#,
        r#"surface: "panel"; x: 900; width: 120; height: 40 }
        WidgetHost { layoutId: "nightLight"; widgetType: "nightLight"; surface: "panel"; x: 1800; y: 6; width: 28; height: 28 }"#,
    );
    assert!(layout != SHIPPED_LAYOUT && main != SHIPPED_MAIN);
    let mut t = start_with("toggle-trip", &manifest, &layout, &main);
    t.call("start", &[]);
    let before = t.layout();
    let resizable = |t: &T, id: &str| {
        let Value::Obj(a) = t.read("frames") else {
            panic!("frames");
        };
        let a = a.borrow();
        a.array
            .iter()
            .flatten()
            .find_map(|f| match f {
                Value::Obj(o)
                    if o.borrow()
                        .props
                        .get("id")
                        .map(|v| v.to_js_string())
                        .as_deref()
                        == Some(id) =>
                {
                    o.borrow().props.get("resizable").map(|v| v.truthy())
                }
                _ => None,
            })
            .unwrap()
    };
    // A panel toggle is a fixed circle: no resize handle. The sidebar tile spans 1 or 2 cells.
    assert!(!resizable(&t, "nightLight"));
    assert!(resizable(&t, "darkMode"));
    assert!(resizable(&t, "desktop-weather"));
    // Selected, a press at its centre (inside where a corner handle would be) moves it.
    t.call("press", &[1814.0, 20.0]);
    t.call("release", &[1814.0, 20.0]);
    assert_eq!(t.read("selectedId").to_js_string(), "nightLight");
    assert!(t.drag((1814.0, 20.0), (1600.0, 300.0)).truthy());
    assert_eq!(t.container_of("nightLight"), "side");
    // The sidebar tile onto the bar takes no width from its cells: the panel variant's own.
    assert!(t.drag((1528.0, 128.0), (1700.0, 20.0)).truthy());
    assert_eq!(t.container_of("darkMode"), "bar");
    let widget = |l: &Layout, id: &str| {
        l.monitors[0]
            .containers
            .iter()
            .flat_map(|c| &c.widgets)
            .find(|w| w.id == id)
            .unwrap()
            .clone()
    };
    let after = t.layout();
    assert!(matches!(
        widget(&after, "darkMode").place,
        qml::layout_model::Place::Panel {
            width: qml::layout_model::Width::Auto,
            ..
        }
    ));
    assert!(matches!(
        widget(&after, "nightLight").place,
        qml::layout_model::Place::Sidebar { size: 1 }
    ));
    assert!(!t.file().contains("width ="), "{}", t.file());
    for id in ["nightLight", "darkMode"] {
        assert_eq!(
            widget(&after, id).settings,
            widget(&before, id).settings,
            "{id}"
        );
    }
    // Back the other way: each is where it started.
    assert!(t.drag((1814.0, 20.0), (1850.0, 20.0)).truthy());
    assert_eq!(t.container_of("nightLight"), "bar");
    assert!(t.drag((1528.0, 128.0), (1600.0, 300.0)).truthy());
    assert_eq!(t.container_of("darkMode"), "side");
    for id in ["nightLight", "darkMode"] {
        assert_eq!(
            widget(&t.layout(), id).place,
            widget(&before, id).place,
            "{id}"
        );
    }
}

#[test]
fn a_stretching_widget_needs_its_least_length_in_the_zone_it_goes_to() {
    let layout = LAYOUT.replace(
        "id = \"c\"\ntype = \"clock\"\nzone = \"center\"\n",
        "id = \"c\"\ntype = \"clock\"\nzone = \"center\"\nwidth = \"stretch\"\n",
    );
    assert_ne!(layout, LAYOUT);
    let mut t = start_with("full-stretch", MANIFEST, &layout, &full_panel_main());
    t.call("start", &[]);
    let before = t.file();
    let (x, y) = (960.0, 20.0);
    // The clock stretches in the center; its panel variant keeps at least 40 px, and the end
    // zone has 10 left: refused with the badge, nothing written.
    t.call("press", &[x, y]);
    t.call("move", &[1500.0, 20.0]);
    t.call("move", &[1800.0, 20.0]);
    assert!(
        matches!(t.read("refusal"), Value::Obj(_)),
        "the refusal badge shows"
    );
    assert!(!t.call("release", &[1800.0, 20.0]).truthy());
    assert_eq!(t.container_of("c"), "bar");
    assert_eq!(t.file(), before, "a refused drop writes nothing");
}
