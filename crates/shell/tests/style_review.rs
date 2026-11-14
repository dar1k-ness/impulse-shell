//! The review of a style the wallpaper picker has put on the screen (qml services/StyleReview.qml
//! and the bar it adds to the edit overlay, editMode/StyleReviewBar.qml): a pick that ends well
//! asks the shell for the review, the review opens the edit mode with like, dislike and confirm
//! at the top of the overlay, and it ends with the edit mode. Without a pick the overlay is the
//! one it always was.
#![cfg(feature = "interp")]

use qml::edit_mode::WinRect;
use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, scene};
use std::path::{Path, PathBuf};

const BTN_LEFT: u32 = 0x110;
const W: f64 = 1280.0;
const H: f64 = 720.0;

const MANIFEST: &str = r#"
[[widget]]
type = "clock"
name = "Clock"
icon = "schedule"
placements = ["panel", "desktop"]
component = "W.qml"
size = { min = [40, 20], max = [800, 400], default = [200, 100] }
"#;

const LAYOUT: &str = r#"version = 5

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desk"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "clock"
x = 100
y = 300
w = 200
h = 100
"#;

// The background surface with the desktop container, a controller that reads the edit mode,
// and what the tests ask of the two services.
const BACKGROUND: &str = r#"import QtQuick
import Impulse
import ri.services
Item {
    id: root
    width: 1280
    height: 720
    property var ctl: controller
    EditController {
        id: controller
        connector: "DP-1"
    }
    DesktopContainer {
        id: desk
        connector: "DP-1"
        width: 1280
        height: 720
        Repeater {
            model: desk.widgets
            WidgetHost {
                required property var modelData
                layoutId: modelData.id
                widgetType: modelData.type
                x: modelData.x
                y: modelData.y
                width: modelData.width
                height: modelData.height
            }
        }
    }
    function startEditing(): bool { return controller.start() }
    function load() { StyleReview.load() }
    function ended(args, code) { Wallpick.finished(args, code) }
    function state() {
        return JSON.stringify([StyleReview.open, StyleReview.liked, StyleReview.busy, StyleReview.failed]);
    }
    function status(): string { return StyleReview.status }
}
"#;

const OVERLAY: &str = r#"import QtQuick
import Rustshell
import ri.modules.rusty.editMode
EditOverlay {
    screen: Rustshell.screens[0]
}
"#;

struct Review {
    rt: Runtime,
    dir: PathBuf,
    bg: usize,
    overlay: Option<usize>,
    ctl: usize,
    ptr: Pointer,
    fonts: scene::Fonts,
    wake: testenv::Wakeups,
    now: f64,
}

impl Review {
    fn new(tag: &str) -> Self {
        let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../qml")
            .canonicalize()
            .unwrap();
        let dir = std::env::temp_dir().join(format!("style-review-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for e in std::fs::read_dir(&ri).unwrap() {
            let e = e.unwrap();
            if e.file_name() != "widgets.toml" {
                std::os::unix::fs::symlink(e.path(), dir.join(e.file_name())).unwrap();
            }
        }
        std::fs::write(dir.join("widgets.toml"), MANIFEST).unwrap();
        std::fs::write(dir.join("W.qml"), "import QtQuick\nItem {}\n").unwrap();
        std::fs::write(dir.join("layout.toml"), LAYOUT).unwrap();
        std::fs::write(dir.join("bg.qml"), BACKGROUND).unwrap();
        std::fs::write(dir.join("overlay.qml"), OVERLAY).unwrap();
        let mut rt = Runtime::new(Some(dir.clone()));
        // Before anything loads: a service that had work before there was a ping does not
        // ping for more.
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        rt.set_screens(&[("DP-1".into(), 0, 0, W as i32, H as i32)]);
        let dirs: Vec<PathBuf> = [
            "modules/common",
            "modules/common/functions",
            "modules/common/utils",
            "services",
            "",
        ]
        .iter()
        .map(|d| dir.join(d))
        .collect();
        rt.load_singletons_lazy(&dirs);
        rt.world.borrow_mut().open_layout(dir.join("layout.toml"));
        let bg = rt.load_file(&dir.join("bg.qml")).unwrap();
        rt.settle();
        let ctl = match rt.world.borrow_mut().read(bg, "ctl") {
            Some(Value::Item(i)) => i,
            v => panic!("ctl: {v:?}"),
        };
        // As RustyImpulseFamily.qml does when the shell starts.
        rt.call_method(bg, "load", vec![]).unwrap().unwrap();
        rt.settle();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        Review {
            rt,
            dir,
            bg,
            overlay: None,
            ctl,
            ptr: Pointer::default(),
            fonts: scene::Fonts::default(),
            wake,
            now: 1000.0,
        }
    }

    fn call(&mut self, f: &str, args: Vec<Value>) -> Value {
        let v = self.rt.call_method(self.bg, f, args).unwrap().unwrap();
        self.rt.settle();
        v
    }

    /// `rusty-impulse ipc call styleReview open`, what Wallpick.qml asks of the shell: the
    /// function of the handler with that target.
    fn ipc_open(&mut self) {
        let handler = {
            let mut w = self.rt.world.borrow_mut();
            let found = w.of_type("IpcHandler").into_iter().find(|&h| {
                w.read(h, "target")
                    .is_some_and(|t| t.to_js_string() == "styleReview")
            });
            found.expect("no IpcHandler with the target styleReview")
        };
        self.rt
            .call_method(handler, "open", vec![])
            .unwrap()
            .unwrap();
        self.rt.settle();
    }

    /// EditMode.qml loads the overlay of a monitor while the edit mode is on.
    fn show_overlay(&mut self) {
        assert!(self.active(), "the edit mode is off");
        let overlay = self.rt.load_file(&self.dir.join("overlay.qml")).unwrap();
        self.rt.settle();
        assert!(self.rt.errors().is_empty(), "{:?}", self.rt.errors());
        {
            let mut w = self.rt.world.borrow_mut();
            for win in [self.bg, overlay] {
                w.window_rects.insert(
                    win,
                    WinRect {
                        connector: "DP-1".into(),
                        x: 0.0,
                        y: 0.0,
                        w: W,
                        h: H,
                    },
                );
            }
        }
        self.overlay = Some(overlay);
        self.frames(5);
    }

    /// Lets animations run and lays both surfaces out, as frames do.
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.now += 16.0;
            self.rt.tick(self.now);
            self.rt.settle();
            for win in [Some(self.bg), self.overlay].into_iter().flatten() {
                scene::layout_window(&mut self.rt, &mut self.fonts, win, [W, H]);
            }
            self.rt.settle();
        }
    }

    fn active(&self) -> bool {
        let mut w = self.rt.world.borrow_mut();
        w.read(self.ctl, "active").is_some_and(|v| v.truthy())
    }

    /// StyleReview's [open, liked, busy, failed].
    fn state(&mut self) -> String {
        self.call("state", vec![]).to_js_string().to_string()
    }

    fn status(&mut self) -> String {
        self.call("status", vec![]).to_js_string().to_string()
    }

    /// The overlay's visible button with the icon `icon` and, beside it, the text `label`.
    fn find(&self, icon: &str, label: &str) -> Option<usize> {
        let mut w = self.rt.world.borrow_mut();
        let text = |w: &mut qml::runtime::World, b: usize, k: &str| {
            w.read(b, k)
                .filter(|t| !matches!(t, Value::Undefined))
                .map(|t| t.to_js_string().to_string())
        };
        w.of_type("RippleButton").into_iter().find(|&b| {
            !w.dead.contains(&b)
                && text(&mut w, b, "iconText").as_deref() == Some(icon)
                && text(&mut w, b, "text").as_deref() == Some(label)
                && w.read(b, "visible").is_some_and(|v| v.truthy())
        })
    }

    fn button(&self, icon: &str, label: &str) -> usize {
        self.find(icon, label)
            .unwrap_or_else(|| panic!("no visible button {icon} {label:?}"))
    }

    fn click(&mut self, item: usize) {
        let win = self.overlay.expect("no overlay");
        let (x, y) = {
            let mut w = self.rt.world.borrow_mut();
            let (x, y) = w.scene_pos(item, Some(win));
            let mut num = |k: &str| w.read(item, k).unwrap_or_default().to_number();
            (x + num("width") / 2.0, y + num("height") / 2.0)
        };
        self.ptr.enter(&mut self.rt, win, x, y);
        self.ptr.motion(&mut self.rt, win, x, y);
        self.ptr.press(&mut self.rt, BTN_LEFT);
        self.frames(1);
        self.ptr.release(&mut self.rt, BTN_LEFT);
        self.frames(3);
    }

    /// Runs frames until the picker's job in progress has ended, sleeping between two of them
    /// as the shell does: until an event or the runtime's next deadline.
    fn idle(&mut self) {
        while self.state().split(',').nth(2) == Some("true") {
            let next = self.rt.next_deadline(self.now);
            let slept = std::time::Instant::now();
            let woke = self.wake.wait(
                self.rt.process_fd(),
                next.is_finite()
                    .then(|| std::time::Duration::from_secs_f64(next / 1000.0)),
            );
            self.now += slept.elapsed().as_secs_f64() * 1000.0;
            if woke.fd {
                self.rt.process_fd_ready();
            }
            self.rt.poll_native();
            self.frames(1);
        }
        self.frames(3);
    }
}

impl Drop for Review {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Runs `test`'s body in a process whose HOME and XDG directories are a directory of its own:
/// the picker keeps its state and reads config.json there. True in that process.
fn own_home(test: &str) -> Option<PathBuf> {
    if testenv::in_own_process(test) {
        return Some(PathBuf::from(std::env::var_os("HOME").unwrap()));
    }
    let home = std::env::temp_dir().join(format!("style-review-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    testenv::run_in_own_process(
        test,
        [
            ("HOME", home.clone()),
            ("XDG_CONFIG_HOME", home.join(".config")),
            ("XDG_STATE_HOME", home.join(".local/state")),
            ("XDG_CACHE_HOME", home.join(".cache")),
        ],
    );
    let _ = std::fs::remove_dir_all(&home);
    None
}

fn state_file(home: &Path) -> PathBuf {
    home.join(".local/state/rustshell/user/wallpick/state.json")
}

/// The picker's state after a pick: `w1.jpg` is the wallpaper on the screen.
fn picked(home: &Path) -> PathBuf {
    let wall = home.join("w1.jpg");
    std::fs::write(&wall, b"x").unwrap();
    let f = state_file(home);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    let st = serde_json::json!({ "current": {
        "id": "w1", "path": wall, "page": "https://example.org/w1", "accent": null, "emb": [1.0, 0.0],
    }});
    std::fs::write(&f, st.to_string()).unwrap();
    wall
}

fn saved(home: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(state_file(home)).unwrap()).unwrap()
}

/// Wallpick.qml asks the shell for a review after a run that put a new style on the screen and
/// ended well, and after no other.
#[test]
fn only_a_pick_that_ended_well_asks_for_a_review() {
    let mut r = Review::new("asks");
    let args = |a: &[&str]| Value::array(a.iter().map(|s| Value::str(s)).collect());
    let asked = |r: &mut Review, a: &[&str], code: f64| {
        r.call("ended", vec![args(a), Value::Num(code)]);
        std::mem::take(&mut r.rt.world.borrow_mut().self_ipc)
    };
    let open = vec![("styleReview".to_string(), "open".to_string())];
    assert_eq!(asked(&mut r, &["pick"], 0.0), open);
    assert_eq!(asked(&mut r, &["dislike", "--next"], 0.0), open);
    assert_eq!(asked(&mut r, &["pick"], 1.0), vec![]);
    assert_eq!(asked(&mut r, &["dislike", "--next"], 1.0), vec![]);
    for other in [&["like"][..], &["style"], &["reset-taste"], &["dislike"]] {
        assert_eq!(asked(&mut r, other, 0.0), vec![], "{other:?}");
    }
    assert_eq!(r.state(), "[false,false,false,false]");
    assert!(!r.active());
    assert!(r.rt.errors().is_empty(), "{:?}", r.rt.errors());
}

/// A user who never starts a pick: the edit overlay has its own toolbar and no review bar.
#[test]
fn the_overlay_has_no_review_bar_without_a_review() {
    let mut r = Review::new("none");
    r.call("startEditing", vec![]);
    r.show_overlay();
    assert_eq!(r.state(), "[false,false,false,false]");
    assert!(r.find("check", "Done").is_some());
    for (icon, label) in [
        ("thumb_up", "Like"),
        ("thumb_down", "Dislike"),
        ("check", "Confirm"),
    ] {
        assert!(r.find(icon, label).is_none(), "{icon} {label}");
    }
    assert!(r.rt.errors().is_empty(), "{:?}", r.rt.errors());
}

/// The review opens the edit mode with its bar at the top of the overlay; Like records the
/// wallpaper on the screen as liked once, Confirm leaves the edit mode and ends the review.
#[test]
fn the_review_bar_likes_and_confirms() {
    const TEST: &str = concat!(module_path!(), "::the_review_bar_likes_and_confirms");
    let Some(home) = own_home(TEST) else { return };
    let wall = picked(&home);
    let mut r = Review::new("like");
    r.ipc_open();
    assert_eq!(r.state(), "[true,false,false,false]");
    assert!(r.active(), "the review did not open the edit mode");
    r.show_overlay();
    let like = r.button("thumb_up", "Like");
    let confirm = r.button("check", "Confirm");
    r.button("thumb_down", "Dislike");
    let y = r.rt.world.borrow_mut().scene_pos(like, r.overlay).1;
    assert!(y < 100.0, "the review bar is at the top, not at {y}");
    assert_eq!(r.status(), "");

    r.click(like);
    r.idle();
    assert_eq!(r.state(), "[true,true,false,false]");
    assert_eq!(r.status(), "Noted: more like this");
    assert_eq!(saved(&home)["liked"], serde_json::json!([wall]));
    let liked = r.button("thumb_up", "Liked");
    assert!(
        r.rt.world
            .borrow_mut()
            .read(liked, "toggled")
            .unwrap()
            .truthy()
    );
    // Liked already: a second click runs nothing.
    r.click(liked);
    assert_eq!(r.state(), "[true,true,false,false]");
    assert_eq!(saved(&home)["liked"], serde_json::json!([wall]));

    r.click(confirm);
    assert!(!r.active(), "Confirm left the edit mode on");
    assert_eq!(r.state(), "[false,false,false,false]");
    // The next review starts unrated.
    r.ipc_open();
    assert_eq!(r.state(), "[true,false,false,false]");
    assert_eq!(r.status(), "");
    assert!(r.rt.errors().is_empty(), "{:?}", r.rt.errors());
}

/// Dislike records the dislike and picks another style; a pick that fails (here config.json
/// cannot be read) says so on the bar and leaves the review open. Leaving the edit mode with
/// its own Done ends the review as Confirm does.
#[test]
fn dislike_picks_another_and_done_ends_the_review() {
    const TEST: &str = concat!(
        module_path!(),
        "::dislike_picks_another_and_done_ends_the_review"
    );
    let Some(home) = own_home(TEST) else { return };
    picked(&home);
    let config = home.join(".config/rusty-impulse/config.json");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "{\"background\": {}}}").unwrap();
    let before = saved(&home);
    let mut r = Review::new("dislike");
    r.ipc_open();
    r.show_overlay();
    let dislike = r.button("thumb_down", "Dislike");
    r.click(dislike);
    r.idle();
    assert_eq!(r.state(), "[true,false,false,true]");
    assert!(r.status().contains("config.json"), "{}", r.status());
    let after = saved(&home);
    assert_ne!(
        after["taste"], before["taste"],
        "the dislike was not recorded"
    );
    assert_eq!(after["current"], before["current"]);
    assert!(r.active());

    let done = r.button("check", "Done");
    r.click(done);
    assert!(!r.active());
    assert_eq!(r.state(), "[false,false,false,false]");
    assert!(r.rt.errors().is_empty(), "{:?}", r.rt.errors());
}
