//! The edit overlay's toolbar (qml EditOverlay.qml with the original's own Toolbar and
//! RippleButton) under the pointer: a toolbar button acts only on a release inside it, as a
//! Qt Quick click does; a press dragged off a button and released elsewhere cancels it and
//! writes nothing. The toolbar takes presses only on its buttons: a press on its background
//! reaches the overlay below it, so a desktop widget under the toolbar can still be grabbed.
#![cfg(feature = "interp")]

use qml::edit_mode::WinRect;
use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, scene};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const BTN_LEFT: u32 = 0x110;
const W: f64 = 1280.0;
const H: f64 = 720.0;

const MANIFEST: &str = r#"
[[widget]]
type = "clock"
name = "Clock"
icon = "schedule"
placements = ["panel", "desktop", "sidebar"]
component = "W.qml"
size = { min = [40, 20], max = [800, 400], default = [200, 100] }
placedBy = "background.widgets.clock.placementStrategy"
"#;

fn layout(x: i32, y: i32) -> String {
    format!(
        r#"version = 5

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desk"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "clock"
x = {x}
y = {y}
w = 200
h = 100

[[monitor.container]]
id = "bar"
kind = "panel"
edge = "top"
thickness = 40
length = "fill"
align = "center"
floating = false

[[monitor.container.widget]]
id = "pclock"
type = "clock"
zone = "center"

[[monitor.container]]
id = "side"
kind = "sidebar"
side = "right"
columns = 2

[[monitor.container.widget]]
id = "sclock"
type = "clock"
size = 2
"#
    )
}

// The background surface: the desktop container with its widget hosts, as the original's background
// draws them, a bar with one widget, and a controller to enter edit mode with.
const BACKGROUND: &str = r#"import QtQuick
import Impulse
import ri.modules.common
Item {
    id: root
    width: 1280
    height: 720
    property var ctl: controller
    property string clockPlacedBy: Config.options.background.widgets.clock.placementStrategy
    EditController {
        id: controller
        connector: "DP-1"
    }
    DesktopContainer {
        id: desk
        connector: "DP-1"
        width: 1280
        height: 720
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
    PanelWindow {
        objectName: "bar"
        implicitWidth: 1280
        implicitHeight: 40
        PanelContainer { connector: "DP-1" }
        WidgetHost { layoutId: "pclock"; widgetType: "clock"; x: 600; width: 120; height: 40 }
    }
    // A sidebar 100 high: it measures a widget (80) only once the widget is in it, as the
    // shell's sidebar does, and with its own widget (40) that is more than it has.
    PanelWindow {
        id: side
        objectName: "sidebar"
        implicitWidth: 300
        implicitHeight: 720
        SidebarContainer {
            connector: "DP-1"
            available: 100
            sidebarHeights: side.measured ? ({ "sclock": 40, "clock-1": 80 }) : ({ "sclock": 40 })
        }
        // Later than the catalog's closing animation, as a sidebar that has to open first.
        property bool measured: false
        Timer {
            interval: 600
            running: controller.selectedId === "clock-1"
            onTriggered: side.measured = true
        }
    }
    function startEditing(): bool { return controller.start() }
    function setSize(width: real, height: real): bool { return controller.setSize(width, height) }
}
"#;

const OVERLAY: &str = r#"import QtQuick
import Rustshell
import ri.modules.rusty.editMode
EditOverlay {
    screen: Rustshell.screens[0]
}
"#;

struct Edit {
    rt: Runtime,
    dir: PathBuf,
    bg: usize,
    overlay: usize,
    ctl: usize,
    ptr: Pointer,
    fonts: scene::Fonts,
    now: f64,
}

impl Edit {
    fn new(tag: &str, x: i32, y: i32) -> Self {
        let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../qml")
            .canonicalize()
            .unwrap();
        let dir = std::env::temp_dir().join(format!("edit-toolbar-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for e in std::fs::read_dir(&ri).unwrap() {
            let e = e.unwrap();
            if e.file_name() != "widgets.toml" {
                std::os::unix::fs::symlink(e.path(), dir.join(e.file_name())).unwrap();
            }
        }
        std::fs::write(dir.join("widgets.toml"), MANIFEST).unwrap();
        std::fs::write(dir.join("W.qml"), "import QtQuick\nItem {}\n").unwrap();
        std::fs::write(dir.join("layout.toml"), layout(x, y)).unwrap();
        std::fs::write(dir.join("bg.qml"), BACKGROUND).unwrap();
        std::fs::write(dir.join("overlay.qml"), OVERLAY).unwrap();
        let mut rt = Runtime::new(Some(dir.clone()));
        rt.set_screens(&[("DP-1".into(), 0, 0, W as i32, H as i32)]);
        let dirs: Vec<PathBuf> = [
            "modules/common",
            "modules/common/functions",
            "modules/common/utils",
            "services",
            "",
        ]
        .iter()
        .map(|d| dir.join(d))
        .collect();
        rt.load_singletons_lazy(&dirs);
        rt.world.borrow_mut().open_layout(dir.join("layout.toml"));
        let bg = rt.load_file(&dir.join("bg.qml")).unwrap();
        rt.settle();
        let ctl = match rt.world.borrow_mut().read(bg, "ctl") {
            Some(Value::Item(i)) => i,
            v => panic!("ctl: {v:?}"),
        };
        rt.call_method(bg, "startEditing", vec![]).unwrap().unwrap();
        rt.settle();
        let overlay = rt.load_file(&dir.join("overlay.qml")).unwrap();
        rt.settle();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        {
            let mut w = rt.world.borrow_mut();
            for win in [bg, overlay] {
                w.window_rects.insert(
                    win,
                    WinRect {
                        connector: "DP-1".into(),
                        x: 0.0,
                        y: 0.0,
                        w: W,
                        h: H,
                    },
                );
            }
            let side = w.of_type("PanelWindow")[1];
            w.window_rects.insert(
                side,
                WinRect {
                    connector: "DP-1".into(),
                    x: W - 300.0,
                    y: 0.0,
                    w: 300.0,
                    h: H,
                },
            );
            let bar = w.of_type("PanelWindow")[0];
            w.window_rects.insert(
                bar,
                WinRect {
                    connector: "DP-1".into(),
                    x: 0.0,
                    y: 0.0,
                    w: W,
                    h: 40.0,
                },
            );
        }
        let mut e = Edit {
            rt,
            dir,
            bg,
            overlay,
            ctl,
            ptr: Pointer::default(),
            fonts: scene::Fonts::default(),
            now: 1000.0,
        };
        e.frames(5);
        assert!(e.read("active").truthy());
        e
    }

    /// Lets animations run and lays both surfaces out, as frames do.
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.now += 16.0;
            self.rt.tick(self.now);
            self.rt.settle();
            for win in [self.bg, self.overlay] {
                scene::layout_window(&mut self.rt, &mut self.fonts, win, [W, H]);
            }
            self.rt.settle();
        }
    }

    fn read(&self, k: &str) -> Value {
        self.rt
            .world
            .borrow_mut()
            .read(self.ctl, k)
            .unwrap_or(Value::Undefined)
    }

    fn read_of(&self, i: usize, k: &str) -> Value {
        self.rt
            .world
            .borrow_mut()
            .read(i, k)
            .unwrap_or(Value::Undefined)
    }

    /// The overlay's toolbar button showing `icon` (its `text`, or `iconText` beside a label):
    /// the bottom toolbar's, or one of the two by the selected widget (its settings, its
    /// removal).
    fn button(&self, icon: &str) -> usize {
        let mut w = self.rt.world.borrow_mut();
        let found = w.of_type("RippleButton").into_iter().rfind(|&b| {
            !w.dead.contains(&b)
                && ["iconText", "text"]
                    .iter()
                    .find_map(|k| w.read(b, k).filter(|t| !matches!(t, Value::Undefined)))
                    .is_some_and(|t| t.to_js_string() == icon)
                && w.read(b, "visible").is_some_and(|v| v.truthy())
        });
        found.unwrap_or_else(|| panic!("no visible toolbar button {icon}"))
    }

    /// `item`'s rectangle in the overlay.
    fn rect(&self, item: usize) -> (f64, f64, f64, f64) {
        let mut w = self.rt.world.borrow_mut();
        let (x, y) = w.scene_pos(item, Some(self.overlay));
        let num =
            |w: &mut qml::runtime::World, k: &str| w.read(item, k).unwrap_or_default().to_number();
        (x, y, num(&mut w, "width"), num(&mut w, "height"))
    }

    fn center(&self, item: usize) -> (f64, f64) {
        let (x, y, w, h) = self.rect(item);
        (x + w / 2.0, y + h / 2.0)
    }

    /// A left-button press at `from`, moved in steps to `to` and released there.
    fn drag(&mut self, from: (f64, f64), to: (f64, f64)) {
        let win = self.overlay;
        self.ptr.enter(&mut self.rt, win, from.0, from.1);
        self.ptr.motion(&mut self.rt, win, from.0, from.1);
        self.ptr.press(&mut self.rt, BTN_LEFT);
        self.frames(1);
        for k in 1..=8 {
            let t = k as f64 / 8.0;
            let (x, y) = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            self.ptr.motion(&mut self.rt, win, x, y);
            self.frames(1);
        }
        self.ptr.release(&mut self.rt, BTN_LEFT);
        self.frames(3);
    }

    fn click(&mut self, at: (f64, f64)) {
        self.drag(at, at);
    }

    /// The pointer moved to `at` with no button down.
    fn hover(&mut self, at: (f64, f64)) {
        let win = self.overlay;
        self.ptr.motion(&mut self.rt, win, at.0, at.1);
        self.frames(2);
    }

    /// The numbers `keys` of the controller's object property `prop`; `None` when it is null.
    fn nums<const N: usize>(&self, prop: &str, keys: [&str; N]) -> Option<[f64; N]> {
        match self.read(prop) {
            Value::Obj(o) => {
                Some(keys.map(|k| o.borrow().props.get(k).map_or(f64::NAN, |v| v.to_number())))
            }
            _ => None,
        }
    }

    fn clock_place(&self) -> (i32, i32, Option<(u32, u32)>) {
        let w = self.rt.world.borrow();
        let l = w.layout.as_ref().unwrap().editor().layout();
        match l.monitors[0].containers[0].widgets[0].place {
            qml::layout_model::Place::Desktop { x, y, size } => (x, y, size),
            ref p => panic!("{p:?}"),
        }
    }

    /// layout.toml's bytes and inode: a save replaces the file (write_atomic renames over it).
    fn saved(&self) -> (String, u64) {
        let p = self.dir.join("layout.toml");
        (
            std::fs::read_to_string(&p).unwrap(),
            std::fs::metadata(&p).unwrap().ino(),
        )
    }

    fn clock_xy(&self) -> (i32, i32) {
        let w = self.rt.world.borrow();
        let l = w.layout.as_ref().unwrap().editor().layout();
        match l.monitors[0].containers[0].widgets[0].place {
            qml::layout_model::Place::Desktop { x, y, .. } => (x, y),
            ref p => panic!("{p:?}"),
        }
    }

    /// Whether the overlay's widget settings dialog (EditOverlay.qml `settingsDialog`) is up.
    fn settings_open(&self) -> bool {
        let mut w = self.rt.world.borrow_mut();
        let dialog = w
            .of_type("Loader")
            .into_iter()
            .find(|&l| !w.dead.contains(&l) && w.read(l, "shown").is_some());
        let dialog = dialog.expect("no settings dialog");
        w.read(dialog, "shown").is_some_and(|v| v.truthy())
    }

    /// Where the layout has the desktop's second widget.
    fn second_xy(&self) -> (i32, i32) {
        let w = self.rt.world.borrow();
        let l = w.layout.as_ref().unwrap().editor().layout();
        match l.monitors[0].containers[0].widgets[1].place {
            qml::layout_model::Place::Desktop { x, y, .. } => (x, y),
            ref p => panic!("{p:?}"),
        }
    }

    /// Whether the widget catalog (EditOverlay.qml `catalogDialog`) is up.
    fn palette_open(&self) -> bool {
        let mut w = self.rt.world.borrow_mut();
        let palette = w
            .of_type("Loader")
            .into_iter()
            .find(|&l| !w.dead.contains(&l) && w.read(l, "showing").is_some());
        let palette = palette.expect("no widget palette");
        w.read(palette, "showing").is_some_and(|v| v.truthy())
    }

    /// The catalog's tile for widget type `ty`, where it is offered.
    fn tile(&self, ty: &str) -> Option<usize> {
        let mut w = self.rt.world.borrow_mut();
        w.of_type("RippleButton").into_iter().find(|&b| {
            !w.dead.contains(&b)
                && match w.read(b, "modelData") {
                    Some(Value::Obj(o)) => o
                        .borrow()
                        .props
                        .get("type")
                        .is_some_and(|t| t.to_js_string() == ty),
                    _ => false,
                }
        })
    }

    /// The settings dialog's segment that picks `value`, where it is shown.
    fn segment(&self, value: &str) -> Option<usize> {
        let mut w = self.rt.world.borrow_mut();
        w.of_type("Item").into_iter().find(|&b| {
            !w.dead.contains(&b)
                && match w.read(b, "modelData") {
                    Some(Value::Obj(o)) => o
                        .borrow()
                        .props
                        .get("value")
                        .is_some_and(|t| t.to_js_string() == value),
                    _ => false,
                }
        })
    }

    /// The catalog's button for surface `value`, where the catalog is shown.
    fn surface(&self, value: &str) -> Option<usize> {
        let mut w = self.rt.world.borrow_mut();
        w.of_type("SelectionGroupButton").into_iter().find(|&b| {
            !w.dead.contains(&b)
                && match w.read(b, "modelData") {
                    Some(Value::Obj(o)) => o
                        .borrow()
                        .props
                        .get("value")
                        .is_some_and(|t| t.to_js_string() == value),
                    _ => false,
                }
        })
    }

    /// Whether the catalog says the last pick was refused.
    fn palette_refused(&self) -> bool {
        let mut w = self.rt.world.borrow_mut();
        w.of_type("WidgetPalette")
            .into_iter()
            .any(|p| w.read(p, "refused").is_some_and(|v| v.truthy()))
    }

    /// The ids of the widgets in container `c` of the layout.
    fn ids(&self, c: usize) -> Vec<String> {
        let w = self.rt.world.borrow();
        w.layout.as_ref().unwrap().editor().layout().monitors[0].containers[c]
            .widgets
            .iter()
            .map(|wd| wd.id.clone())
            .collect()
    }

    fn has_clock(&self) -> bool {
        let w = self.rt.world.borrow();
        !w.layout.as_ref().unwrap().editor().layout().monitors[0].containers[0]
            .widgets
            .is_empty()
    }
}

impl Drop for Edit {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Every toolbar button, each enabled: pressed, dragged 300 px off it and released there, does
/// nothing. (A disabled button takes no press, as in Qt Quick: it reaches the overlay below.)
#[test]
fn a_toolbar_press_released_outside_its_button_is_canceled() {
    let mut e = Edit::new("cancel", 100, 100);
    // Two moves and an undo: both undo and redo have something to do. A move starts where
    // the pointer has gone its 5 px, so 40 px of pointer carry the clock 35.
    e.drag((200.0, 150.0), (240.0, 150.0));
    e.drag((240.0, 150.0), (280.0, 150.0));
    let undo = e.button("undo");
    let at = e.center(undo);
    e.click(at);
    assert_eq!(e.clock_xy(), (135, 100));
    e.click((240.0, 150.0));
    assert_eq!(e.read("selectedId").to_js_string(), "clock");
    assert!(e.read("canUndo").truthy() && e.read("canRedo").truthy());
    let before = e.saved();
    let icons = ["undo", "redo", "side_navigation", "check"];
    for icon in icons {
        let b = e.button(icon);
        assert!(e.read_of(b, "enabled").truthy(), "{icon} is disabled");
        let (x, y) = e.center(b);
        assert!(y > 300.0, "{icon} at {y}: the toolbar sits at the bottom");
        e.drag((x, y), (x, y - 300.0));
        assert_eq!(
            e.read("selectedId").to_js_string(),
            "clock",
            "{icon} changed the selection"
        );
        assert!(e.read("active").truthy(), "{icon} left edit mode");
        assert_eq!(e.clock_xy(), (135, 100), "{icon} moved the clock");
        assert!(
            e.read("canUndo").truthy() && e.read("canRedo").truthy(),
            "{icon} changed the history"
        );
        assert_eq!(e.saved(), before, "{icon} wrote layout.toml");
        assert!(!e.read_of(b, "down").truthy(), "{icon} stays down");
        assert!(!e.settings_open(), "{icon} opened the settings");
    }
    let sidebar =
        e.rt.world
            .borrow()
            .singletons
            .get("GlobalStates")
            .copied()
            .unwrap();
    assert!(
        !e.read_of(sidebar, "sidebarRightOpen").truthy(),
        "side_navigation toggled the sidebar"
    );
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// A bar widget's width buttons likewise: released outside, nothing; clicked, one edit.
#[test]
fn the_width_buttons_act_only_on_a_click() {
    let mut e = Edit::new("width", 100, 100);
    e.click((660.0, 20.0));
    assert_eq!(e.read("selectedId").to_js_string(), "pclock");
    assert_eq!(e.read("selectedKind").to_js_string(), "panel");
    e.frames(2);
    let before = e.saved();
    for icon in ["fit_width", "expand"] {
        let b = e.button(icon);
        let (x, y) = e.center(b);
        e.drag((x, y), (x + 200.0, y - 300.0));
        assert_eq!(
            e.read("selectedId").to_js_string(),
            "pclock",
            "{icon} changed the selection"
        );
        assert!(!e.read("canUndo").truthy(), "{icon} made an edit");
        assert_eq!(e.saved(), before, "{icon} wrote layout.toml");
    }
    let expand = e.button("expand");
    let at = e.center(expand);
    e.click(at);
    assert!(e.read("canUndo").truthy());
    assert_ne!(e.saved().0, before.0);
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// A click on the trash removes the selected widget with one atomic save; undo puts back the
/// file's exact bytes.
#[test]
fn a_trash_click_removes_the_widget_and_undo_restores_the_file() {
    let mut e = Edit::new("click", 100, 100);
    e.click((200.0, 150.0));
    assert_eq!(e.read("selectedId").to_js_string(), "clock");
    let (bytes, ino) = e.saved();
    // The trash by the selected widget, once it has faded in.
    e.frames(30);
    let trash = e.button("delete");
    let at = e.center(trash);
    e.click(at);
    assert!(!e.has_clock());
    let (after, after_ino) = e.saved();
    assert_ne!(after_ino, ino, "the save replaces the file");
    assert!(!after.contains("id = \"clock\""), "{after}");
    let leftovers: Vec<_> = std::fs::read_dir(&e.dir)
        .unwrap()
        .filter_map(|d| d.ok())
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("layout.toml") && n != "layout.toml" && n != "layout.toml.prev")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    let undo = e.button("undo");
    let at = e.center(undo);
    e.click(at);
    assert!(e.has_clock());
    assert_eq!(e.saved().0, bytes);
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// A widget under the toolbar: a press on the toolbar's own background (its padding, beside
/// the buttons) is no button's, so it reaches the overlay and grabs the widget; a press on a
/// button over it and dragged off is canceled and leaves the widget alone.
#[test]
fn a_widget_under_the_toolbar_can_still_be_grabbed() {
    let probe = Edit::new("probe", 100, 100);
    // The bottom toolbar, made after the one by the selected widget.
    let toolbar = *probe
        .rt
        .world
        .borrow_mut()
        .of_type("Toolbar")
        .last()
        .unwrap();
    let (tx, ty, tw, th) = probe.rect(toolbar);
    let first = probe.button("add");
    let (bx, by, bw, _) = probe.rect(first);
    drop(probe);
    // The clock straddles the toolbar's left end, its first button over it as well.
    let (cx, cy) = ((tx - 20.0) as i32, (ty - 20.0) as i32);
    let mut e = Edit::new("under", cx, cy);
    assert!(
        bx < cx as f64 + 200.0 + 20.0 && by < cy as f64 + 100.0,
        "the clock is not under the first button"
    );
    let pad = (tx + 4.0, ty + th / 2.0);
    assert!(pad.0 < bx && tw > 0.0 && bw > 0.0);
    e.drag(pad, (pad.0 - 100.0, pad.1 - 200.0));
    assert_eq!(e.read("selectedId").to_js_string(), "clock");
    let (x, y) = e.clock_xy();
    assert!(
        x < cx - 50 && y < cy - 150,
        "the clock stayed at {x},{y}, was {cx},{cy}"
    );
    let saved = e.saved().0;
    assert!(
        saved.contains(&format!("x = {x}")) && saved.contains(&format!("y = {y}")),
        "{saved}"
    );
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// "Add widget" opens the catalog; a click on a type's tile adds one widget of it to the
/// desktop, selected and saved, and closes the catalog; undo takes it away again.
#[test]
fn a_catalog_tile_adds_the_widget_selected_and_undo_removes_it() {
    let mut e = Edit::new("add", 100, 100);
    let (bytes, _) = e.saved();
    assert!(!e.palette_open());
    let add = e.button("add");
    let at = e.center(add);
    e.click(at);
    e.frames(30);
    assert!(e.palette_open(), "the catalog did not open");
    assert!(
        e.tile("workspaces").is_none(),
        "a bar-only type is offered for the desktop"
    );
    let tile = e.tile("clock").expect("no clock tile");
    let at = e.center(tile);
    e.click(at);
    e.frames(30);
    assert_eq!(e.ids(0), ["clock", "clock-1"]);
    assert_eq!(e.read("selectedId").to_js_string(), "clock-1");
    assert!(
        !e.palette_open(),
        "the catalog stayed open: {:?}",
        e.rt.errors()
    );
    let saved = e.saved().0;
    assert!(saved.contains("id = \"clock-1\""), "{saved}");
    let undo = e.button("undo");
    let at = e.center(undo);
    e.click(at);
    assert_eq!(e.ids(0), ["clock"]);
    assert_eq!(e.saved().0, bytes);
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// A desktop widget dragged off its catalog tile is added and carried: it is let go under the
/// pointer, not where a click on the tile would have put it, as one more widget of the layout.
#[test]
fn a_catalog_tile_dragged_out_puts_the_widget_where_it_is_let_go() {
    let mut e = Edit::new("carry", 100, 100);
    let add = e.button("add");
    let at = e.center(add);
    e.click(at);
    e.frames(30);
    assert!(e.palette_open(), "the catalog did not open");
    let tile = e.tile("clock").expect("no clock tile");
    let from = e.center(tile);
    let to = (900.0, 600.0);
    e.drag(from, to);
    e.frames(30);
    assert_eq!(e.ids(0), ["clock", "clock-1"]);
    assert_eq!(e.read("selectedId").to_js_string(), "clock-1");
    // A new clock is 200 by 100 here and is held by its middle, give or take the 5 px a
    // pointer goes before it is a drag.
    let (x, y) = e.second_xy();
    assert!(
        (x - 800).abs() <= 5 && (y - 550).abs() <= 5,
        "{x}, {y}: {:?}",
        e.rt.errors()
    );
    assert!(!e.palette_open(), "the catalog stayed open");
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// The settings dialog of a bar widget shows where it sits, and a click on another zone moves
/// it there as one saved edit; undo puts the file back.
#[test]
fn the_settings_dialog_moves_a_bar_widget_to_another_zone() {
    let mut e = Edit::new("zone", 100, 100);
    e.click((660.0, 20.0));
    assert_eq!(e.read("selectedId").to_js_string(), "pclock");
    e.frames(2);
    let (bytes, _) = e.saved();
    let place = |e: &Edit, k: &str| match e.read("selectedPlace") {
        Value::Obj(o) => o.borrow().props.get(k).map(|v| v.to_js_string()),
        _ => None,
    };
    assert_eq!(place(&e, "zone").as_deref(), Some("center"));
    assert_eq!(place(&e, "width").as_deref(), Some("auto"));
    e.frames(30);
    let gear = e.button("tune");
    let at = e.center(gear);
    e.click(at);
    e.frames(30);
    assert!(e.settings_open(), "the settings dialog did not open");
    let end = e.segment("end").expect("no End segment");
    let at = e.center(end);
    e.click(at);
    e.frames(30);
    assert_eq!(
        place(&e, "zone").as_deref(),
        Some("end"),
        "{:?}",
        e.rt.errors()
    );
    assert_eq!(e.read("selectedId").to_js_string(), "pclock");
    let saved = e.saved().0;
    assert!(saved.contains("zone = \"end\""), "{saved}");
    assert!(e.settings_open(), "the dialog closed on a pick");
    let stretch = e.segment("stretch").expect("no Stretch segment");
    let at = e.center(stretch);
    e.click(at);
    e.frames(30);
    assert_eq!(place(&e, "width").as_deref(), Some("stretch"));
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
    // The panel is beside the work, not over it: the toolbar acts with it open, and the two
    // picks are two undo steps.
    let undo = e.button("undo");
    let at = e.center(undo);
    e.click(at);
    e.frames(30);
    assert!(e.settings_open(), "a click on the toolbar closed the panel");
    assert_eq!(place(&e, "width").as_deref(), Some("auto"));
    e.click(at);
    assert_eq!(e.saved().0, bytes);
    // Its close button closes it.
    let close = e.button("close");
    let at = e.center(close);
    e.click(at);
    e.frames(30);
    assert!(!e.settings_open(), "the panel stayed open");
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// The desktop clock places itself by the wallpaper until it is put somewhere by hand: a drop
/// in edit mode makes its placement the original's "free", so it stays where it was dropped.
/// A click that only selects it, and a drop of the bar's clock, leave the option alone.
#[test]
fn a_desktop_widget_dropped_by_hand_is_placed_freely_from_then_on() {
    let mut e = Edit::new("byhand", 100, 100);
    let placed_by = |e: &Edit| {
        let mut w = e.rt.world.borrow_mut();
        w.read(e.bg, "clockPlacedBy").map(|v| v.to_js_string())
    };
    assert_eq!(placed_by(&e).as_deref(), Some("leastBusy"));
    e.click((200.0, 150.0));
    assert_eq!(e.read("selectedId").to_js_string(), "clock");
    e.click((660.0, 20.0));
    let expand = e.button("expand");
    let at = e.center(expand);
    e.click(at);
    e.frames(2);
    assert_eq!(placed_by(&e).as_deref(), Some("leastBusy"));
    e.drag((200.0, 150.0), (500.0, 400.0));
    e.frames(2);
    assert_ne!(e.clock_xy(), (100, 100), "the clock did not move");
    assert_eq!(placed_by(&e).as_deref(), Some("free"));
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// A sidebar knows a new widget's height only once it has laid it out. One the catalog added
/// that the sidebar then has no room for (it would hide it) is taken back: the layout is as
/// before, and the catalog is open again on the sidebar, saying there is no room.
#[test]
fn a_widget_the_sidebar_has_no_room_for_is_taken_back_with_a_notice() {
    let mut e = Edit::new("sidefull", 100, 100);
    let (bytes, _) = e.saved();
    let add = e.button("add");
    let at = e.center(add);
    e.click(at);
    e.frames(30);
    let side = e.surface("sidebar").expect("no sidebar button");
    let at = e.center(side);
    e.click(at);
    e.frames(5);
    assert!(!e.palette_refused());
    let tile = e.tile("clock").expect("no clock tile");
    let at = e.center(tile);
    e.click(at);
    e.frames(30);
    assert_eq!(e.ids(2), ["sclock", "clock-1"]);
    assert!(!e.palette_open(), "the catalog stayed open");
    e.frames(60);
    assert_eq!(e.ids(2), ["sclock"], "the widget stayed in a full sidebar");
    assert_eq!(e.saved().0, bytes);
    assert!(e.palette_open(), "the catalog did not come back");
    assert!(e.palette_refused(), "the catalog does not say why");
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// The selected widget has its outline with a handle on each side and corner; the pointer over
/// one names it (the overlay's pointer shape), over another widget outlines that one. A side's
/// handle pulls that side alone, the opposite one stays; pulled far past the type's largest
/// size the widget stops there and the overlay says so.
#[test]
fn a_handle_resizes_the_widget_from_its_side_and_stops_at_the_types_limits() {
    const RECT: [&str; 4] = ["x", "y", "width", "height"];
    let mut e = Edit::new("handles", 500, 300);
    assert!(e.nums("selectedFrame", RECT).is_none());
    e.click((600.0, 350.0));
    assert_eq!(e.read("selectedId").to_js_string(), "clock");
    assert_eq!(
        e.nums("selectedFrame", RECT),
        Some([500.0, 300.0, 200.0, 100.0])
    );
    for (at, handle) in [
        ((501.0, 350.0), "left"),
        ((699.0, 350.0), "right"),
        ((600.0, 302.0), "top"),
        ((503.0, 398.0), "bottom-left"),
        ((600.0, 350.0), ""),
    ] {
        e.hover(at);
        assert_eq!(e.read("handle").to_js_string(), handle, "at {at:?}");
    }
    // The bar's clock under the pointer: outlined as thick as the bar less its margin,
    // whatever the widget's own height.
    assert!(e.nums("hovered", RECT).is_none(), "the selected widget");
    e.hover((660.0, 20.0));
    let [_, y, _, h] = e
        .nums("hovered", RECT)
        .expect("the bar's clock is not outlined");
    assert_eq!((y, h), (3.0, 34.0));
    // The left side pulled 40 px out, and the pointer 30 px down with it: only the width
    // grows, to the left, less what the pointer went before it was a pull (2 px, 1.6 of
    // them across).
    let before = e.saved().1;
    e.drag((500.0, 350.0), (460.0, 380.0));
    assert_eq!(e.clock_place(), (462, 300, Some((238, 100))));
    assert_ne!(e.saved().1, before, "the resize was not saved");
    assert_eq!(
        e.nums("selectedFrame", RECT),
        Some([462.0, 300.0, 238.0, 100.0])
    );
    // The bottom-right corner pulled past the clock's 800x400: mid-pull the outline is the
    // size it would get and the overlay says it is the largest; dropped, it is that size.
    let win = e.overlay;
    e.ptr.motion(&mut e.rt, win, 699.0, 399.0);
    e.ptr.press(&mut e.rt, BTN_LEFT);
    e.frames(1);
    for to in [(900.0, 500.0), (1279.0, 719.0)] {
        e.ptr.motion(&mut e.rt, win, to.0, to.1);
        e.frames(1);
    }
    assert_eq!(e.read("handle").to_js_string(), "bottom-right");
    assert_eq!(
        e.nums("selectedFrame", RECT),
        Some([462.0, 300.0, 800.0, 400.0])
    );
    let hint = e.read("sizeHint");
    let Value::Obj(hint) = hint else {
        panic!("no size hint mid-resize: {hint:?}")
    };
    let of = |k: &str| hint.borrow().props.get(k).cloned().unwrap();
    assert_eq!(
        (of("width").to_number(), of("height").to_number()),
        (800.0, 400.0)
    );
    assert!(of("largest").truthy() && !of("smallest").truthy());
    e.ptr.release(&mut e.rt, BTN_LEFT);
    e.frames(3);
    assert_eq!(e.clock_place(), (462, 300, Some((800, 400))));
    assert!(matches!(e.read("sizeHint"), Value::Null));
    // The settings panel's size rows go through the same limits.
    assert_eq!(
        e.nums("selectedPlace", ["sizeWidth", "minWidth", "maxWidth"]),
        Some([800.0, 40.0, 800.0])
    );
    let sized =
        e.rt.call_method(e.bg, "setSize", vec![Value::Num(300.0), Value::Num(10.0)])
            .unwrap()
            .unwrap();
    assert!(sized.truthy());
    e.frames(2);
    assert_eq!(e.clock_place(), (462, 300, Some((300, 20))));
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// Let go after a carry, the widget has one outline, at once: the selection's, whole, around
/// the place it landed on, with no hover outline and no landing box beside it, in the frame of
/// the release and in every one after. (It came back in over 220 ms once, declared to; the
/// frames of a carry on a panel had it whole at once beside the carried widget's own.)
#[test]
fn a_widget_let_go_after_a_carry_has_one_outline_at_once() {
    let mut e = Edit::new("carry-outline", 400, 300);
    let find = |e: &Edit, k: &str| {
        let mut w = e.rt.world.borrow_mut();
        (0..w.items.len())
            .find(|&i| {
                !w.dead.contains(&i)
                    && w.read(i, k).is_some_and(|v| !matches!(v, Value::Undefined))
                    && w.read(i, "rect").is_some()
            })
            .unwrap_or_else(|| panic!("no item with {k}"))
    };
    let sel = find(&e, "handleSet");
    let ghost = find(&e, "eased");
    let hover = {
        let mut w = e.rt.world.borrow_mut();
        (0..w.items.len())
            .find(|&i| {
                !w.dead.contains(&i)
                    && i != sel
                    && i != ghost
                    && w.items[i].types.iter().any(|t| t == "Rectangle")
                    && w.read(i, "now")
                        .is_some_and(|v| !matches!(v, Value::Undefined))
            })
            .unwrap()
    };
    let opacity = |e: &Edit, i: usize| e.read_of(i, "opacity").to_number();
    let win = e.overlay;
    let from = (500.0, 350.0);
    e.ptr.enter(&mut e.rt, win, from.0, from.1);
    e.ptr.motion(&mut e.rt, win, from.0, from.1);
    e.frames(20);
    assert_eq!(opacity(&e, hover), 1.0, "the widget under the pointer");
    e.ptr.press(&mut e.rt, BTN_LEFT);
    e.frames(1);
    for k in 1..=30 {
        let t = k as f64 / 30.0;
        e.ptr
            .motion(&mut e.rt, win, from.0 + 203.0 * t, from.1 + 97.0 * t);
        e.frames(1);
    }
    assert!(e.read("dragging").truthy());
    assert_eq!(opacity(&e, sel), 0.0, "no outline while it is carried");
    e.ptr.release(&mut e.rt, BTN_LEFT);
    let landed = e.clock_xy();
    assert_ne!(landed, (400, 300), "the clock moved");
    for k in 0..24 {
        let (x, y, w, h) = e.rect(sel);
        assert_eq!(
            (opacity(&e, sel), opacity(&e, hover), opacity(&e, ghost)),
            (1.0, 0.0, 0.0),
            "frame {k} after the release: the selection, the hover outline, the landing box"
        );
        assert_eq!(
            (x.round(), y.round(), w.round(), h.round()),
            (
                f64::from(landed.0) - 6.0,
                f64::from(landed.1) - 6.0,
                212.0,
                112.0
            ),
            "frame {k} after the release: around the place it landed on"
        );
        e.frames(1);
    }
    assert!(e.rt.errors().is_empty(), "{:?}", e.rt.errors());
}

/// An end of the bar's clock pulled through the overlay's own input (EditOverlay.qml): while
/// the pointer goes on, a step each frame, the panel is not given the pull, only the outline
/// goes with the pointer; 50 ms after the pointer's last step the panel is given the length
/// the outline has. The pointer going on again leaves the panel at that, and the end let go
/// while it moves is written as the outline had it.
#[test]
fn the_panel_is_given_a_pull_once_the_pointer_has_rested() {
    let mut e = Edit::new("rest", 500, 300);
    e.click((660.0, 20.0));
    assert_eq!(e.read("selectedId").to_js_string(), "pclock");
    let [x0, len0] = e.nums("selectedFrame", ["x", "width"]).unwrap();
    // The length the panel shows the pulled clock at, once it is given one.
    let given = |e: &Edit| -> Option<u32> {
        let w = e.rt.world.borrow();
        let (_, c) = w.edit.pulled_panel()?;
        match c.widgets.iter().find(|w| w.id == "pclock")?.place {
            qml::layout_model::Place::Panel {
                width: qml::layout_model::Width::Px(n),
                ..
            } => Some(n),
            _ => None,
        }
    };
    let win = e.overlay;
    let end = x0 + len0 - 2.0;
    e.ptr.motion(&mut e.rt, win, end, 20.0);
    e.ptr.press(&mut e.rt, BTN_LEFT);
    e.frames(1);
    assert_eq!(e.read("handle").to_js_string(), "right");
    let step = |e: &mut Edit, x: f64| -> f64 {
        e.ptr.motion(&mut e.rt, win, x, 20.0);
        e.frames(1);
        let [x, len] = e.nums("selectedFrame", ["x", "width"]).unwrap();
        assert_eq!(x, x0, "the other end");
        len
    };
    let mut len = len0;
    for k in 1..=8 {
        let was = len;
        len = step(&mut e, end + f64::from(k) * 4.0);
        assert!(len > was, "step {k}: the outline goes with the pointer");
        assert_eq!(given(&e), None, "step {k}: the pointer goes on");
    }
    assert_eq!(len, len0 + 30.0);
    // 16 ms a frame: one has gone since the last step, and 48 ms are not yet a rest.
    e.frames(2);
    assert_eq!(given(&e), None, "48 ms after the last step");
    e.frames(1);
    assert_eq!(given(&e), Some(len as u32), "64 ms after the last step");
    assert_eq!(
        e.nums("selectedFrame", ["x", "width"]),
        Some([x0, len]),
        "the outline as the pointer rests"
    );

    let rested = len;
    for k in 9..=10 {
        len = step(&mut e, end + f64::from(k) * 4.0);
        assert_eq!(given(&e), Some(rested as u32), "step {k}: on again");
    }
    assert_eq!(len, rested + 8.0);
    let before = e.saved().1;
    e.ptr.release(&mut e.rt, BTN_LEFT);
    e.frames(1);
    assert_eq!(given(&e), None, "let go");
    let written = e.saved().1;
    assert_ne!(written, before, "written");
    assert!(
        e.saved().0.contains(&format!("width = {len}\n")),
        "{len} px written:\n{}",
        e.saved().0
    );
    // The rest the last step asked for comes to nothing once the end is let go.
    e.frames(5);
    assert_eq!(given(&e), None);
    assert_eq!(e.saved().1, written, "written once");
}
