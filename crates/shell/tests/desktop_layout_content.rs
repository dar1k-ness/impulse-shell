//! The tree's `DesktopLayoutContent` with the original's own desktop clock: a clock the layout gives no
//! size draws as the original's background draws it in free placement: at its natural size (scale 1),
//! at `x = max(0, min(x, screenWidth - width))` (AbstractBackgroundWidget's targetX).

use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, scene};
use std::path::{Path, PathBuf};

const LAYOUT: &str = r#"version = 2

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "desktop-clock"
x = 1800
y = 900
settings = { placementStrategy = "free" }
"#;

fn tree(tag: &str, w: f64, h: f64) -> PathBuf {
    tree_with(tag, w, h, LAYOUT, "")
}

/// The tree with `layout`, and `types` added to its widgets.toml.
fn tree_with(tag: &str, w: f64, h: f64, layout: &str, types: &str) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!(
        "desktop-layout-content-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        if e.file_name() == "widgets.toml" && !types.is_empty() {
            let reg = std::fs::read_to_string(e.path()).unwrap();
            std::fs::write(d.join("widgets.toml"), reg + types).unwrap();
            continue;
        }
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("layout.toml"), layout).unwrap();
    std::fs::write(
        d.join("main.qml"),
        format!(
            "import QtQuick\nimport ri.modules.rusty.background\nItem {{\n    width: {w}\n    height: {h}\n    \
             DesktopLayoutContent {{ anchors.fill: parent; connector: \"DP-1\" }}\n}}\n"
        ),
    )
    .unwrap();
    d
}

/// Whether this is `test`'s own process (see `testenv::run_in_own_process`). Elsewhere runs
/// it there with a fresh HOME: Config reads `config.json` under it and writes the options a
/// test sets back to it, so in a shared HOME a run would start from what the last one left.
fn own_home(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let home = tempfile::tempdir().unwrap();
    testenv::run_in_own_process(test, testenv::home_vars(home.path()));
    false
}

/// The shell's main loop over one window on a virtual frame clock: each frame runs the
/// runtime's timers, lays the window out and then takes every event the frame set off (pool
/// jobs, the waker's pings, the epoll set: process I/O and inotify) until none is left.
struct Host {
    wake: testenv::Wakeups,
    ep: Option<i32>,
    fonts: scene::Fonts,
    main: usize,
    size: [f64; 2],
    now: f64,
}

impl Host {
    fn new(rt: &mut Runtime, main: usize, size: [f64; 2]) -> Host {
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Host {
            wake,
            ep,
            fonts: scene::Fonts::default(),
            main,
            size,
            now: 1000.0,
        }
    }

    fn frame(&mut self, rt: &mut Runtime) {
        self.now += 16.0;
        rt.tick(self.now);
        rt.settle();
        scene::layout_window(rt, &mut self.fonts, self.main, self.size);
        rt.settle();
        loop {
            workpool::wait_idle();
            let woke = self.wake.wait(self.ep, Some(std::time::Duration::ZERO));
            if !woke.any() {
                return;
            }
            self.turn(rt, woke);
        }
    }

    fn turn(&mut self, rt: &mut Runtime, woke: testenv::Woke) {
        if woke.fd {
            rt.process_fd_ready();
        }
        rt.poll_native();
        rt.settle();
    }

    fn frames(&mut self, rt: &mut Runtime, n: usize) {
        for _ in 0..n {
            self.frame(rt);
        }
    }

    /// Runs frames until `done`, between two frames sleeping as the shell does: until an
    /// event or the runtime's next deadline.
    fn until(&mut self, rt: &mut Runtime, done: impl Fn(&Runtime) -> bool) {
        loop {
            self.frame(rt);
            if done(rt) {
                return;
            }
            let next = rt.next_deadline(self.now);
            let slept = std::time::Instant::now();
            let woke = self.wake.wait(
                self.ep,
                next.is_finite()
                    .then(|| std::time::Duration::from_secs_f64(next / 1000.0)),
            );
            // Time the loop slept passes for the runtime's timers too.
            self.now += slept.elapsed().as_secs_f64() * 1000.0;
            if woke.any() {
                self.turn(rt, woke);
            }
        }
    }
}

/// Whether an item of type `t` is live.
fn live(rt: &Runtime, t: &str) -> bool {
    let wd = rt.world.borrow();
    (0..wd.items.len()).any(|i| !wd.dead.contains(&i) && wd.items[i].types.iter().any(|x| x == t))
}

/// The original's clock placement setting at "free" (the original's default is "leastBusy", by the wallpaper).
fn free_clock(rt: &Runtime) {
    let config = *rt.world.borrow().singletons.get("Config").expect("Config");
    let mut wd = rt.world.borrow_mut();
    let mut o = config;
    for k in ["options", "background", "widgets", "clock"] {
        o = match wd.read(o, k) {
            Some(Value::Item(i)) => i,
            v => panic!("{k}: {v:?}"),
        };
    }
    wd.write(o, "placementStrategy", Value::Str("free".into()));
}

/// The clock's (x on the monitor, width, implicitWidth, scale) and its (y, height).
fn clock(w: f64, h: f64) -> ((f64, f64, f64, f64), (f64, f64)) {
    let d = tree(&format!("{w}x{h}"), w, h);
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    free_clock(&rt);
    let mut host = Host::new(&mut rt, main, [w, h]);
    // The clock comes in on the runtime's events, then its loaders settle over a few frames.
    host.until(&mut rt, |rt| live(rt, "ClockWidget"));
    host.frames(&mut rt, 300);
    let mut wd = rt.world.borrow_mut();
    let c = (0..wd.items.len())
        .find(|&i| !wd.dead.contains(&i) && wd.items[i].types.iter().any(|t| t == "ClockWidget"))
        .expect("no ClockWidget");
    let num = |wd: &mut qml::runtime::World, i: usize, k: &str| {
        wd.read(i, k).unwrap_or(Value::Num(0.0)).to_number()
    };
    let (mut x, mut y) = (0.0, 0.0);
    let mut i = Some(c);
    while let Some(k) = i.filter(|&k| k != main) {
        x += num(&mut wd, k, "x");
        y += num(&mut wd, k, "y");
        i = wd.items[k].parent;
    }
    let geometry = (
        (
            x,
            num(&mut wd, c, "width"),
            num(&mut wd, c, "implicitWidth"),
            num(&mut wd, c, "scale"),
        ),
        (y, num(&mut wd, c, "height")),
    );
    drop(wd);
    std::fs::remove_dir_all(&d).unwrap();
    geometry
}

/// Each size in its own test: one runtime per thread.
fn draws_as_the_original_draws_it(w: f64, h: f64) {
    let ((x, width, implicit, scale), (y, height)) = clock(w, h);
    assert!(implicit > 0.0, "{w}x{h}: the clock is {implicit} wide");
    assert_eq!(
        (scale, width),
        (1.0, implicit),
        "{w}x{h}: natural size, not scaled"
    );
    let want_x = 1800f64.min(w - width.ceil()).max(0.0);
    let want_y = 900f64.min(h - height.ceil()).max(0.0);
    assert!(
        (x - want_x).abs() <= 1.0,
        "{w}x{h}: x {x}, the original's {want_x}"
    );
    assert!(
        (y - want_y).abs() <= 1.0,
        "{w}x{h}: y {y}, the original's {want_y}"
    );
}

#[test]
fn a_free_clock_without_a_size_draws_as_the_original_draws_it_at_1x() {
    if !own_home("a_free_clock_without_a_size_draws_as_the_original_draws_it_at_1x") {
        return;
    }
    draws_as_the_original_draws_it(1920.0, 1080.0);
}

#[test]
fn a_free_clock_without_a_size_draws_as_the_original_draws_it_at_1_5x() {
    if !own_home("a_free_clock_without_a_size_draws_as_the_original_draws_it_at_1_5x") {
        return;
    }
    // 2560x1440 at scale 1.5.
    draws_as_the_original_draws_it(1706.0, 960.0);
}

#[test]
fn a_free_clock_without_a_size_draws_as_the_original_draws_it_at_1280x720() {
    if !own_home("a_free_clock_without_a_size_draws_as_the_original_draws_it_at_1280x720") {
        return;
    }
    draws_as_the_original_draws_it(1280.0, 720.0);
}

/// The original's background widgets with an enable setting (`background.widgets.<name>.enable`): the
/// layout widget type and the item it shows.
const TOGGLED: [(&str, &str, &str); 2] = [
    ("clock", "clock", "ClockWidget"),
    ("weather", "weather", "WeatherWidget"),
];

/// Turned off in the original's settings, each of the original's background widgets goes, as the original's own background
/// widgets do, and takes no place on the desktop; turned back on, it comes back, live both
/// ways and each on its own setting.
#[test]
fn widgets_turned_off_in_settings_are_not_shown() {
    if !own_home("widgets_turned_off_in_settings_are_not_shown") {
        return;
    }
    let layout = format!(
        "{LAYOUT}\n[[monitor.container.widget]]\nid = \"weather\"\ntype = \"desktop-weather\"\n\
         x = 400\ny = 100\nsettings = {{ placementStrategy = \"free\" }}\n"
    );
    let (w, h) = (1920.0, 1080.0);
    let d = tree_with("off", w, h, &layout, "");
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut host = Host::new(&mut rt, main, [w, h]);
    let count = |rt: &Runtime, t: &str| {
        let wd = rt.world.borrow();
        (0..wd.items.len())
            .filter(|&i| !wd.dead.contains(&i) && wd.items[i].types.iter().any(|x| x == t))
            .count()
    };
    let item = |v: Value| match v {
        Value::Item(i) => i,
        v => panic!("{v:?}"),
    };
    let has_place = |rt: &Runtime, id: &str| {
        let wd = rt.world.borrow();
        let c = (0..wd.items.len())
            .find(|&i| {
                !wd.dead.contains(&i) && wd.items[i].types.iter().any(|x| x == "DesktopContainer")
            })
            .expect("no DesktopContainer");
        drop(wd);
        match rt.world.borrow_mut().read(c, "shown").unwrap_or_default() {
            Value::Obj(o) => o.borrow().props.contains_key(id),
            v => panic!("shown: {v:?}"),
        }
    };
    host.until(&mut rt, |rt| live(rt, "ClockWidget"));
    host.frames(&mut rt, 300);
    let config = *rt.world.borrow().singletons.get("Config").expect("Config");
    let enable = |rt: &mut Runtime, name: &str, on: bool| {
        let mut wd = rt.world.borrow_mut();
        let mut o = item(wd.read(config, "options").unwrap_or_default());
        for k in ["background", "widgets", name] {
            o = item(wd.read(o, k).unwrap_or_default());
        }
        wd.write(o, "enable", Value::Bool(on));
        assert_eq!(wd.read(o, "enable").unwrap_or_default().truthy(), on);
    };
    let check = |rt: &Runtime, on: [bool; 2], when: &str| {
        for ((_, id, t), on) in TOGGLED.iter().zip(on) {
            assert_eq!(count(rt, t), usize::from(on), "{when}: {t}");
            assert_eq!(has_place(rt, id), on, "{when}: {id}'s place");
        }
    };
    for (name, _, _) in TOGGLED {
        enable(&mut rt, name, true);
    }
    host.frames(&mut rt, 60);
    check(&rt, [true, true], "both on");
    for (i, (name, _, _)) in TOGGLED.iter().enumerate() {
        enable(&mut rt, name, false);
        host.frames(&mut rt, 60);
        let mut on = [true, true];
        on[i] = false;
        check(&rt, on, &format!("{name} off"));
        enable(&mut rt, name, true);
        host.frames(&mut rt, 60);
        check(&rt, [true, true], &format!("{name} on again"));
    }
    for (name, _, _) in TOGGLED {
        enable(&mut rt, name, false);
    }
    host.frames(&mut rt, 60);
    check(&rt, [false, false], "both off");
    assert_eq!(
        std::fs::read_to_string(d.join("layout.toml")).unwrap(),
        layout,
        "the layout keeps them"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

/// A desktop widget type whose widgets.toml entry names no `enabledBy` flag shows whatever
/// the original's settings say.
#[test]
fn a_type_without_a_flag_is_always_shown() {
    if !own_home("a_type_without_a_flag_is_always_shown") {
        return;
    }
    let types = "\n[[widget]]\ntype = \"desktop-plain\"\nname = \"Plain\"\nplacements = [\"desktop\"]\n\
                 component = \"modules/rusty/background/widgets/weather/DesktopWeather.qml\"\n\
                 size = { min = [100, 100], default = [200, 200] }\n";
    let layout = LAYOUT
        .replace("id = \"clock\"", "id = \"plain\"")
        .replace("type = \"desktop-clock\"", "type = \"desktop-plain\"")
        .replace("settings = { placementStrategy = \"free\" }\n", "");
    let (w, h) = (1920.0, 1080.0);
    let d = tree_with("plain", w, h, &layout, types);
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut host = Host::new(&mut rt, main, [w, h]);
    let find = |rt: &Runtime, t: &str| {
        let wd = rt.world.borrow();
        (0..wd.items.len())
            .filter(|&i| !wd.dead.contains(&i) && wd.items[i].types.iter().any(|x| x == t))
            .collect::<Vec<_>>()
    };
    let item = |v: Value| match v {
        Value::Item(i) => i,
        v => panic!("{v:?}"),
    };
    let shown = |rt: &Runtime| {
        let c = find(rt, "DesktopContainer")[0];
        match rt.world.borrow_mut().read(c, "shown").unwrap_or_default() {
            Value::Obj(o) => o.borrow().props.contains_key("plain"),
            v => panic!("shown: {v:?}"),
        }
    };
    host.until(&mut rt, |rt| live(rt, "WeatherWidget"));
    host.frames(&mut rt, 300);
    let config = *rt.world.borrow().singletons.get("Config").expect("Config");
    for on in [true, false, true, false] {
        for name in ["clock", "weather"] {
            let mut wd = rt.world.borrow_mut();
            let mut o = item(wd.read(config, "options").unwrap_or_default());
            for k in ["background", "widgets", name] {
                o = item(wd.read(o, k).unwrap_or_default());
            }
            wd.write(o, "enable", Value::Bool(on));
        }
        host.frames(&mut rt, 60);
        assert_eq!(find(&rt, "WeatherWidget").len(), 1, "flags {on}: shown");
        assert!(shown(&rt), "flags {on}: has its place");
    }
    std::fs::remove_dir_all(&d).unwrap();
}

/// Each of the original's background widgets on the desktop is placed as the original's settings say
/// (`background.widgets.<name>.placementStrategy`), as the original's own background widgets are: what
/// Settings > Background picks moves it live, whatever placement the layout file once copied.
#[test]
fn widgets_follow_the_original_s_placement_setting_live() {
    if !own_home("widgets_follow_the_original_s_placement_setting_live") {
        return;
    }
    let layout = format!(
        "{LAYOUT}\n[[monitor.container.widget]]\nid = \"weather\"\ntype = \"desktop-weather\"\n\
         x = 400\ny = 100\nsettings = {{ placementStrategy = \"free\" }}\n"
    );
    let (w, h) = (1920.0, 1080.0);
    let d = tree_with("placement", w, h, &layout, "");
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut host = Host::new(&mut rt, main, [w, h]);
    let item = |v: Value| match v {
        Value::Item(i) => i,
        v => panic!("{v:?}"),
    };
    let widget = |rt: &Runtime, t: &str| {
        let wd = rt.world.borrow();
        (0..wd.items.len())
            .find(|&i| !wd.dead.contains(&i) && wd.items[i].types.iter().any(|x| x == t))
            .unwrap_or_else(|| panic!("no {t}"))
    };
    let config = *rt.world.borrow().singletons.get("Config").expect("Config");
    let options = |rt: &Runtime, name: &str| {
        let mut wd = rt.world.borrow_mut();
        let mut o = item(wd.read(config, "options").unwrap_or_default());
        for k in ["background", "widgets", name] {
            o = item(wd.read(o, k).unwrap_or_default());
        }
        o
    };
    for name in ["clock", "weather"] {
        let o = options(&rt, name);
        rt.world.borrow_mut().write(o, "enable", Value::Bool(true));
    }
    host.until(&mut rt, |rt| {
        live(rt, "ClockWidget") && live(rt, "WeatherWidget")
    });
    // The desktop widget (DesktopClock, DesktopWeather) is the parent of the original's own one.
    let placed = |rt: &Runtime, t: &str| {
        let own = widget(rt, t);
        let mut wd = rt.world.borrow_mut();
        let i = wd.items[own].parent.expect("parent");
        let strategy = wd.read(i, "placementStrategy").unwrap_or_default();
        (
            strategy.to_js_string(),
            wd.read(i, "wallpaperPlaced").unwrap_or_default().truthy(),
        )
    };
    let set = |rt: &mut Runtime, name: &str, strategy: &str| {
        let o = options(rt, name);
        rt.world
            .borrow_mut()
            .write(o, "placementStrategy", Value::Str(strategy.into()));
    };
    // The original's defaults: the clock by the wallpaper, the weather free.
    host.frame(&mut rt);
    assert_eq!(placed(&rt, "ClockWidget"), ("leastBusy".into(), true));
    assert_eq!(placed(&rt, "WeatherWidget"), ("free".into(), false));
    for (name, t) in [("clock", "ClockWidget"), ("weather", "WeatherWidget")] {
        for strategy in ["free", "mostBusy", "leastBusy", "free"] {
            set(&mut rt, name, strategy);
            host.frame(&mut rt);
            assert_eq!(
                placed(&rt, t),
                (strategy.to_string(), strategy != "free"),
                "{name} set to {strategy}"
            );
        }
    }
    std::fs::remove_dir_all(&d).unwrap();
}

const SELF_PLACED: &str = r#"version = 5

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "clock"
x = 100
y = 100
w = 230
h = 250

[[monitor.container.widget]]
id = "weather"
type = "weather"
x = 60
y = 120
"#;

/// The original's clock placed by the wallpaper (leastBusy, the original's default) shows where it
/// draws, not at its stored box: the container gives that place as the clock's (its cell, and so
/// its edit-mode frame, is there), and the widget the layout puts under the clock's stored box
/// stays where `layout.toml` says.
#[test]
fn a_wallpaper_placed_clock_shows_where_it_draws_and_its_neighbour_keeps_its_place() {
    if !own_home("a_wallpaper_placed_clock_shows_where_it_draws_and_its_neighbour_keeps_its_place")
    {
        return;
    }
    let (w, h) = (1920.0, 1080.0);
    let d = tree_with("selfplaced", w, h, SELF_PLACED, "");
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut host = Host::new(&mut rt, main, [w, h]);
    let item = |v: Value| match v {
        Value::Item(i) => i,
        v => panic!("{v:?}"),
    };
    let find = |rt: &Runtime, t: &str| {
        let wd = rt.world.borrow();
        (0..wd.items.len())
            .find(|&i| !wd.dead.contains(&i) && wd.items[i].types.iter().any(|x| x == t))
            .unwrap_or_else(|| panic!("no {t}"))
    };
    let config = *rt.world.borrow().singletons.get("Config").expect("Config");
    for name in ["clock", "weather"] {
        let mut wd = rt.world.borrow_mut();
        let mut o = item(wd.read(config, "options").unwrap_or_default());
        for k in ["background", "widgets", name] {
            o = item(wd.read(o, k).unwrap_or_default());
        }
        wd.write(o, "enable", Value::Bool(true));
    }
    host.until(&mut rt, |rt| {
        live(rt, "ClockWidget") && live(rt, "WeatherWidget")
    });
    host.frames(&mut rt, 300);
    let shown = |rt: &Runtime, key: &str| {
        let c = find(rt, "DesktopContainer");
        let Value::Obj(o) = rt.world.borrow_mut().read(c, "shown").unwrap_or_default() else {
            panic!("no shown")
        };
        let Some(Value::Obj(r)) = o.borrow().props.get(key).cloned() else {
            panic!("{key} not shown")
        };
        let r = r.borrow();
        let n = |k: &str| r.props[k].to_number().round() as i64;
        (n("x"), n("y"), n("width"), n("height"))
    };
    // Where an item draws on the monitor.
    let at = |rt: &Runtime, i: usize| {
        let mut wd = rt.world.borrow_mut();
        let (mut x, mut y) = (0.0, 0.0);
        let mut k = Some(i);
        while let Some(j) = k.filter(|&j| j != main) {
            x += wd.read(j, "x").unwrap_or(Value::Num(0.0)).to_number();
            y += wd.read(j, "y").unwrap_or(Value::Num(0.0)).to_number();
            k = wd.items[j].parent;
        }
        (x.round() as i64, y.round() as i64)
    };
    let own = find(&rt, "ClockWidget");
    let desktop_clock = rt.world.borrow().items[own].parent.expect("parent");
    let placed = rt
        .world
        .borrow_mut()
        .read(desktop_clock, "wallpaperPlaced")
        .unwrap_or_default()
        .truthy();
    assert!(
        placed,
        "the original's default places the clock by the wallpaper"
    );
    let clock = shown(&rt, "clock");
    assert_eq!(
        (clock.0, clock.1),
        at(&rt, own),
        "the clock's place is where it draws"
    );
    assert_eq!(
        at(&rt, desktop_clock),
        at(&rt, own),
        "its cell is there too"
    );
    assert!(
        clock.0 >= 0 && clock.1 >= 0 && clock.0 + clock.2 <= 1920 && clock.1 + clock.3 <= 1080,
        "{clock:?}"
    );
    let weather = shown(&rt, "weather");
    assert_eq!(
        (weather.0, weather.1),
        (60, 120),
        "the clock shows at {clock:?}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

const BTN_LEFT: u32 = 0x110;

/// The original's "Draggable" placement (Settings > Background, `placementStrategy: "free"`): the pointer
/// drags the clock and the weather as it drags the original's own, and where each is dropped is its place
/// in the layout, written to `layout.toml` (and to the original's options, as the original writes them). A widget
/// the wallpaper places is not dragged.
#[test]
fn a_free_widget_is_dragged_and_the_layout_keeps_where_it_is_dropped() {
    if !own_home("a_free_widget_is_dragged_and_the_layout_keeps_where_it_is_dropped") {
        return;
    }
    let layout = format!(
        "{}\n[[monitor.container.widget]]\nid = \"weather\"\ntype = \"desktop-weather\"\n\
         x = 400\ny = 100\nsettings = {{ placementStrategy = \"free\" }}\n",
        LAYOUT.replace("x = 1800\ny = 900", "x = 900\ny = 500")
    );
    let (w, h) = (1920.0, 1080.0);
    let d = tree_with("drag", w, h, &layout, "");
    // The original's widgets show while the screen is not locked (GlobalStates.screenLocked); the rest
    // of GlobalStates (the panels' states, the shortcuts) is not part of this.
    std::fs::remove_file(d.join("GlobalStates.qml")).unwrap();
    std::fs::write(
        d.join("GlobalStates.qml"),
        "pragma Singleton\nimport QtQuick\nQtObject { property bool screenLocked: false }\n",
    )
    .unwrap();
    // Midday for the engine: the original's night light schedule (Hyprsunset, loaded with the services)
    // decides nothing here from the hour the test runs at.
    qml::builtins::set_clock(Some(testenv::local_time_ms(12, 0)));
    let mut rt = Runtime::new(Some(d.clone()));
    rt.load_singletons(&[
        d.clone(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut host = Host::new(&mut rt, main, [w, h]);
    let item = |v: Value| match v {
        Value::Item(i) => i,
        v => panic!("{v:?}"),
    };
    let widget = |rt: &Runtime, t: &str| {
        let wd = rt.world.borrow();
        (0..wd.items.len())
            .find(|&i| !wd.dead.contains(&i) && wd.items[i].types.iter().any(|x| x == t))
            .unwrap_or_else(|| panic!("no {t}"))
    };
    let config = *rt.world.borrow().singletons.get("Config").expect("Config");
    let options = |rt: &Runtime, name: &str| {
        let mut wd = rt.world.borrow_mut();
        let mut o = item(wd.read(config, "options").unwrap_or_default());
        for k in ["background", "widgets", name] {
            o = item(wd.read(o, k).unwrap_or_default());
        }
        o
    };
    let set = |rt: &mut Runtime, name: &str, key: &str, v: Value| {
        let o = options(rt, name);
        rt.world.borrow_mut().write(o, key, v);
    };
    for name in ["clock", "weather"] {
        set(&mut rt, name, "enable", Value::Bool(true));
        set(
            &mut rt,
            name,
            "placementStrategy",
            Value::Str("free".into()),
        );
    }
    // Turned on, the widgets fade in over the next frames.
    host.frames(&mut rt, 30);
    assert!(live(&rt, "ClockWidget") && live(&rt, "WeatherWidget"));
    // (x, y) on the monitor and (width, height) of the original's widget of type `t`.
    let on_monitor = |rt: &Runtime, t: &str| {
        let own = widget(rt, t);
        let mut wd = rt.world.borrow_mut();
        let n = |wd: &mut qml::runtime::World, k: &str| wd.read(own, k).unwrap().to_number();
        let size = (n(&mut wd, "width"), n(&mut wd, "height"));
        (wd.scene_pos(own, Some(main)), size)
    };
    // Where the layout file puts widget `id`.
    let in_file = |id: &str| {
        let text = std::fs::read_to_string(d.join("layout.toml")).unwrap();
        let l = qml::layout_model::Layout::parse(&text).unwrap();
        let c = &l.monitors[0].containers[0];
        let wd = c.widgets.iter().find(|wd| wd.id == id).expect("widget");
        match wd.place {
            qml::layout_model::Place::Desktop { x, y, size } => (x as f64, y as f64, size),
            ref p => panic!("{p:?}"),
        }
    };
    let option = |rt: &Runtime, name: &str, k: &str| {
        let o = options(rt, name);
        rt.world.borrow_mut().read(o, k).unwrap().to_number()
    };
    let mut ptr = Pointer::default();
    ptr.enter(&mut rt, main, 1.0, 1.0);
    // Presses the middle of the original's widget of type `t` and moves the pointer by (`dx`, `dy`) in
    // steps; returns where the widget then is, still held.
    let drag = |rt: &mut Runtime, host: &mut Host, ptr: &mut Pointer, t: &str, dx: f64, dy: f64| {
        let ((x, y), (w, h)) = on_monitor(rt, t);
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        ptr.motion(rt, main, cx, cy);
        ptr.press(rt, BTN_LEFT);
        for k in 1..=50 {
            let f = k as f64 / 50.0;
            ptr.motion(rt, main, cx + dx * f, cy + dy * f);
            host.frame(rt);
        }
        on_monitor(rt, t).0
    };
    let moves = [
        ("clock", "ClockWidget", (900.0, 500.0), (-300.0, 120.0)),
        ("weather", "WeatherWidget", (400.0, 100.0), (250.0, 100.0)),
    ];
    for (name, t, start, (dx, dy)) in moves {
        let (from, _) = on_monitor(&rt, t);
        assert_eq!(from, start, "{name} before the drag");
        assert_eq!(in_file(name), (start.0, start.1, None));
        let held = drag(&mut rt, &mut host, &mut ptr, t, dx, dy);
        // The drag starts once the pointer is past the threshold, from where it then is.
        assert!(
            (held.0 - from.0 - dx).abs() <= 20.0 && (held.1 - from.1 - dy).abs() <= 20.0,
            "{name} dragged from {from:?} by ({dx}, {dy}) is at {held:?}"
        );
        assert_eq!(in_file(name), (start.0, start.1, None), "{name} held");
        ptr.release(&mut rt, BTN_LEFT);
        host.frames(&mut rt, 30);
        let want = (held.0.round(), held.1.round());
        assert_eq!(on_monitor(&rt, t).0, want, "{name} dropped");
        assert_eq!(in_file(name), (want.0, want.1, None), "{name} in the file");
        assert_eq!(
            (option(&rt, name, "x"), option(&rt, name, "y")),
            want,
            "{name} in the original's options"
        );
    }
    // A click that does not move the widget changes nothing.
    let before = std::fs::read_to_string(d.join("layout.toml")).unwrap();
    let ((x, y), (cw, ch)) = on_monitor(&rt, "ClockWidget");
    ptr.motion(&mut rt, main, x + cw / 2.0, y + ch / 2.0);
    ptr.press(&mut rt, BTN_LEFT);
    host.frame(&mut rt);
    ptr.release(&mut rt, BTN_LEFT);
    host.frames(&mut rt, 30);
    assert_eq!(on_monitor(&rt, "ClockWidget").0, (x, y));
    // Placed by the wallpaper, the weather is not dragged.
    set(
        &mut rt,
        "weather",
        "placementStrategy",
        Value::Str("leastBusy".into()),
    );
    host.frames(&mut rt, 30);
    let (from, _) = on_monitor(&rt, "WeatherWidget");
    let held = drag(
        &mut rt,
        &mut host,
        &mut ptr,
        "WeatherWidget",
        -200.0,
        -100.0,
    );
    ptr.release(&mut rt, BTN_LEFT);
    host.frames(&mut rt, 30);
    assert_eq!(held, from, "a wallpaper-placed weather follows the pointer");
    assert_eq!(
        std::fs::read_to_string(d.join("layout.toml")).unwrap(),
        before
    );
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    drop(rt);
    std::fs::remove_dir_all(&d).unwrap();
}
