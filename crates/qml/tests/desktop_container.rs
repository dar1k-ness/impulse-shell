//! `Impulse/DesktopContainer` publishes the desktop container of the edit-mode layout and edits
//! it: widgets land where they are dropped (or on the nearest free spot, never overlapping,
//! never off the monitor, on the grid when `gridSize` is set), resize within their type's size
//! limits, and every accepted edit is written to `layout.toml` and survives a restart.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};

const MANIFEST: &str = r##"
[[widget]]
type = "clock"
name = "Clock"
icon = "schedule"
placements = ["desktop"]
component = "widgets/Clock.qml"
size = { min = [100, 50], max = [800, 400], default = [300, 150] }

[[widget.setting]]
key = "style"
kind = "choice"
options = ["digital", "analog"]
default = "analog"

[[widget.setting]]
key = "placementStrategy"
kind = "choice"
options = ["free", "leastBusy", "mostBusy"]
default = "free"

[[widget]]
type = "bar-only"
name = "Bar only"
icon = "x"
placements = ["panel"]
component = "widgets/Clock.qml"
size = { min = [10, 10], default = [20, 20] }
"##;

const LAYOUT: &str = r#"version = 2

[[monitor]]
connector = "DP-1"
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
settings = { style = "digital" }

[[monitor.container.widget]]
id = "weather"
type = "clock"
x = 1000
y = 600
w = 200
h = 100
"#;

const MAIN: &str = r#"import QtQuick
import Impulse
DesktopContainer {
    width: 1920
    height: 1080
    connector: "DP-1"
    function mv(id: string, x: real, y: real): bool { return placeWidget(id, x, y) }
    function rs(id: string, w: real, h: real): bool { return resizeWidget(id, w, h) }
    function add(t: string, x: real, y: real): string { return addWidget(t, x, y) }
    function rm(id: string): bool { return removeWidget(id) }
    function fit(id: string, x: real, y: real, w: real, h: real): var { return fitWidget(id, x, y, w, h) }
    function natural(id: string, w: real, h: real) { setNaturalSize(id, w, h) }
    function placed(id: string, p: bool) { setSelfPlaced(id, p) }
    function hide(id: string, h: bool) { setHidden(id, h) }
    function move(id: string, x: real, y: real): bool { return moveWidget(id, x, y) }
    function placedAt(id: string, x: real, y: real, w: real, h: real) { setPlacedAt(id, x, y, w, h) }
    function unplacedAt(id: string) { clearPlacedAt(id) }
}
"#;

fn root(tag: &str, layout: Option<&str>) -> PathBuf {
    let d = std::env::temp_dir().join(format!("desktop-container-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("widgets")).unwrap();
    std::fs::write(d.join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(d.join("widgets/Clock.qml"), "import QtQuick\nItem {}\n").unwrap();
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    if let Some(l) = layout {
        std::fs::write(d.join("layout.toml"), l).unwrap();
    }
    d
}

fn start(d: &Path) -> (Runtime, usize) {
    let mut rt = Runtime::new(Some(d.to_path_buf()));
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    (rt, id)
}

fn call(rt: &mut Runtime, id: usize, f: &str, args: Vec<Value>) -> Value {
    let v = rt.call_method(id, f, args).unwrap().unwrap();
    rt.settle();
    v
}

fn valid(rt: &Runtime, id: usize) -> bool {
    rt.world
        .borrow_mut()
        .read(id, "valid")
        .is_some_and(|v| v.truthy())
}

fn n(v: f64) -> Value {
    Value::Num(v)
}

fn s(v: &str) -> Value {
    Value::str(v)
}

/// `widgets` as (id, type, x, y, width, height).
fn widgets(rt: &Runtime, id: usize) -> Vec<(String, String, i64, i64, i64, i64)> {
    let v = rt
        .world
        .borrow_mut()
        .read(id, "widgets")
        .unwrap_or_default();
    let Value::Obj(o) = v else {
        panic!("widgets: {v:?}")
    };
    let arr = o.borrow().array.clone().unwrap_or_default();
    arr.iter()
        .map(|w| {
            let Value::Obj(w) = w else { panic!() };
            let w = w.borrow();
            let num = |k: &str| w.props[k].to_number() as i64;
            let st = |k: &str| w.props[k].to_js_string();
            (
                st("id"),
                st("type"),
                num("x"),
                num("y"),
                num("width"),
                num("height"),
            )
        })
        .collect()
}

fn rect(rt: &Runtime, id: usize, wid: &str) -> (i64, i64, i64, i64) {
    let w = widgets(rt, id);
    let w = w
        .iter()
        .find(|w| w.0 == wid)
        .unwrap_or_else(|| panic!("{wid} in {w:?}"));
    (w.2, w.3, w.4, w.5)
}

/// Where widget `wid` shows (`shown`).
fn shown(rt: &Runtime, id: usize, wid: &str) -> (i64, i64, i64, i64) {
    let v = rt.world.borrow_mut().read(id, "shown").unwrap_or_default();
    let Value::Obj(o) = v else {
        panic!("shown: {v:?}")
    };
    let r = o
        .borrow()
        .props
        .get(wid)
        .cloned()
        .unwrap_or_else(|| panic!("{wid} not shown"));
    let Value::Obj(r) = r else { panic!() };
    let r = r.borrow();
    let n = |k: &str| r.props[k].to_number() as i64;
    (n("x"), n("y"), n("width"), n("height"))
}

fn overlap(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

#[test]
fn publishes_the_layouts_desktop_widgets() {
    let d = root("publish", Some(LAYOUT));
    let (rt, id) = start(&d);
    assert!(valid(&rt, id));
    assert_eq!(
        widgets(&rt, id),
        vec![
            ("clock".into(), "clock".into(), 100, 100, 300, 150),
            ("weather".into(), "clock".into(), 1000, 600, 200, 100),
        ]
    );
    let v = rt.world.borrow_mut().read(id, "widgets").unwrap();
    let Value::Obj(o) = v else { panic!() };
    let first = o.borrow().array.as_ref().unwrap()[0].clone();
    let Value::Obj(first) = first else { panic!() };
    let settings = first.borrow().props["settings"].clone();
    let Value::Obj(settings) = settings else {
        panic!()
    };
    assert_eq!(settings.borrow().props["style"].to_js_string(), "digital");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

#[test]
fn no_layout_means_no_container() {
    let d = root("none", None);
    let (mut rt, id) = start(&d);
    assert!(!valid(&rt, id));
    assert!(widgets(&rt, id).is_empty());
    assert_eq!(
        call(&mut rt, id, "mv", vec![s("clock"), n(1.0), n(1.0)]).to_js_string(),
        "false"
    );
    assert!(
        !d.join("layout.toml").exists(),
        "nothing to save without a container"
    );
}

#[test]
fn free_move_lands_where_dropped_and_is_saved() {
    let d = root("move", Some(LAYOUT));
    let (mut rt, id) = start(&d);
    assert_eq!(
        call(&mut rt, id, "mv", vec![s("clock"), n(412.4), n(287.6)]).to_js_string(),
        "true"
    );
    assert_eq!(rect(&rt, id, "clock"), (412, 288, 300, 150));
    let saved = std::fs::read_to_string(d.join("layout.toml")).unwrap();
    assert!(
        saved.contains("x = 412") && saved.contains("y = 288"),
        "{saved}"
    );
    // A restart reads the saved layout.
    drop(rt);
    let (rt, id) = start(&d);
    assert_eq!(rect(&rt, id, "clock"), (412, 288, 300, 150));
}

#[test]
fn drop_onto_a_widget_lands_beside_it() {
    let d = root("collide", Some(LAYOUT));
    let (mut rt, id) = start(&d);
    // Dropped over weather: weather spans y 600..700, so the clock (150 high) fits below it
    // 120 px away (y 700), nearer than above it 130 px away (y 450).
    call(&mut rt, id, "mv", vec![s("clock"), n(1050.0), n(580.0)]).to_js_string();
    let (c, w) = (rect(&rt, id, "clock"), rect(&rt, id, "weather"));
    assert!(!overlap(c, w), "{c:?} {w:?}");
    assert_eq!(c, (1050, 700, 300, 150));
}

#[test]
fn drop_past_the_edge_stays_on_the_monitor() {
    let d = root("edge", Some(LAYOUT));
    let (mut rt, id) = start(&d);
    call(&mut rt, id, "mv", vec![s("clock"), n(1850.0), n(-90.0)]).to_js_string();
    assert_eq!(rect(&rt, id, "clock"), (1620, 0, 300, 150));
}

#[test]
fn grid_snaps_moves_and_resizes() {
    let d = root("grid", Some(LAYOUT));
    let (mut rt, id) = start(&d);
    rt.world.borrow_mut().write(id, "gridSize", n(40.0));
    rt.settle();
    call(&mut rt, id, "mv", vec![s("clock"), n(133.0), n(219.0)]).to_js_string();
    assert_eq!(rect(&rt, id, "clock"), (120, 200, 300, 150));
    call(&mut rt, id, "rs", vec![s("clock"), n(333.0), n(171.0)]);
    assert_eq!(rect(&rt, id, "clock"), (120, 200, 320, 160));
}

#[test]
fn resize_respects_type_limits_and_neighbours() {
    let d = root("resize", Some(LAYOUT));
    let (mut rt, id) = start(&d);
    // Type max is 800x400.
    call(&mut rt, id, "rs", vec![s("clock"), n(5000.0), n(5000.0)]);
    assert_eq!(rect(&rt, id, "clock"), (100, 100, 800, 400));
    // Type min is 100x50.
    call(&mut rt, id, "rs", vec![s("clock"), n(1.0), n(1.0)]);
    assert_eq!(rect(&rt, id, "clock"), (100, 100, 100, 50));
    // Growing into weather stops at its edge.
    call(&mut rt, id, "mv", vec![s("clock"), n(600.0), n(600.0)]).to_js_string();
    call(&mut rt, id, "rs", vec![s("clock"), n(700.0), n(100.0)]);
    let (c, w) = (rect(&rt, id, "clock"), rect(&rt, id, "weather"));
    assert!(!overlap(c, w), "{c:?} {w:?}");
    assert_eq!(c, (600, 600, 400, 100));
}

#[test]
fn add_and_remove_widgets() {
    let d = root("add", Some(LAYOUT));
    let (mut rt, id) = start(&d);
    // Dropped onto the clock: placed free, default size, fresh id.
    let new = call(&mut rt, id, "add", vec![s("clock"), n(150.0), n(120.0)]);
    assert_eq!(new.to_js_string(), "clock-2");
    let r = rect(&rt, id, "clock-2");
    assert_eq!((r.2, r.3), (300, 150));
    assert!(!overlap(r, rect(&rt, id, "clock")));
    // A panel-only type is not accepted on the desktop.
    let bad = call(&mut rt, id, "add", vec![s("bar-only"), n(0.0), n(0.0)]);
    assert_eq!(bad.to_js_string(), "");
    assert_eq!(
        call(&mut rt, id, "rm", vec![s("clock")]).to_js_string(),
        "true"
    );
    let ids: Vec<_> = widgets(&rt, id).into_iter().map(|w| w.0).collect();
    assert_eq!(ids, vec!["weather", "clock-2"]);
    assert_eq!(
        call(&mut rt, id, "rm", vec![s("nope")]).to_js_string(),
        "false"
    );
}

#[test]
fn edits_are_undoable_and_own_saves_keep_history() {
    let d = root("undo", Some(LAYOUT));
    let (mut rt, id) = start(&d);
    call(&mut rt, id, "mv", vec![s("clock"), n(500.0), n(100.0)]).to_js_string();
    // The inotify event of our own save must not reset the editor.
    Loop::new(&mut rt).drain(&mut rt);
    let can_undo = rt
        .world
        .borrow()
        .layout
        .as_ref()
        .unwrap()
        .editor()
        .can_undo();
    assert!(can_undo);
    let undone = rt.world.borrow_mut().layout.as_mut().unwrap().undo();
    assert_eq!(undone, Ok(true));
    rt.settle();
    assert_eq!(rect(&rt, id, "clock"), (100, 100, 300, 150));
}

#[test]
fn external_edits_to_the_file_are_picked_up() {
    let d = root("reload", Some(LAYOUT));
    let (mut rt, id) = start(&d);
    let mut lp = Loop::new(&mut rt);
    // Change the file behind the shell's back.
    edit_layout(
        &mut rt,
        &mut lp,
        &d,
        &LAYOUT.replace("x = 100", "x = 700"),
        |rt| rect(rt, id, "clock").0 == 700,
    );
    assert_eq!(rect(&rt, id, "clock"), (700, 100, 300, 150));
}

/// A size the layout holds above the type's largest (written when the limit was higher, or by
/// hand) is shown at the largest, at the start and after the file changes; the file keeps
/// its numbers until an edit writes the layout, and going back over that edit does not bring
/// the oversize back.
#[test]
fn a_stored_size_over_the_limit_is_shown_at_the_limit() {
    // Type max is 800x400.
    let big = LAYOUT.replace("w = 300\nh = 150", "w = 1600\nh = 1000");
    assert_ne!(big, LAYOUT);
    let d = root("oversize", Some(&big));
    let (mut rt, id) = start(&d);
    assert_eq!(rect(&rt, id, "clock"), (100, 100, 800, 400));
    assert_eq!(shown(&rt, id, "clock"), (100, 100, 800, 400));
    // One side over only: the other is left as stored.
    assert_eq!(rect(&rt, id, "weather"), (1000, 600, 200, 100));
    let on_disk = || std::fs::read_to_string(d.join("layout.toml")).unwrap();
    assert_eq!(on_disk(), big, "the start rewrote the file");

    let mut lp = Loop::new(&mut rt);
    edit_layout(
        &mut rt,
        &mut lp,
        &d,
        &big.replace("w = 200\nh = 100", "w = 900\nh = 100"),
        |rt| rect(rt, id, "weather").2 == 800,
    );
    assert_eq!(rect(&rt, id, "weather"), (1000, 600, 800, 100));
    assert_eq!(rect(&rt, id, "clock"), (100, 100, 800, 400));

    // An edit writes what is shown.
    call(&mut rt, id, "mv", vec![s("clock"), n(40.0), n(40.0)]).to_js_string();
    lp.drain(&mut rt);
    assert!(!on_disk().contains("1600"), "{}", on_disk());
    assert!(on_disk().contains("w = 800\nh = 400"), "{}", on_disk());
    let undone = rt.world.borrow_mut().layout.as_mut().unwrap().undo();
    assert_eq!(undone, Ok(true));
    rt.settle();
    assert_eq!(rect(&rt, id, "clock"), (100, 100, 800, 400));
}

/// The original's `LayoutState.hasContainer("desktop")` (the switch between Background.qml's own widgets
/// and this container) follows the layout natively.
#[test]
fn layout_state_reports_the_desktop_container() {
    let d = root("state", Some(LAYOUT));
    std::fs::create_dir_all(d.join("modules/common")).unwrap();
    let ri = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    std::fs::copy(
        ri.join("modules/common/LayoutState.qml"),
        d.join("modules/common/LayoutState.qml"),
    )
    .unwrap();
    std::fs::write(
        d.join("state.qml"),
        "import QtQuick\nimport ri.modules.common\nItem { property bool desktop: LayoutState.hasContainer(\"desktop\")\n property bool panel: LayoutState.hasContainer(\"panel\") }\n",
    )
    .unwrap();
    let mut rt = Runtime::new(Some(d.clone()));
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.load_singletons(&[d.join("modules/common")]);
    rt.settle();
    let id = rt.load_file(&d.join("state.qml")).unwrap();
    rt.settle();
    let b = |rt: &Runtime, k: &str| {
        rt.world
            .borrow_mut()
            .read(id, k)
            .is_some_and(|v| v.truthy())
    };
    assert!(b(&rt, "desktop"));
    assert!(!b(&rt, "panel"));
    // Removing the container (an external edit) switches the original back to its own widgets.
    let no_desktop = "version = 2\n\n[[monitor]]\nconnector = \"DP-1\"\nprimary = true\n";
    let mut lp = Loop::new(&mut rt);
    edit_layout(&mut rt, &mut lp, &d, no_desktop, |rt| !b(rt, "desktop"));
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
}

/// No config directory at startup (a fresh account): the layout written there later is read.
#[test]
fn layout_created_after_startup_is_picked_up() {
    let d = root("late", None);
    let dir = d.join("config/rusty-impulse");
    let mut rt = Runtime::new(Some(d.clone()));
    rt.world.borrow_mut().open_layout(dir.join("layout.toml"));
    let id = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    assert!(!valid(&rt, id));
    let mut lp = Loop::new(&mut rt);
    edit_layout(&mut rt, &mut lp, &dir, LAYOUT, |rt| valid(rt, id));
    assert_eq!(rect(&rt, id, "clock"), (100, 100, 300, 150));
}

/// Placed for 1920x1080: the clock at the bottom left, the weather above it.
const BOTTOM_LEFT: &str = r#"version = 2

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "clock"
x = 100
y = 800
w = 300
h = 150

[[monitor.container.widget]]
id = "weather"
type = "clock"
x = 100
y = 500
w = 200
h = 100
"#;

/// The monitor sizes a 1920x1080 layout meets after a mode or scale change, in logical pixels.
const SMALLER: [(&str, f64, f64); 3] = [
    ("1280x720", 1280.0, 720.0),
    ("1920x1080 at scale 1.5", 1280.0, 720.0),
    ("2560x1440 at scale 1.5", 1707.0, 960.0),
];

fn resize(rt: &mut Runtime, id: usize, w: f64, h: f64) {
    rt.set(id, "width", n(w));
    rt.set(id, "height", n(h));
    rt.settle();
}

fn inside(r: (i64, i64, i64, i64), w: f64, h: f64) -> bool {
    r.0 >= 0 && r.1 >= 0 && (r.0 + r.2) as f64 <= w && (r.1 + r.3) as f64 <= h
}

/// After a resolution or scale change the widgets stay on the monitor and off each other, as
/// near their place as they fit; the file keeps the layout, and the monitor it was made for
/// shows it unchanged again.
#[test]
fn a_smaller_monitor_shows_the_widgets_fitted_and_the_layout_unchanged() {
    let d = root("fit", Some(BOTTOM_LEFT));
    let (mut rt, id) = start(&d);
    assert_eq!(shown(&rt, id, "clock"), (100, 800, 300, 150));
    assert_eq!(shown(&rt, id, "weather"), (100, 500, 200, 100));
    for (what, w, h) in SMALLER {
        resize(&mut rt, id, w, h);
        let (clock, weather) = (shown(&rt, id, "clock"), shown(&rt, id, "weather"));
        assert!(
            inside(clock, w, h) && inside(weather, w, h),
            "{what}: {clock:?} {weather:?}"
        );
        assert!(!overlap(clock, weather), "{what}: {clock:?} {weather:?}");
        // Kept where it is when that still fits, else just pulled up onto the monitor.
        assert_eq!(clock, (100, 800.min(h as i64 - 150), 300, 150), "{what}");
    }
    // The layout's entries stay as laid out.
    assert_eq!(rect(&rt, id, "clock"), (100, 800, 300, 150));
    resize(&mut rt, id, 1920.0, 1080.0);
    assert_eq!(shown(&rt, id, "clock"), (100, 800, 300, 150));
    assert_eq!(shown(&rt, id, "weather"), (100, 500, 200, 100));
    assert_eq!(
        std::fs::read_to_string(d.join("layout.toml")).unwrap(),
        BOTTOM_LEFT
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

/// A widget placed by the wallpaper (the original's least busy region) asks where it shows: where it
/// wants when that fits, otherwise the nearest place on the monitor and off the other widgets.
#[test]
fn a_wallpaper_placed_widget_is_fitted_on_the_monitor_and_off_the_others() {
    let d = root("fitwidget", Some(BOTTOM_LEFT));
    let (mut rt, id) = start(&d);
    let fit = |rt: &mut Runtime, x: f64, y: f64| {
        let v = call(
            rt,
            id,
            "fit",
            vec![s("clock"), n(x), n(y), n(300.0), n(150.0)],
        );
        let Value::Obj(o) = v else { panic!("{v:?}") };
        let o = o.borrow();
        (
            o.props["x"].to_number() as i64,
            o.props["y"].to_number() as i64,
            300,
            150,
        )
    };
    // Where the original put it on 1920x1080 fits there: no jump.
    assert_eq!(fit(&mut rt, 100.0, 800.0), (100, 800, 300, 150));
    for (what, w, h) in SMALLER {
        resize(&mut rt, id, w, h);
        let weather = shown(&rt, id, "weather");
        let at = fit(&mut rt, 100.0, 800.0);
        assert!(inside(at, w, h), "{what}: {at:?}");
        assert!(!overlap(at, weather), "{what}: {at:?} on {weather:?}");
        // Straight onto the weather: beside it instead.
        let at = fit(&mut rt, weather.0 as f64, weather.1 as f64);
        assert!(
            inside(at, w, h) && !overlap(at, weather),
            "{what}: {at:?} on {weather:?}"
        );
        // A place that fits stays.
        assert_eq!(fit(&mut rt, 700.0, 100.0), (700, 100, 300, 150), "{what}");
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

/// A view of the desktop (the original's DesktopLayoutContent) places its delegates by `shown`: a
/// resolution or scale change moves them without making them again, so what the widgets
/// computed (the original's wallpaper placement) is kept.
#[test]
fn a_monitor_size_change_keeps_the_views_delegates() {
    let d = root("keep", Some(BOTTOM_LEFT));
    std::fs::write(
        d.join("main.qml"),
        r#"import QtQuick
import Impulse
DesktopContainer {
    id: root
    width: 1920
    height: 1080
    connector: "DP-1"
    property int made: 0
    property real clockY: -1
    Repeater {
        model: root.widgets
        delegate: Item {
            required property var modelData
            readonly property var box: root.shown[modelData.id] ?? modelData
            y: box.y
            onYChanged: if (modelData.id === "clock") root.clockY = y
            Component.onCompleted: root.made++
        }
    }
}
"#,
    )
    .unwrap();
    let (mut rt, id) = start(&d);
    let num = |rt: &Runtime, k: &str| rt.world.borrow_mut().read(id, k).unwrap().to_number();
    assert_eq!(num(&rt, "made"), 2.0);
    for (what, w, h) in SMALLER {
        resize(&mut rt, id, w, h);
        assert_eq!(num(&rt, "made"), 2.0, "{what}: delegates made again");
        assert_eq!(num(&rt, "clockY"), 800f64.min(h - 150.0), "{what}");
    }
    resize(&mut rt, id, 1920.0, 1080.0);
    assert_eq!((num(&rt, "made"), num(&rt, "clockY")), (2.0, 800.0));
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

/// The clock fits by what it paints: with its status row empty it is 15 px shorter, so on
/// 1280x720 a clock whose painted part ends at the bottom edge stays exactly where it is.
#[test]
fn a_wallpaper_placed_clock_on_the_bottom_edge_does_not_move() {
    let d = root("paintedfit", Some(BOTTOM_LEFT));
    let (mut rt, id) = start(&d);
    for (what, w, h) in SMALLER {
        resize(&mut rt, id, w, h);
        let painted = 160.0;
        let y = h - painted;
        let v = call(
            &mut rt,
            id,
            "fit",
            vec![s("clock"), n(700.0), n(y), n(225.0), n(painted)],
        );
        let Value::Obj(o) = v else { panic!("{v:?}") };
        let o = o.borrow();
        assert_eq!(
            (o.props["x"].to_number(), o.props["y"].to_number()),
            (700.0, y),
            "{what}: 0 px shift"
        );
        let whole = call(
            &mut rt,
            id,
            "fit",
            vec![s("clock"), n(700.0), n(y), n(225.0), n(painted + 15.0)],
        );
        let Value::Obj(o) = whole else {
            panic!("{whole:?}")
        };
        assert_eq!(
            o.borrow().props["y"].to_number(),
            y - 15.0,
            "{what}: the whole box would be lifted"
        );
    }
    std::fs::remove_dir_all(&d).unwrap();
}

const NATURAL: &str = r#"version = 2

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "clock"
x = 1800
y = 900
settings = { placementStrategy = "free" }
"#;

#[test]
fn a_widget_without_a_size_takes_its_natural_size() {
    let d = root("natural", Some(NATURAL));
    let (mut rt, id) = start(&d);
    // Until its view measured it: the type's default box, kept on the monitor.
    assert_eq!(shown(&rt, id, "clock"), (1620, 900, 300, 150));
    let delegates = rt.world.borrow_mut().read(id, "widgets");
    // QML sizes an item by its implicit size; the original keeps the clock on the monitor by it
    // (`x = min(x, screenWidth - width)`), it never shrinks it.
    call(&mut rt, id, "natural", vec![s("clock"), n(230.0), n(254.6)]);
    assert_eq!(shown(&rt, id, "clock"), (1690, 825, 230, 255));
    assert!(
        matches!((delegates, rt.world.borrow_mut().read(id, "widgets")), (Some(Value::Obj(a)), Some(Value::Obj(b))) if std::rc::Rc::ptr_eq(&a, &b)),
        "a measured size keeps the views' delegates"
    );
    // A move keeps it natural (nothing to save for its size); a resize in edit mode sets one.
    assert!(call(&mut rt, id, "mv", vec![s("clock"), n(1000.0), n(100.0)]).truthy());
    assert_eq!(shown(&rt, id, "clock"), (1000, 100, 230, 255));
    let saved = std::fs::read_to_string(d.join("layout.toml")).unwrap();
    assert!(!saved.contains("w =") && !saved.contains("h ="), "{saved}");
    assert!(call(&mut rt, id, "rs", vec![s("clock"), n(400.0), n(300.0)]).truthy());
    assert_eq!(rect(&rt, id, "clock"), (1000, 100, 400, 300));
    call(&mut rt, id, "natural", vec![s("clock"), n(240.0), n(260.0)]);
    assert_eq!(
        shown(&rt, id, "clock"),
        (1000, 100, 400, 300),
        "a set size stays"
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

/// `widgets[id]`'s `settings` (as `key=value` sorted) and `settingErrors` count.
fn settings_of(rt: &Runtime, id: usize, widget: &str) -> (String, usize) {
    let Some(Value::Obj(o)) = rt.world.borrow_mut().read(id, "widgets") else {
        panic!("widgets")
    };
    let arr = o.borrow().array.clone().unwrap_or_default();
    let w = arr
        .iter()
        .find_map(|w| match w {
            Value::Obj(w) if w.borrow().props["id"].to_js_string() == widget => Some(w.clone()),
            _ => None,
        })
        .expect("widget");
    let w = w.borrow();
    let Value::Obj(set) = &w.props["settings"] else {
        panic!("settings")
    };
    let mut kv: Vec<String> = set
        .borrow()
        .props
        .iter()
        .map(|(k, v)| format!("{k}={}", v.to_js_string()))
        .collect();
    kv.sort();
    let Value::Obj(errs) = &w.props["settingErrors"] else {
        panic!("settingErrors")
    };
    let n = errs.borrow().array.as_ref().map_or(0, Vec::len);
    (kv.join(","), n)
}

#[test]
fn widgets_carry_resolved_settings_and_their_errors() {
    let src = LAYOUT.replace(
        "settings = { style = \"digital\" }",
        "settings = { style = \"sundial\", extra = 1 }",
    );
    assert_ne!(src, LAYOUT);
    let d = root("resolved", Some(&src));
    let (rt, id) = start(&d);
    assert_eq!(
        settings_of(&rt, id, "clock"),
        ("placementStrategy=free,style=analog".to_string(), 2)
    );
    assert_eq!(
        settings_of(&rt, id, "weather"),
        ("placementStrategy=free,style=analog".to_string(), 0)
    );
    assert_eq!(rt.errors().len(), 2, "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

const TWO_NATURAL: &str = r#"version = 2

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "desktop-clock"
x = 100
y = 100
settings = { placementStrategy = "free" }

[[monitor.container.widget]]
id = "weather"
type = "desktop-clock"
x = 500
y = 100
settings = { placementStrategy = "free" }
"#;

/// A natural-size widget that grows onto another (its text changed, a font loaded) moves
/// aside itself; the one it grew onto stays, whatever their order in the file.
#[test]
fn a_widget_that_grows_onto_another_moves_aside_itself() {
    let d = root("grow", Some(TWO_NATURAL));
    let (mut rt, id) = start(&d);
    call(&mut rt, id, "natural", vec![s("clock"), n(200.0), n(100.0)]);
    call(
        &mut rt,
        id,
        "natural",
        vec![s("weather"), n(200.0), n(100.0)],
    );
    assert_eq!(shown(&rt, id, "clock"), (100, 100, 200, 100));
    assert_eq!(shown(&rt, id, "weather"), (500, 100, 200, 100));
    // The clock, first in the file, grows onto the weather: the clock moves.
    call(&mut rt, id, "natural", vec![s("clock"), n(500.0), n(100.0)]);
    let clock = shown(&rt, id, "clock");
    assert_eq!(shown(&rt, id, "weather"), (500, 100, 200, 100));
    assert!(!overlap(clock, (500, 100, 200, 100)), "{clock:?}");
    assert_eq!((clock.2, clock.3), (500, 100));
    // Then the weather grows onto the clock's own place: now the weather moves, and the
    // clock, no longer the last to change, shows where the layout puts it again.
    call(
        &mut rt,
        id,
        "natural",
        vec![s("weather"), n(200.0), n(400.0)],
    );
    let (clock, weather) = (shown(&rt, id, "clock"), shown(&rt, id, "weather"));
    assert_eq!(clock, (100, 100, 500, 100));
    assert!(!overlap(clock, weather), "{clock:?} {weather:?}");
    assert_eq!((weather.2, weather.3), (200, 400));
    assert_eq!(
        std::fs::read_to_string(d.join("layout.toml")).unwrap(),
        TWO_NATURAL
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

/// `fitWidget` answers from the fitted widgets, which follow every input they come from: the
/// monitor size, a natural size and the layout file.
#[test]
fn fit_widget_follows_size_natural_size_and_layout_changes() {
    let d = root("fitcache", Some(TWO_NATURAL));
    let (mut rt, id) = start(&d);
    let fit = |rt: &mut Runtime| {
        let v = call(
            rt,
            id,
            "fit",
            vec![s("w"), n(500.0), n(150.0), n(10.0), n(10.0)],
        );
        let Value::Obj(o) = v else { panic!("{v:?}") };
        let o = o.borrow();
        (
            o.props["x"].to_number() as i64,
            o.props["y"].to_number() as i64,
        )
    };
    // The weather shows at its default 300x150 box: (500, 150) is taken, so just left of it.
    assert_eq!(fit(&mut rt), (490, 150));
    // Asking again answers the same.
    assert_eq!(fit(&mut rt), (490, 150));
    // A smaller natural size frees the spot.
    call(
        &mut rt,
        id,
        "natural",
        vec![s("weather"), n(100.0), n(40.0)],
    );
    assert_eq!(fit(&mut rt), (500, 150));
    // The layout moves the weather onto it: just above it then.
    let mut lp = Loop::new(&mut rt);
    edit_layout(
        &mut rt,
        &mut lp,
        &d,
        &TWO_NATURAL.replace("x = 500\ny = 100", "x = 480\ny = 140"),
        |rt| fit(rt) != (500, 150),
    );
    assert_eq!(fit(&mut rt), (500, 130));
    // A monitor too small for the spot: inside it, off the widgets as they show there.
    resize(&mut rt, id, 400.0, 300.0);
    let (x, y) = fit(&mut rt);
    let at = (x, y, 10, 10);
    assert!(inside(at, 400.0, 300.0), "{at:?}");
    for wid in ["clock", "weather"] {
        let r = shown(&rt, id, wid);
        assert!(
            inside(r, 400.0, 300.0) && !overlap(at, r),
            "{at:?} on {wid} {r:?}"
        );
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

/// A widget that places itself (the original's wallpaper placement, `setSelfPlaced`) is not where the
/// layout puts it: the others show where they are laid out instead of keeping off that empty
/// place, and it fits itself off them (`fitWidget`). Laid out again, the others keep off it.
#[test]
fn a_self_placed_widget_keeps_no_place_in_the_layout() {
    let layout = LAYOUT.replace("x = 1000\ny = 600", "x = 200\ny = 150");
    let d = root("selfplaced", Some(&layout));
    let (mut rt, id) = start(&d);
    // Laid out, the clock keeps its place and the weather moves off it.
    assert_eq!(shown(&rt, id, "clock"), (100, 100, 300, 150));
    let laid = shown(&rt, id, "weather");
    assert!(!overlap(laid, (100, 100, 300, 150)), "{laid:?}");
    call(&mut rt, id, "placed", vec![s("clock"), Value::Bool(true)]);
    assert_eq!(shown(&rt, id, "weather"), (200, 150, 200, 100));
    let fit = |rt: &mut Runtime, wid: &str, x: f64, y: f64| {
        let v = call(rt, id, "fit", vec![s(wid), n(x), n(y), n(300.0), n(150.0)]);
        let Value::Obj(o) = v else { panic!("{v:?}") };
        let o = o.borrow();
        (
            o.props["x"].to_number() as i64,
            o.props["y"].to_number() as i64,
            300,
            150,
        )
    };
    // Where the wallpaper puts it: off the weather, which is where it is laid out.
    let at = fit(&mut rt, "clock", 150.0, 150.0);
    assert!(!overlap(at, (200, 150, 200, 100)), "{at:?}");
    assert_eq!(fit(&mut rt, "clock", 1200.0, 700.0), (1200, 700, 300, 150));
    // Two widgets that place themselves: neither keeps off the other's layout place.
    call(&mut rt, id, "placed", vec![s("weather"), Value::Bool(true)]);
    assert_eq!(fit(&mut rt, "clock", 150.0, 150.0), (150, 150, 300, 150));
    call(
        &mut rt,
        id,
        "placed",
        vec![s("weather"), Value::Bool(false)],
    );
    call(&mut rt, id, "placed", vec![s("clock"), Value::Bool(false)]);
    assert_eq!(shown(&rt, id, "clock"), (100, 100, 300, 150));
    assert_eq!(shown(&rt, id, "weather"), laid);
    assert_eq!(
        std::fs::read_to_string(d.join("layout.toml")).unwrap(),
        layout
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

/// `moveWidget` (the drag of the original's "Draggable" clock and weather): the widget gets the place in
/// the layout, at its size, the view shows it there at once and `layout.toml` holds it, so
/// the next start shows it there too. A place off the monitor is the layout's as asked and
/// shows fitted; a widget the container does not have changes nothing.
#[test]
fn a_moved_widget_shows_at_its_new_place_and_the_layout_file_keeps_it() {
    let d = root("move", Some(LAYOUT));
    let (mut rt, id) = start(&d);
    let mv = |rt: &mut Runtime, wid: &str, x: f64, y: f64| {
        call(rt, id, "move", vec![s(wid), n(x), n(y)]).truthy()
    };
    assert!(mv(&mut rt, "clock", 640.4, 219.6));
    assert_eq!(rect(&rt, id, "clock"), (640, 220, 300, 150));
    assert_eq!(shown(&rt, id, "clock"), (640, 220, 300, 150));
    assert_eq!(rect(&rt, id, "weather"), (1000, 600, 200, 100));
    let on_disk = |d: &Path| {
        let text = std::fs::read_to_string(d.join("layout.toml")).unwrap();
        let t: toml::Table = text.parse().unwrap();
        let ws = t["monitor"][0]["container"][0]["widget"]
            .as_array()
            .unwrap();
        let places: Vec<(String, i64, i64, i64, i64)> = ws
            .iter()
            .map(|w| {
                let n = |k: &str| w[k].as_integer().unwrap();
                let id = w["id"].as_str().unwrap().to_string();
                (id, n("x"), n("y"), n("w"), n("h"))
            })
            .collect();
        places
    };
    assert_eq!(
        on_disk(&d),
        [
            ("clock".to_string(), 640, 220, 300, 150),
            ("weather".to_string(), 1000, 600, 200, 100)
        ]
    );
    // The same place again is no change.
    assert!(!mv(&mut rt, "clock", 640.0, 220.0));
    // Dropped half off the monitor: the layout has the place, the view shows it on the monitor.
    assert!(mv(&mut rt, "weather", 1900.0, 1050.0));
    assert_eq!(rect(&rt, id, "weather"), (1900, 1050, 200, 100));
    assert_eq!(shown(&rt, id, "weather"), (1720, 980, 200, 100));
    assert!(!mv(&mut rt, "nobody", 1.0, 2.0));
    assert!(!mv(&mut rt, "clock", f64::NAN, 2.0));
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    drop(rt);
    // The next start reads the file the move wrote.
    let (rt, id) = start(&d);
    assert_eq!(rect(&rt, id, "clock"), (640, 220, 300, 150));
    assert_eq!(rect(&rt, id, "weather"), (1900, 1050, 200, 100));
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

/// A hidden widget (the original's settings turned it off, `setHidden`) shows nowhere: it has no place
/// in `shown`, the others show where they are laid out instead of keeping off it, and the
/// layout keeps it. Shown again, it takes its place back.
#[test]
fn a_hidden_widget_takes_no_place() {
    let layout = LAYOUT.replace("x = 1000\ny = 600", "x = 200\ny = 150");
    let d = root("hidden", Some(&layout));
    let (mut rt, id) = start(&d);
    assert_eq!(shown(&rt, id, "clock"), (100, 100, 300, 150));
    let laid = shown(&rt, id, "weather");
    assert!(!overlap(laid, (100, 100, 300, 150)), "{laid:?}");
    let has = |rt: &Runtime, wid: &str| match rt.world.borrow_mut().read(id, "shown") {
        Some(Value::Obj(o)) => o.borrow().props.contains_key(wid),
        v => panic!("shown: {v:?}"),
    };
    call(&mut rt, id, "hide", vec![s("clock"), Value::Bool(true)]);
    assert!(!has(&rt, "clock"));
    assert_eq!(shown(&rt, id, "weather"), (200, 150, 200, 100));
    call(&mut rt, id, "hide", vec![s("clock"), Value::Bool(false)]);
    assert_eq!(shown(&rt, id, "clock"), (100, 100, 300, 150));
    assert_eq!(shown(&rt, id, "weather"), laid);
    assert_eq!(
        std::fs::read_to_string(d.join("layout.toml")).unwrap(),
        layout
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

/// Writes `text` to `d`/layout.toml behind the shell's back once everything pending has run
/// (the watch is armed), then runs the loop on the events that write causes until `done`.
fn edit_layout(
    rt: &mut Runtime,
    lp: &mut Loop,
    d: &Path,
    text: &str,
    done: impl FnMut(&mut Runtime) -> bool,
) {
    lp.drain(rt);
    std::fs::write(d.join("layout.toml"), text).unwrap();
    assert!(lp.until(rt, done), "the edit did not reach the view");
}

/// What a view measured goes with its widget: one removed and added again, or with other
/// settings under the same id, starts from its type's default box, while a widget the edit
/// only moves keeps its measured size.
#[test]
fn a_measured_size_goes_with_its_widget() {
    let d = root("natural-lifecycle", Some(NATURAL));
    let (mut rt, id) = start(&d);
    let mut lp = Loop::new(&mut rt);
    call(&mut rt, id, "natural", vec![s("clock"), n(230.0), n(254.6)]);
    assert_eq!(shown(&rt, id, "clock"), (1690, 825, 230, 255));
    // Moved: the same widget, still its measured size.
    let moved = NATURAL.replace("x = 1800", "x = 1700");
    edit_layout(&mut rt, &mut lp, &d, &moved, |rt| {
        rect(rt, id, "clock").0 == 1700
    });
    assert_eq!(shown(&rt, id, "clock"), (1690, 825, 230, 255));
    // Removed, then added again: the type's default box until its new view measures it.
    let empty = NATURAL
        .split("[[monitor.container.widget]]")
        .next()
        .unwrap();
    edit_layout(&mut rt, &mut lp, &d, empty, |rt| widgets(rt, id).is_empty());
    edit_layout(&mut rt, &mut lp, &d, NATURAL, |rt| {
        widgets(rt, id).len() == 1
    });
    assert_eq!(shown(&rt, id, "clock"), (1620, 900, 300, 150));
    // Other settings (which size it) under the same id take nothing of it either.
    call(&mut rt, id, "natural", vec![s("clock"), n(230.0), n(254.6)]);
    assert_eq!(shown(&rt, id, "clock"), (1690, 825, 230, 255));
    let restyled = NATURAL.replace(
        "placementStrategy = \"free\"",
        "placementStrategy = \"free\", style = \"digital\"",
    );
    edit_layout(&mut rt, &mut lp, &d, &restyled, |rt| {
        shown(rt, id, "clock").2 == 300
    });
    assert_eq!(shown(&rt, id, "clock"), (1620, 900, 300, 150));
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

/// A layout made from the original before its desktop kept the widgets the original's settings had off (schema
/// version 1): a hand-placed clock and no weather.
const V1_WITHOUT_WEATHER: &str = r#"version = 1

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "desktop-clock"
type = "desktop-clock"
x = 235
y = 545
w = 300
h = 150
settings = { style = "digital", placementStrategy = "free" }
"#;

const WEATHER_TYPE: &str = r#"
[[widget]]
type = "weather"
name = "Weather"
icon = "partly_cloudy_day"
placements = ["desktop"]
component = "widgets/Clock.qml"
enabledBy = "background.widgets.weather.enable"
size = { min = [100, 100], max = [800, 800], default = [200, 200] }

[[widget.setting]]
key = "placementStrategy"
kind = "choice"
options = ["free", "leastBusy", "mostBusy"]
default = "free"
"#;

/// An old layout without the original's weather gets it where the original puts it, tied to the original's weather setting,
/// so turning that setting on shows it; the clock keeps its place and the file is not written.
#[test]
fn an_old_layout_gets_the_weather_its_setting_shows() {
    let d = root("v1-weather", Some(V1_WITHOUT_WEATHER));
    std::fs::write(d.join("widgets.toml"), format!("{MANIFEST}{WEATHER_TYPE}")).unwrap();
    let (rt, id) = start(&d);
    assert!(valid(&rt, id));
    assert_eq!(
        widgets(&rt, id),
        vec![
            (
                "desktop-weather".into(),
                "weather".into(),
                400,
                100,
                200,
                200
            ),
            ("desktop-clock".into(), "clock".into(), 235, 545, 300, 150),
        ]
    );
    let enabled_by = match rt.world.borrow_mut().read(id, "widgets") {
        Some(Value::Obj(o)) => match &o.borrow().array.as_ref().unwrap()[0] {
            Value::Obj(w) => w.borrow().props["enabledBy"].to_js_string(),
            v => panic!("{v:?}"),
        },
        v => panic!("widgets: {v:?}"),
    };
    assert_eq!(enabled_by, "background.widgets.weather.enable");
    assert_eq!(shown(&rt, id, "desktop-weather"), (400, 100, 200, 200));
    assert_eq!(shown(&rt, id, "desktop-clock"), (235, 545, 300, 150));
    assert_eq!(
        std::fs::read_to_string(d.join("layout.toml")).unwrap(),
        V1_WITHOUT_WEATHER
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}

const SELF_PLACED: &str = r#"version = 2

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "clock"
x = 100
y = 100
w = 230
h = 250
settings = { placementStrategy = "leastBusy" }

[[monitor.container.widget]]
id = "calendar"
type = "clock"
x = 60
y = 120
w = 300
h = 200
"#;

/// A widget that places itself (the original's clock by the wallpaper's least busy region) shows
/// where its view says it draws, and its place in the layout, which it does not use, is in
/// nobody's way: a widget the layout places there stays where `layout.toml` puts it, on every
/// load, and an edit-mode drop keeps off the clock where it shows.
#[test]
fn a_self_placed_widget_shows_where_it_draws_and_a_drop_keeps_off_it_there() {
    let d = root("selfplaced", Some(SELF_PLACED));
    let (mut rt, id) = start(&d);
    // Its stored box would push the calendar off (100,100 230x250 covers 60,120).
    assert_ne!(shown(&rt, id, "calendar"), (60, 120, 300, 200));
    call(&mut rt, id, "placed", vec![s("clock"), Value::Bool(true)]);
    call(
        &mut rt,
        id,
        "placedAt",
        vec![s("clock"), n(905.0), n(305.0), n(230.0), n(250.0)],
    );
    assert_eq!(shown(&rt, id, "clock"), (905, 305, 230, 250));
    assert_eq!(shown(&rt, id, "calendar"), (60, 120, 300, 200));
    // A drop collides with it where it shows: onto its stored box lands as dropped...
    assert!(call(&mut rt, id, "mv", vec![s("calendar"), n(110.0), n(110.0)]).truthy());
    assert_eq!(rect(&rt, id, "calendar"), (110, 110, 300, 200));
    // ...and onto where it shows lands beside it.
    assert!(call(&mut rt, id, "mv", vec![s("calendar"), n(905.0), n(305.0)]).truthy());
    let at = rect(&rt, id, "calendar");
    assert!(!overlap(at, (905, 305, 230, 250)), "{at:?}");
    assert!(call(&mut rt, id, "mv", vec![s("calendar"), n(60.0), n(120.0)]).truthy());
    // A fresh load: still where the layout says once the clock reported where it shows.
    let saved = std::fs::read_to_string(d.join("layout.toml")).unwrap();
    assert!(
        saved.contains("x = 60") && saved.contains("y = 120"),
        "{saved}"
    );
    drop(rt);
    let (mut rt, id) = start(&d);
    call(&mut rt, id, "placed", vec![s("clock"), Value::Bool(true)]);
    call(
        &mut rt,
        id,
        "placedAt",
        vec![s("clock"), n(905.0), n(305.0), n(230.0), n(250.0)],
    );
    assert_eq!(shown(&rt, id, "calendar"), (60, 120, 300, 200));
    // No longer self-placed: the stored box counts again.
    call(&mut rt, id, "unplacedAt", vec![s("clock")]);
    call(&mut rt, id, "placed", vec![s("clock"), Value::Bool(false)]);
    assert_ne!(shown(&rt, id, "calendar"), (60, 120, 300, 200));
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&d).unwrap();
}
