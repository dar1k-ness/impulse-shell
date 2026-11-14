//! The quick toggles of the right sidebar in edit mode (the pencil), with the pointer: a right
//! click changes a toggle's size, a click on an unused one adds it, on a used one removes it,
//! the wheel moves it. After each the config holds what was asked for, and every button on
//! screen is the toggle of its own type, as wide as its size. What a change moves is drawn on its
//! way over the frames that follow, and the unused toggles come in after they were made.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, scene};
use std::path::{Path, PathBuf};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

const MAIN: &str = r#"import QtQuick
import ri.modules.common
import ri.modules.rusty.sidebarRight.quickToggles
Item {
    width: 460
    height: 1000
    property alias panel: panel
    property bool ready: Config.ready
    function used() { return JSON.stringify(Config.options.sidebar.quickToggles.android.toggles) }
    function typeOf(b) { return b.buttonData.type }
    function edit(on) { panel.editMode = on }
    function panelHeight() { return panel.height }
    AndroidQuickPanel { id: panel; width: 440; editMode: true }
}
"#;

const SIZE: [f64; 2] = [460.0, 1000.0];

/// Frames after a click: two seconds of the frame clock, past every animation of the panel.
const FRAMES: usize = 125;

fn tree(tag: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("quick-toggle-edit-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("zzmain.qml"), MAIN).unwrap();
    d
}

/// The shell's main loop over one window on a virtual frame clock: each frame runs the
/// runtime's timers, lays the window out and then takes the events that have come (the waker's
/// pings, the epoll set). The panel's services start children that live as long as the shell
/// (`workpool::wait_idle` would wait for them); the test waits for the one thing it needs of
/// them, the config that was read, and what it checks then depends on no other.
struct T {
    rt: Runtime,
    main: usize,
    ptr: Pointer,
    fonts: std::rc::Rc<std::cell::RefCell<scene::Fonts>>,
    wake: testenv::Wakeups,
    ep: Option<i32>,
    now: f64,
}

impl T {
    fn new(compiled: bool) -> T {
        let d = tree(&format!("{compiled}"));
        let mut rt = Runtime::new(Some(d.clone()));
        // As in the shell: a Behavior of an item does not animate before its first layout.
        rt.world.borrow_mut().track_fresh = true;
        // Before anything loads: a service that had work before there was a ping does not
        // ping for more.
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        if compiled {
            let root = PathBuf::from(ricompiled::ROOT);
            rt.eng()
                .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&root))));
        }
        rt.load_singletons(&[
            d.clone(),
            d.join("modules/common"),
            d.join("modules/common/functions"),
            d.join("services"),
        ]);
        rt.settle();
        let main = rt.load_file(&d.join("zzmain.qml")).unwrap();
        rt.settle();
        let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
        let mut ptr = Pointer::default();
        ptr.fonts = fonts.clone();
        let ep = rt.process_fd();
        let mut t = T {
            rt,
            main,
            ptr,
            fonts,
            wake,
            ep,
            now: 1000.0,
        };
        t.until_ready();
        t.frames(FRAMES);
        t.ptr.enter(&mut t.rt, main, 1.0, 999.0);
        t
    }
    fn frame(&mut self) {
        self.now += 16.0;
        self.rt.tick(self.now);
        self.rt.settle();
        let fonts = self.fonts.clone();
        scene::layout_window(&mut self.rt, &mut fonts.borrow_mut(), self.main, SIZE);
        self.rt.settle();
        loop {
            let woke = self.wake.wait(self.ep, Some(std::time::Duration::ZERO));
            if !woke.any() {
                return;
            }
            self.turn(woke);
        }
    }
    fn turn(&mut self, woke: testenv::Woke) {
        if woke.fd {
            self.rt.process_fd_ready();
        }
        self.rt.poll_native();
        self.rt.settle();
    }
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }
    /// Runs frames until Config has read its file, between two frames sleeping as the shell
    /// does: until an event or the runtime's next deadline.
    fn until_ready(&mut self) {
        loop {
            self.frame();
            let ready = self.rt.world.borrow_mut().read(self.main, "ready");
            if matches!(ready, Some(Value::Bool(true))) {
                return;
            }
            let next = self.rt.next_deadline(self.now);
            let slept = std::time::Instant::now();
            let woke = self.wake.wait(
                self.ep,
                next.is_finite()
                    .then(|| std::time::Duration::from_secs_f64(next / 1000.0)),
            );
            self.now += slept.elapsed().as_secs_f64() * 1000.0;
            if woke.any() {
                self.turn(woke);
            }
        }
    }
    /// Every toggle button on screen, in reading order.
    fn buttons(&mut self) -> Vec<Button> {
        let ids: Vec<(usize, String)> = {
            let w = self.rt.world.borrow_mut();
            (0..w.items.len())
                .filter(|i| !w.dead.contains(i))
                .filter(|i| (w.items[*i].types.iter()).any(|t| t == "AndroidQuickToggleButton"))
                .map(|i| (i, w.items[i].types[0].to_string()))
                .collect()
        };
        let mut out = vec![];
        for (i, component) in ids {
            let ty = (self
                .rt
                .call_method(self.main, "typeOf", vec![Value::Item(i)]))
            .unwrap()
            .unwrap()
            .to_js_string();
            let mut w = self.rt.world.borrow_mut();
            let num = |w: &mut qml::runtime::World, p: &str| match w.read(i, p) {
                Some(Value::Num(n)) => n,
                _ => f64::NAN,
            };
            let (x, y) = w.scene_pos(i, Some(self.main));
            out.push(Button {
                ty,
                component,
                size: num(&mut w, "cellSize"),
                x,
                y,
                width: num(&mut w, "width"),
                drawn: (x + num(&mut w, "slideX"), y + num(&mut w, "slideY")),
            });
        }
        out.sort_by(|a, b| (a.y, a.x).partial_cmp(&(b.y, b.x)).unwrap());
        out
    }
    /// The toggles of the config, as (type, size).
    fn used(&mut self) -> Vec<(String, f64)> {
        let used = (self.rt.call_method(self.main, "used", vec![]))
            .unwrap()
            .unwrap()
            .to_js_string();
        let used: serde_json::Value = serde_json::from_str(&used).unwrap();
        (used.as_array().unwrap().iter())
            .map(|t| {
                (
                    t["type"].as_str().unwrap().to_string(),
                    t["size"].as_f64().unwrap(),
                )
            })
            .collect()
    }
    fn at(&mut self, ty: &str) {
        let b = (self.buttons().into_iter())
            .find(|b| b.ty == ty)
            .unwrap_or_else(|| panic!("no {ty}"));
        self.ptr
            .motion(&mut self.rt, self.main, b.x + 10.0, b.y + 10.0);
        self.frames(6);
    }
    fn click(&mut self, ty: &str, button: u32) {
        self.at(ty);
        self.ptr.press(&mut self.rt, button);
        self.frames(3);
        self.ptr.release(&mut self.rt, button);
        self.frames(FRAMES);
    }
    /// A click on the toggle `ty`, and where every toggle is drawn before it and at each of
    /// the frames after it.
    fn click_watched(&mut self, ty: &str, button: u32) -> Vec<Drawn> {
        self.at(ty);
        let mut seen = vec![self.drawn()];
        self.ptr.press(&mut self.rt, button);
        self.frames(3);
        self.ptr.release(&mut self.rt, button);
        for _ in 0..FRAMES {
            self.frame();
            seen.push(self.drawn());
        }
        seen
    }
    fn drawn(&mut self) -> Drawn {
        (self.buttons().into_iter())
            .map(|b| (b.ty, (b.drawn.0, b.drawn.1, b.width)))
            .collect()
    }
    fn edit(&mut self, on: bool) {
        let done = self
            .rt
            .call_method(self.main, "edit", vec![Value::Bool(on)]);
        assert!(matches!(done, Some(Ok(_))), "edit: {done:?}");
    }
    fn panel_height(&mut self) -> f64 {
        match self.rt.call_method(self.main, "panelHeight", vec![]) {
            Some(Ok(Value::Num(h))) => h,
            other => panic!("panelHeight: {other:?}"),
        }
    }
    /// One notch of the wheel over the toggle `ty`, towards the user.
    fn wheel_down(&mut self, ty: &str) {
        self.at(ty);
        let now = self.now;
        (self.ptr).wheel(&mut self.rt, (0.0, -120.0), (0.0, 0.0), (now, now));
        self.frames(FRAMES);
    }
    /// The config holds `want`, and the buttons are these toggles in this order, then the
    /// unused ones in the panel's order, each the component of its type and as wide as its
    /// size (a row's buttons share what is left of its width, so a toggle of two cells is
    /// wider than any of one cell).
    fn check(&mut self, what: &str, want: &[(&str, f64)]) {
        let used = self.used();
        let want: Vec<(String, f64)> = want.iter().map(|(t, s)| (t.to_string(), *s)).collect();
        assert_eq!(used, want, "{what}: config");
        let choices = choices();
        let buttons = self.buttons();
        let unused = choices
            .iter()
            .filter(|(t, _)| !want.iter().any(|(u, _)| u == t));
        let all: Vec<(String, f64)> = (want.iter().cloned())
            .chain(unused.map(|(t, _)| (t.clone(), 1.0)))
            .collect();
        let shown: Vec<(String, f64)> = buttons.iter().map(|b| (b.ty.clone(), b.size)).collect();
        assert_eq!(shown, all, "{what}: buttons");
        for b in &buttons {
            let component = &choices.iter().find(|(t, _)| *t == b.ty).unwrap().1;
            assert_eq!(&b.component, component, "{what}: the button of {}", b.ty);
        }
        let width = |size: f64| {
            let of = buttons.iter().filter(|b| b.size == size).map(|b| b.width);
            (of.clone().fold(f64::MAX, f64::min), of.fold(0.0, f64::max))
        };
        let (one, two) = (width(1.0), width(2.0));
        assert!(
            one.1 < two.0,
            "{what}: widths of one cell {one:?}, of two {two:?}"
        );
        assert!(
            self.rt.errors().is_empty(),
            "{what}: {:?}",
            self.rt.errors()
        );
    }
}

struct Button {
    ty: String,
    /// The type of its delegate: the `DelegateChoice` it was made from.
    component: String,
    size: f64,
    x: f64,
    y: f64,
    width: f64,
    /// Where it is drawn: its place and the offset it slides by.
    drawn: (f64, f64),
}

/// Where each toggle is drawn and how wide, by type.
type Drawn = std::collections::BTreeMap<String, (f64, f64, f64)>;

/// Every toggle that `seen` ends elsewhere, or of another width, than it began is drawn on
/// the way there at frames in between, and no frame takes it more than half of the way.
fn gradual(what: &str, seen: &[Drawn]) {
    let (first, last) = (&seen[0], seen.last().unwrap());
    let far = |a: (f64, f64, f64), b: (f64, f64, f64)| {
        (a.0 - b.0).hypot(a.1 - b.1).max((a.2 - b.2).abs())
    };
    let mut moved = 0;
    for (ty, &to) in last {
        let from = first[ty];
        let way = far(from, to);
        if way < 2.0 {
            continue;
        }
        moved += 1;
        let at: Vec<(f64, f64, f64)> = seen.iter().filter_map(|d| d.get(ty).copied()).collect();
        let between = (at.iter())
            .filter(|p| far(**p, from) > 1.0 && far(**p, to) > 1.0)
            .count();
        assert!(
            between >= 8,
            "{what}: {ty} is drawn between {from:?} and {to:?} at {between} frames: {at:?}"
        );
        let step = at.windows(2).map(|w| far(w[0], w[1])).fold(0.0, f64::max);
        assert!(
            step <= way / 2.0 + 1.0,
            "{what}: {ty} goes {step} of {way} in one frame: {at:?}"
        );
    }
    assert!(moved > 0, "{what}: nothing moved");
}

/// (type, component) of each toggle, in the order of the panel's `availableToggleTypes`: the
/// `DelegateChoice`s of `AndroidToggleDelegateChooser.qml`.
fn choices() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml/modules/rusty/sidebarRight");
    let read = |f: &str| std::fs::read_to_string(dir.join("quickToggles").join(f)).unwrap();
    let chooser = read("androidStyle/AndroidToggleDelegateChooser.qml");
    let of: Vec<(String, String)> = (chooser.lines())
        .filter_map(|l| {
            let l = l.trim().strip_prefix("DelegateChoice { roleValue: \"")?;
            let (ty, rest) = l.split_once("\"; ")?;
            Some((ty.to_string(), rest.strip_suffix(" {")?.to_string()))
        })
        .collect();
    let panel = read("AndroidQuickPanel.qml");
    let types = (panel.lines())
        .find_map(|l| {
            l.trim()
                .strip_prefix("readonly property list<string> availableToggleTypes: [")
        })
        .unwrap();
    let types: Vec<&str> = (types.trim_end_matches(']').split(','))
        .map(|t| t.trim().trim_matches('"'))
        .collect();
    assert_eq!(types.len(), of.len());
    (types.iter())
        .map(|t| of.iter().find(|(ty, _)| ty == t).unwrap().clone())
        .collect()
}

fn own(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::create_dir_all(d.join("config")).unwrap();
    std::fs::create_dir_all(d.join("state")).unwrap();
    testenv::run_in_own_process(
        test,
        [
            ("XDG_CONFIG_HOME", d.join("config")),
            ("XDG_STATE_HOME", d.join("state")),
        ],
    );
    false
}

fn edit(compiled: bool) {
    let mut t = T::new(compiled);
    let mut want = vec![
        ("network", 2.0),
        ("bluetooth", 2.0),
        ("idleInhibitor", 1.0),
        ("mic", 1.0),
        ("audio", 2.0),
        ("nightLight", 2.0),
    ];
    t.check("the default toggles", &want);
    t.click("network", BTN_RIGHT);
    want[0].1 = 1.0;
    t.check("a right click on Internet", &want);
    t.click("darkMode", BTN_LEFT);
    want.push(("darkMode", 1.0));
    t.check("a click on the unused Dark Mode", &want);
    t.click("gameMode", BTN_LEFT);
    want.push(("gameMode", 1.0));
    t.check("a click on the unused Game mode", &want);
    t.click("bluetooth", BTN_LEFT);
    want.remove(1);
    t.check("a click on the used Bluetooth", &want);
    t.click("darkMode", BTN_RIGHT);
    want[5].1 = 2.0;
    t.check("a right click on Dark Mode", &want);
    t.wheel_down("idleInhibitor");
    want.swap(1, 2);
    t.check("the wheel over Keep awake", &want);
    t.click("bluetooth", BTN_LEFT);
    want.push(("bluetooth", 1.0));
    t.check("a click on the unused Bluetooth", &want);
}

/// The pencil and the changes made with it move what they move over frames, not at once.
fn motion(compiled: bool) {
    let mut t = T::new(compiled);
    let all = t.buttons().len();
    let open = t.panel_height();

    // Out of edit mode and back. Making the unused toggles takes the shell longer than a
    // frame, here 300 ms: the panel begins to grow at the frame after the one that made
    // them, not while they were being made.
    t.edit(false);
    t.frames(FRAMES);
    let shut = t.panel_height();
    assert!(
        shut < open - 50.0,
        "the panel out of edit mode: {shut}, in it {open}"
    );
    assert!(t.buttons().len() < all);
    t.edit(true);
    t.now += 300.0;
    t.frame();
    assert_eq!(t.buttons().len(), all, "the unused toggles after a frame");
    let mut heights = vec![t.panel_height()];
    for _ in 0..FRAMES {
        t.frame();
        heights.push(t.panel_height());
    }
    assert_eq!(heights[0], shut, "the panel at the frame that made them");
    assert_eq!(*heights.last().unwrap(), open);
    let between = (heights.iter())
        .filter(|h| **h > shut + 1.0 && **h < open - 1.0)
        .count();
    let step = (heights.windows(2).map(|w| (w[1] - w[0]).abs())).fold(0.0, f64::max);
    assert!(
        between >= 8 && step <= (open - shut) / 2.0,
        "the panel grows over {between} frames, {step} at one: {heights:?}"
    );

    // With the default toggles: Dark Mode goes up among the used ones and the unused ones
    // after it close the gap; Bluetooth goes down and the used ones after it move up a row;
    // a toggle made wider pushes the next one to the row below.
    gradual(
        "a click on the unused Dark Mode",
        &t.click_watched("darkMode", BTN_LEFT),
    );
    gradual(
        "a click on the used Bluetooth",
        &t.click_watched("bluetooth", BTN_LEFT),
    );
    gradual(
        "a right click on Dark Mode",
        &t.click_watched("darkMode", BTN_RIGHT),
    );
    gradual(
        "a right click on Internet",
        &t.click_watched("network", BTN_RIGHT),
    );
    assert!(t.rt.errors().is_empty(), "{:?}", t.rt.errors());
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn motion_interp() {
    if own(concat!(module_path!(), "::motion_interp")) {
        motion(false);
    }
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn motion_compiled() {
    if own(concat!(module_path!(), "::motion_compiled")) {
        motion(true);
    }
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn edit_interp() {
    if own(concat!(module_path!(), "::edit_interp")) {
        edit(false);
    }
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn edit_compiled() {
    if own(concat!(module_path!(), "::edit_compiled")) {
        edit(true);
    }
}
