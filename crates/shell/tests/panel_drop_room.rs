//! Edit mode on qml's real bar, with the layout merge train's guest seeds (every quick
//! toggle in the end zone) and the original's resources widget in the sidebar: dropping the sidebar's
//! resources into the panel's start zone charges the zone what PanelZone.fit charges it once
//! it is there, its panel form's natural length as the panel lays it out. At 1280 logical
//! (1920 at 1.5x) the start zone has no room for it, so the drop is refused (badge, no write)
//! instead of being written and then hidden; at 1920 it fits, is written and shows.
#![cfg(feature = "interp")]

use qml::edit_mode::WinRect;
use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

thread_local!(static CONSOLE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) });

const TOGGLES: [&str; 17] = [
    "antiFlashbang",
    "audio",
    "bluetooth",
    "cloudflareWarp",
    "colorPicker",
    "darkMode",
    "easyEffects",
    "gameMode",
    "idleInhibitor",
    "mic",
    "musicRecognition",
    "network",
    "nightLight",
    "notifications",
    "onScreenKeyboard",
    "powerProfile",
    "screenSnip",
];

/// The sidebar window: its place on the monitor and the resources widget's in it.
const SIDEBAR_W: f64 = 440.0;
const HOST: (f64, f64, f64, f64) = (20.0, 20.0, 200.0, 100.0);

/// The original's bar as the layout imports it, the toggles before the tray and indicators, and a
/// sidebar with the original's resources widget.
fn layout(toggles: &[&str]) -> String {
    let mut s = String::from(
        "version = 5\n[[monitor]]\nconnector = \"DP-1\"\nprimary = true\n\n\
         [[monitor.container]]\nid = \"panel\"\nkind = \"panel\"\nedge = \"rusty\"\n",
    );
    let mut w = |id: &str, ty: &str, zone: &str, width: &str| {
        s += &format!(
            "\n[[monitor.container.widget]]\nid = \"{id}\"\ntype = \"{ty}\"\nzone = \"{zone}\"\nwidth = \"{width}\"\n"
        );
    };
    for (id, zone, width) in [
        ("left-sidebar-button", "start", "auto"),
        ("active-window", "start", "stretch"),
        ("resources", "center", "auto"),
        ("media", "center", "stretch"),
        ("separator-1", "center", "auto"),
        ("workspaces", "center", "auto"),
        ("separator-2", "center", "auto"),
        ("clock", "center", "stretch"),
        ("util-buttons", "center", "auto"),
        ("battery", "center", "auto"),
        ("spacer", "end", "stretch"),
    ] {
        let ty = if id.starts_with("separator") {
            "separator"
        } else {
            id
        };
        w(id, ty, zone, width);
    }
    for t in toggles {
        w(&format!("t-{t}"), t, "end", "auto");
    }
    w("systray", "systray", "end", "auto");
    w("indicators", "indicators", "end", "auto");
    s += "\n[[monitor.container]]\nid = \"side\"\nkind = \"sidebar\"\nside = \"right\"\ncolumns = 2\n\
          \n[[monitor.container.widget]]\nid = \"side-resources\"\ntype = \"resources\"\n";
    s
}

/// The original's bar content on a `width` wide screen, the sidebar with its resources widget and the
/// edit overlay's controller.
fn main_qml(width: u32) -> String {
    let (x, y, w, h) = HOST;
    format!(
        r#"import QtQuick
import Rustshell
import Impulse
import ri.modules.common
import ri.modules.rusty.bar.panel
Item {{
    id: root
    property var ctl: overlay
    property var bar: barContent
    EditController {{ id: overlay; connector: "DP-1" }}
    function start(): bool {{ return overlay.start() }}
    function press(x: real, y: real): bool {{ return overlay.press(x, y) }}
    function move(x: real, y: real): bool {{ return overlay.move(x, y) }}
    function rest(): bool {{ return overlay.rest() }}
    function key(key: int, mods: int): bool {{ return overlay.key(key, mods) }}
    function release(x: real, y: real): bool {{ return overlay.release(x, y) }}
    function weather(): bool {{ Config.options.bar.weather.enable = true; return true }}
    PanelWindow {{
        objectName: "bar"
        implicitWidth: {width}
        implicitHeight: 40
        PanelContent {{
            id: barContent
            width: {width}
            height: 40
            screen: ({{ name: "DP-1", width: {width}, height: 1080 }})
        }}
    }}
    PanelWindow {{
        objectName: "sidebar"
        implicitWidth: {SIDEBAR_W}
        implicitHeight: 1040
        SidebarContainer {{}}
        WidgetHost {{ layoutId: "side-resources"; widgetType: "resources"; surface: "sidebar"; x: {x}; y: {y}; width: {w}; height: {h} }}
    }}
}}
"#
    )
}

/// What `f` gives on a thread of its own: the runtime keeps what it has learnt of the types
/// of one tree per thread, so a second bar with another layout starts on another.
fn apart<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        s.spawn(f)
            .join()
            .unwrap_or_else(|e| std::panic::resume_unwind(e))
    })
}

fn tree(width: u32, layout: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    static TREES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "panel-drop-room-{width}-{}-{}",
        TREES.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("layout.toml"), layout).unwrap();
    std::fs::write(d.join("main.qml"), main_qml(width)).unwrap();
    d
}

struct Bar {
    rt: Runtime,
    root: usize,
    ctl: usize,
    panel: usize,
    bar: usize,
    width: u32,
    fonts: std::rc::Rc<std::cell::RefCell<scene::Fonts>>,
    now: f64,
    dir: PathBuf,
}

impl Bar {
    fn new(width: u32) -> Bar {
        Bar::with(width, &TOGGLES)
    }

    /// With only `toggles` of the quick toggles in its end zone.
    fn with(width: u32, toggles: &[&str]) -> Bar {
        Bar::of(width, &layout(toggles))
    }

    /// With the layout file `layout`.
    fn of(width: u32, layout: &str) -> Bar {
        let dir = tree(width, layout);
        qml::builtins::CONSOLE_SINK
            .get_or_init(|| |_, line| CONSOLE.with_borrow_mut(|c| c.push(line.to_owned())));
        CONSOLE.with_borrow_mut(Vec::clear);
        let mut rt = Runtime::new(Some(dir.clone()));
        rt.load_singletons(&[
            dir.join("modules/common"),
            dir.join("modules/common/functions"),
            dir.join("services"),
            dir.clone(),
        ]);
        rt.world.borrow_mut().open_layout(dir.join("layout.toml"));
        // Measured as the shell measures: a widget made again while the panel is edited has
        // its length before the bindings that read it settle.
        let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
        {
            rt.world.borrow_mut().track_fresh = true;
            let f = fonts.clone();
            rt.world.borrow_mut().pre_complete = Some(Box::new(move |w, n0| {
                f.try_borrow_mut()
                    .is_ok_and(|mut f| scene::measure_new(w, &mut f, n0))
            }));
            scene::loaded_layout_hook(&mut rt.world.borrow_mut(), &fonts);
            scene::text_hooks(&mut rt.world.borrow_mut(), &fonts);
        }
        {
            let w = rt.world.borrow();
            let problems = w.layout.as_ref().unwrap().problems();
            assert!(problems.is_empty(), "{problems:?}");
        }
        rt.settle();
        let root = rt.load_file(&dir.join("main.qml")).unwrap();
        rt.settle();
        let (ctl, bar, panel) = {
            let mut w = rt.world.borrow_mut();
            let item = |w: &mut World, k: &str| match w.read(root, k) {
                Some(Value::Item(i)) => i,
                v => panic!("{k}: {v:?}"),
            };
            let ctl = item(&mut w, "ctl");
            let bar = item(&mut w, "bar");
            let panel = w.of_type("PanelContainer")[0];
            // Where the shell would place them: the bar on top, the sidebar against the right
            // edge below it.
            for (name, x, y, ww, hh) in [
                ("bar", 0.0, 0.0, f64::from(width), 40.0),
                (
                    "sidebar",
                    f64::from(width) - SIDEBAR_W,
                    40.0,
                    SIDEBAR_W,
                    1040.0,
                ),
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
            (ctl, bar, panel)
        };
        let mut b = Bar {
            rt,
            root,
            ctl,
            panel,
            bar,
            width,
            fonts,
            now: 1000.0,
            dir,
        };
        b.frames(200);
        b
    }

    /// `n` frames of the bar: ticks, bindings, layout.
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.now += 16.0;
            self.rt.tick(self.now);
            self.rt.settle();
            scene::layout_window(
                &mut self.rt,
                &mut self.fonts.borrow_mut(),
                self.bar,
                [f64::from(self.width), 40.0],
            );
            self.rt.settle();
        }
        assert!(self.rt.errors().is_empty(), "{:?}", self.rt.errors());
    }

    fn call(&mut self, f: &str, args: &[f64]) -> Value {
        let v = self
            .rt
            .call_method(self.root, f, args.iter().map(|&a| Value::Num(a)).collect())
            .unwrap()
            .unwrap();
        self.frames(1);
        v
    }

    fn num(&self, i: usize, k: &str) -> f64 {
        self.rt
            .world
            .borrow_mut()
            .read(i, k)
            .unwrap_or(Value::Num(f64::NAN))
            .to_number()
    }

    fn ctl(&self, k: &str) -> Value {
        self.rt
            .world
            .borrow_mut()
            .read(self.ctl, k)
            .unwrap_or_default()
    }

    /// The length the panel measured for the sidebar's resources in its panel form.
    fn measured(&self) -> Option<f64> {
        match self.rt.world.borrow_mut().read(self.panel, "panelLengths") {
            Some(Value::Obj(o)) => o
                .borrow()
                .props
                .get("side-resources")
                .map(|v| v.to_number()),
            _ => None,
        }
    }

    /// The shown panel host of layout id `id`: its width and the length its zone charges for it.
    fn shown(&self, id: &str) -> Option<(f64, f64)> {
        let mut w = self.rt.world.borrow_mut();
        let hosts: Vec<usize> = (0..w.items.len())
            .filter(|&i| !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "WidgetHost"))
            .collect();
        hosts.into_iter().find_map(|h| {
            let shown = w.read(h, "visible").is_some_and(|v| v.truthy())
                && w.read(h, "surface")
                    .is_some_and(|v| v.to_js_string() == "panel")
                && w.read(h, "layoutId")
                    .is_some_and(|v| v.to_js_string() == id);
            shown.then(|| {
                let n = |w: &mut World, k: &str| w.read(h, k).unwrap_or_default().to_number();
                (n(&mut w, "width"), n(&mut w, "minimumLength"))
            })
        })
    }

    /// The outline of the selected widget along the bar: its left end and its length.
    fn selected(&self) -> (f64, f64) {
        match self.ctl("selectedFrame") {
            Value::Obj(o) => {
                let o = o.borrow();
                let n = |k: &str| o.props.get(k).map_or(f64::NAN, |v| v.to_number());
                (n("x"), n("width"))
            }
            v => panic!("selectedFrame {v:?}"),
        }
    }

    /// Selects the widget at `x` on the bar with a click; its outline.
    fn select(&mut self, x: f64) -> (f64, f64) {
        self.call("press", &[x, 20.0]);
        self.call("release", &[x, 20.0]);
        self.selected()
    }

    fn file(&self) -> String {
        std::fs::read_to_string(self.dir.join("layout.toml")).unwrap()
    }

    fn panel_log(&self) -> Vec<String> {
        CONSOLE.with_borrow(|c| {
            c.iter()
                .filter(|l| l.starts_with("[Panel]"))
                .cloned()
                .collect()
        })
    }

    /// Drags the sidebar's resources to (x, y) on the bar; returns whether the preview was
    /// valid there, whether the refusal badge showed, and what the release returned.
    fn drag_to(&mut self, x: f64, y: f64) -> (bool, bool, bool) {
        let from = (
            f64::from(self.width) - SIDEBAR_W + HOST.0 + HOST.2 / 2.0,
            40.0 + HOST.1 + HOST.3 / 2.0,
        );
        self.call("press", &[from.0, from.1]);
        for k in 1..=8 {
            let t = f64::from(k) / 8.0;
            self.call(
                "move",
                &[from.0 + (x - from.0) * t, from.1 + (y - from.1) * t],
            );
        }
        let valid = match self.ctl("ghost") {
            Value::Obj(o) => o.borrow().props.get("valid").is_some_and(|v| v.truthy()),
            v => panic!("ghost {v:?}"),
        };
        let badge = matches!(self.ctl("refusal"), Value::Obj(_));
        let released = self.call("release", &[x, y]).truthy();
        (valid, badge, released)
    }
}

impl Drop for Bar {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A point in the start zone, past the original's left sidebar button.
fn start_zone() -> (f64, f64) {
    (60.0, 20.0)
}

#[test]
fn a_drop_the_1280_logical_start_zone_would_hide_is_refused_without_a_write() {
    let mut b = Bar::new(1280);
    assert!(b.call("start", &[]).truthy());
    // The probes come in animated, as the original's widgets do (Behavior on implicitWidth): a second of
    // frames for them to settle.
    b.frames(60);
    let room = b.num(b.panel, "startRoom");
    let measured = b
        .measured()
        .expect("the panel measured the sidebar's resources");
    // What it measured is what the zone charges the panel form: the center zone's resources,
    // same type and settings, is charged the same.
    let (_, charged) = b.shown("resources").expect("the bar's resources shows");
    assert_eq!(measured, charged, "the probe lays it out as the bar does");
    assert!(
        measured > room,
        "no room in the start zone: {measured} > {room}"
    );
    let before = b.file();
    let log = b.panel_log().len();
    let (x, y) = start_zone();
    let (valid, badge, released) = b.drag_to(x, y);
    assert!(!valid, "the preview is refused");
    assert!(badge, "the refusal badge shows");
    assert!(!released, "the release drops nothing");
    b.frames(60);
    assert_eq!(b.file(), before, "a refused drop writes nothing");
    assert_eq!(
        b.panel_log()[log..],
        [] as [String; 0],
        "no zone hid anything"
    );
}

#[test]
fn a_drop_the_1920_start_zone_has_room_for_is_written_and_shows() {
    let mut b = Bar::new(1920);
    assert!(b.call("start", &[]).truthy());
    // The probes come in animated, as the original's widgets do (Behavior on implicitWidth): a second of
    // frames for them to settle.
    b.frames(60);
    let room = b.num(b.panel, "startRoom");
    let measured = b
        .measured()
        .expect("the panel measured the sidebar's resources");
    assert!(
        measured > 0.0 && measured <= room,
        "room in the start zone: {measured} <= {room}"
    );
    let log = b.panel_log().len();
    let (x, y) = start_zone();
    let (valid, badge, released) = b.drag_to(x, y);
    assert!(
        valid && !badge && released,
        "accepted: {valid} {badge} {released}"
    );
    // It comes in animated too.
    b.frames(60);
    let file = b.file();
    let at = file.find("id = \"side-resources\"").unwrap();
    assert!(file[at..].contains("zone = \"start\""), "{file}");
    assert!(
        file.find("kind = \"sidebar\"").unwrap() > at,
        "moved onto the panel: {file}"
    );
    let (width, charged) = b.shown("side-resources").expect("it shows on the bar");
    assert_eq!(charged, measured, "its zone charges it the length measured");
    assert!(
        width >= measured,
        "at its natural size: {width} >= {measured}"
    );
    assert_eq!(
        b.panel_log()[log..],
        [] as [String; 0],
        "no zone hid anything"
    );
}

/// The first run's workspaces are a group of their own in the middle of the bar: pulled by an
/// end they grow to both sides. The widget itself takes each step's length, its held end stays
/// under the pointer whichever way its zone grows, and the layout is written at the drop only:
/// with the end where the pointer left it, so a second pull finds the handle there.
#[test]
fn a_widget_pulled_by_its_end_stretches_itself_with_that_end_under_the_pointer() {
    pulled(false);
}

#[test]
fn a_widget_pulled_by_its_leading_end_stretches_itself_with_that_end_under_the_pointer() {
    pulled(true);
}

fn pulled(left: bool) {
    let mut b = Bar::with(1920, &TOGGLES[..2]);
    assert!(b.call("start", &[]).truthy());
    b.frames(60);
    let before = b.file();
    let (x0, len0) = b.select(960.0);
    assert_eq!(
        b.shown("workspaces").map(|s| s.0),
        Some(len0),
        "the workspaces"
    );
    let (end, out) = if left { (x0, -1.0) } else { (x0 + len0, 1.0) };
    b.call("press", &[end, 20.0]);
    // The pull starts once the pointer has gone 2 px: the end keeps that far behind it.
    let mut at = end;
    for k in 1..=12 {
        let pointer = end + out * f64::from(k) * 5.0;
        at = pointer - out * 2.0;
        b.call("move", &[pointer, 20.0]);
        // The pointer rests after each step: the panel shows it.
        b.call("rest", &[]);
        let (x, len) = b.selected();
        let held = if left { x } else { x + len };
        if k >= 3 {
            assert!(
                (held - at).abs() <= 1.5,
                "step {k}: the end at {held}, the pointer at {at}"
            );
            assert_eq!(
                b.shown("workspaces").map(|s| s.0),
                Some(len),
                "step {k}: the widget is as long as its outline"
            );
        }
    }
    assert_eq!(b.file(), before, "nothing is written before the drop");
    let (_, len) = b.selected();
    // By as far as its end went and no more: its zone, in the middle of the bar, used to grow
    // to both sides, and the widget by twice that.
    assert_eq!(len, len0 + 58.0, "{len0} grew to {len}");
    assert!(b.call("release", &[at + out * 2.0, 20.0]).truthy());
    b.frames(10);
    assert!(
        b.file().contains(&format!("width = {len}\n")),
        "{len} px written:\n{}",
        b.file()
    );
    let (x, len) = b.selected();
    let held = if left { x } else { x + len };
    assert!(
        (held - at).abs() <= 1.5,
        "after the drop: the end at {held}, the pointer at {at}"
    );
}

/// Pulled in as far as the pointer goes, a widget stops at the length that shows all of it: the
/// workspaces are never a strip with a cut dot in it.
#[test]
fn a_widget_pulled_in_stops_at_the_length_its_contents_take() {
    let mut b = Bar::with(1920, &TOGGLES[..2]);
    assert!(b.call("start", &[]).truthy());
    b.frames(60);
    let (x0, len0) = b.select(960.0);
    let end = x0 + len0;
    b.call("press", &[end, 20.0]);
    for k in 1..=10 {
        b.call("move", &[end - f64::from(k) * 30.0, 20.0]);
    }
    let (_, len) = b.selected();
    assert_eq!(len, len0, "as long as it was on its own");
    assert_eq!(b.shown("workspaces").map(|s| s.0), Some(len0));
    b.call("release", &[end - 300.0, 20.0]);
    b.frames(10);
    assert_eq!(b.shown("workspaces").map(|s| s.0), Some(len0));
}

impl Bar {
    /// The panel's outlines from left to right: id, left end, length, whether it has handles.
    fn outlines(&self) -> Vec<(String, f64, f64, bool)> {
        let Value::Obj(a) = self.ctl("frames") else {
            panic!("frames");
        };
        let mut v: Vec<_> = a
            .borrow()
            .array
            .iter()
            .flatten()
            .filter_map(|f| {
                let Value::Obj(o) = f else { return None };
                let o = o.borrow();
                let s = |k: &str| o.props.get(k).map(|v| v.to_js_string()).unwrap_or_default();
                let n = |k: &str| o.props.get(k).map_or(f64::NAN, |v| v.to_number());
                (s("kind") == "panel").then(|| {
                    (
                        s("id"),
                        n("x"),
                        n("width"),
                        o.props.get("resizable").is_some_and(|v| v.truthy()),
                    )
                })
            })
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    }

    fn order(&self) -> Vec<String> {
        self.outlines().into_iter().map(|o| o.0).collect()
    }

    /// Whether the gesture under way carries a widget to another place.
    fn carries(&self) -> bool {
        match self.ctl("ghost") {
            Value::Obj(o) => o.borrow().props.get("carried").is_some_and(|v| v.truthy()),
            _ => false,
        }
    }
}

/// The clock stretches between the workspaces and the buttons: it has handles as the workspaces
/// have, and a pull of either end, however far, past the middle of its neighbour and on, sizes
/// it. It is never picked up, so the widgets stay in the order they were in.
#[test]
fn a_stretching_clock_pulled_by_an_end_is_sized_and_stays_in_its_place() {
    for (left, out) in [(true, -1.0), (true, 1.0), (false, -1.0), (false, 1.0)] {
        let mut b = Bar::with(1920, &TOGGLES[..2]);
        assert!(b.call("start", &[]).truthy());
        b.frames(60);
        let before = b.order();
        let (_, x0, len0, _) = b
            .outlines()
            .into_iter()
            .find(|o| o.0 == "clock")
            .expect("the clock's outline");
        assert_eq!(b.select(x0 + len0 / 2.0), (x0, len0), "the clock");
        let clock = b.outlines().into_iter().find(|o| o.0 == "clock").unwrap();
        assert!(clock.3, "the selected clock has handles");
        // 2 px inside the end, where the pointer of the run on the owner's screen was.
        let end = if left { x0 + 2.0 } else { x0 + len0 - 2.0 };
        b.call("press", &[end, 20.0]);
        let mut lens = vec![];
        for k in 1..=5 {
            b.call("move", &[end + out * f64::from(k) * 10.0, 20.0]);
            assert!(!b.carries(), "left {left} out {out} step {k}: picked up");
            assert_eq!(b.order(), before, "left {left} out {out} step {k}");
            lens.push(b.selected().1);
        }
        let grows = left == (out < 0.0);
        let len = *lens.last().unwrap();
        assert!(
            if grows { len > len0 + 20.0 } else { len < len0 },
            "left {left} out {out}: {len0} became {lens:?}"
        );
        b.call("release", &[end + out * 50.0, 20.0]);
        b.frames(10);
        assert_eq!(b.order(), before, "left {left} out {out}: after the drop");
        let file = b.file();
        let at = |id: &str| file.find(&format!("id = \"{id}\"")).unwrap();
        assert!(
            at("workspaces") < at("clock") && at("clock") < at("util-buttons"),
            "left {left} out {out}:\n{file}"
        );
        let (shown, _) = b.shown("clock").expect("the clock shows");
        assert!(shown >= 24.0, "left {left} out {out}: {shown} px of clock");
    }
}

impl Bar {
    /// The words the panel widget `id` shows: each one's text, left end, length and the length
    /// all of it takes.
    fn words(&self, id: &str) -> Vec<(String, f64, f64, f64)> {
        self.texts(id, false)
    }

    /// Those ([`Bar::words`]), and with `symbols` the icons it shows too.
    fn texts(&self, id: &str, symbols: bool) -> Vec<(String, f64, f64, f64)> {
        let mut w = self.rt.world.borrow_mut();
        let mut words = vec![];
        for i in Self::parts(&mut w, id) {
            if !Self::is(&w, i, "StyledText") || (!symbols && Self::is(&w, i, "MaterialSymbol")) {
                continue;
            }
            let text = w.read(i, "text").unwrap_or_default().to_js_string();
            if text.is_empty() {
                continue;
            }
            let n = |w: &mut World, k: &str| w.read(i, k).map_or(f64::NAN, |v| v.to_number());
            let x = w.scene_pos(i, Some(self.bar)).0;
            words.push((
                text.to_string(),
                x,
                n(&mut w, "width"),
                n(&mut w, "implicitWidth"),
            ));
        }
        words.sort_by(|a, b| a.1.total_cmp(&b.1));
        words
    }

    /// The left end and the length of the progress ring the panel's player `id` shows.
    fn ring(&self, id: &str) -> (f64, f64) {
        let mut w = self.rt.world.borrow_mut();
        let ring = Self::parts(&mut w, id)
            .into_iter()
            .find(|&i| Self::is(&w, i, "ClippedFilledCircularProgress"))
            .unwrap_or_else(|| panic!("the ring of {id}"));
        let x = w.scene_pos(ring, Some(self.bar)).0;
        (x, w.read(ring, "width").map_or(f64::NAN, |v| v.to_number()))
    }

    fn is(w: &World, i: usize, ty: &str) -> bool {
        w.items[i]
            .types
            .iter()
            .any(|t| t.rsplit('.').next() == Some(ty))
    }

    /// The items the panel widget `id` shows: all in it that are visible.
    fn parts(w: &mut World, id: &str) -> Vec<usize> {
        let host = (0..w.items.len())
            .find(|&i| {
                !w.dead.contains(&i)
                    && w.items[i].types.iter().any(|t| t == "WidgetHost")
                    && w.read(i, "visible").is_some_and(|v| v.truthy())
                    && w.read(i, "surface")
                        .is_some_and(|v| v.to_js_string() == "panel")
                    && w.read(i, "layoutId")
                        .is_some_and(|v| v.to_js_string() == id)
            })
            .unwrap_or_else(|| panic!("the panel's {id}"));
        let mut parts = vec![];
        let mut stack = vec![host];
        while let Some(i) = stack.pop() {
            if !w.read(i, "visible").is_none_or(|v| v.truthy()) {
                continue;
            }
            stack.extend(w.items[i].children.iter().copied());
            parts.push(i);
        }
        parts
    }

    /// Pulls the right end of the widget at `x` in as far as the pointer goes and lets go; the
    /// widget's left end and length after.
    fn pulled_in(&mut self, id: &str) -> (f64, f64) {
        let (_, x0, len0, _) = self
            .outlines()
            .into_iter()
            .find(|o| o.0 == id)
            .unwrap_or_else(|| panic!("the outline of {id}"));
        assert_eq!(self.select(x0 + len0 / 2.0), (x0, len0), "{id}");
        let end = x0 + len0 - 2.0;
        self.call("press", &[end, 20.0]);
        for k in 1..=12 {
            self.call("move", &[end - f64::from(k) * 30.0, 20.0]);
            assert!(!self.carries(), "{id} step {k}: picked up");
        }
        self.call("release", &[end - 360.0, 20.0]);
        self.frames(10);
        let o = self.outlines().into_iter().find(|o| o.0 == id).unwrap();
        (o.1, o.2)
    }
}

/// Pulled in as far as the pointer goes, the player stops where its title still reads, with
/// room left between its ring and title and the handles at its ends. It went down to its
/// progress ring, 24 px, with the title gone, and the ring stood under the left handle.
#[test]
fn a_player_pulled_in_stops_where_its_title_still_reads() {
    let mut b = Bar::with(1920, &TOGGLES[..2]);
    assert!(b.call("start", &[]).truthy());
    b.frames(60);
    let (x, len) = b.pulled_in("media");
    let words = b.words("media");
    assert_eq!(words.len(), 1, "the title in {len} px: {words:?}");
    let (_, tx, tw, all) = words[0].clone();
    assert!(tw >= all, "{len} px: the title has {tw} px of its {all}");
    let (rx, rw) = b.ring("media");
    // A handle reaches 4 px into the widget.
    assert!(rx - x >= 8.0, "{} px before the ring", rx - x);
    assert!(rx + rw <= tx, "the ring {rx}+{rw} before {words:?}");
    let after = x + len - (tx + tw);
    assert!(after >= 8.0, "{after} px after {words:?}");
    assert!(len < all + 80.0, "{len} px for a title of {all}");
    assert_eq!(b.shown("media").map(|s| s.0), Some(len));
    assert!(b.file().contains(&format!("width = {len}")), "{}", b.file());
}

/// Pulled in as far as the pointer goes, the clock stops with room left between its time and
/// date and the handles at its ends. It stopped at the length of its text, the first and the
/// last figure under the handles.
#[test]
fn a_clock_pulled_in_keeps_its_text_clear_of_its_handles() {
    let mut b = Bar::with(1920, &TOGGLES[..2]);
    assert!(b.call("start", &[]).truthy());
    b.frames(60);
    let (x, len) = b.pulled_in("clock");
    let words = b.words("clock");
    assert_eq!(words.len(), 3, "the time, a dot, the date: {words:?}");
    let (first, last) = (&words[0], &words[2]);
    // A handle reaches 4 px into the widget.
    assert!(first.1 - x >= 8.0, "{} px before {words:?}", first.1 - x);
    let after = x + len - (last.1 + last.2);
    assert!(after >= 8.0, "{after} px after {words:?}");
    assert!(len < 200.0, "{len} px of clock");
}

/// The panel's weather is in the layout whether the bar's weather option is on or not, and
/// shows nothing while it is off: no length, no outline. Switched on, it has its icon and
/// temperature, handles at its ends, and pulled in it stops where both still show.
#[test]
fn the_weather_has_a_length_and_handles_once_its_option_is_on() {
    let mut b = Bar::with(1920, &["weather"]);
    assert!(b.call("start", &[]).truthy());
    b.frames(60);
    assert_eq!(b.shown("t-weather"), None, "the option is off");
    assert!(!b.order().contains(&"t-weather".to_string()));

    assert!(b.call("weather", &[]).truthy());
    b.frames(60);
    let (len0, _) = b.shown("t-weather").expect("the weather shows");
    assert!(len0 >= 40.0, "{len0} px of weather");
    let (x, len) = b.pulled_in("t-weather");
    let o = b.outlines().into_iter().find(|o| o.0 == "t-weather");
    assert!(o.is_some_and(|o| o.3), "the selected weather has handles");
    assert_eq!(len, len0, "no shorter than its icon and temperature");
    let words = b.words("t-weather");
    assert_eq!(words.len(), 1, "the temperature: {words:?}");
    assert!(
        words[0].1 >= x && words[0].1 + words[0].2 <= x + len,
        "{words:?} in {x}+{len}"
    );
}

impl Bar {
    /// The panel's shown widgets from left to right: id, the item that shows it, its left end
    /// and its length.
    fn hosts(&self) -> Vec<(String, usize, f64, f64)> {
        let mut w = self.rt.world.borrow_mut();
        let mut v = vec![];
        for h in 0..w.items.len() {
            if w.dead.contains(&h)
                || !w.items[h].types.iter().any(|t| t == "WidgetHost")
                || !w.read(h, "visible").is_some_and(|v| v.truthy())
                || !w
                    .read(h, "surface")
                    .is_some_and(|v| v.to_js_string() == "panel")
            {
                continue;
            }
            let id = w.read(h, "layoutId").unwrap_or_default().to_js_string();
            // The edit overlay's own host, for what is carried, has none.
            if id.is_empty() {
                continue;
            }
            let x = w.scene_pos(h, Some(self.bar)).0;
            let len = w.read(h, "width").map_or(f64::NAN, |v| v.to_number());
            v.push((id.to_string(), h, x, len));
        }
        v.sort_by(|a, b| a.2.total_cmp(&b.2));
        v
    }

    fn host(&self, id: &str) -> (f64, f64) {
        let h = self.hosts().into_iter().find(|h| h.0 == id);
        let h = h.unwrap_or_else(|| panic!("the panel's {id}"));
        (h.2, h.3)
    }

    fn outline(&self, id: &str) -> (f64, f64) {
        let o = self.outlines().into_iter().find(|o| o.0 == id);
        let o = o.unwrap_or_else(|| panic!("the outline of {id}"));
        (o.1, o.2)
    }

    /// Selects panel widget `id`, takes it by its left or right end and pulls that `by` px
    /// along the bar, 4 px a step (the pull starts once the pointer has gone 2 px, so the end
    /// goes 2 px less); after each step the other end must be where it was and the held one
    /// under the pointer. Where the pointer is then; the button is still down. The pointer rests
    /// after every step, so the panel shows each.
    fn pull(&mut self, id: &str, left: bool, by: f64) -> f64 {
        let (x0, len0) = self.outline(id);
        assert_eq!(self.select(x0 + len0 / 2.0), (x0, len0), "{id}");
        let order = self.order();
        let end = if left { x0 + 2.0 } else { x0 + len0 - 2.0 };
        self.call("press", &[end, 20.0]);
        let steps = (by.abs() / 4.0).ceil() as i32;
        let mut pointer = end;
        for k in 1..=steps {
            let far = (f64::from(k) * 4.0).min(by.abs());
            pointer = end + by.signum() * far;
            self.call("move", &[pointer, 20.0]);
            // The pointer rests after each step: the panel shows it.
            self.call("rest", &[]);
            let what = format!("{id} left {left} by {by} step {k}");
            assert!(!self.carries(), "{what}: picked up");
            assert_eq!(self.order(), order, "{what}");
            let (x, len) = self.selected();
            assert_eq!(
                self.host(id),
                (x, len),
                "{what}: the widget is where its outline is"
            );
            let gone = by.signum() * (far - 2.0);
            let (stays, held, to) = if left {
                (x + len, x, x0 + gone)
            } else {
                (x, x + len, x0 + len0 + gone)
            };
            let stood = if left { x0 + len0 } else { x0 };
            assert!(
                (stays - stood).abs() <= 0.5,
                "{what}: the other end went from {stood} to {stays}"
            );
            assert!(
                (held - to).abs() <= 1.5,
                "{what}: the held end at {held}, the pointer has it at {to}"
            );
        }
        pointer
    }

    /// The gaps the layout file has: id and length.
    fn gaps(&self) -> Vec<(String, f64)> {
        let file = self.file();
        file.split("[[monitor.container.widget]]")
            .filter_map(|w| {
                let key = |k: &str| {
                    w.lines()
                        .find_map(|l| l.strip_prefix(&format!("{k} = ")))
                        .map(|v| v.trim_matches('"').to_string())
                };
                let id = key("id")?;
                (id.starts_with("gap") && key("type").as_deref() == Some("spacer")).then(|| {
                    (
                        id,
                        key("width")
                            .and_then(|w| w.parse().ok())
                            .unwrap_or(f64::NAN),
                    )
                })
            })
            .collect()
    }
}

/// The clock and the player each stretch in a group of the middle of the bar. Pulled in by
/// either end, that end comes with the pointer and the other stays where it was, as does
/// every other widget of the bar: the room the pull leaves stays beside the widget, on the
/// side it was pulled from, and the layout has it as a spacer of that length. The clock kept
/// its left end whichever end was pulled, and the player's two ends both came in.
#[test]
fn a_widget_pulled_in_by_an_end_keeps_its_other_end_and_leaves_room_on_that_side() {
    for id in ["clock", "media"] {
        for left in [true, false] {
            let mut b = Bar::with(1920, &TOGGLES[..2]);
            assert!(b.call("start", &[]).truthy());
            b.frames(60);
            let around: Vec<_> = b.hosts().into_iter().filter(|h| h.0 != id).collect();
            let (x0, len0) = b.outline(id);
            let by = if left { 24.0 } else { -24.0 };
            let pointer = b.pull(id, left, by);
            let pulled = b.host(id);
            assert_eq!(pulled.1, len0 - 22.0, "{id} left {left}");
            assert_eq!(
                b.hosts()
                    .into_iter()
                    .filter(|h| h.0 != id)
                    .collect::<Vec<_>>(),
                around,
                "{id} left {left}: the widgets around it, while it is pulled"
            );
            assert!(b.gaps().is_empty(), "nothing is written before the drop");
            assert!(b.call("release", &[pointer, 20.0]).truthy());
            // Let go, the bar is as it was shown: nothing is made anew or laid out again.
            for frames in [0, 10] {
                b.frames(frames);
                let what = format!("{id} left {left}, {frames} frames after the drop");
                assert_eq!(b.host(id), pulled, "{what}");
                assert_eq!(b.outline(id), pulled, "{what}");
                assert_eq!(
                    b.hosts()
                        .into_iter()
                        .filter(|h| h.0 != id)
                        .collect::<Vec<_>>(),
                    around,
                    "{what}: the widgets around it"
                );
            }
            // The room is a spacer in the file, which the panel shows as no widget.
            let gaps = b.gaps();
            assert_eq!(gaps.len(), 1, "{id} left {left}: {}", b.file());
            assert_eq!(gaps[0].1, 22.0, "{id} left {left}: {}", b.file());
            assert_eq!(gaps[0].0.starts_with("gap-before"), left, "{gaps:?}");
            let file = b.file();
            let at = |id: &str| file.find(&format!("id = \"{id}\"")).unwrap();
            assert_eq!(at(&gaps[0].0) < at(id), left, "{file}");
            assert!(
                file.contains(&format!("width = {}\n", len0 - 22.0)),
                "{file}"
            );
            // Written as release 0ba10907 reads it (its reader takes no key it does not
            // know): the layout's version as it was, and a spacer with a length in px.
            assert!(file.starts_with("version = 5\n"), "{file}");
            let zone = "center";
            let written = format!(
                "[[monitor.container.widget]]\nid = \"{}\"\ntype = \"spacer\"\nzone = \"{zone}\"\nwidth = 22\n\n",
                gaps[0].0
            );
            assert!(file.contains(&written), "{file}");
            assert!(
                !b.hosts().iter().any(|h| h.0.starts_with("gap")),
                "a gap is no widget of the panel: {:?}",
                b.hosts()
            );
            assert!(
                !b.order().iter().any(|o| o.starts_with("gap")),
                "{:?}",
                b.order()
            );

            // Read from the file by a bar that starts with it, it stands the same.
            let shows = |b: &Bar| -> Vec<(String, f64, f64)> {
                b.hosts().into_iter().map(|h| (h.0, h.2, h.3)).collect()
            };
            let again = apart(|| shows(&Bar::of(1920, &file)));
            assert_eq!(again, shows(&b), "{id} left {left}: read from the file");

            // Pulled in further, the room is still one spacer, longer; pulled back out all
            // the way, the widget is as long as it was and no spacer is left.
            let pointer = b.pull(id, left, by);
            assert!(b.call("release", &[pointer, 20.0]).truthy());
            b.frames(10);
            let gaps = b.gaps();
            assert_eq!(gaps.len(), 1, "{id} left {left}: {}", b.file());
            assert_eq!(gaps[0].1, 44.0, "{id} left {left}: {}", b.file());
            let pointer = b.pull(id, left, -by.signum() * 46.0);
            assert!(b.call("release", &[pointer, 20.0]).truthy());
            b.frames(10);
            assert!(b.gaps().is_empty(), "{id} left {left}: {}", b.file());
            assert_eq!(b.host(id), (x0, len0), "{id} left {left}: pulled back out");
            assert_eq!(
                b.hosts()
                    .into_iter()
                    .filter(|h| h.0 != id)
                    .collect::<Vec<_>>(),
                around,
                "{id} left {left}: the widgets around it, after all three pulls"
            );
        }
    }
}

/// The clock and the player each stretch in a group of the middle of the bar, which stands in
/// the middle of the screen. Pulled out by either end as far as on the owner's monitor (the
/// clock 150 px, the player 120), that end comes with the pointer and the other stays where
/// it was, as does every widget on that other side: the middle of the bar is held off the
/// middle of the screen by half of what the widget grew, and the layout has that as a spacer
/// at the far end of the zone. The layout has the length the widget shows, where both ends
/// went and twice the growth was written.
#[test]
fn a_widget_pulled_out_by_an_end_grows_at_that_end_alone() {
    for (id, far) in [("clock", 150.0), ("media", 120.0)] {
        for left in [false, true] {
            let what = format!("{id} left {left}");
            let mut b = Bar::with(1920, &TOGGLES[..2]);
            assert!(b.call("start", &[]).truthy());
            b.frames(60);
            let (x0, len0) = b.outline(id);
            // The widgets the pulled end does not push: those on the other side of it.
            let stand = |b: &Bar| -> Vec<_> {
                b.hosts()
                    .into_iter()
                    .filter(|h| h.0 != id && if left { h.2 > x0 } else { h.2 < x0 })
                    .collect()
            };
            let stood = stand(&b);
            assert!(stood.len() >= 3, "{what}: {stood:?}");
            let by = if left { -far } else { far };
            let pointer = b.pull(id, left, by);
            let pulled = b.host(id);
            assert_eq!(pulled.1, len0 + far - 2.0, "{what}");
            assert_eq!(stand(&b), stood, "{what}: the widgets on the other side");

            assert!(b.call("release", &[pointer, 20.0]).truthy());
            b.frames(10);
            assert_eq!(b.host(id), pulled, "{what}: after the drop");
            assert_eq!(b.outline(id), pulled, "{what}: after the drop");
            assert_eq!(stand(&b), stood, "{what}: after the drop");
            let file = b.file();
            assert!(
                file.contains(&format!("width = {}\n", pulled.1)),
                "{what}: the length it shows is written:\n{file}"
            );
            let gaps = b.gaps();
            assert_eq!(gaps.len(), 1, "{what}: {file}");
            assert!(gaps[0].0.starts_with("gap-shift"), "{what}: {gaps:?}");
            assert_eq!(gaps[0].1, far - 2.0, "{what}: {file}");
            // At the far end of the zone, as release 0ba10907 reads it: a spacer in px.
            let at = |id: &str| file.find(&format!("id = \"{id}\"")).unwrap();
            let (first, last) = (at("resources"), at("battery"));
            assert!(
                if left {
                    at(&gaps[0].0) > last
                } else {
                    at(&gaps[0].0) < first
                },
                "{what}: {file}"
            );
            assert!(
                file.contains(&format!(
                    "[[monitor.container.widget]]\nid = \"{}\"\ntype = \"spacer\"\nzone = \"center\"\nwidth = {}\n\n",
                    gaps[0].0,
                    far - 2.0
                )),
                "{what}: {file}"
            );
            assert!(
                !b.hosts().iter().any(|h| h.0.starts_with("gap")),
                "{what}: a gap is no widget of the panel: {:?}",
                b.hosts()
            );

            // Read from the file by a bar that starts with it, it stands the same.
            let shows = |b: &Bar| -> Vec<(String, f64, f64)> {
                b.hosts().into_iter().map(|h| (h.0, h.2, h.3)).collect()
            };
            let again = apart(|| shows(&Bar::of(1920, &file)));
            assert_eq!(again, shows(&b), "{what}: read from the file");

            // Pulled out again, the bar is held off by one spacer, longer.
            let pointer = b.pull(id, left, by.signum() * 22.0);
            assert!(b.call("release", &[pointer, 20.0]).truthy());
            b.frames(10);
            let gaps = b.gaps();
            assert_eq!(gaps.len(), 1, "{what}: {}", b.file());
            assert_eq!(gaps[0].1, far + 18.0, "{what}: {}", b.file());
            assert_eq!(stand(&b), stood, "{what}: after the second pull");
        }
    }
}

/// While the pointer moves with an end of a panel's widget, only the outline goes with it: the
/// panel stands as it was, so a step costs no frame of the panel. Once the pointer rests the
/// panel shows the widget just where the outline has it, and so it does when the end is let
/// go while the pointer still moves: nothing jumps, no widget is made anew, and of the widget
/// only the held end has moved.
#[test]
fn a_panel_stands_while_an_end_is_pulled_and_shows_the_pull_once_the_pointer_rests() {
    for (id, by) in [
        ("clock", 150.0_f64),
        ("media", 120.0),
        ("clock", -40.0),
        ("media", -40.0),
    ] {
        for left in [false, true] {
            let what = format!("{id} left {left} by {by}");
            let mut b = Bar::with(1920, &TOGGLES[..2]);
            assert!(b.call("start", &[]).truthy());
            b.frames(60);
            let (x0, len0) = b.outline(id);
            assert_eq!(b.select(x0 + len0 / 2.0), (x0, len0), "{what}");
            let stood = b.hosts();
            let other = if left { x0 + len0 } else { x0 };
            let end = if left { x0 + 2.0 } else { x0 + len0 - 2.0 };
            // Out is away from the widget: to the left by its left end.
            let dir = if left { -by.signum() } else { by.signum() };
            let ends = |(x, len): (f64, f64)| if left { (x + len, x) } else { (x, x + len) };
            b.call("press", &[end, 20.0]);
            let mut pointer = end;
            let mut outline = (x0, len0);
            for k in 1..=(by.abs() / 4.0) as i32 {
                pointer = end + dir * f64::from(k) * 4.0;
                b.call("move", &[pointer, 20.0]);
                assert_eq!(b.hosts(), stood, "{what} step {k}: the panel stands");
                outline = b.selected();
                let (stays, held) = ends(outline);
                assert!(
                    (stays - other).abs() <= 0.5,
                    "{what} step {k}: the other end went from {other} to {stays}"
                );
                if by > 0.0 {
                    let to = if left { x0 } else { x0 + len0 } + dir * (f64::from(k) * 4.0 - 2.0);
                    assert!(
                        (held - to).abs() <= 1.5,
                        "{what} step {k}: the held end at {held}, the pointer has it at {to}"
                    );
                }
            }
            assert_ne!(outline.1, len0, "{what}: pulled");

            // The pointer rests: the panel shows the widget where the outline is, and the
            // widgets on the other side of it where they were.
            assert!(b.call("rest", &[]).truthy(), "{what}");
            assert_eq!(
                b.selected(),
                outline,
                "{what}: the outline as the pointer rests"
            );
            assert_eq!(
                b.host(id),
                outline,
                "{what}: the widget as the pointer rests"
            );
            let stand = |b: &Bar| -> Vec<_> {
                b.hosts()
                    .into_iter()
                    .filter(|h| h.0 != id && if left { h.2 > x0 } else { h.2 < x0 })
                    .collect()
            };
            let on_the_other_side: Vec<_> = stood
                .iter()
                .filter(|h| h.0 != id && if left { h.2 > x0 } else { h.2 < x0 })
                .cloned()
                .collect();
            assert!(
                on_the_other_side.len() >= 3,
                "{what}: {on_the_other_side:?}"
            );
            assert_eq!(stand(&b), on_the_other_side, "{what}: as the pointer rests");
            // A pointer that still rests has nothing more to show.
            assert!(!b.call("rest", &[]).truthy(), "{what}");

            // It goes on and the end is let go while it moves: the panel stands until then,
            // and one frame after it shows what the outline had.
            let rested = b.hosts();
            for _ in 0..2 {
                pointer += dir * 4.0;
                b.call("move", &[pointer, 20.0]);
                assert_eq!(b.hosts(), rested, "{what}: the panel stands again");
            }
            let outline = b.selected();
            assert!(
                (ends(outline).0 - other).abs() <= 0.5,
                "{what}: the other end before the drop, {outline:?}"
            );
            let items = b.rt.world.borrow().items.len();
            assert!(b.call("release", &[pointer, 20.0]).truthy(), "{what}");
            assert_eq!(b.host(id), outline, "{what}: one frame after the drop");
            assert_eq!(b.outline(id), outline, "{what}: its outline after the drop");
            assert_eq!(stand(&b), on_the_other_side, "{what}: after the drop");
            let shown = b.hosts();
            assert_eq!(
                shown.iter().map(|h| (h.0.clone(), h.1)).collect::<Vec<_>>(),
                stood.iter().map(|h| (h.0.clone(), h.1)).collect::<Vec<_>>(),
                "{what}: the items that show the widgets"
            );
            b.frames(20);
            assert_eq!(b.hosts(), shown, "{what}: twenty frames on");
            assert_eq!(
                b.rt.world.borrow().items.len(),
                items,
                "{what}: items made after the drop"
            );
            assert!(
                b.file().contains(&format!("width = {}\n", outline.1)),
                "{what}: the length it shows is written:\n{}",
                b.file()
            );
        }
    }
}

/// A pull dropped with Esc leaves the panel as it stood, whether the pointer had rested or
/// not, and a rest after it has nothing to show.
#[test]
fn a_pull_dropped_with_esc_leaves_the_panel_as_it_stood() {
    const ESC: f64 = 16_777_216.0;
    for rested in [false, true] {
        let mut b = Bar::with(1920, &TOGGLES[..2]);
        assert!(b.call("start", &[]).truthy());
        b.frames(60);
        let (x0, len0) = b.outline("clock");
        assert_eq!(b.select(x0 + len0 / 2.0), (x0, len0));
        let stood = b.hosts();
        let file = b.file();
        let end = x0 + len0 - 2.0;
        b.call("press", &[end, 20.0]);
        for k in 1..=10 {
            b.call("move", &[end + f64::from(k) * 4.0, 20.0]);
        }
        assert_ne!(b.selected(), (x0, len0), "rested {rested}: pulled");
        if rested {
            assert!(b.call("rest", &[]).truthy());
            assert_ne!(b.hosts(), stood, "the panel shows the pull");
        }
        assert!(b.call("key", &[ESC, 0.0]).truthy(), "rested {rested}");
        assert!(
            !b.call("rest", &[]).truthy(),
            "rested {rested}: a rest after Esc"
        );
        b.frames(2);
        assert_eq!(b.hosts(), stood, "rested {rested}: the panel after Esc");
        assert_eq!(b.selected(), (x0, len0), "rested {rested}: the outline");
        assert_eq!(b.file(), file, "rested {rested}: nothing is written");
    }
}

/// A widget carried to another place neither takes the room beside it along nor leaves it
/// behind: the layout has no spacer for it after.
#[test]
fn a_widget_carried_away_leaves_no_room_behind_and_takes_none_along() {
    let mut b = Bar::with(1920, &TOGGLES[..2]);
    assert!(b.call("start", &[]).truthy());
    b.frames(60);
    for left in [true, false] {
        let pointer = b.pull("clock", left, if left { 24.0 } else { -24.0 });
        assert!(b.call("release", &[pointer, 20.0]).truthy());
        b.frames(10);
    }
    assert_eq!(b.gaps().len(), 2, "{}", b.file());
    let (x, len) = b.outline("clock");
    let (to, _) = start_zone();
    b.call("press", &[x + len / 2.0, 20.0]);
    for k in 1..=8 {
        let t = f64::from(k) / 8.0;
        b.call("move", &[x + len / 2.0 + (to - x - len / 2.0) * t, 20.0]);
    }
    assert!(b.carries(), "the clock is picked up");
    assert!(b.call("release", &[to, 20.0]).truthy());
    b.frames(10);
    let file = b.file();
    assert!(b.gaps().is_empty(), "{file}");
    let clock = file
        .split("[[monitor.container.widget]]")
        .find(|w| w.contains("id = \"clock\""))
        .unwrap();
    assert!(clock.contains("zone = \"start\""), "{clock}");
    assert!(!b.hosts().iter().any(|h| h.0.starts_with("gap")));
}

/// A clock the layout has shorter than its text and the room at its handles take (what an
/// older release wrote: 112 px and 106 where the owner's font takes 134; here the text takes
/// less, so 40 px and 34 stand for them) stays as long as the layout has it: on the bar, in
/// edit mode, when an end of it is pressed, and when the pointer then goes nowhere or
/// inwards. Its outline and the handles on it are around the length that keeps its text 10 px
/// clear of them, past the ends of the clock itself: the text reached from handle to handle.
/// It is not made shorter than it is, and pulled out it grows from its length, the other end
/// where it was and the handle under the pointer. It jumped to the length its contents take
/// as soon as an end was pressed.
#[test]
fn a_clock_the_layout_has_shorter_than_its_contents_keeps_its_length_under_a_press() {
    let stretching = "id = \"clock\"\ntype = \"clock\"\nzone = \"center\"\nwidth = \"stretch\"\n";
    let text = layout(&TOGGLES[..2]);
    assert!(text.contains(stretching));
    // What its time and date take with the room at the handles, and a length at which they
    // reach from end to end.
    let full = {
        let b = Bar::of(1920, &text);
        let h = b.hosts().into_iter().find(|h| h.0 == "clock").unwrap().1;
        let w = b.rt.world.borrow_mut().read(h, "contentLength");
        w.map_or(f64::NAN, |v| v.to_number())
    };
    assert!(full > 112.0, "the clock's contents take {full}");
    let tight = (full - 18.0).round();
    for (has, left) in [
        (112.0, true),
        (112.0, false),
        (tight, true),
        (tight, false),
        (44.0, true),
        (44.0, false),
        (40.0, true),
        (40.0, false),
    ] {
        let what = format!("a clock of {has} px, left {left}");
        let text = text.replace(
            stretching,
            &stretching.replace("\"stretch\"", &format!("{has}")),
        );
        let mut b = Bar::of(1920, &text);
        let on_bar = b.hosts();
        let (hx, _) = b.host("clock");
        assert_eq!(b.host("clock").1, has, "{what}: on the bar");
        assert!(b.call("start", &[]).truthy());
        b.frames(60);
        assert_eq!(b.hosts(), on_bar, "{what}: the bar in edit mode");
        let least = {
            let h = b.hosts().into_iter().find(|h| h.0 == "clock").unwrap().1;
            let w = b.rt.world.borrow_mut().read(h, "contentLength");
            w.map_or(f64::NAN, |v| v.to_number())
        };
        // The outline is on whole pixels.
        let pad = ((least - has) / 2.0).max(0.0);
        let around = |(x, len): (f64, f64), (hx, has): (f64, f64)| {
            let pad = ((least - has) / 2.0).max(0.0);
            (x - (hx - pad)).abs() <= 0.5 && (len - (has + pad * 2.0)).abs() <= 1.0
        };
        assert_eq!(b.outline("clock"), (hx, has), "{what}: in edit mode");
        let (x0, len0) = b.select(hx + has / 2.0);
        assert!(
            around((x0, len0), (hx, has)),
            "{what}: the outline {x0} {len0} of the clock at {hx}, its contents take {least}"
        );
        // The text is 10 px clear of the outline's ends; a handle reaches 4 px in.
        let words = b.texts("clock", true);
        let (first, last) = (&words[0], &words[words.len() - 1]);
        assert!(
            first.1 - x0 >= 8.0 && x0 + len0 - (last.1 + last.2) >= 8.0,
            "{what}: the outline {x0} {len0} around {words:?}, its contents take {least}"
        );
        let (end, inward) = if left {
            (x0 + 2.0, 1.0)
        } else {
            (x0 + len0 - 2.0, -1.0)
        };
        b.call("press", &[end, 20.0]);
        assert_eq!(b.host("clock"), (hx, has), "{what}: pressed");
        // Nowhere, then the 2 px a pull starts at.
        for gone in [0.0, 1.0, 2.0] {
            b.call("move", &[end + inward * gone, 20.0]);
            assert!(!b.carries(), "{what}, {gone} px in: picked up");
            assert_eq!(b.host("clock"), (hx, has), "{what}, {gone} px in");
            assert_eq!(b.selected(), (x0, len0), "{what}, {gone} px in");
        }
        if has > least {
            assert_eq!(
                (x0, len0),
                (hx, has),
                "{what}: no shorter than its contents"
            );
            continue;
        }
        assert!(pad >= 8.0, "{what}: its contents take {least}");
        // Inwards it is no shorter.
        for gone in [3.0, 8.0, 30.0] {
            b.call("move", &[end + inward * gone, 20.0]);
            assert!(!b.carries(), "{what}, {gone} px in: picked up");
            assert_eq!(b.host("clock"), (hx, has), "{what}, {gone} px in");
            assert_eq!(b.selected(), (x0, len0), "{what}, {gone} px in");
        }
        // Outwards it grows from its length, its other end where it was and the handle with
        // the pointer, past what its contents take without a step there.
        // (The pull began 2 px in, where the pointer first went.)
        // The pointer rests after each step: the panel shows it.
        b.call("move", &[end, 20.0]);
        b.call("rest", &[]);
        let mut was = b.host("clock").1;
        assert!(
            was >= has && was - was.min(has) <= 4.0,
            "{what}: {has} became {was}"
        );
        for k in 1..=12 {
            let gone = f64::from(k) * 4.0;
            b.call("move", &[end - inward * gone, 20.0]);
            b.call("rest", &[]);
            let (x, len) = b.selected();
            let host = b.host("clock");
            assert!(
                around((x, len), host),
                "{what}, {gone} px out: the outline {x} {len} of the clock at {host:?}"
            );
            assert!(
                host.1 >= was && host.1 - was <= 9.0,
                "{what}, {gone} px out: {was} became {}",
                host.1
            );
            was = host.1;
            let (stays, stood) = if left {
                (host.0 + host.1, hx + has)
            } else {
                (host.0, hx)
            };
            assert!(
                (stays - stood).abs() <= 0.5,
                "{what}, {gone} px out: the other end went from {stood} to {stays}"
            );
            // As far from where it was as the pointer is from where the pull began.
            let (held, to) = if left {
                (x, x0 - (gone + 2.0))
            } else {
                (x + len, x0 + len0 + gone + 2.0)
            };
            assert!(
                (held - to).abs() <= 1.5,
                "{what}, {gone} px out: the handle at {held}, the pointer has it at {to}"
            );
        }
        assert!(
            was > least,
            "{what}: grew to {was}, past the {least} its contents take"
        );
        assert_eq!(
            b.selected(),
            b.host("clock"),
            "{what}: grown past its contents"
        );
        // Back where it was pressed: as long as the layout has it, and nothing to write.
        b.call("move", &[end + inward * 2.0, 20.0]);
        b.call("rest", &[]);
        assert_eq!(b.selected(), (x0, len0), "{what}: back");
        assert_eq!(b.host("clock"), (hx, has), "{what}: back");
        b.call("release", &[end + inward * 2.0, 20.0]);
        b.frames(10);
        assert_eq!(b.hosts(), on_bar, "{what}: let go");
        assert!(
            b.file().contains(&format!("width = {has}\n")),
            "{}",
            b.file()
        );
        assert!(b.gaps().is_empty(), "{}", b.file());
    }
}

/// The bar of the owner's layout: the clock stretches in the middle zone, after workspaces
/// 300 px long.
const OWNER_BAR: &str = r#"version = 5

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "panel"
kind = "panel"
edge = "rusty"
length = "fill"
align = "center"
floating = false

[[monitor.container.widget]]
id = "left-sidebar-button"
type = "left-sidebar-button"
zone = "start"

[[monitor.container.widget]]
id = "active-window"
type = "active-window"
zone = "start"
width = "stretch"

[[monitor.container.widget]]
id = "resources"
type = "resources"
zone = "center"

[[monitor.container.widget]]
id = "media"
type = "media"
zone = "center"
width = "stretch"

[[monitor.container.widget]]
id = "separator-1"
type = "separator"
zone = "center"

[[monitor.container.widget]]
id = "separator-2"
type = "separator"
zone = "center"

[[monitor.container.widget]]
id = "workspaces"
type = "workspaces"
zone = "center"
width = 300

[[monitor.container.widget]]
id = "clock"
type = "clock"
zone = "center"
width = "stretch"

[[monitor.container.widget]]
id = "util-buttons"
type = "util-buttons"
zone = "center"

[[monitor.container.widget]]
id = "battery"
type = "battery"
zone = "center"

[[monitor.container.widget]]
id = "weather"
type = "weather"
zone = "end"

[[monitor.container.widget]]
id = "spacer"
type = "spacer"
zone = "end"
width = "stretch"

[[monitor.container.widget]]
id = "systray"
type = "systray"
zone = "end"

[[monitor.container.widget]]
id = "indicators"
type = "indicators"
zone = "end"
"#;

/// A stretching clock is as long as its time and date where its zone has no more for it (the
/// owner's bar: 112 px), and is that on the bar and in edit mode alike. Its outline and the
/// handles on it are 10 px clear of the text, past the ends of the clock: the right handle
/// lay on the date.
#[test]
fn a_stretching_clock_as_long_as_its_text_has_its_handles_clear_of_it() {
    let mut b = Bar::of(1920, OWNER_BAR);
    let on_bar = b.hosts();
    let (hx, has) = b.host("clock");
    let words = b.words("clock");
    assert_eq!(words.len(), 3, "the time, a dot, the date: {words:?}");
    let (from, to) = (words[0].1, words[2].1 + words[2].2);
    assert!(
        from - hx < 2.0 && hx + has - to < 2.0,
        "the clock {hx} {has} is as long as its text {from}..{to}"
    );
    assert!(b.call("start", &[]).truthy());
    b.frames(60);
    assert_eq!(b.hosts(), on_bar, "the bar in edit mode");
    assert_eq!(b.outline("clock"), (hx, has));
    let (x, len) = b.select(hx + has / 2.0);
    assert_eq!(b.hosts(), on_bar, "the bar with the clock selected");
    // A handle reaches 4 px in from the outline's end.
    assert!(
        from - x >= 9.0 && x + len - to >= 9.0,
        "the outline {x} {len} around the text {from}..{to}"
    );
    assert!(
        from - x <= 11.0 && x + len - to <= 11.0,
        "the outline {x} {len} around the text {from}..{to}"
    );
    // Either handle is held where the outline has it, and a press there moves nothing.
    for end in [x + 2.0, x + len - 2.0] {
        b.call("press", &[end, 20.0]);
        b.call("move", &[end, 20.0]);
        assert!(!b.carries(), "picked up by the handle at {end}");
        assert_eq!(b.selected(), (x, len), "pressed at {end}");
        b.call("release", &[end, 20.0]);
        b.frames(10);
        assert_eq!(b.hosts(), on_bar, "pressed at {end} and let go");
    }
}

/// Letting go of an end leaves the bar as the pull had it: the same items show the widgets,
/// where they were on the last move, and nothing is made anew. The zone made each of its
/// widgets again from the layout that was written, a frame or more after the drop.
#[test]
fn letting_go_of_an_end_makes_no_widget_of_the_bar_anew() {
    for (id, left, by) in [
        ("media", false, -40.0_f64),
        ("clock", true, 24.0),
        ("workspaces", false, 40.0),
    ] {
        let mut b = Bar::with(1920, &TOGGLES[..2]);
        assert!(b.call("start", &[]).truthy());
        b.frames(60);
        let made = b
            .hosts()
            .into_iter()
            .map(|h| (h.0, h.1))
            .collect::<Vec<_>>();
        let (x0, len0) = b.outline(id);
        assert_eq!(b.select(x0 + len0 / 2.0), (x0, len0), "{id}");
        let end = if left { x0 + 2.0 } else { x0 + len0 - 2.0 };
        b.call("press", &[end, 20.0]);
        for k in 1..=(by.abs() / 4.0) as i32 {
            b.call("move", &[end + by.signum() * f64::from(k) * 4.0, 20.0]);
        }
        b.call("rest", &[]);
        let shown = b.hosts();
        assert_ne!(b.host(id).1, len0, "{id}: pulled");
        assert_eq!(
            shown.iter().map(|h| (h.0.clone(), h.1)).collect::<Vec<_>>(),
            made,
            "{id}: the items that show the widgets, while it is pulled"
        );
        let items = b.rt.world.borrow().items.len();
        assert!(b.call("release", &[end + by, 20.0]).truthy());
        assert_eq!(b.hosts(), shown, "{id}: one frame after the drop");
        b.frames(20);
        assert_eq!(b.hosts(), shown, "{id}: twenty frames on");
        assert_eq!(
            b.rt.world.borrow().items.len(),
            items,
            "{id}: items made after the drop"
        );
        assert_ne!(b.file(), layout(&TOGGLES[..2]), "{id}: written");
    }
}
