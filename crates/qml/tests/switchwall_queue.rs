//! Every original place that switches the wallpaper or its colors goes through services/Switchwall.qml: one run at a
//! time, a request made during a run waits, a newer waiting request replaces an older one, and
//! the queue settles once the last run has exited and the colors file has been read. Each test
//! asks one launch site three times in a row; detached launches started three overlapping runs,
//! and whichever finished last decided the theme or wallpaper. The wallpaper buttons ask for
//! the file first, outside the queue (`Switchwall.pick`): an open dialog holds no run back.
use qml::runtime::Runtime;
use qml::value::Value;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

type Runs = Rc<RefCell<Vec<Vec<String>>>>;

struct Shell {
    rt: Runtime,
    runs: Runs,
    /// The wallpaper dialogs opened.
    picks: Runs,
    /// What the next dialog answers: the chosen file, `None` for Cancel.
    answer: Rc<RefCell<Option<String>>>,
}

impl Drop for Shell {
    fn drop(&mut self) {
        qml::native::set_spawn_hook(None);
        qml::builtins::set_clock(None);
    }
}

/// In `test`'s own process (see `testenv::run_in_own_process`): a fresh home with `config` as
/// config.json, every spawn recorded and ended at once (its exit is delivered on the runtime's
/// next turn), the engine's clock pinned, and the original's singletons on demand. `None` in the process
/// that ran it there.
fn shell(test: &str, config: &str) -> Option<Shell> {
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        let cfg = home.path().join(".config/impulse-shell");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("config.json"), config).unwrap();
        testenv::run_in_own_process(test, testenv::home_vars(home.path()));
        return None;
    }
    let runs: Runs = Rc::default();
    let picks: Runs = Rc::default();
    let answer: Rc<RefCell<Option<String>>> = Rc::default();
    let (log, picked, chosen) = (runs.clone(), picks.clone(), answer.clone());
    qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
        if argv.first().is_some_and(|a| a == "WallpaperSwitch") {
            log.borrow_mut().push(argv.to_vec());
        }
        if argv.first().is_some_and(|a| a == "WallpaperPicker") {
            picked.borrow_mut().push(argv.to_vec());
            return chosen.borrow().as_ref().map(|f| format!("{f}\n"));
        }
        Some(String::new())
    })));
    // Midday: Hyprsunset's night schedule (hyprsunset_auto_theme sets its `shouldBeOn`) is off
    // and no minute tick re-evaluates it during the test.
    qml::builtins::set_clock(Some(testenv::local_time_ms(12, 0)));
    let r = ri();
    let mut rt = Runtime::new(Some(r.clone()));
    let dirs: Vec<PathBuf> = [
        "modules/common",
        "modules/common/functions",
        "modules/common/utils",
        "services",
        "",
    ]
    .iter()
    .map(|d| r.join(d))
    .collect();
    rt.load_singletons_lazy(&dirs);
    Some(Shell {
        rt,
        runs,
        picks,
        answer,
    })
}

impl Shell {
    /// One step of the loop, after the runs the hook answered have exited (they end on a
    /// pool thread).
    fn turn(&mut self) {
        self.rt.poll_native();
        while self.answered_running() {
            self.rt.wait_native();
            self.rt.poll_native();
        }
        self.rt.tick(1000.0);
    }

    /// Whether a wallpaper run or dialog the hook answered has not delivered its exit yet.
    fn answered_running(&self) -> bool {
        let n = self.rt.world.borrow().items.len();
        (0..n).any(|i| {
            let hooked = {
                let w = self.rt.world.borrow();
                !w.dead.contains(&i)
                    && w.items.get(i).is_some_and(|it| {
                        it.types
                            .iter()
                            .any(|t| *t == "WallpaperSwitch" || *t == "WallpaperPicker")
                    })
            };
            hooked && self.rt.get(i, "running").truthy()
        })
    }

    /// What the spawn hook gets for a run asked `r`.
    fn argv(&self, r: &Req) -> Vec<String> {
        let flag = |on: bool| on.to_string();
        vec![
            "WallpaperSwitch".to_string(),
            "run".into(),
            r.image.to_string(),
            r.mode.to_string(),
            r.scheme.to_string(),
            r.color.to_string(),
            flag(r.clear_color),
            flag(r.pick_color),
            flag(r.no_switch),
        ]
    }

    /// `fixture`'s `go(arg)` as a QML function of an object importing the original's modules.
    fn fixture(&mut self, body: &str) -> usize {
        let src = format!(
            "import QtQuick\nimport Quickshell\nimport ri\nimport ri.services\nimport ri.modules.common\nimport ri.modules.common.widgets\nimport ri.modules.common.models.quickToggles\n{body}"
        );
        let o = self.rt.load_str(&src).unwrap_or_else(|e| panic!("{e}"));
        self.turn();
        o
    }

    /// The one live item `pred` holds for.
    fn find(&self, pred: impl Fn(&Runtime, usize) -> bool) -> usize {
        let n = self.rt.world.borrow().items.len();
        let hits: Vec<usize> = (0..n)
            .filter(|&i| {
                let w = self.rt.world.borrow();
                w.items.get(i).is_some() && !w.dead.contains(&i)
            })
            .filter(|&i| pred(&self.rt, i))
            .collect();
        assert_eq!(hits.len(), 1, "{hits:?}");
        hits[0]
    }

    /// Whether the queue still has a run going or waiting; a tree without the queue never
    /// has one waiting.
    fn busy(&mut self) -> bool {
        let o = self.fixture(
            "QtObject { function go() { return typeof Switchwall !== 'undefined' && Switchwall.busy } }",
        );
        let v = self.rt.call_method(o, "go", vec![]).unwrap().unwrap();
        v.truthy()
    }

    /// Three requests from one site in a row (`ask(i)`): only the first runs at once, the
    /// last replaces the second while it waits and runs once the first has exited, and the
    /// queue is busy until that run has exited and the colors file has been read.
    fn check(&mut self, mut ask: impl FnMut(&mut Runtime, usize), want: [Req; 3]) {
        // Startup first (Config loading, FirstRunExperience's own first run): no time passes.
        for _ in 0..8 {
            self.turn();
        }
        assert!(!self.busy(), "{:?}", self.runs.borrow());
        self.runs.borrow_mut().clear();
        for i in 0..3 {
            ask(&mut self.rt, i);
        }
        assert_eq!(
            *self.runs.borrow(),
            vec![self.argv(&want[0])],
            "the second and third requests wait for the first run"
        );
        let mut turns = vec![];
        for _ in 0..8 {
            self.turn();
            let busy = self.busy();
            turns.push((self.runs.borrow().len(), busy));
        }
        assert_eq!(
            *self.runs.borrow(),
            vec![self.argv(&want[0]), self.argv(&want[2])],
            "only the last waiting request runs: {turns:?}"
        );
        let idle = turns.iter().position(|&(_, b)| !b);
        let idle = idle.unwrap_or_else(|| panic!("the queue never settled: {turns:?}"));
        assert_eq!(turns[idle].0, 2, "settled before the last run: {turns:?}");
        assert!(turns[idle..].iter().all(|&(_, b)| !b), "{turns:?}");
    }
}

/// A `WallpaperSwitch`'s properties, in the order of its `.qmltypes` entry.
#[derive(Clone, Copy, Default)]
struct Req<'a> {
    image: &'a str,
    mode: &'a str,
    scheme: &'a str,
    color: &'a str,
    clear_color: bool,
    pick_color: bool,
    no_switch: bool,
}

/// New colors in `mode` for the wallpaper in use.
fn colors(mode: &str) -> Req<'_> {
    Req {
        mode,
        no_switch: true,
        ..Default::default()
    }
}

/// A switch to `image` in `mode`.
fn switch<'a>(mode: &'a str, image: &'a str) -> Req<'a> {
    Req {
        mode,
        image,
        ..Default::default()
    }
}

const DARK: Req = Req {
    image: "",
    mode: "dark",
    scheme: "",
    color: "",
    clear_color: false,
    pick_color: false,
    no_switch: true,
};

fn click(rt: &mut Runtime, b: usize) {
    rt.emit(b, "clicked", &[], vec![]);
    rt.settle();
}

fn go(o: usize, arg: Option<&str>) -> impl FnMut(&mut Runtime, usize) {
    move |rt, _| {
        let args = arg.map(Value::str).into_iter().collect();
        rt.call_method(o, "go", args).unwrap().unwrap();
    }
}

impl Shell {
    /// What a light/dark toggle asks for: the mode the colors are not in now.
    fn other_mode(&mut self) -> &'static str {
        let o = self.fixture("QtObject { function go() { return Appearance.m3colors.darkmode } }");
        let dark = self.rt.call_method(o, "go", vec![]).unwrap().unwrap();
        if dark.truthy() { "light" } else { "dark" }
    }
}

#[test]
fn bar_dark_mode_button() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::bar_dark_mode_button"),
        r#"{"bar":{"utilButtons":{"showScreenSnip":false,"showScreenRecord":false,"showColorPicker":false,"showKeyboardToggle":false,"showMicToggle":false,"showDarkModeToggle":true,"showPerformanceProfileToggle":false}}}"#,
    ) else {
        return;
    };
    sh.rt
        .load_file(&ri().join("modules/rusty/bar/UtilButtons.qml"))
        .unwrap();
    sh.turn();
    let b = sh.find(|rt, i| {
        rt.has_handler(i, "onClicked") && rt.get(i, "buttonRadius").to_js_string() != "undefined"
    });
    let mode = sh.other_mode();
    let t = colors(mode);
    sh.check(move |rt, _| click(rt, b), [t, t, t]);
}

#[test]
fn dark_mode_quick_toggle() {
    let Some(mut sh) = shell(concat!(module_path!(), "::dark_mode_quick_toggle"), "{}\n") else {
        return;
    };
    let toggle = sh
        .rt
        .load_file(&ri().join("modules/common/models/quickToggles/DarkModeToggle.qml"))
        .unwrap();
    sh.turn();
    let mode = sh.other_mode();
    let t = colors(mode);
    sh.check(
        move |rt, _| {
            rt.call_method(toggle, "mainAction", vec![])
                .unwrap()
                .unwrap();
        },
        [t, t, t],
    );
}

#[test]
fn light_dark_preference_button() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::light_dark_preference_button"),
        "{}\n",
    ) else {
        return;
    };
    // `dark` is a required property: set it at creation, as welcome.qml does.
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("User.qml");
    std::fs::write(
        &user,
        "import ri.modules.common.widgets\nLightDarkPreferenceButton { dark: true }\n",
    )
    .unwrap();
    let b = sh.rt.load_file(&user).unwrap();
    sh.turn();
    sh.check(move |rt, _| click(rt, b), [DARK, DARK, DARK]);
}

#[test]
fn material_theme_toggle_light_dark() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::material_theme_toggle_light_dark"),
        "{}\n",
    ) else {
        return;
    };
    let o = sh.fixture("QtObject { function go() { MaterialThemeLoader.toggleLightDark() } }");
    let mode = sh.other_mode();
    let t = colors(mode);
    sh.check(go(o, None), [t, t, t]);
}

#[test]
fn wallpapers_apply() {
    let Some(mut sh) = shell(concat!(module_path!(), "::wallpapers_apply"), "{}\n") else {
        return;
    };
    let o = sh.fixture("QtObject { function go(p) { Wallpapers.apply(p, true) } }");
    let paths = ["/w/a.png", "/w/b c.png", "/w/d.png"];
    sh.check(
        move |rt, i| go(o, Some(paths[i]))(rt, i),
        [
            switch("dark", "/w/a.png"),
            switch("dark", "/w/b c.png"),
            switch("dark", "/w/d.png"),
        ],
    );
}

#[test]
fn wallpapers_fallback_picker() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::wallpapers_fallback_picker"),
        "{}\n",
    ) else {
        return;
    };
    let o = sh.fixture("QtObject { function go() { Wallpapers.openFallbackPicker(false) } }");
    sh.check_pick(go(o, None), "light");
}

#[test]
fn first_run_default_wallpaper() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::first_run_default_wallpaper"),
        "{}\n",
    ) else {
        return;
    };
    let o = sh.fixture(
        "QtObject { function go() { FirstRunExperience.handleFirstRun() } function wall() { return FirstRunExperience.defaultWallpaperPath } }",
    );
    let wall = sh.rt.call_method(o, "wall", vec![]).unwrap().unwrap();
    let wall = wall.to_js_string();
    let w = switch("", &wall);
    sh.check(go(o, None), [w, w, w]);
}

#[test]
fn hyprsunset_auto_theme() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::hyprsunset_auto_theme"),
        r#"{"appearance":{"autoDarkMode":true}}"#,
    ) else {
        return;
    };
    let o = sh.fixture(
        "QtObject { function go() { Hyprsunset.shouldBeOn = !Appearance.m3colors.darkmode; Hyprsunset.ensureThemeMode() } }",
    );
    let mode = sh.other_mode();
    let m = colors(mode);
    sh.check(go(o, None), [m, m, m]);
}

fn launcher(action: &str) -> String {
    format!(
        "QtObject {{ function go(a) {{ LauncherSearch.searchActions.find(x => x.action === \"{action}\").execute(a ?? \"\") }} }}"
    )
}

fn accent(color: &str) -> Req<'_> {
    Req {
        color,
        no_switch: true,
        ..Default::default()
    }
}

#[test]
fn launcher_accent_color() {
    let Some(mut sh) = shell(concat!(module_path!(), "::launcher_accent_color"), "{}\n") else {
        return;
    };
    let o = sh.fixture(&launcher("accentcolor"));
    let colors = ["#111111", "#222222", "#333333"];
    sh.check(
        move |rt, i| go(o, Some(colors[i]))(rt, i),
        [accent("#111111"), accent("#222222"), accent("#333333")],
    );
}

/// The accent action with "clear" removes the accent color; with anything that is no color
/// (here nothing) it asks for one picked on the screen.
#[test]
fn launcher_accent_clear_and_pick() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::launcher_accent_clear_and_pick"),
        "{}\n",
    ) else {
        return;
    };
    let o = sh.fixture(&launcher("accentcolor"));
    let asked = ["clear", "#222222", ""];
    let clear = Req {
        clear_color: true,
        no_switch: true,
        ..Default::default()
    };
    let pick = Req {
        pick_color: true,
        no_switch: true,
        ..Default::default()
    };
    sh.check(
        move |rt, i| go(o, Some(asked[i]))(rt, i),
        [clear, accent("#222222"), pick],
    );
}

#[test]
fn launcher_dark() {
    let Some(mut sh) = shell(concat!(module_path!(), "::launcher_dark"), "{}\n") else {
        return;
    };
    let o = sh.fixture(&launcher("dark"));
    sh.check(go(o, None), [DARK, DARK, DARK]);
}

#[test]
fn launcher_light() {
    let Some(mut sh) = shell(concat!(module_path!(), "::launcher_light"), "{}\n") else {
        return;
    };
    let o = sh.fixture(&launcher("light"));
    let m = colors("light");
    sh.check(go(o, None), [m, m, m]);
}

/// `/konachanwallpaper` starts the launcher's `RandomWallpaper` with the Konachan source.
#[test]
fn launcher_konachan_wallpaper() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::launcher_konachan_wallpaper"),
        "{}\n",
    ) else {
        return;
    };
    sh.settled();
    let seen: Runs = Rc::default();
    let log = seen.clone();
    qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
        if argv.first().is_some_and(|a| a == "RandomWallpaper") {
            log.borrow_mut().push(argv.to_vec());
        }
        Some(String::new())
    })));
    let o = sh.fixture(&launcher("konachanwallpaper"));
    go(o, None)(&mut sh.rt, 0);
    sh.turn();
    assert_eq!(
        *seen.borrow(),
        vec![vec!["RandomWallpaper", "run", "konachan"]]
    );
}

/// A file with a space and a trailing space: only the dialog's newline is dropped.
const PICKED: &str = "/home/u/Pictures/Wallpapers/a b.png ";

impl Shell {
    fn settled(&mut self) {
        // Startup first (Config loading, FirstRunExperience's own first run): no time passes.
        for _ in 0..8 {
            self.turn();
        }
        assert!(!self.busy(), "{:?}", self.runs.borrow());
        self.runs.borrow_mut().clear();
        self.picks.borrow_mut().clear();
    }

    /// `Switchwall.run(DARK)` and `Switchwall.picking` from QML.
    fn queue(&mut self) -> usize {
        self.fixture(
            "QtObject { function go() { Switchwall.run({ mode: \"dark\", noSwitch: true }) } function picking() { return Switchwall.picking } function busy() { return Switchwall.busy } }",
        )
    }

    /// `Switchwall.picking` / `Switchwall.busy` without a turn of the runtime.
    fn ask(&mut self, q: usize, what: &str) -> bool {
        self.rt
            .call_method(q, what, vec![])
            .unwrap()
            .unwrap()
            .truthy()
    }

    /// A wallpaper button pressed three times (`ask`) while the dialog it opened is up: one
    /// dialog and no run; a dark-mode request then runs at once, and once the dialog answers
    /// with a file, the switch to it (in `mode`) is queued behind that run.
    fn check_pick(&mut self, mut ask: impl FnMut(&mut Runtime, usize), mode: &str) {
        self.settled();
        let q = self.queue();
        *self.answer.borrow_mut() = Some(PICKED.into());
        for i in 0..3 {
            ask(&mut self.rt, i);
        }
        let picker = vec!["WallpaperPicker".to_string(), "run".into()];
        assert_eq!(*self.picks.borrow(), vec![picker], "one dialog at a time");
        assert!(self.runs.borrow().is_empty(), "{:?}", self.runs.borrow());
        assert!(self.ask(q, "picking"));
        assert!(!self.ask(q, "busy"), "an open dialog is not a run");
        go(q, None)(&mut self.rt, 0);
        assert_eq!(
            *self.runs.borrow(),
            vec![self.argv(&DARK)],
            "a request made while the dialog is open runs at once"
        );
        for _ in 0..8 {
            self.turn();
        }
        assert_eq!(
            *self.runs.borrow(),
            vec![self.argv(&DARK), self.argv(&switch(mode, PICKED))]
        );
        assert!(!self.ask(q, "picking"));
        assert!(!self.busy());
        assert_eq!(self.picks.borrow().len(), 1);
    }

    /// The dialog cancelled: nothing is queued, and the next request runs at once.
    fn check_cancel(&mut self, mut ask: impl FnMut(&mut Runtime, usize)) {
        self.settled();
        let q = self.queue();
        *self.answer.borrow_mut() = None;
        ask(&mut self.rt, 0);
        for _ in 0..8 {
            self.turn();
        }
        assert_eq!(self.picks.borrow().len(), 1);
        assert!(self.runs.borrow().is_empty(), "{:?}", self.runs.borrow());
        assert!(!self.ask(q, "picking"));
        assert!(!self.busy());
        go(q, None)(&mut self.rt, 0);
        assert_eq!(*self.runs.borrow(), vec![self.argv(&DARK)]);
    }
}

fn quick_config(sh: &mut Shell) -> usize {
    sh.rt
        .load_file(&ri().join("modules/settings/QuickConfig.qml"))
        .unwrap();
    sh.turn();
    sh.find(|rt, i| {
        rt.has_handler(i, "onClicked") && rt.get(i, "materialIcon").to_js_string() == "folder_open"
    })
}

fn welcome(sh: &mut Shell) -> usize {
    sh.rt.load_file(&ri().join("welcome.qml")).unwrap();
    sh.turn();
    sh.find(|rt, i| {
        rt.has_handler(i, "onClicked") && rt.get(i, "materialIcon").to_js_string() == "wallpaper"
    })
}

#[test]
fn quick_config_change_wallpaper() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::quick_config_change_wallpaper"),
        "{}\n",
    ) else {
        return;
    };
    let b = quick_config(&mut sh);
    sh.check_pick(move |rt, _| click(rt, b), "");
}

#[test]
fn quick_config_change_wallpaper_cancelled() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::quick_config_change_wallpaper_cancelled"),
        "{}\n",
    ) else {
        return;
    };
    let b = quick_config(&mut sh);
    sh.check_cancel(move |rt, _| click(rt, b));
}

#[test]
fn welcome_pick_wallpaper() {
    let Some(mut sh) = shell(concat!(module_path!(), "::welcome_pick_wallpaper"), "{}\n") else {
        return;
    };
    let b = welcome(&mut sh);
    sh.check_pick(move |rt, _| click(rt, b), "");
}

#[test]
fn welcome_pick_wallpaper_cancelled() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::welcome_pick_wallpaper_cancelled"),
        "{}\n",
    ) else {
        return;
    };
    let b = welcome(&mut sh);
    sh.check_cancel(move |rt, _| click(rt, b));
}

/// A request made in the same event batch as a run's exit, while an older request waits
/// (here from a `busyChanged` handler): the waiting one starts from the exit itself, so the
/// newer one, made as soon as the queue looks idle, waits for it, runs last and its colors stay.
#[test]
fn request_during_queued_start() {
    let Some(mut sh) = shell(
        concat!(module_path!(), "::request_during_queued_start"),
        "{}\n",
    ) else {
        return;
    };
    sh.settled();
    let o = sh.fixture(
        "QtObject { property bool armed: false; property var q: Connections { target: Switchwall; function onBusyChanged() { if (Switchwall.busy || !armed) return; armed = false; Switchwall.run({ mode: \"light\", noSwitch: true }) } } function go(a) { Switchwall.run({ mode: a, noSwitch: true }) } function arm() { armed = true } }",
    );
    go(o, Some("dark"))(&mut sh.rt, 0);
    go(o, Some("light"))(&mut sh.rt, 1);
    go(o, Some("dark"))(&mut sh.rt, 2);
    sh.rt.call_method(o, "arm", vec![]).unwrap().unwrap();
    for _ in 0..8 {
        sh.turn();
    }
    let light = colors("light");
    assert_eq!(
        *sh.runs.borrow(),
        vec![sh.argv(&DARK), sh.argv(&DARK), sh.argv(&light)],
        "the newest request runs last"
    );
    assert!(!sh.busy());
}
