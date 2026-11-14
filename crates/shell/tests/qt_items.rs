//! What every item does with `opacity`, `clip`, `enabled`, corner radii, `elide` and a disabled
//! `Connections`, as Qt 6.12 does it: the scenes in `qt_items/` painted and clicked here, with
//! the values `qt_items/tst_qt_items.qml` checks in Qt itself (qmltestrunner in the
//! tools/testenv image; the command is in that file).

use qml::runtime::Runtime;
use qml::value::Value;
use qsg::raster::Canvas;
use shell::input::Pointer;
use shell::scene::{self, Target};
use std::path::Path;

const BTN_LEFT: u32 = 0x110;

fn load(name: &str) -> (Runtime, usize, [f64; 2]) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/qt_items");
    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join(name)).expect("scene loads");
    rt.settle();
    let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
    (rt, root, size)
}

struct Painted {
    c: Canvas,
    w: usize,
}

impl Painted {
    fn px(&self, x: usize, y: usize) -> [u8; 3] {
        let p = self.c.px[y * self.w + x];
        assert_eq!(p[3], 255, "the window is opaque at {x},{y}");
        [p[0], p[1], p[2]]
    }

    /// First and last column with ink in the 20 rows of text `row`, from the text's left edge.
    fn ink(&self, row: usize) -> Option<(usize, usize)> {
        let cols: Vec<usize> = (0..100)
            .filter(|x| (0..20).any(|y| self.px(140 + x, 60 + 20 * row + y) != BLACK))
            .collect();
        Some((*cols.first()?, *cols.last()?))
    }
}

fn paint(gpu: bool) -> Painted {
    let (mut rt, root, size) = load("Paint.qml");
    let (w, h) = (size[0] as usize, size[1] as usize);
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, root, size);
    }
    let errors = rt.errors();
    assert!(errors.is_empty(), "{errors:?}");
    if !gpu {
        let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
        scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut c));
        return Painted { c, w };
    }
    let (mut g, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let mut fr = shell::gpu::Frame::new(w, h);
    scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Gpu(&mut fr));
    let mut s = g.pbuffer(w as i32, h as i32).expect("pbuffer");
    s.reswap();
    g.render(&mut s, &mut fr, || {});
    Painted {
        c: g.read_surface(&s),
        w,
    }
}

const BLACK: [u8; 3] = [0, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];
const GREEN: [u8; 3] = [0, 255, 0];

fn parent_opacity(gpu: bool) {
    let p = paint(gpu);
    assert_eq!(
        p.px(25, 15),
        [100, 50, 20],
        "a child of an item with opacity 0.5 is drawn at 0.5"
    );
    assert_eq!(
        p.px(25, 25),
        [50, 25, 10],
        "the parent's opacity 0.5 times the child's 0.5 is 0.25"
    );
}

fn clip(gpu: bool) {
    let p = paint(gpu);
    assert_eq!(p.px(80, 30), GREEN, "the child shows inside the clip");
    assert_eq!(
        [p.px(65, 30), p.px(95, 30), p.px(80, 15), p.px(80, 45)],
        [BLACK; 4],
        "`clip: true` cuts the child off left, right, above and below the item"
    );
    assert_eq!(
        [p.px(65, 90), p.px(95, 90), p.px(80, 75), p.px(80, 105)],
        [GREEN; 4],
        "without `clip` the child shows past the item on every side"
    );
}

fn corner_radii(gpu: bool) {
    let p = paint(gpu);
    assert_eq!(
        (p.px(143, 13), p.px(149, 19)),
        (BLACK, WHITE),
        "topLeftRadius 20 cuts the corner 3 px in and fills it 9 px in"
    );
    assert_eq!(p.px(178, 11), WHITE, "topRightRadius 0 is a square corner");
    assert_eq!(
        (p.px(141, 47), p.px(145, 44)),
        (BLACK, WHITE),
        "bottomLeftRadius 10 cuts the corner 2 px in and fills it 5 px in"
    );
    assert_eq!(
        (p.px(179, 49), p.px(176, 46)),
        (BLACK, WHITE),
        "the bottom right corner, not set, has `radius` 6"
    );
}

/// Qt's ink of the four texts (ElideRight, ElideLeft, ElideMiddle, ElideNone), in columns from
/// the text's left edge.
const QT_INK: [(usize, usize); 4] = [(0, 64), (1, 66), (0, 61), (0, 99)];

fn elide(gpu: bool) {
    let p = paint(gpu);
    let ink: Vec<_> = (0..4)
        .map(|row| p.ink(row).expect("the text has ink"))
        .collect();
    for (row, mode) in ["ElideRight", "ElideLeft", "ElideMiddle"]
        .iter()
        .enumerate()
    {
        assert!(
            ink[row].1 < 70,
            "{mode}: the text ends inside its 70 px, ink in columns {:?}",
            ink[row]
        );
    }
    assert!(
        ink[3].1 >= 99,
        "ElideNone: the text runs past its 70 px, ink in columns {:?}",
        ink[3]
    );
    // The same characters at the same places; the faint column at a glyph's edge is the
    // rasterizer's (Qt's "t" ends one column later than ours).
    let near = |a: usize, b: usize| a.abs_diff(b) <= 1;
    assert!(
        ink.iter()
            .zip(QT_INK)
            .all(|(i, q)| near(i.0, q.0) && near(i.1, q.1)),
        "ink in columns {ink:?}, Qt's in {QT_INK:?}"
    );
}

macro_rules! both_paths {
    ($($name:ident),*) => {$(
        mod $name {
            #[test]
            fn cpu() {
                super::$name(false);
            }
            #[test]
            fn gpu() {
                super::$name(true);
            }
        }
    )*};
}
both_paths!(parent_opacity, clip, corner_radii, elide);

struct Clicks {
    rt: Runtime,
    root: usize,
    ptr: Pointer,
}

impl Clicks {
    fn new() -> Self {
        let (mut rt, root, size) = load("Input.qml");
        let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
        scene::layout_window(&mut rt, &mut fonts.borrow_mut(), root, size);
        let mut ptr = Pointer::default();
        ptr.fonts = fonts;
        ptr.enter(&mut rt, root, 1.0, 1.0);
        Clicks { rt, root, ptr }
    }

    fn click(&mut self, x: f64, y: f64) {
        self.ptr.motion(&mut self.rt, self.root, x, y);
        self.ptr.press(&mut self.rt, BTN_LEFT);
        self.ptr.release(&mut self.rt, BTN_LEFT);
        self.rt.settle();
    }

    fn item(&mut self, name: &str) -> usize {
        match self.rt.get(self.root, name) {
            Value::Item(i) => i,
            v => panic!("{name} is {v:?}"),
        }
    }

    /// What the scene logged since the last call.
    fn log(&mut self) -> String {
        let log = self.rt.get(self.root, "log").to_js_string();
        self.rt.set(self.root, "log", Value::Str("".into()));
        let errors = self.rt.errors();
        assert!(errors.is_empty(), "{errors:?}");
        log
    }
}

#[test]
fn click_outside_the_parents_clip_misses_the_child() {
    let mut s = Clicks::new();
    s.click(30.0, 30.0);
    assert_eq!(s.log(), "clipped ", "inside the clip the area is clicked");
    s.click(80.0, 30.0);
    assert_eq!(
        s.log(),
        "back ",
        "the part of the area outside its parent's clip takes no click"
    );
    s.click(80.0, 75.0);
    assert_eq!(
        s.log(),
        "unclipped ",
        "without `clip` the area is clicked past its parent"
    );
}

#[test]
fn disabled_parent_disables_its_children() {
    let mut s = Clicks::new();
    let (gate, inner, plain) = (s.item("gate"), s.item("inner"), s.item("plain"));
    // QQuickItem::isEnabled is false under a disabled parent; MouseArea.enabled is a property
    // of its own, which stays as set.
    assert_eq!(
        [plain, inner].map(|i| s.rt.get(i, "enabled").truthy()),
        [false, true],
        "`enabled` of an item and of an area under a disabled parent"
    );
    s.click(170.0, 30.0);
    assert_eq!(
        s.log(),
        "back ",
        "an area under a disabled parent takes no click"
    );
    s.rt.set(gate, "enabled", Value::Bool(true));
    s.rt.settle();
    assert_eq!(
        [plain, inner].map(|i| s.rt.get(i, "enabled").truthy()),
        [true, true]
    );
    s.click(170.0, 30.0);
    assert_eq!(s.log(), "inner ", "enabled again, it is clicked");
}

#[test]
fn disabled_mouse_area_leaves_its_children_enabled() {
    let mut s = Clicks::new();
    let under = s.item("under");
    assert!(
        s.rt.get(under, "enabled").truthy(),
        "`enabled` of an item in a disabled MouseArea"
    );
    s.click(160.0, 75.0);
    assert_eq!(s.log(), "under ", "an area in a disabled area is clicked");
    s.click(180.0, 75.0);
    assert_eq!(s.log(), "back ", "the disabled area itself takes no click");
}

#[test]
fn disabled_connections_runs_no_handler() {
    let mut s = Clicks::new();
    let conn = s.item("conn");
    let ping = |s: &mut Clicks| {
        s.rt.emit(s.root, "ping", &[], Vec::new());
        s.rt.settle();
        s.log()
    };
    assert_eq!(ping(&mut s), "on ", "`enabled: false` runs no handler");
    s.rt.set(conn, "enabled", Value::Bool(true));
    assert_eq!(
        ping(&mut s),
        "on off ",
        "enabled later, it is connected later and runs after the other"
    );
    s.rt.set(conn, "enabled", Value::Bool(false));
    assert_eq!(ping(&mut s), "on ", "disabled again, it stops");
}
