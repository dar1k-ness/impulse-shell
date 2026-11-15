//! A pick in the widget catalog's «Right sidebar» with the sidebar full: the nine widgets across
//! all five columns of the PC this was reported from, on a screen they fill. The sidebar has a
//! new widget's height only once it has laid it out, so the pick is written, the sidebar builds
//! its content again, finds no room, and the edit overlay takes the widget back, which builds
//! the content once more.
//!
//! While the sidebar is open it is in the tree's focus grab (GlobalFocusGrab.qml), whose
//! `windows` asked each item of the sidebar for `activeFocus`: every item of the content being
//! built changed a list of children it had read, so it asked the whole sidebar again, some
//! hundreds of times a build. The overlay was frozen for seconds. It asks the window's root
//! item now, so a build leaves the grab alone.
//!
//! With the sidebar closed the same pick opens it. The content of the layout before the pick was
//! replaced inside the handlers of that opening, under the list views that were scrolling to
//! their start: `Cannot read property 'contentY' of undefined`.
#![cfg(feature = "interp")]

use qml::edit_mode::WinRect;
use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

thread_local!(static CONSOLE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) });

const SCREEN_W: f64 = 1920.0;
const SIDEBAR_W: f64 = 460.0;
const HEIGHT: f64 = 900.0;

/// A bar, a desktop with one widget and the full sidebar.
fn layout() -> String {
    r##"version = 5

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
id = "workspaces"
type = "workspaces"
zone = "center"

[[monitor.container.widget]]
id = "separator-1"
type = "separator"
zone = "center"

[[monitor.container.widget]]
id = "separator-2"
type = "separator"
zone = "center"

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
id = "resources"
type = "resources"
zone = "start"

[[monitor.container.widget]]
id = "media"
type = "media"
zone = "start"
width = "stretch"

[[monitor.container.widget]]
id = "battery-2"
type = "battery"
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
id = "battery-1"
type = "battery"
zone = "end"

[[monitor.container.widget]]
id = "weather"
type = "weather"
zone = "end"

[[monitor.container.widget]]
id = "indicators"
type = "indicators"
zone = "end"

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "desktop-weather"
type = "weather"
x = 400
y = 100

[[monitor.container]]
id = "sidebar"
kind = "sidebar"
side = "right"
columns = 5

[[monitor.container.widget]]
id = "sliders"
type = "sliders"
size = 5

[[monitor.container.widget]]
id = "notificationCenter"
type = "notificationCenter"
size = 5

[[monitor.container.widget]]
id = "easyEffects-1"
type = "easyEffects"
size = 5

[[monitor.container.widget]]
id = "musicRecognition-1"
type = "musicRecognition"
size = 5

[[monitor.container.widget]]
id = "quickToggles"
type = "quickToggles"
size = 5

[[monitor.container.widget]]
id = "calendarTasks"
type = "calendarTasks"
size = 5

[[monitor.container.widget]]
id = "sliders-1"
type = "sliders"
size = 5

[[monitor.container.widget]]
id = "cloudflareWarp-1"
type = "cloudflareWarp"
size = 1

[[monitor.container.widget]]
id = "sliders-2"
type = "sliders"
size = 5
"##
    .into()
}

/// The right sidebar, a desktop window and the edit controller of the monitor.
fn main_qml(open: bool) -> String {
    format!(
        r#"import QtQuick
import Rustshell
import Impulse
import ri
import ri.modules.common
import ri.services
import ri.modules.rusty.sidebarRight
Item {{
    id: root
    property var ctl: overlay
    EditController {{ id: overlay; connector: "DP-1" }}
    function start(): bool {{ return overlay.start() }}
    function span(cells): bool {{ return overlay.setSpan(cells) }}
    function undo(): bool {{ return overlay.undo() }}
    property int notifications: Notifications.list.length
    function readNotifications(path) {{
        Notifications.filePath = path;
        Notifications.refresh();
    }}
    Component.onCompleted: {{
        Persistent.states.sidebar.bottomGroup.tab = 1;
        GlobalStates.sidebarRightOpen = {open};
    }}
    SidebarRight {{}}
    PanelWindow {{
        objectName: "desktop"
        implicitWidth: {dw}
        implicitHeight: {height}
    }}
}}
"#,
        dw = SCREEN_W - SIDEBAR_W,
        height = HEIGHT,
    )
}

/// The edit overlay of the monitor, with its widget catalog.
const OVERLAY: &str = r#"import QtQuick
import Rustshell
import ri.modules.rusty.editMode
EditOverlay {
    screen: Rustshell.screens[0]
}
"#;

fn tree(name: &str, layout: &str, main: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!(
        "sidebar-catalog-pick-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("layout.toml"), layout).unwrap();
    std::fs::write(d.join("main.qml"), main).unwrap();
    std::fs::write(d.join("overlay.qml"), OVERLAY).unwrap();
    d
}

struct Sidebar {
    rt: Runtime,
    root: usize,
    ctl: usize,
    windows: Vec<(usize, f64, f64)>,
    fonts: scene::Fonts,
    wake: testenv::Wakeups,
    ep: Option<i32>,
    now: f64,
    dir: PathBuf,
}

impl Sidebar {
    fn new(name: &str, open: bool) -> Sidebar {
        Sidebar::of(name, open, &layout())
    }

    fn of(name: &str, open: bool, layout: &str) -> Sidebar {
        qml::builtins::CONSOLE_SINK
            .get_or_init(|| |_, line| CONSOLE.with_borrow_mut(|c| c.push(line.to_owned())));
        CONSOLE.with_borrow_mut(Vec::clear);
        let dir = tree(name, layout, &main_qml(open));
        let mut rt = Runtime::new(Some(dir.clone()));
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
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
        rt.set_screens(&[("DP-1".into(), 0, 0, SCREEN_W as i32, HEIGHT as i32)]);
        rt.settle();
        rt.world.borrow_mut().frame_paced = true;
        let root = rt.load_file(&dir.join("main.qml")).unwrap();
        rt.settle();
        let overlay = rt.load_file(&dir.join("overlay.qml")).unwrap();
        rt.settle();
        let (ctl, sidebar, desktop) = {
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
                (overlay, 0.0, SCREEN_W),
            ] {
                w.window_rects.insert(
                    win,
                    WinRect {
                        connector: "DP-1".into(),
                        x,
                        y: 0.0,
                        w: ww,
                        h: HEIGHT,
                    },
                );
            }
            (ctl, sidebar, desktop)
        };
        let ep = rt.process_fd();
        let mut s = Sidebar {
            rt,
            wake,
            ep,
            root,
            ctl,
            windows: vec![
                (sidebar, SIDEBAR_W, HEIGHT),
                (desktop, SCREEN_W - SIDEBAR_W, HEIGHT),
                (overlay, SCREEN_W, HEIGHT),
            ],
            fonts: scene::Fonts::default(),
            now: 1000.0,
            dir,
        };
        // The widgets come in animated (Behavior on implicitHeight): a few seconds of frames.
        s.frames(200);
        s
    }

    /// `n` frames of the windows: ticks, bindings, layout.
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.now += 16.0;
            self.rt.tick(self.now);
            self.rt.settle();
            // The shell paces the frames of the windows it shows: a view in one makes its rows
            // in the frame's polish, one in no shown window when a call needs them.
            let shown: Vec<usize> = (self.windows.iter())
                .map(|w| w.0)
                .filter(|&win| self.rt.get(win, "visible").truthy())
                .collect();
            self.rt.set_polish_wins(shown.iter().copied());
            let held = self.rt.held_windows();
            for &win in shown.iter().filter(|w| held.contains(w)) {
                self.rt.polish(&[win]);
            }
            for (win, w, h) in self.windows.clone() {
                if !shown.contains(&win) {
                    continue;
                }
                scene::layout_window(&mut self.rt, &mut self.fonts, win, [w, h]);
            }
            self.rt.settle();
        }
        assert!(self.rt.errors().is_empty(), "{:?}", self.rt.errors());
    }

    /// Notifications of one program arrive, `n` in all: read from the file the service keeps
    /// them in.
    fn notifications(&mut self, n: usize) {
        self.notifications_of(&["app"].repeat(n));
    }

    /// The notifications are now these, one of each program named, oldest first.
    fn notifications_of(&mut self, apps: &[&str]) {
        let n = apps.len();
        let file = self.dir.join("notifications.json");
        let one = serde_json::Value::Array(
            (1..=n)
                .map(|id| {
                    serde_json::json!({
                        "notificationId": id,
                        "actions": [],
                        "appIcon": "",
                        "appName": apps[id - 1],
                        "body": "body",
                        "image": "",
                        "summary": "summary",
                        "time": 1_700_000_000_000_f64 + id as f64,
                        "urgency": "normal",
                    })
                })
                .collect(),
        );
        std::fs::write(&file, one.to_string()).unwrap();
        self.rt
            .call_method(
                self.root,
                "readNotifications",
                vec![Value::Str(file.to_str().unwrap().into())],
            )
            .unwrap()
            .unwrap();
        for _ in 0..200 {
            self.frames(1);
            let have = self.rt.world.borrow_mut().read(self.root, "notifications");
            if matches!(have, Some(Value::Num(have)) if have == n as f64) {
                self.frames(10);
                return;
            }
            let woke = (self.wake).wait(self.ep, Some(std::time::Duration::from_millis(50)));
            if woke.fd {
                self.rt.process_fd_ready();
            }
            self.rt.poll_native();
            self.rt.settle();
        }
        panic!("the notifications are not read");
    }

    /// Edit mode on.
    fn start(&mut self) {
        let on = self.rt.call_method(self.root, "start", vec![]);
        assert!(on.unwrap().unwrap().truthy(), "edit mode did not start");
        self.frames(200);
    }

    /// The overlay's widget catalog and the Loader it is in (EditOverlay.qml `catalogDialog`).
    fn catalog(&self) -> (usize, usize) {
        let mut w = self.rt.world.borrow_mut();
        let palette = w.of_type("WidgetPalette")[0];
        let loader = (w.of_type("Loader").into_iter())
            .find(|&l| matches!(w.read(l, "item"), Some(Value::Item(i)) if i == palette))
            .expect("the catalog's Loader");
        (palette, loader)
    }

    /// Whether the catalog is up, and whether it says the last pick had no room.
    fn catalog_says(&self) -> (bool, bool) {
        let (palette, loader) = self.catalog();
        let mut w = self.rt.world.borrow_mut();
        let on = |w: &mut World, i, k| w.read(i, k).is_some_and(|v| v.truthy());
        (
            on(&mut w, loader, "showing"),
            on(&mut w, palette, "refused"),
        )
    }

    /// «Add widget», «Right sidebar», a click on the tile of `ty`: what the tile's click emits.
    fn pick(&mut self, ty: &str) {
        let (palette, loader) = self.catalog();
        self.rt
            .call_method(loader, "open", vec![])
            .unwrap()
            .unwrap();
        self.rt.set(palette, "surface", Value::str("sidebar"));
        self.frames(30);
        assert_eq!(self.catalog_says(), (true, false));
        self.rt
            .call_method(
                palette,
                "picked",
                vec![Value::str(ty), Value::str("sidebar")],
            )
            .unwrap()
            .unwrap();
        self.frames(1);
    }

    fn ctl(&self, k: &str) -> Value {
        self.rt
            .world
            .borrow_mut()
            .read(self.ctl, k)
            .unwrap_or_default()
    }

    /// Whether the controller says the selected widget's sidebar hides a widget.
    fn overflows(&self) -> bool {
        match self.ctl("selectedPlace") {
            Value::Obj(o) => o
                .borrow()
                .props
                .get("overflows")
                .is_some_and(|v| v.truthy()),
            _ => false,
        }
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

    /// The widgets the sidebar shows, by layout id, each with the item that holds it.
    fn hosts(&self) -> Vec<(String, usize)> {
        let mut w = self.rt.world.borrow_mut();
        let text = |w: &mut World, i, k| w.read(i, k).unwrap_or_default().to_js_string();
        let mut hosts = Vec::new();
        for h in w.of_type("WidgetHost") {
            let id = text(&mut w, h, "layoutId");
            if text(&mut w, h, "surface") == "sidebar" && !id.is_empty() {
                hosts.push((id, h));
            }
        }
        hosts.sort();
        hosts
    }

    /// The list the tree's one HyprlandFocusGrab whitelists, as the object its binding made.
    fn grab_windows(&self) -> (Value, Vec<usize>) {
        let mut w = self.rt.world.borrow_mut();
        let grabs = w.of_type("HyprlandFocusGrab");
        assert_eq!(grabs.len(), 1);
        let v = w.read(grabs[0], "windows").unwrap_or_default();
        let wins = match &v {
            Value::Obj(o) => o
                .borrow()
                .array
                .iter()
                .flatten()
                .filter_map(|x| match x {
                    Value::Item(c) => Some(*c),
                    _ => None,
                })
                .collect(),
            v => panic!("windows: {v:?}"),
        };
        (v, wins)
    }
}

fn same_object(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Obj(a), Value::Obj(b)) => std::rc::Rc::ptr_eq(a, b),
        _ => false,
    }
}

impl Drop for Sidebar {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_pick_into_the_full_open_sidebar_is_taken_back_and_leaves_the_focus_grab_alone() {
    let mut s = Sidebar::new("open", true);
    s.start();
    let before = s.file();
    assert_eq!(
        s.sidebar_log(),
        [""; 0],
        "the sidebar shows all its widgets"
    );
    let sidebar = s.windows[0].0;
    let (grab, wins) = s.grab_windows();
    let overlay = s.windows[2].0;
    assert_eq!(wins, [sidebar, overlay], "the open sidebar and the overlay");

    // The pick is written and the sidebar lays the widget out: no room, it hides it.
    let hosts = s.hosts();
    assert_eq!(hosts.len(), 9);
    s.pick("weather");
    let mut with = hosts.clone();
    with.extend(s.hosts().into_iter().filter(|(id, _)| id == "weather-1"));
    assert_eq!(with.len(), 10, "the widget added is made");
    assert_eq!(
        s.hosts(),
        with,
        "and the nine there were are not made again"
    );
    assert_ne!(s.file(), before, "the pick is written");
    assert_eq!(s.ctl("selectedId").to_js_string(), "weather-1");
    assert_eq!(s.catalog_says(), (false, false), "the catalog closes");
    s.frames(120);
    assert_eq!(
        s.sidebar_log(),
        [
            "[Sidebar] DP-1 has no room for weather-1: hidden",
            "[Sidebar] DP-1 shows all its widgets",
        ],
    );
    // The overlay took it back: the layout is as it was, the catalog is back and says why.
    assert_eq!(s.file(), before, "the pick is taken back");
    assert!(!s.overflows());
    assert_eq!(s.catalog_says(), (true, true));
    assert_eq!(s.hosts(), hosts, "taking it back made no widget again");
    let (now, wins) = s.grab_windows();
    assert_eq!(wins, [sidebar, overlay]);
    assert!(
        same_object(&grab, &now),
        "the sidebar building its content made the focus grab list its windows again"
    );
}

/// The sidebar is closed, its content kept loaded (`sidebar.keepRightSidebarLoaded`, on by
/// default), and a notification came since it was last shown: its list has the row yet to
/// make. The pick opens the sidebar, and no handler of that opening is left running on the
/// content of the layout before the pick.
#[test]
fn a_pick_into_the_full_closed_sidebar_opens_it_and_is_taken_back() {
    let mut s = Sidebar::new("closed", false);
    s.start();
    let before = s.file();
    let sidebar = s.windows[0].0;
    assert!(!s.rt.get(sidebar, "visible").truthy());
    s.notifications(1);

    // `frames` fails on an error of the runtime. Nothing paces a window that was not shown:
    // the sidebar has laid the widget out and the overlay taken it back within the frame.
    let hosts = s.hosts();
    assert_eq!(hosts.len(), 9, "the closed sidebar keeps its content");
    s.pick("weather");
    assert!(s.rt.get(sidebar, "visible").truthy(), "the pick opens it");
    s.frames(120);
    assert_eq!(s.hosts(), hosts, "no widget there was is made again");
    assert_eq!(s.file(), before, "the pick is taken back");
    assert_eq!(s.catalog_says(), (true, true));
    assert!(
        s.rt.get(sidebar, "visible").truthy(),
        "the sidebar stays open"
    );
}

/// A closed sidebar makes the rows of the notifications that came while it was closed before
/// it is opened (NotificationList.qml `fillWhileClosed`): the click that opens it makes none.
#[test]
fn a_closed_sidebar_has_made_its_notifications_before_it_is_opened() {
    let mut s = Sidebar::new("prefill", false);
    let sidebar = s.windows[0].0;
    let of = |s: &Sidebar, ty: &str| s.rt.world.borrow_mut().of_type(ty);
    let rows = |s: &Sidebar| (of(s, "NotificationGroup"), of(s, "NotificationItem"));
    assert_eq!(rows(&s), (vec![], vec![]));
    s.notifications(3);
    s.frames(100);
    assert!(!s.rt.get(sidebar, "visible").truthy());
    let made = rows(&s);
    assert_eq!(made.0.len(), 1, "the closed sidebar made the program's row");
    assert_eq!(
        made.1.len(),
        2,
        "and the two notifications a folded row shows"
    );

    s.start();
    s.pick("weather");
    assert!(s.rt.get(sidebar, "visible").truthy(), "the pick opens it");
    s.frames(120);
    assert_eq!(rows(&s), made, "opening it made no row again");
    assert_eq!(s.catalog_says(), (true, true));
}

/// A notification that comes while the sidebar is closed gets its rows then too, and the rows
/// the closed sidebar already made stay the ones it shows.
#[test]
fn a_closed_sidebar_makes_the_rows_of_a_notification_that_comes_later() {
    let mut s = Sidebar::new("prefill-later", false);
    let sidebar = s.windows[0].0;
    let of = |s: &Sidebar, ty: &str| s.rt.world.borrow_mut().of_type(ty);
    let rows = |s: &Sidebar| (of(s, "NotificationGroup"), of(s, "NotificationItem"));
    s.notifications_of(&["app", "app"]);
    s.frames(100);
    let first = rows(&s);
    assert_eq!((first.0.len(), first.1.len()), (1, 2));

    s.notifications_of(&["app", "app", "other"]);
    s.frames(100);
    assert!(!s.rt.get(sidebar, "visible").truthy());
    let made = rows(&s);
    assert_eq!(
        (made.0.len(), made.1.len()),
        (2, 3),
        "the closed sidebar made the second program's row and its notification"
    );

    s.start();
    s.pick("weather");
    assert!(s.rt.get(sidebar, "visible").truthy(), "the pick opens it");
    s.frames(120);
    assert_eq!(rows(&s), made, "opening it made no row again");
}

/// The layout with the sidebar's widgets `ids` taken out.
fn without(ids: &[&str]) -> String {
    let layout = layout();
    let mut out = String::new();
    for (k, block) in layout.split("[[monitor.container.widget]]\n").enumerate() {
        if ids
            .iter()
            .any(|id| block.starts_with(&format!("id = \"{id}\"\n")))
        {
            continue;
        }
        if k > 0 {
            out.push_str("[[monitor.container.widget]]\n");
        }
        out.push_str(block);
    }
    out
}

/// A sidebar with room: the widget picked is made, narrowed into the row of the toggle above
/// it and widened back, and through all of it the widgets that were there stay the ones made.
#[test]
fn a_change_of_the_sidebar_makes_only_the_widget_it_changes() {
    let layout = without(&[
        "easyEffects-1",
        "musicRecognition-1",
        "quickToggles",
        "calendarTasks",
        "sliders-1",
        "sliders-2",
    ]);
    let mut s = Sidebar::of("room", true, &layout);
    s.start();
    let hosts = s.hosts();
    let ids = |hosts: &[(String, usize)]| hosts.iter().map(|h| h.0.clone()).collect::<Vec<_>>();
    assert_eq!(
        ids(&hosts),
        ["cloudflareWarp-1", "notificationCenter", "sliders"]
    );
    let others = |s: &Sidebar| -> Vec<(String, usize)> {
        (s.hosts().into_iter())
            .filter(|(id, _)| id != "weather-1")
            .collect()
    };

    s.pick("weather");
    s.frames(60);
    assert!(!s.overflows());
    assert_eq!(s.catalog_says(), (false, false), "the pick has room");
    assert_eq!(s.hosts().len(), 4, "the widget picked is made");
    assert_eq!(others(&s), hosts);

    // Two cells of five: it joins the row of the toggle.
    let narrowed = s.rt.call_method(s.root, "span", vec![Value::Num(2.0)]);
    assert!(narrowed.unwrap().unwrap().truthy());
    s.frames(60);
    assert!(
        s.file()
            .contains("id = \"weather-1\"\ntype = \"weather\"\nsize = 2\n")
    );
    assert_eq!(s.hosts().len(), 4);
    assert_eq!(
        others(&s),
        hosts,
        "the toggle whose row it joins is not made again"
    );

    let undone = s.rt.call_method(s.root, "undo", vec![]);
    assert!(undone.unwrap().unwrap().truthy());
    s.frames(60);
    assert_eq!(s.hosts().len(), 4);
    assert_eq!(others(&s), hosts);
    assert_eq!(s.sidebar_log(), [""; 0], "nothing was ever hidden");
}
