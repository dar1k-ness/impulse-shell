//! `Item.transform` while the scene lives, as Qt 6.12: a Translate that a binding, a Behavior
//! or an animation moves takes its item along in what is drawn and in `mapToItem`; hover
//! areas inside turned, scaled and clipped items are entered where they show; an area that a
//! Translate slides under a still pointer becomes hovered; a drag inside a turned item moves
//! its target along the item's own axis. The steps of `transform_live/tst_transform_live.qml`,
//! which checks the same values in Qt itself (qmltestrunner in the tools/testenv image; the
//! command is in that file).

use qml::runtime::Runtime;
use qml::value::Value;
use qsg::raster::Canvas;
use shell::input::Pointer;
use shell::scene::{self, Target};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

const BTN_LEFT: u32 = 0x110;

struct Live {
    rt: Runtime,
    root: usize,
    size: [f64; 2],
    fonts: Rc<RefCell<scene::Fonts>>,
    gpu: Option<shell::gpu::Gpu>,
    ptr: Pointer,
    now: f64,
}

impl Live {
    fn new(gpu: bool) -> Self {
        Self::open("Live.qml", gpu)
    }

    fn open(file: &str, gpu: bool) -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/transform_live");
        let mut rt = Runtime::new(Some(dir.clone()));
        let root = rt.load_file(&dir.join(file)).expect("scene loads");
        rt.settle();
        let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number());
        let fonts = Rc::new(RefCell::new(scene::Fonts::default()));
        let gpu = gpu.then(|| {
            shell::gpu::Gpu::headless("surfaceless")
                .expect("surfaceless EGL/GLES2 context (Mesa)")
                .0
        });
        let mut ptr = Pointer::default();
        ptr.fonts = fonts.clone();
        let mut s = Live {
            rt,
            root,
            size,
            fonts,
            gpu,
            ptr,
            now: 0.0,
        };
        s.frame();
        s
    }

    /// One frame, 16 ms on: animations step, the scene is laid out and hover is delivered
    /// again where the pointer rests.
    fn frame(&mut self) {
        self.now += 16.0;
        self.rt.tick(self.now);
        self.rt.settle();
        scene::layout_window(
            &mut self.rt,
            &mut self.fonts.borrow_mut(),
            self.root,
            self.size,
        );
        self.rt.settle();
        // Qt's interval of frame-synchronous hover at 0: every frame delivers it.
        self.ptr.hover_every = Some(0);
        self.ptr.frame(self.root, std::time::Instant::now());
        self.ptr.rehover(&mut self.rt);
        self.rt.settle();
        let errors = self.rt.errors();
        assert!(errors.is_empty(), "{errors:?}");
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    fn at(&mut self, pts: &[(usize, usize)]) -> String {
        let (w, h) = (self.size[0] as usize, self.size[1] as usize);
        let c = match &mut self.gpu {
            None => {
                let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
                scene::paint_window(
                    &mut self.rt,
                    &mut self.fonts.borrow_mut(),
                    self.root,
                    &mut Target::Cpu(&mut c),
                );
                c
            }
            Some(g) => {
                let mut fr = shell::gpu::Frame::new(w, h);
                scene::paint_window(
                    &mut self.rt,
                    &mut self.fonts.borrow_mut(),
                    self.root,
                    &mut Target::Gpu(&mut fr),
                );
                let mut s = g.pbuffer(w as i32, h as i32).expect("pbuffer");
                s.reswap();
                g.render(&mut s, &mut fr, || {});
                g.read_surface(&s)
            }
        };
        pts.iter()
            .map(|&(x, y)| {
                let p = c.px[y * w + x];
                format!("{},{},{}", p[0], p[1], p[2])
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn where_(&mut self) -> String {
        let v = self.rt.call_method(self.root, "where", Vec::new());
        v.unwrap().unwrap().to_js_string()
    }

    fn num(&self, alias: &str, prop: &str) -> f64 {
        let Value::Item(o) = self.rt.get(self.root, alias) else {
            panic!("{alias} is an object")
        };
        self.rt.get(o, prop).to_number()
    }

    fn set(&mut self, alias: &str, prop: &str, v: f64) {
        let Value::Item(o) = self.rt.get(self.root, alias) else {
            panic!("{alias} is an object")
        };
        self.rt.set(o, prop, Value::Num(v));
        self.rt.settle();
    }

    fn call(&mut self, function: &str) {
        let r = self.rt.call_method(self.root, function, Vec::new());
        r.unwrap().unwrap();
        self.rt.settle();
    }

    fn took(&mut self) -> String {
        self.rt.settle();
        let log = self.rt.get(self.root, "log").to_js_string();
        self.rt.set(self.root, "log", Value::Str("".into()));
        log.trim().to_string()
    }

    /// A wheel notch (`angle` is Qt's angleDelta.y) with the pointer at (`x`, `y`).
    fn wheel(&mut self, x: f64, y: f64, angle: f64) -> String {
        self.ptr.motion(&mut self.rt, self.root, x, y);
        self.took();
        self.now += 16.0;
        let t = (self.now, self.now);
        self.ptr.wheel(&mut self.rt, (0.0, angle), (0.0, 0.0), t);
        self.took()
    }

    fn xy(&self, alias: &str) -> String {
        let r = |v: f64| (v * 10.0).round() / 10.0;
        format!("{},{}", r(self.num(alias, "x")), r(self.num(alias, "y")))
    }

    fn moved(&mut self, x: f64, y: f64) -> String {
        self.ptr.motion(&mut self.rt, self.root, x, y);
        self.took()
    }

    /// Presses at the first point, moves through the others and lets go at the last: where
    /// `alias` is after each move and after the release. By the mouse, or by a finger.
    fn drag(&mut self, alias: &str, pts: &[(f64, f64)], finger: bool) -> String {
        let (x, y) = pts[0];
        if finger {
            self.ptr.touch_down(&mut self.rt, self.root, 0, x, y);
        } else {
            self.ptr.enter(&mut self.rt, self.root, x, y);
            self.ptr.motion(&mut self.rt, self.root, x, y);
            self.ptr.press(&mut self.rt, BTN_LEFT);
        }
        self.rt.settle();
        let mut out = Vec::new();
        for &(x, y) in &pts[1..] {
            if finger {
                self.ptr.touch_motion(&mut self.rt, 0, x, y);
            } else {
                self.ptr.motion(&mut self.rt, self.root, x, y);
            }
            self.rt.settle();
            out.push(self.xy(alias));
        }
        if finger {
            self.ptr.touch_up(&mut self.rt, 0);
        } else {
            self.ptr.release(&mut self.rt, BTN_LEFT);
        }
        self.rt.settle();
        out.push(self.xy(alias));
        out.join(" ")
    }
}

/// test_1_binding_and_behavior and test_3_animation of tst_transform_live.qml.
fn moves(gpu: bool) {
    let mut s = Live::new(gpu);
    assert_eq!(s.where_(), "10,10 10,50 10,90 10,130");
    assert_eq!(s.at(&[(15, 15), (55, 25), (65, 35)]), "255,0,0 0,0,0 0,0,0");
    s.rt.set(s.root, "p", Value::Num(20.0));
    s.rt.settle();
    assert_eq!(
        s.where_(),
        "50,20 10,50 10,90 10,130",
        "the binding moves its item at once; the Behavior has not moved its own yet"
    );
    assert_eq!(
        s.at(&[(15, 15), (55, 25), (65, 35)]),
        "0,0,0 255,0,0 255,0,0",
        "the bound Translate is drawn where it moved"
    );

    // The Behavior: on its way the item is where the Translate says, and drawn there.
    s.frames(6);
    let x = s.num("eased", "x");
    assert!(
        x > 2.0 && x < 18.0,
        "the Behavior is on its way after 96 ms: {x}"
    );
    let w = s.where_();
    let ex: f64 = w
        .split(' ')
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!((ex - (10.0 + x)).abs() < 0.11, "mid way: {w} for {x}");
    let px = (10.0 + x) as usize;
    assert_eq!(
        s.at(&[(px - 2, 60), (px + 2, 60), (px + 18, 60), (px + 23, 60)]),
        "0,0,0 0,255,0 0,255,0 0,0,0",
        "mid way the item is drawn at {x}"
    );
    s.frames(24);
    assert_eq!((s.num("eased", "x"), s.num("eased", "y")), (20.0, 5.0));
    assert_eq!(s.where_(), "50,20 30,55 10,90 10,130");
    assert_eq!(
        s.at(&[(15, 55), (35, 60), (45, 70), (55, 60)]),
        "0,0,0 0,255,0 0,255,0 0,0,0",
        "the Behavior ended"
    );

    s.call("startRun");
    s.frames(6);
    let w = s.where_();
    let rx: f64 = w
        .split(' ')
        .nth(2)
        .unwrap()
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(rx > 15.0 && rx < 65.0, "the animation is on its way: {w}");
    let px = rx as usize;
    assert_eq!(
        s.at(&[
            (px - 2, rx as usize + 90),
            (px + 2, rx as usize + 90),
            (px + 38, rx as usize + 90),
            (px + 43, rx as usize + 90)
        ]),
        "0,0,0 0,0,255 0,0,255 0,0,0",
        "mid way the animated item is drawn at {rx}"
    );
    s.frames(14);
    s.call("startLoop");
    s.frames(20);
    assert_eq!(s.where_(), "50,20 30,55 70,150 10,170");
    assert_eq!(
        s.at(&[
            (15, 95),
            (75, 155),
            (105, 165),
            (115, 160),
            (15, 135),
            (15, 175),
            (25, 185)
        ]),
        "0,0,0 0,0,255 0,0,255 0,0,0 0,0,0 255,255,0 255,255,0",
        "both animations ended"
    );
}

#[test]
fn a_translate_that_changes_moves_its_item_cpu() {
    moves(false);
}

#[test]
fn a_translate_that_changes_moves_its_item_gpu() {
    moves(true);
}

/// test_4_hover.
#[test]
fn hover_enters_areas_where_they_show() {
    let mut s = Live::new(false);
    s.ptr.enter(&mut s.rt, s.root, 290.0, 5.0);
    s.took();
    assert_eq!(s.moved(130.0, 30.0), "turned-in turned 10,15");
    assert_eq!(s.moved(140.0, 55.0), "turned 35,5");
    assert_eq!(s.moved(150.0, 55.0), "turned-out");
    assert_eq!(s.moved(230.0, 115.0), "scaled-in scaled 5,10");
    assert_eq!(s.moved(275.0, 125.0), "scaled 27.5,30");
    assert_eq!(s.moved(285.0, 125.0), "scaled-out");
    assert_eq!(
        s.moved(210.0, 125.0),
        "",
        "the clip cuts the area's left part"
    );
}

/// test_5_hover_follows_a_moving_item.
#[test]
fn an_area_slid_under_a_still_pointer_is_hovered() {
    let mut s = Live::new(false);
    s.ptr.enter(&mut s.rt, s.root, 290.0, 5.0);
    assert_eq!(s.moved(150.0, 165.0), "");
    s.set("slide", "x", -60.0);
    s.frame();
    assert_eq!(s.took(), "slid true");
    s.set("slide", "x", 0.0);
    s.frame();
    assert_eq!(s.took(), "slid false");
}

/// test_6_drag.
#[test]
fn a_drag_in_a_turned_item_goes_along_its_own_axis() {
    let mut s = Live::new(false);
    s.ptr.enter(&mut s.rt, s.root, 290.0, 5.0);
    s.moved(110.0, 30.0);
    s.ptr.press(&mut s.rt, BTN_LEFT);
    s.moved(110.0, 40.0);
    assert_eq!(
        s.num("knob", "x"),
        0.0,
        "the drag starts with the first move"
    );
    s.moved(115.0, 62.0);
    assert_eq!(s.num("knob", "x"), 22.0, "22 down the window is 22 along x");
    s.moved(115.0, 150.0);
    assert_eq!(s.num("knob", "x"), 80.0, "stops at drag.maximumX");
    s.ptr.release(&mut s.rt, BTN_LEFT);
    s.rt.settle();
    assert_eq!(s.num("knob", "x"), 80.0);
}

/// test_1_drag_handler_moves_its_target of tst_transform_handlers.qml.
#[test]
fn a_drag_handler_in_a_turned_item_moves_its_target_along_the_items_axes() {
    let mut s = Live::open("Handlers.qml", false);
    s.ptr.enter(&mut s.rt, s.root, 290.0, 5.0);
    s.moved(30.0, 30.0);
    s.ptr.press(&mut s.rt, BTN_LEFT);
    s.moved(30.0, 36.0);
    assert_eq!(s.xy("card"), "0,0", "under the drag threshold");
    s.moved(30.0, 52.0);
    assert_eq!(s.xy("card"), "22,0", "22 down the window is 22 along x");
    s.moved(35.0, 62.0);
    assert_eq!(
        s.xy("card"),
        "32,-5",
        "5 right in the window is 5 against y"
    );
    s.moved(25.0, 72.0);
    assert_eq!(s.xy("card"), "42,5");
    s.moved(10.0, 150.0);
    assert_eq!(s.xy("card"), "70,8", "stops at both maximums");
    s.ptr.release(&mut s.rt, BTN_LEFT);
    s.rt.settle();
    assert_eq!(s.xy("card"), "70,8");
}

/// test_2_drag_handler_without_a_target.
#[test]
fn a_drag_handler_without_a_target_in_a_scaled_item_reports_the_travel_in_the_window() {
    let mut s = Live::open("Handlers.qml", false);
    s.ptr.enter(&mut s.rt, s.root, 290.0, 5.0);
    s.moved(130.0, 40.0);
    s.ptr.press(&mut s.rt, BTN_LEFT);
    assert_eq!(s.took(), "");
    assert_eq!(s.moved(130.0, 46.0), "");
    assert_eq!(s.moved(150.0, 46.0), "free true");
    assert_eq!(s.moved(160.0, 70.0), "x 30 y 30");
    s.ptr.release(&mut s.rt, BTN_LEFT);
    assert_eq!(s.took(), "free false");
}

/// test_3_wheel_handler.
#[test]
fn a_wheel_handler_takes_the_wheel_where_its_item_shows() {
    let mut s = Live::open("Handlers.qml", false);
    s.ptr.enter(&mut s.rt, s.root, 290.0, 5.0);
    assert_eq!(s.wheel(245.0, 150.0, -120.0), "wheel -120 at 45,15");
    assert_eq!(s.wheel(280.0, 75.0, 120.0), "", "where it is laid out");
    assert_eq!(s.wheel(235.0, 62.0, 120.0), "wheel 120 at 1,25");
}

/// test_4_drag_handler_moves_a_scaled_target.
#[test]
fn a_drag_handler_moves_a_scaled_target_as_far_as_the_pointer_goes() {
    let mut s = Live::open("Handlers.qml", false);
    s.ptr.enter(&mut s.rt, s.root, 290.0, 5.0);
    s.moved(230.0, 25.0);
    s.ptr.press(&mut s.rt, BTN_LEFT);
    s.moved(230.0, 27.0);
    assert_eq!(s.xy("small"), "200,5", "under the drag threshold");
    s.moved(230.0, 31.0);
    assert_eq!(
        s.xy("small"),
        "200,11",
        "6 in the window is 12 in the target"
    );
    s.moved(230.0, 45.0);
    assert_eq!(s.xy("small"), "200,25");
    s.moved(220.0, 55.0);
    assert_eq!(s.xy("small"), "190,35");
    s.ptr.release(&mut s.rt, BTN_LEFT);
    s.rt.settle();
    assert_eq!(s.xy("small"), "190,35");
}

/// test_5_handlers_in_a_pane_a_little_over_its_size.
#[test]
fn handlers_in_a_pane_a_little_over_its_size_work_where_it_shows() {
    let mut s = Live::open("Handlers.qml", false);
    s.ptr.enter(&mut s.rt, s.root, 290.0, 5.0);
    assert_eq!(s.wheel(86.0, 149.0, 120.0), "zoom 120");
    assert_eq!(s.wheel(60.0, 149.0, 120.0), "", "where it is laid out");
    s.moved(86.0, 149.0);
    s.ptr.press(&mut s.rt, BTN_LEFT);
    s.moved(86.0, 159.0);
    assert_eq!(s.xy("widget"), "10,10", "under the drag threshold");
    s.moved(113.0, 149.0);
    assert_eq!(
        s.xy("widget"),
        "35,10",
        "27 in the window is 25 in the pane"
    );
    s.moved(140.0, 160.0);
    assert_eq!(s.xy("widget"), "60,20.2");
    s.moved(290.0, 100.0);
    assert_eq!(s.xy("widget"), "110,0", "kept inside the pane");
    s.moved(10.0, 199.0);
    assert_eq!(s.xy("widget"), "0,40");
    s.ptr.release(&mut s.rt, BTN_LEFT);
    s.rt.settle();
    assert_eq!(s.xy("widget"), "0,40");
}

// ---- Drags.qml: the steps of tst_transform_drags.qml.

#[test]
fn a_drag_handler_moves_a_turned_target_one_for_one() {
    let mut s = Live::open("Drags.qml", false);
    let pts = [
        (40.0, 30.0),
        (40.0, 36.0),
        (40.0, 45.0),
        (50.0, 55.0),
        (30.0, 60.0),
        (100.0, 150.0),
    ];
    assert_eq!(
        s.drag("turned", &pts, false),
        "20,20 20,35 30,45 10,50 80,140 80,140"
    );
}

#[test]
fn a_drag_handler_maps_a_target_with_a_transform_list_as_qt_does() {
    let mut s = Live::open("Drags.qml", false);
    let pts = [
        (85.0, 50.0),
        (85.0, 56.0),
        (85.0, 62.0),
        (85.0, 80.0),
        (70.0, 95.0),
        (150.0, 60.0),
    ];
    assert_eq!(
        s.drag("tilted", &pts, false),
        "100,20 100,20 65,70 50,85 130,50 130,50",
        "12 in the window is 8 in the target; the target jumps when the drag starts"
    );
}

#[test]
fn a_drag_handlers_axis_limits_are_in_the_scaled_items_units() {
    let mut s = Live::open("Drags.qml", false);
    let pts = [
        (30.0, 90.0),
        (40.0, 90.0),
        (52.0, 90.0),
        (72.0, 110.0),
        (200.0, 90.0),
        (10.0, 130.0),
    ];
    assert_eq!(s.drag("boxed", &pts, false), "0,0 11,0 21,0 30,0 2,0 2,0");
}

#[test]
fn a_mouse_areas_drag_axis_limits_and_threshold_are_in_the_scaled_items_units() {
    let mut s = Live::open("Drags.qml", false);
    let pts = [
        (190.0, 30.0),
        (196.0, 30.0),
        (210.0, 30.0),
        (212.0, 30.0),
        (232.0, 50.0),
        (299.0, 30.0),
        (150.0, 30.0),
    ];
    assert_eq!(
        s.drag("slider", &pts, false),
        "10,0 10,0 10,0 20,0 40,0 4,0 4,0",
        "20 in the window is 10 in the item, not over the threshold"
    );
}

#[test]
fn a_mouse_areas_own_drag_threshold_counts_in_the_turned_scaled_item() {
    let mut s = Live::open("Drags.qml", false);
    let pts = [
        (230.0, 120.0),
        (230.0, 121.0),
        (230.0, 122.0),
        (226.0, 122.0),
        (226.0, 116.0),
        (240.0, 100.0),
        (200.0, 140.0),
    ];
    assert_eq!(
        s.drag("puck", &pts, false),
        "20,10 20,10 28,6 28,18 0,20 80,0 80,0"
    );
}

#[test]
fn a_finger_drags_a_drag_handlers_target_in_a_scaled_item() {
    let mut s = Live::open("Drags.qml", false);
    let pts = [
        (30.0, 160.0),
        (36.0, 160.0),
        (48.0, 160.0),
        (60.0, 170.0),
        (90.0, 150.0),
    ];
    assert_eq!(
        s.drag("finger", &pts, true),
        "0,0 12,0 20,6.7 40,-6.7 40,-6.7"
    );
}

#[test]
fn a_finger_drags_a_mouse_areas_target_in_a_scaled_item() {
    let mut s = Live::open("Drags.qml", false);
    let pts = [
        (120.0, 160.0),
        (126.0, 160.0),
        (138.0, 160.0),
        (150.0, 170.0),
        (100.0, 150.0),
    ];
    assert_eq!(
        s.drag("thumb", &pts, true),
        "60,0 60,0 68,6.7 34.7,-6.7 34.7,-6.7"
    );
}
