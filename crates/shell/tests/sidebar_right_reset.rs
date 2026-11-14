//! The right sidebar opens as it first did (docs/original-differences.md): what was left open in it,
//! paged or scrolled is put back as it closes and as it opens. The tree's `SidebarRightContent`
//! stays loaded while the sidebar is closed (`sidebar.keepRightSidebarLoaded`), so this is the
//! state the next opening would show: the calendar's month, the pencil's edit mode, the detail
//! dialogs of the toggles, the lists' scroll positions, the expanded notification groups, the
//! tabs of the to-do list and of the timer, and the "Add task" dialog.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};

const MAIN: &str = r#"import QtQuick
import ri
import ri.services
import ri.modules.common
import ri.modules.rusty.sidebarRight
Item {
    width: 460
    height: 1000
    property alias content: content
    property bool ready: Config.ready
    property int notifications: Notifications.list.length
    function open(v) { GlobalStates.sidebarRightOpen = v }
    function readNotifications(path) {
        Notifications.filePath = path;
        Notifications.refresh();
    }
    function fill(n) {
        const tasks = [], laps = [];
        for (let i = 0; i < n; i++) {
            tasks.push({ content: "task " + i, done: i % 2 == 1 });
            laps.push(i * 100);
        }
        Todo.list = tasks;
        TimerService.stopwatchLaps = laps;
    }
    function tab(bar, i) { bar.setCurrentIndex(i) }
    function expand(group) { group.toggleExpanded() }
    function scroll(view) {
        view.contentY = Math.min(150, view.contentHeight - view.height);
    }
    SidebarRightContent { id: content; anchors.fill: parent }
}
"#;

const LAYOUT: &str = r#"version = 4
[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "right"
kind = "sidebar"
side = "right"
columns = 5

[[monitor.container.widget]]
id = "toggles"
type = "quickToggles"
size = 5

[[monitor.container.widget]]
id = "calendar"
type = "calendar"
size = 5

[[monitor.container.widget]]
id = "notes"
type = "notes"
size = 5

[[monitor.container.widget]]
id = "notifications"
type = "notificationCenter"
size = 5
"#;

const SIZE: [f64; 2] = [460.0, 1000.0];
/// The window of the layout's sidebar. It shows only the widgets it has room for (a widget that
/// does not fit is hidden, not laid out past the screen's edge), and the four of `LAYOUT` with
/// the quick toggles in their edit mode need more than the original's blocks do.
const PLACED_SIZE: [f64; 2] = [460.0, 1400.0];

/// Two seconds of the frame clock, past every animation of the sidebar.
const FRAMES: usize = 125;

const DIALOGS: [&str; 5] = [
    "showAudioOutputDialog",
    "showAudioInputDialog",
    "showBluetoothDialog",
    "showNightLightDialog",
    "showWifiDialog",
];

/// Thirty apps with one notification each, and one with three.
fn notifications() -> String {
    let mut all = vec![];
    for i in 0..33 {
        all.push(serde_json::json!({
            "notificationId": i + 1,
            "actions": [],
            "appIcon": "",
            "appName": format!("app {}", i.min(30)),
            "body": format!("body {i}"),
            "image": "",
            "summary": format!("summary {i}"),
            "time": 1_700_000_000_000_f64 + f64::from(i),
            "urgency": "normal",
        }));
    }
    serde_json::Value::Array(all).to_string()
}

fn tree(tag: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("sidebar-right-reset-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("zzmain.qml"), MAIN).unwrap();
    std::fs::write(d.join("zzlayout.toml"), LAYOUT).unwrap();
    std::fs::write(d.join("zznotifications.json"), notifications()).unwrap();
    d
}

/// The shell's main loop over one window on a virtual frame clock, as in `quick_toggle_edit`.
struct T {
    rt: Runtime,
    main: usize,
    content: usize,
    fonts: std::rc::Rc<std::cell::RefCell<scene::Fonts>>,
    wake: testenv::Wakeups,
    ep: Option<i32>,
    size: [f64; 2],
    now: f64,
}

impl T {
    /// The sidebar with the original's own blocks, or with the widgets of `LAYOUT`.
    fn new(compiled: bool, layout: bool) -> T {
        let d = tree(&format!("{compiled}-{layout}"));
        let mut rt = Runtime::new(Some(d.clone()));
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
        if layout {
            rt.world.borrow_mut().open_layout(d.join("zzlayout.toml"));
        }
        rt.settle();
        let main = rt.load_file(&d.join("zzmain.qml")).unwrap();
        rt.settle();
        let Some(Value::Item(content)) = rt.world.borrow_mut().read(main, "content") else {
            panic!("no content")
        };
        let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
        let ep = rt.process_fd();
        let mut t = T {
            rt,
            main,
            content,
            fonts,
            wake,
            ep,
            size: if layout { PLACED_SIZE } else { SIZE },
            now: 1000.0,
        };
        t.until(|t| t.truthy(t.main, "ready"));
        let file = d.join("zznotifications.json");
        t.call(
            "readNotifications",
            vec![Value::Str(file.to_str().unwrap().into())],
        );
        t.until(|t| t.num(t.main, "notifications") == 33.0);
        t.call("fill", vec![Value::Num(80.0)]);
        t.call("open", vec![Value::Bool(true)]);
        t.frames(FRAMES);
        t
    }
    fn frame(&mut self) {
        self.now += 16.0;
        self.rt.tick(self.now);
        self.rt.settle();
        let fonts = self.fonts.clone();
        scene::layout_window(&mut self.rt, &mut fonts.borrow_mut(), self.main, self.size);
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
    /// Runs frames until `done`, between two frames sleeping as the shell does: until an event
    /// or the runtime's next deadline.
    fn until(&mut self, done: impl Fn(&mut T) -> bool) {
        loop {
            self.frame();
            if done(self) {
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
    fn call(&mut self, f: &str, args: Vec<Value>) {
        self.rt.call_method(self.main, f, args).unwrap().unwrap();
    }
    fn set(&mut self, item: usize, prop: &str, v: Value) {
        self.rt.set(item, prop, v);
        self.rt.settle();
    }
    fn num(&mut self, item: usize, prop: &str) -> f64 {
        match self.rt.world.borrow_mut().read(item, prop) {
            Some(Value::Num(n)) => n,
            other => panic!("{prop}: {other:?}"),
        }
    }
    fn truthy(&mut self, item: usize, prop: &str) -> bool {
        let v = self.rt.world.borrow_mut().read(item, prop);
        v.unwrap_or_default().truthy()
    }
    /// The live items of the QML type `ty`, in the order they were made.
    fn of(&mut self, ty: &str) -> Vec<usize> {
        let w = self.rt.world.borrow_mut();
        (0..w.items.len())
            .filter(|i| !w.dead.contains(i))
            .filter(|i| w.items[*i].types.iter().any(|t| t == ty))
            .collect()
    }
    fn one(&mut self, ty: &str) -> usize {
        let of = self.of(ty);
        assert_eq!(of.len(), 1, "{ty}");
        of[0]
    }
    /// The live items that declare the property `prop`.
    fn having(&mut self, prop: &str) -> Vec<usize> {
        let mut w = self.rt.world.borrow_mut();
        let mut out = vec![];
        for i in 0..w.items.len() {
            if !w.dead.contains(&i) && w.read(i, prop).is_some() {
                out.push(i);
            }
        }
        out
    }
    /// The calendar: the one item with a `monthShift`.
    fn calendar(&mut self) -> usize {
        let of = self.having("monthShift");
        let types: Vec<_> = (of.iter())
            .map(|&i| self.rt.world.borrow().items[i].types.clone())
            .collect();
        assert_eq!(of.len(), 1, "calendars: {types:?}");
        of[0]
    }
    /// The lists that have more rows than they show, with their `contentY`.
    fn scrollable(&mut self) -> Vec<(usize, f64)> {
        let mut out = vec![];
        for l in self.of("StyledListView") {
            let room = self.num(l, "contentHeight") - self.num(l, "height");
            if room > 1.0 && self.num(l, "height") > 0.0 {
                out.push((l, self.num(l, "contentY")));
            }
        }
        out
    }
    /// Scrolls every such list down and returns how many there are.
    fn scroll(&mut self) -> usize {
        let lists = self.scrollable();
        for &(l, _) in &lists {
            self.call("scroll", vec![Value::Item(l)]);
        }
        self.frames(FRAMES);
        for (l, y) in self.scrollable() {
            assert!(y > 0.0, "list {l} did not scroll");
        }
        lists.len()
    }
    /// Closes the sidebar and opens it again. What the first frame of the open sidebar shows
    /// is what `check` reads.
    fn reopen(&mut self) {
        self.call("open", vec![Value::Bool(false)]);
        self.frames(FRAMES);
        self.call("open", vec![Value::Bool(true)]);
        self.frame();
    }
    fn check(&mut self, what: &str) {
        let c = self.content;
        assert!(!self.truthy(c, "editMode"), "{what}: edit mode");
        for d in DIALOGS {
            assert!(!self.truthy(c, d), "{what}: {d}");
        }
        for l in self.having("shownPropertyString") {
            let item = self.rt.world.borrow_mut().read(l, "item");
            assert!(
                !matches!(item, Some(Value::Item(_))),
                "{what}: a dialog is still there"
            );
        }
        for (l, y) in self.scrollable() {
            assert_eq!(y, 0.0, "{what}: contentY of list {l}");
            assert_eq!(self.num(l, "scrollTargetY"), 0.0, "{what}: list {l}");
        }
        for g in self.of("NotificationGroup") {
            assert!(!self.truthy(g, "expanded"), "{what}: group {g}");
        }
        // The Bluetooth dialog turns the adapter's discovery on and off, and here is no adapter.
        let errors: Vec<_> = (self.rt.errors().into_iter())
            .filter(|e| !e.starts_with("ToggleDialog/Loader.onShownChanged: Value is null"))
            .collect();
        assert!(errors.is_empty(), "{what}: {errors:?}");
    }
    /// Opens every dialog and the edit mode, expands the first notification group and scrolls
    /// the lists.
    fn disturb(&mut self) -> usize {
        let c = self.content;
        self.set(c, "editMode", Value::Bool(true));
        for d in DIALOGS {
            self.set(c, d, Value::Bool(true));
        }
        let group = self.of("NotificationGroup")[0];
        self.call("expand", vec![Value::Item(group)]);
        assert!(self.truthy(group, "expanded"));
        self.scroll()
    }
    /// The to-do list's tabs, at "Done", with the "Add task" dialog open.
    fn disturb_todo(&mut self) -> (usize, usize) {
        let todo = self.having("showAddDialog");
        assert_eq!(todo.len(), 1, "to-do lists");
        let (todo, bar) = (todo[0], self.one("SecondaryTabBar"));
        self.call("tab", vec![Value::Item(bar), Value::Num(1.0)]);
        self.set(todo, "showAddDialog", Value::Bool(true));
        self.frames(FRAMES);
        assert_eq!(self.num(bar, "currentIndex"), 1.0);
        (todo, bar)
    }
    fn check_todo(&mut self, what: &str, (todo, bar): (usize, usize)) {
        assert!(!self.truthy(todo, "showAddDialog"), "{what}: Add task");
        assert_eq!(self.num(bar, "currentIndex"), 0.0, "{what}: to-do tab");
        let swipe = self.one("SwipeView");
        assert_eq!(self.num(swipe, "currentIndex"), 0.0, "{what}: to-do page");
    }
}

fn own(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    for sub in ["config", "state", "cache"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    testenv::run_in_own_process(
        test,
        [
            ("XDG_CONFIG_HOME", d.join("config")),
            ("XDG_STATE_HOME", d.join("state")),
            ("XDG_CACHE_HOME", d.join("cache")),
        ],
    );
    false
}

/// The original's own blocks: the notification list over the tabbed calendar, to-do list and timer.
fn blocks(compiled: bool) {
    let mut t = T::new(compiled, false);
    t.check("as loaded");
    let group = t.one("BottomWidgetGroup");

    // The calendar tab.
    let calendar = t.calendar();
    assert_eq!(t.disturb(), 1, "the notification list scrolls");
    t.set(calendar, "monthShift", Value::Num(10.0));
    t.reopen();
    t.check("calendar tab");
    assert_eq!(t.num(calendar, "monthShift"), 0.0);

    // The to-do tab: both of its lists have 40 tasks.
    t.set(group, "selectedTab", Value::Num(1.0));
    t.frames(FRAMES);
    assert_eq!(
        t.disturb(),
        3,
        "the notifications and the two lists of tasks"
    );
    let todo = t.disturb_todo();
    t.reopen();
    t.check("to-do tab");
    t.check_todo("to-do tab", todo);
    assert_eq!(
        t.num(group, "selectedTab"),
        1.0,
        "the tab of the block stays"
    );

    // The timer tab: the stopwatch has 80 laps.
    t.set(group, "selectedTab", Value::Num(2.0));
    t.frames(FRAMES);
    let bar = t.one("SecondaryTabBar");
    t.call("tab", vec![Value::Item(bar), Value::Num(1.0)]);
    t.frames(FRAMES);
    assert_eq!(t.disturb(), 2, "the notifications and the laps");
    t.reopen();
    t.check("timer tab");
    assert_eq!(t.num(bar, "currentIndex"), 0.0, "timer tab");
    let swipe = t.one("SwipeView");
    assert_eq!(t.num(swipe, "currentIndex"), 0.0, "timer page");
}

/// The layout's sidebar: the widgets `calendar`, `notes` and `notificationCenter`, and the
/// quick toggles that take the edit mode.
fn placed(compiled: bool) {
    let mut t = T::new(compiled, true);
    t.check("as loaded");
    // `SidebarLayoutContent.qml`, which a Loader makes from its file.
    let with_mode = t.having("editMode");
    let mut layout = t.having("connector");
    layout.retain(|i| with_mode.contains(i));
    assert_eq!(layout.len(), 1, "SidebarLayoutContent");
    let layout = layout[0];
    let calendar = t.calendar();
    assert_eq!(
        t.disturb(),
        3,
        "the notifications and the two lists of tasks"
    );
    assert!(t.truthy(layout, "editMode"));
    t.set(calendar, "monthShift", Value::Num(-3.0));
    let todo = t.disturb_todo();
    t.reopen();
    t.check("placed");
    assert!(!t.truthy(layout, "editMode"), "the layout's edit mode");
    assert_eq!(t.num(calendar, "monthShift"), 0.0);
    t.check_todo("placed", todo);
}

macro_rules! tests {
    ($($name:ident: $body:ident($compiled:expr);)*) => {$(
        #[test]
        #[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
        fn $name() {
            if own(concat!(module_path!(), "::", stringify!($name))) {
                $body($compiled);
            }
        }
    )*};
}

tests! {
    blocks_interp: blocks(false);
    blocks_compiled: blocks(true);
    placed_interp: placed(false);
    placed_compiled: placed(true);
}
