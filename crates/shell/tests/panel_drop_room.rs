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
fn layout() -> String {
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
    for t in TOGGLES {
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
import ri.modules.rusty.bar.panel
Item {{
    id: root
    property var ctl: overlay
    property var bar: barContent
    EditController {{ id: overlay; connector: "DP-1" }}
    function start(): bool {{ return overlay.start() }}
    function press(x: real, y: real): bool {{ return overlay.press(x, y) }}
    function move(x: real, y: real): bool {{ return overlay.move(x, y) }}
    function release(x: real, y: real): bool {{ return overlay.release(x, y) }}
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

fn tree(width: u32) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("panel-drop-room-{width}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("layout.toml"), layout()).unwrap();
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
    fonts: scene::Fonts,
    now: f64,
    dir: PathBuf,
}

impl Bar {
    fn new(width: u32) -> Bar {
        let dir = tree(width);
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
            fonts: scene::Fonts::default(),
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
                &mut self.fonts,
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
