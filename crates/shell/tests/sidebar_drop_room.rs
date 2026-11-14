//! Edit mode on qml's real right sidebar (SidebarRight, its content and the layout's sidebar
//! container) on a 700 logical high screen, the original's three sidebar widgets in it (quick toggles,
//! notifications, calendar and tasks) and the original's resources widget on the desktop. A drop of the
//! resources into the sidebar charges it what SidebarLayoutContent lays out for it; the sidebar
//! has no room for it, so the drop is refused (badge, no write) instead of being written and
//! then laid out past the screen's bottom. A layout that already holds all four hides what has
//! no room, from the bottom up, and says so; nothing shows past the screen's bottom.
#![cfg(feature = "interp")]

use qml::edit_mode::WinRect;
use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

thread_local!(static CONSOLE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) });

const SCREEN_W: f64 = 1280.0;
const SIDEBAR_W: f64 = 460.0;
/// The desktop resources widget's host, in the desktop window (at the screen's origin).
const HOST: (f64, f64, f64, f64) = (20.0, 20.0, 200.0, 100.0);

/// The original's sidebar as the layout imports it, with the original's resources widget either among its widgets
/// (after the quick toggles) or on the desktop.
fn layout(resources_in_sidebar: bool) -> String {
    let mut s = String::from(
        "version = 5\n[[monitor]]\nconnector = \"DP-1\"\nprimary = true\n\n\
         [[monitor.container]]\nid = \"sidebar\"\nkind = \"sidebar\"\nside = \"right\"\ncolumns = 5\n",
    );
    let mut w = |id: &str, ty: &str, extra: &str| {
        s += &format!("\n[[monitor.container.widget]]\nid = \"{id}\"\ntype = \"{ty}\"\n{extra}");
    };
    w("quickToggles", "quickToggles", "size = 5\n");
    if resources_in_sidebar {
        w("resources", "resources", "size = 5\n");
    }
    w("notificationCenter", "notificationCenter", "size = 5\n");
    w("calendarTasks", "calendarTasks", "size = 5\n");
    s += "\n[[monitor.container]]\nid = \"desktop\"\nkind = \"desktop\"\n";
    if !resources_in_sidebar {
        s += "\n[[monitor.container.widget]]\nid = \"resources\"\ntype = \"resources\"\nx = 20\ny = 20\n";
    }
    s
}

/// The original's right sidebar, open, a desktop window (with the desktop's resources widget unless it is
/// in the sidebar) and the edit overlay's controller.
fn main_qml(height: f64, resources_in_sidebar: bool) -> String {
    let (x, y, w, h) = HOST;
    let host = if resources_in_sidebar {
        String::new()
    } else {
        format!(
            r#"WidgetHost {{ layoutId: "resources"; widgetType: "resources"; surface: "desktop"; x: {x}; y: {y}; width: {w}; height: {h} }}"#
        )
    };
    format!(
        r#"import QtQuick
import Rustshell
import Impulse
import ri
import ri.modules.rusty.sidebarRight
Item {{
    id: root
    property var ctl: overlay
    EditController {{ id: overlay; connector: "DP-1" }}
    function start(): bool {{ return overlay.start() }}
    function add(): string {{ return overlay.addWidget("calendar", "sidebar") }}
    function open(on: real) {{ GlobalStates.sidebarRightOpen = on > 0 }}
    function press(x: real, y: real): bool {{ return overlay.press(x, y) }}
    function move(x: real, y: real): bool {{ return overlay.move(x, y) }}
    function release(x: real, y: real): bool {{ return overlay.release(x, y) }}
    Component.onCompleted: GlobalStates.sidebarRightOpen = true
    SidebarRight {{}}
    PanelWindow {{
        objectName: "desktop"
        implicitWidth: {dw}
        implicitHeight: {height}
        {host}
    }}
}}
"#,
        dw = SCREEN_W - SIDEBAR_W,
    )
}

fn tree(name: &str, layout: &str, main: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("sidebar-drop-room-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("layout.toml"), layout).unwrap();
    std::fs::write(d.join("main.qml"), main).unwrap();
    d
}

struct Sidebar {
    rt: Runtime,
    root: usize,
    ctl: usize,
    container: usize,
    windows: [(usize, f64, f64); 2],
    height: f64,
    fonts: scene::Fonts,
    now: f64,
    dir: PathBuf,
}

impl Sidebar {
    fn new(name: &str, height: f64, resources_in_sidebar: bool) -> Sidebar {
        qml::builtins::CONSOLE_SINK
            .get_or_init(|| |_, line| CONSOLE.with_borrow_mut(|c| c.push(line.to_owned())));
        CONSOLE.with_borrow_mut(Vec::clear);
        let dir = tree(
            name,
            &layout(resources_in_sidebar),
            &main_qml(height, resources_in_sidebar),
        );
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
        // The monitor the windows show on: the sidebar layout names its connector after it.
        rt.set_screens(&[("DP-1".into(), 0, 0, SCREEN_W as i32, height as i32)]);
        rt.settle();
        let root = rt.load_file(&dir.join("main.qml")).unwrap();
        rt.settle();
        let (ctl, sidebar, desktop, container) = {
            let mut w = rt.world.borrow_mut();
            let ctl = match w.read(root, "ctl") {
                Some(Value::Item(i)) => i,
                v => panic!("ctl: {v:?}"),
            };
            let wins = w.of_type("PanelWindow");
            let named = |w: &mut World, i: usize| matches!(w.read(i, "objectName"), Some(Value::Str(s)) if s.as_ref() == "desktop");
            let desktop = wins.iter().copied().find(|&i| named(&mut w, i)).unwrap();
            let container = w.of_type("SidebarContainer")[0];
            let sidebar = qml::runtime::window_of(&w, container).expect("the sidebar's window");
            // Where the shell would place them: the sidebar against the screen's right edge,
            // top to bottom, the desktop window left of it.
            for (win, x, ww) in [
                (sidebar, SCREEN_W - SIDEBAR_W, SIDEBAR_W),
                (desktop, 0.0, SCREEN_W - SIDEBAR_W),
            ] {
                w.window_rects.insert(
                    win,
                    WinRect {
                        connector: "DP-1".into(),
                        x,
                        y: 0.0,
                        w: ww,
                        h: height,
                    },
                );
            }
            (ctl, sidebar, desktop, container)
        };
        let mut s = Sidebar {
            rt,
            root,
            ctl,
            container,
            windows: [
                (sidebar, SIDEBAR_W, height),
                (desktop, SCREEN_W - SIDEBAR_W, height),
            ],
            height,
            fonts: scene::Fonts::default(),
            now: 1000.0,
            dir,
        };
        // The original's widgets come in animated (Behavior on implicitHeight): a few seconds of frames.
        s.frames(200);
        s
    }

    /// `n` frames of both windows: ticks, bindings, layout.
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.now += 16.0;
            self.rt.tick(self.now);
            self.rt.settle();
            for (win, w, h) in self.windows {
                scene::layout_window(&mut self.rt, &mut self.fonts, win, [w, h]);
            }
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

    fn ctl(&self, k: &str) -> Value {
        self.rt
            .world
            .borrow_mut()
            .read(self.ctl, k)
            .unwrap_or_default()
    }

    /// The sidebar's height for its sections and, by id, what each widget costs it there.
    fn measured(&self) -> (f64, Vec<(String, f64)>) {
        let mut w = self.rt.world.borrow_mut();
        let available = w
            .read(self.container, "available")
            .unwrap_or_default()
            .to_number();
        let mut heights: Vec<(String, f64)> = match w.read(self.container, "sidebarHeights") {
            Some(Value::Obj(o)) => o
                .borrow()
                .props
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_number()))
                .collect(),
            _ => vec![],
        };
        heights.sort_by(|a, b| a.0.cmp(&b.0));
        (available, heights)
    }

    /// Each sidebar widget that shows, with the bottom of its host on the screen, unclipped.
    fn shown(&mut self) -> Vec<(String, f64)> {
        let hosts: Vec<usize> = {
            let w = self.rt.world.borrow();
            (0..w.items.len())
                .filter(|&i| {
                    !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "WidgetHost")
                })
                .collect()
        };
        let mut out = vec![];
        for h in hosts {
            let (id, bottom) = {
                let mut w = self.rt.world.borrow_mut();
                if !w
                    .read(h, "surface")
                    .is_some_and(|v| v.to_js_string() == "sidebar")
                    || !shown(&mut w, h)
                {
                    continue;
                }
                let id = w.read(h, "layoutId").unwrap_or_default().to_js_string();
                if id.is_empty() {
                    continue;
                }
                let height = w.read(h, "height").unwrap_or_default().to_number();
                let (_, y) = w.scene_pos(h, None);
                (id, y + height)
            };
            out.push((id, bottom));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn file(&self) -> String {
        std::fs::read_to_string(self.dir.join("layout.toml")).unwrap()
    }

    fn sidebar_log(&self) -> Vec<String> {
        CONSOLE.with_borrow(|c| {
            c.iter()
                .filter(|l| l.starts_with("[Sidebar]"))
                .cloned()
                .collect()
        })
    }

    /// Drags the desktop's resources to (x, y) on the sidebar; returns whether the preview was
    /// valid there, whether the refusal badge showed, and what the release returned.
    fn drag_to(&mut self, x: f64, y: f64) -> (bool, bool, bool) {
        let from = (HOST.0 + HOST.2 / 2.0, HOST.1 + HOST.3 / 2.0);
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

    /// The edit frames of the sidebar's widgets: id and bottom on the screen.
    fn frames_bottoms(&self) -> Vec<(String, f64)> {
        let Value::Obj(list) = self.ctl("frames") else {
            return vec![];
        };
        let list = list.borrow();
        let mut out = vec![];
        for v in list.array.iter().flatten() {
            let Value::Obj(f) = v else { continue };
            let f = f.borrow();
            let get = |k: &str| f.props.get(k).cloned().unwrap_or_default();
            if get("kind").to_js_string() == "sidebar" {
                out.push((
                    get("id").to_js_string(),
                    get("y").to_number() + get("height").to_number(),
                ));
            }
        }
        out
    }
}

/// Whether `i` and every item above it is visible.
fn shown(w: &mut World, mut i: usize) -> bool {
    loop {
        if !w.read(i, "visible").is_none_or(|v| v.truthy()) {
            return false;
        }
        match w.items[i].parent {
            Some(p) => i = p,
            None => return true,
        }
    }
}

impl Drop for Sidebar {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A point on the sidebar, between the quick toggles and the notifications.
fn on_sidebar() -> (f64, f64) {
    (SCREEN_W - SIDEBAR_W / 2.0, 200.0)
}

#[test]
fn a_fourth_widget_the_700_sidebar_has_no_room_for_is_refused_without_a_write() {
    let mut s = Sidebar::new("refused", 700.0, false);
    assert_eq!(
        s.sidebar_log(),
        [] as [String; 0],
        "the original's three widgets fit"
    );
    let bottoms = s.shown();
    assert_eq!(bottoms.len(), 3, "{bottoms:?}");
    assert!(bottoms.iter().all(|b| b.1 <= s.height), "{bottoms:?}");
    assert!(s.call("start", &[]).truthy());
    // The probe comes in animated too.
    s.frames(200);
    let (available, heights) = s.measured();
    let cost = |id: &str| heights.iter().find(|h| h.0 == id).map(|h| h.1);
    let resources = cost("resources").expect("the sidebar measured the desktop's resources");
    assert!(resources > 0.0);
    let spacing = 10.0;
    let need = ["quickToggles", "notificationCenter", "calendarTasks"]
        .iter()
        .map(|id| cost(id).expect(id))
        .sum::<f64>()
        + resources
        + 3.0 * spacing;
    assert!(need > available, "no room: {need} > {available}");
    let before = s.file();
    let log = s.sidebar_log().len();
    let (x, y) = on_sidebar();
    let (valid, badge, released) = s.drag_to(x, y);
    assert!(!valid, "the preview is refused");
    assert!(badge, "the refusal badge shows");
    assert!(!released, "the release drops nothing");
    s.frames(60);
    assert_eq!(s.file(), before, "a refused drop writes nothing");
    assert_eq!(s.sidebar_log()[log..], [] as [String; 0], "nothing hidden");
    let frames = s.frames_bottoms();
    assert_eq!(frames.len(), 3, "{frames:?}");
    assert!(frames.iter().all(|f| f.1 <= s.height), "{frames:?}");
}

#[test]
fn a_700_sidebar_that_already_overflows_hides_what_has_no_room_instead_of_drawing_it_off_screen() {
    let mut s = Sidebar::new("overflow", 700.0, true);
    let log = s.sidebar_log();
    assert_eq!(
        log.last().map(String::as_str),
        Some("[Sidebar] DP-1 has no room for calendarTasks: hidden"),
        "{log:?}"
    );
    let bottoms = s.shown();
    let ids: Vec<&str> = bottoms.iter().map(|b| b.0.as_str()).collect();
    assert_eq!(ids, ["notificationCenter", "quickToggles", "resources"]);
    assert!(
        bottoms.iter().all(|b| b.1 <= s.height),
        "nothing past the screen's bottom: {bottoms:?}"
    );
    // Edit mode frames what shows, all of it on the screen.
    assert!(s.call("start", &[]).truthy());
    s.frames(60);
    let mut frames = s.frames_bottoms();
    frames.sort_by(|a, b| a.0.cmp(&b.0));
    let ids: Vec<&str> = frames.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(ids, ["notificationCenter", "quickToggles", "resources"]);
    assert!(frames.iter().all(|f| f.1 <= s.height), "{frames:?}");
}

/// A widget the catalog adds is measured only once the sidebar has it. In a sidebar with no room
/// for it the controller then says the sidebar overflows, which is what takes the add back.
#[test]
fn a_catalog_widget_the_700_sidebar_then_hides_is_reported_as_overflowing() {
    let mut s = Sidebar::new("catalog", 700.0, true);
    assert!(s.call("start", &[]).truthy());
    s.frames(200);
    // Closed, as when the catalog is opened from the desktop: the add opens it.
    s.call("open", &[0.0]);
    s.frames(200);
    assert!(s.call("add", &[]).truthy());
    s.call("open", &[1.0]);
    s.frames(200);
    let place = s.ctl("selectedPlace");
    let overflows = match place {
        Value::Obj(o) => o
            .borrow()
            .props
            .get("overflows")
            .is_some_and(|v| v.truthy()),
        _ => false,
    };
    assert!(
        overflows,
        "the controller does not say the sidebar overflows"
    );
}
