//! `Item.transform` (Translate, Scale, Rotation, Matrix4x4, a list of several) in what is
//! drawn, what a click lands on and what `clip` cuts, as Qt 6.12 does it: the scenes in
//! `item_transform/` painted and clicked here, with the values
//! `item_transform/tst_item_transform.qml` checks in Qt itself (qmltestrunner in the
//! tools/testenv image; the command is in that file).

use qml::runtime::Runtime;
use qml::value::Value;
use qsg::raster::Canvas;
use shell::input::Pointer;
use shell::scene::{self, Target};
use std::path::Path;

const BTN_LEFT: u32 = 0x110;

fn load(name: &str) -> (Runtime, usize, [f64; 2]) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/item_transform");
    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join(name)).expect("scene loads");
    rt.settle();
    let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
    (rt, root, size)
}

struct Scene {
    rt: Runtime,
    root: usize,
    size: [f64; 2],
    fonts: scene::Fonts,
    gpu: Option<shell::gpu::Gpu>,
}

struct Painted {
    c: Canvas,
    w: usize,
}

impl Painted {
    fn at(&self, pts: &[(usize, usize)]) -> String {
        pts.iter()
            .map(|&(x, y)| {
                let p = self.c.px[y * self.w + x];
                assert_eq!(p[3], 255, "the window is opaque at {x},{y}");
                format!("{},{},{}", p[0], p[1], p[2])
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Scene {
    fn new(gpu: bool) -> Self {
        let (rt, root, size) = load("Paint.qml");
        let gpu = gpu.then(|| {
            shell::gpu::Gpu::headless("surfaceless")
                .expect("surfaceless EGL/GLES2 context (Mesa)")
                .0
        });
        Scene {
            rt,
            root,
            size,
            fonts: scene::Fonts::default(),
            gpu,
        }
    }

    fn paint(&mut self) -> Painted {
        let (w, h) = (self.size[0] as usize, self.size[1] as usize);
        for _ in 0..3 {
            self.rt.settle();
            scene::layout_window(&mut self.rt, &mut self.fonts, self.root, self.size);
        }
        let errors = self.rt.errors();
        assert!(errors.is_empty(), "{errors:?}");
        let Some(g) = &mut self.gpu else {
            let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
            scene::paint_window(
                &mut self.rt,
                &mut self.fonts,
                self.root,
                &mut Target::Cpu(&mut c),
            );
            return Painted { c, w };
        };
        let mut fr = shell::gpu::Frame::new(w, h);
        scene::paint_window(
            &mut self.rt,
            &mut self.fonts,
            self.root,
            &mut Target::Gpu(&mut fr),
        );
        let mut s = g.pbuffer(w as i32, h as i32).expect("pbuffer");
        s.reswap();
        g.render(&mut s, &mut fr, || {});
        Painted {
            c: g.read_surface(&s),
            w,
        }
    }
}

const K: &str = "0,0,0";

fn rep(c: &str, n: usize, k: usize) -> String {
    [vec![c; n], vec![K; k]].concat().join(" ")
}

/// The pixels tst_item_transform.qml's test_paint compares in Qt 6.12.
fn drawn(gpu: bool) {
    let mut s = Scene::new(gpu);
    let p = s.paint();
    assert_eq!(
        p.at(&[(45, 20), (55, 30), (20, 20), (35, 20), (65, 20), (50, 12)]),
        rep("255,0,0", 2, 4),
        "Translate draws the item 30 right and 5 down, and not where it was"
    );
    assert_eq!(
        p.at(&[
            (93, 20),
            (127, 20),
            (110, 12),
            (110, 28),
            (87, 20),
            (133, 20)
        ]),
        rep("0,255,0", 2, 4),
        "Scale about its origin: twice as wide, half as high"
    );
    assert_eq!(
        p.at(&[
            (195, 15),
            (195, 45),
            (220, 15),
            (205, 30),
            (185, 30),
            (195, 55)
        ]),
        rep("0,0,255", 2, 4),
        "Rotation by 90 about the top left corner"
    );
    assert_eq!(
        p.at(&[(313, 13), (347, 47), (305, 20), (355, 20)]),
        rep("255,0,255", 2, 2),
        "the item's own scale applies before its list: the move is not scaled"
    );
    assert_eq!(
        p.at(&[(53, 85), (87, 85), (45, 85), (95, 85)]),
        rep("255,255,0", 2, 2),
        "[Scale, Translate]: scaled, then moved"
    );
    assert_eq!(
        p.at(&[(83, 105), (117, 105), (75, 105), (125, 105)]),
        rep("255,255,0", 2, 2),
        "[Translate, Scale]: moved, then scaled"
    );
    assert_eq!(
        p.at(&[
            (213, 103),
            (247, 117),
            (205, 110),
            (255, 110),
            (230, 95),
            (230, 125)
        ]),
        rep("0,255,255", 2, 4),
        "Matrix4x4"
    );
    assert_eq!(
        p.at(&[(323, 85), (337, 85), (315, 85), (345, 85)]),
        rep("128,128,128", 2, 2),
        "a child's Translate inside its parent's Scale"
    );
    assert_eq!(
        p.at(&[
            (70, 245),
            (50, 245),
            (90, 245),
            (25, 245),
            (115, 245),
            (38, 245),
            (100, 245)
        ]),
        rep("255,128,0", 3, 4),
        "Rotation about the y axis: narrower"
    );
    assert_eq!(
        p.at(&[(70, 215), (70, 275), (46, 224), (92, 222)]),
        "0,0,0 0,0,0 255,128,0 255,128,0",
        "Rotation about the y axis: the near edge is higher than the far one"
    );
}

fn clipped(gpu: bool) {
    let mut s = Scene::new(gpu);
    let p = s.paint();
    assert_eq!(
        p.at(&[
            (73, 153),
            (107, 187),
            (65, 170),
            (115, 170),
            (40, 170),
            (90, 145),
            (90, 195)
        ]),
        rep("255,255,255", 2, 5),
        "`clip` moves with a translated item"
    );
    assert_eq!(
        p.at(&[
            (183, 153),
            (197, 187),
            (175, 170),
            (205, 170),
            (190, 145),
            (190, 195),
            (230, 160)
        ]),
        rep("255,255,255", 2, 5),
        "`clip` turns with a rotated item"
    );
    assert_eq!(
        p.at(&[(333, 153), (337, 167), (345, 160), (325, 160), (335, 175)]),
        rep("255,255,255", 2, 3),
        "the parent's clip cuts a child its Translate moved out"
    );
}

/// A change of a transform draws the item at its new place.
fn redrawn(gpu: bool) {
    let mut s = Scene::new(gpu);
    let p = s.paint();
    assert_eq!(
        p.at(&[(115, 20), (125, 30), (45, 20), (55, 30)]),
        "0,255,0 0,0,0 255,0,0 255,0,0"
    );
    let Value::Item(shift) = s.rt.get(s.root, "shift") else {
        panic!("shift")
    };
    s.rt.set(shift, "x", Value::Num(100.0));
    let p = s.paint();
    assert_eq!(
        p.at(&[(115, 20), (125, 30), (45, 20), (55, 30)]),
        "0,255,0 255,0,0 0,0,0 0,0,0",
        "Translate.x 30 -> 100: the item is drawn 70 further right"
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
both_paths!(drawn, clipped, redrawn);

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

    /// What the scene logs for a click at (`x`, `y`).
    fn click(&mut self, x: f64, y: f64) -> String {
        self.ptr.motion(&mut self.rt, self.root, x, y);
        self.ptr.press(&mut self.rt, BTN_LEFT);
        self.ptr.release(&mut self.rt, BTN_LEFT);
        self.rt.settle();
        let log = self.rt.get(self.root, "log").to_js_string();
        self.rt.set(self.root, "log", Value::Str("".into()));
        let errors = self.rt.errors();
        assert!(errors.is_empty(), "{errors:?}");
        log.trim_end().to_string()
    }

    fn clicks(&mut self, at: &[(f64, f64)]) -> Vec<String> {
        at.iter().map(|&(x, y)| self.click(x, y)).collect()
    }
}

/// The clicks of tst_item_transform.qml's test_input, with what Qt 6.12 logs for each.
#[test]
fn a_click_lands_where_the_item_is_drawn() {
    let mut s = Clicks::new();
    assert_eq!(
        s.clicks(&[(50.0, 20.0), (65.0, 20.0)]),
        ["moved 10,10", "back"],
        "Translate"
    );
    assert_eq!(
        s.clicks(&[(93.0, 20.0), (127.0, 24.0), (110.0, 12.0), (110.0, 28.0)]),
        ["scaled 1.5,10", "scaled 18.5,18", "back", "back"],
        "Scale about its origin"
    );
    assert_eq!(
        s.clicks(&[(195.0, 40.0), (199.0, 11.0), (220.0, 15.0), (195.0, 55.0)]),
        ["turned 30,5", "turned 1,1", "back", "back"],
        "Rotation by 90"
    );
    assert_eq!(
        s.clicks(&[(265.0, 85.0), (231.0, 81.0), (225.0, 85.0), (275.0, 85.0)]),
        ["list 17.5,5", "list 0.5,1", "back", "back"],
        "[Scale, Translate]"
    );
    assert_eq!(
        s.clicks(&[
            (70.0, 145.0),
            (50.0, 145.0),
            (90.0, 145.0),
            (25.0, 145.0),
            (115.0, 145.0)
        ]),
        [
            "tilted 50,25",
            "tilted 11.3,25",
            "tilted 91.4,25",
            "back",
            "back"
        ],
        "Rotation about the y axis"
    );
    assert_eq!(
        s.clicks(&[(46.0, 124.0), (92.0, 122.0)]),
        ["tilted 3.9,4.8", "tilted 95.7,1.1"],
        "Rotation about the y axis: the corners"
    );
}

#[test]
fn a_click_outside_a_moved_clip_misses_the_child() {
    let mut s = Clicks::new();
    assert_eq!(
        s.clicks(&[
            (90.0, 80.0),
            (72.0, 62.0),
            (125.0, 80.0),
            (40.0, 80.0),
            (65.0, 80.0)
        ]),
        ["clipped 20,20", "clipped 2,2", "back", "back", "back"]
    );
}

/// QQuickDeliveryAgent maps a point into an item by the inverse of the item's transform, and
/// by nothing when there is none: an item scaled to nothing takes the clicks on its size at
/// its parent's origin, and reports the scene position.
#[test]
fn an_item_scaled_to_nothing_is_clicked_at_its_parents_origin() {
    let mut s = Clicks::new();
    assert_eq!(
        s.clicks(&[(325.0, 145.0), (300.0, 145.0), (20.0, 20.0), (41.0, 29.0)]),
        ["back", "back", "flat 20,20", "flat 41,29"]
    );
}

/// `ChildAt.qml`: children sized or placed by layout (a Loader by its item, the cells of a
/// Row, a Text by its words, an Item by its implicit size). The answers are the file's
/// `// qt:` line, Qt 6.12's.
#[test]
fn child_at_finds_children_where_layout_put_them() {
    let (mut rt, root, size) = load("ChildAt.qml");
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, root, size);
    }
    rt.set(root, "go", Value::Num(2.0));
    rt.settle();
    let errors = rt.errors();
    assert!(errors.is_empty(), "{errors:?}");
    let src = include_str!("item_transform/ChildAt.qml");
    let qt = src
        .lines()
        .find_map(|l| l.strip_prefix("// qt: "))
        .expect("the Qt line");
    assert_eq!(rt.get(root, "result").to_js_string(), qt);
}
