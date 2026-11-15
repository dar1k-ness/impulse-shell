//! A resize of a sidebar widget in edit mode, on qml's real right sidebar: the player, the
//! width of the sidebar (five columns), pulled by its right end to one column. While the
//! button is held the player itself spans what the resize would give it and takes the form
//! of that size, as a desktop widget does, so at one column it is the circle with no title
//! beside it and the outline is around it; before, it stayed the full card with the outline
//! of the circle over its text until the drop. Nothing is written until the drop, and a
//! resize pulled back leaves the full card.
//!
//! And the blocks with no form for a row of cells (the sliders, the toggles, the notifications,
//! the calendar with tasks): a layout that gives one a single column still shows it the width
//! of the sidebar, where before it drew over the cells beside it, and it has no size to pull.
#![cfg(feature = "interp")]

use qml::edit_mode::WinRect;
use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const SCREEN: (f64, f64) = (1280.0, 1600.0);
const SIDEBAR_W: f64 = 460.0;

const LAYOUT: &str = r#"version = 5
[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "sidebar"
kind = "sidebar"
side = "right"
columns = 5

[[monitor.container.widget]]
id = "player"
type = "media"
size = 5

[[monitor.container.widget]]
id = "audio"
type = "audio"
size = 1
"#;

/// Blocks asked for in one or two columns, with toggles between them.
const BLOCKS: &str = r#"version = 5
[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "sidebar"
kind = "sidebar"
side = "right"
columns = 5

[[monitor.container.widget]]
id = "toggles"
type = "quickToggles"
size = 1

[[monitor.container.widget]]
id = "audio"
type = "audio"
size = 1

[[monitor.container.widget]]
id = "notifications"
type = "notificationCenter"
size = 2

[[monitor.container.widget]]
id = "mic"
type = "mic"
size = 1

[[monitor.container.widget]]
id = "tasks"
type = "calendarTasks"
size = 1
"#;

const MAIN: &str = r#"import QtQuick
import Rustshell
import Impulse
import ri
import ri.modules.rusty.sidebarRight
Item {
    id: root
    property var ctl: overlay
    EditController { id: overlay; connector: "DP-1" }
    function start(): bool { return overlay.start() }
    function press(x: real, y: real): bool { return overlay.press(x, y) }
    function move(x: real, y: real): bool { return overlay.move(x, y) }
    function release(x: real, y: real): bool { return overlay.release(x, y) }
    Component.onCompleted: GlobalStates.sidebarRightOpen = true
    SidebarRight {}
}
"#;

fn tree(name: &str, layout: &str, main: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("sidebar-live-resize-{name}-{}", std::process::id()));
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
    windows: [(usize, f64, f64); 1],
    fonts: scene::Fonts,
    now: f64,
    dir: PathBuf,
}

impl Sidebar {
    fn new(name: &str, layout: &str) -> Sidebar {
        let dir = tree(name, layout, MAIN);
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
        rt.set_screens(&[("DP-1".into(), 0, 0, SCREEN.0 as i32, SCREEN.1 as i32)]);
        rt.settle();
        let root = rt.load_file(&dir.join("main.qml")).unwrap();
        rt.settle();
        let (ctl, sidebar) = {
            let mut w = rt.world.borrow_mut();
            let ctl = match w.read(root, "ctl") {
                Some(Value::Item(i)) => i,
                v => panic!("ctl: {v:?}"),
            };
            let container = w.of_type("SidebarContainer")[0];
            let sidebar = qml::runtime::window_of(&w, container).expect("the sidebar's window");
            w.window_rects.insert(
                sidebar,
                WinRect {
                    connector: "DP-1".into(),
                    x: SCREEN.0 - SIDEBAR_W,
                    y: 0.0,
                    w: SIDEBAR_W,
                    h: SCREEN.1,
                },
            );
            (ctl, sidebar)
        };
        let mut s = Sidebar {
            rt,
            root,
            ctl,
            windows: [(sidebar, SIDEBAR_W, SCREEN.1)],
            fonts: scene::Fonts::default(),
            now: 1000.0,
            dir,
        };
        s.frames(200);
        s
    }

    /// `n` frames of the window: ticks, bindings, layout.
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

    fn file(&self) -> String {
        std::fs::read_to_string(self.dir.join("layout.toml")).unwrap()
    }

    /// The x, y, width and height of rect `k` of the controller, on the screen.
    fn rect(&self, k: &str) -> Option<[f64; 4]> {
        let Value::Obj(o) = self.ctl(k) else {
            return None;
        };
        let o = o.borrow();
        let get = |k: &str| o.props.get(k).map_or(f64::NAN, Value::to_number);
        Some([get("x"), get("y"), get("width"), get("height")])
    }

    /// The edit frame of widget `id`, on the screen.
    fn frame(&self, id: &str) -> [f64; 4] {
        let Value::Obj(list) = self.ctl("frames") else {
            panic!("no frames");
        };
        let list = list.borrow();
        for v in list.array.iter().flatten() {
            let Value::Obj(f) = v else { continue };
            let f = f.borrow();
            let get = |k: &str| f.props.get(k).cloned().unwrap_or_default();
            if get("id").to_js_string() == id {
                return ["x", "y", "width", "height"].map(|k| get(k).to_number());
            }
        }
        panic!("no frame of {id}");
    }

    /// The shown host of widget `id` and where it is on the screen.
    fn host(&mut self, id: &str) -> (usize, [f64; 4]) {
        let mut w = self.rt.world.borrow_mut();
        let hosts: Vec<usize> = w
            .of_type("WidgetHost")
            .into_iter()
            .filter(|&h| {
                w.read(h, "layoutId")
                    .is_some_and(|v| v.to_js_string() == id)
                    && w.read(h, "surface")
                        .is_some_and(|v| v.to_js_string() == "sidebar")
                    && shown(&mut w, h)
            })
            .collect();
        assert_eq!(hosts.len(), 1, "one shown host of {id}");
        let h = hosts[0];
        let (x, y) = w.scene_pos(h, None);
        let size = ["width", "height"].map(|k| w.read(h, k).unwrap_or_default().to_number());
        (h, [SCREEN.0 - SIDEBAR_W + x, y, size[0], size[1]])
    }

    /// Where the player is on the screen, and its card's form.
    fn player(&mut self) -> ([f64; 4], String) {
        let (h, at) = self.host("player");
        let mut w = self.rt.world.borrow_mut();
        let card = w
            .of_type("MediaCard")
            .into_iter()
            .find(|&c| within(&w, c, h))
            .expect("the player's card");
        let form = w.read(card, "form").unwrap_or_default().to_js_string();
        (at, form.to_string())
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

/// Whether item `i` is under `top`.
fn within(w: &World, mut i: usize, top: usize) -> bool {
    loop {
        if i == top {
            return true;
        }
        match w.items[i].parent {
            Some(p) => i = p,
            None => return false,
        }
    }
}

impl Drop for Sidebar {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_sidebar_player_pulled_to_one_column_is_the_circle_while_it_is_pulled() {
    let mut s = Sidebar::new("player", LAYOUT);
    assert!(s.call("start", &[]).truthy());
    s.frames(60);
    let (full, form) = s.player();
    assert_ne!(form, "circle", "five columns: a card with the title");
    let column = full[2] / 5.0;
    // Select it, then take the handle on its right end.
    let mid = (full[0] + full[2] / 2.0, full[1] + full[3] / 2.0);
    s.call("press", &[mid.0, mid.1]);
    s.call("release", &[mid.0, mid.1]);
    s.frames(30);
    let grip = (full[0] + full[2] - 1.0, mid.1);
    assert!(s.call("press", &[grip.0, grip.1]).truthy());
    let before = s.file();
    // To the middle of the first column, a column at a time.
    let to = full[0] + column / 2.0;
    for k in 1..=8 {
        let t = f64::from(k) / 8.0;
        s.call("move", &[grip.0 + (to - grip.0) * t, grip.1]);
        s.frames(4);
        let (at, form) = s.player();
        let columns = (at[2] / column).round();
        assert_eq!(
            form == "circle",
            columns <= 1.0,
            "the form of {columns} columns ({} wide): {form}",
            at[2]
        );
    }
    s.frames(30);
    assert_eq!(s.ctl("handle").to_js_string(), "right", "still resizing");
    let (at, form) = s.player();
    assert_eq!(form, "circle", "pulled to one column, button held");
    assert!(
        at[2] < column * 1.5,
        "the player itself is one column wide: {at:?}, a column is {column}"
    );
    // The outline is around the player as it shows, not over a card that stayed.
    let outline = s.rect("selectedFrame").expect("the outline");
    for i in 0..4 {
        assert!(
            (outline[i] - at[i]).abs() <= 1.0,
            "outline {outline:?}, the player {at:?}"
        );
    }
    // The frame the resize measures from stays the one it started with.
    let frame = s.frame("player");
    assert!((frame[2] - full[2]).abs() <= 1.0, "{frame:?} / {full:?}");
    assert_eq!(
        s.file(),
        before,
        "nothing is written while the button is held"
    );

    // Pulled back to the right end: the full card again.
    s.call("move", &[grip.0, grip.1]);
    s.frames(30);
    let (at, form) = s.player();
    assert_ne!(form, "circle");
    assert!((at[2] - full[2]).abs() <= 1.0, "{at:?} / {full:?}");

    // And dropped at one column: written, and still the circle.
    s.call("move", &[to, grip.1]);
    s.frames(10);
    assert!(s.call("release", &[to, grip.1]).truthy());
    s.frames(60);
    assert!(
        s.file()
            .contains("id = \"player\"\ntype = \"media\"\nsize = 1\n"),
        "{}",
        s.file()
    );
    let (at, form) = s.player();
    assert_eq!(form, "circle");
    assert!(at[2] < column * 1.5, "{at:?}");
}

#[test]
fn a_block_given_one_column_is_the_width_of_the_sidebar_and_over_nothing() {
    let mut s = Sidebar::new("blocks", BLOCKS);
    let before = s.file();
    let ids = ["toggles", "audio", "notifications", "mic", "tasks"];
    let at: Vec<[f64; 4]> = ids.iter().map(|id| s.host(id).1).collect();
    let grid = at[0][2];
    assert!(
        grid > SIDEBAR_W * 0.8,
        "the toggles span the sidebar: {at:?}"
    );
    for (id, r) in ids.iter().zip(&at) {
        if !["audio", "mic"].contains(id) {
            assert!((r[2] - grid).abs() <= 1.0, "{id} spans the sidebar: {r:?}");
        }
        assert!(r[2] > 0.0 && r[3] > 0.0, "{id}: {r:?}");
    }
    for (i, a) in at.iter().enumerate() {
        for (j, b) in at.iter().enumerate().skip(i + 1) {
            let apart = a[0] + a[2] <= b[0] + 0.5
                || b[0] + b[2] <= a[0] + 0.5
                || a[1] + a[3] <= b[1] + 0.5
                || b[1] + b[3] <= a[1] + 0.5;
            assert!(apart, "{} {a:?} over {} {b:?}", ids[i], ids[j]);
        }
    }
    assert_eq!(s.file(), before, "showing them wider writes nothing");

    // In edit mode a block's frame is the block's, and it has no size to pull.
    assert!(s.call("start", &[]).truthy());
    s.frames(60);
    let (_, block) = s.host("toggles");
    let frame = s.frame("toggles");
    for i in 0..4 {
        assert!((frame[i] - block[i]).abs() <= 1.0, "{frame:?} / {block:?}");
    }
    let mid = (block[0] + block[2] / 2.0, block[1] + block[3] / 2.0);
    s.call("press", &[mid.0, mid.1]);
    s.call("release", &[mid.0, mid.1]);
    s.frames(30);
    let grip = (block[0] + block[2] - 1.0, mid.1);
    s.call("press", &[grip.0, grip.1]);
    s.call("move", &[grip.0 - grid / 2.0, grip.1]);
    s.frames(10);
    assert_ne!(
        s.ctl("handle").to_js_string(),
        "right",
        "no resize of a block"
    );
    s.call("release", &[grip.0 - grid / 2.0, grip.1]);
    s.frames(30);
    // It was moved instead, onto its own place: what is written is the width it shows at.
    let lines = |t: &str| -> Vec<String> {
        t.lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    };
    assert_eq!(
        lines(&s.file()),
        lines(&before.replace(
            "type = \"quickToggles\"\nsize = 1\n",
            "type = \"quickToggles\"\nsize = 5\n"
        ))
    );
}
