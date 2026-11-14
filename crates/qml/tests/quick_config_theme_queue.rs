//! QuickConfig.qml's light/dark tiles: one WallpaperSwitch run at a time, in click order. A click
//! while a run is going waits for it, so the last pick's colors are the ones applied; the picked
//! tile shows until that run has ended and the colors file has been read again, not for a fixed
//! 5 s. Two detached runs per double click raced, and the first could finish last.
use qml::runtime::Runtime;
use qml::value::Value;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Waits until the WallpaperSwitch runs the spawn hook answered have exited: they end on a
/// pool thread, the hook only decides what they print.
fn answer_runs(rt: &mut Runtime) {
    let running = |rt: &Runtime| {
        let n = rt.world.borrow().items.len();
        (0..n).any(|i| {
            let switch = {
                let w = rt.world.borrow();
                !w.dead.contains(&i)
                    && w.items
                        .get(i)
                        .is_some_and(|it| it.types.iter().any(|t| *t == "WallpaperSwitch"))
            };
            switch && rt.get(i, "running").truthy()
        })
    };
    while running(rt) {
        rt.wait_native();
        rt.poll_native();
    }
}

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

#[test]
fn theme_runs_are_serialized_and_settle_on_exit() {
    const TEST: &str = concat!(
        module_path!(),
        "::theme_runs_are_serialized_and_settle_on_exit"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join(".config/rusty-impulse")).unwrap();
        std::fs::write(home.join(".config/rusty-impulse/config.json"), "{}\n").unwrap();
        testenv::run_in_own_process(TEST, testenv::home_vars(home));
        return;
    }
    // Every run ends at once (its exit is queued); the runtime delivers it on its next turn.
    let runs = Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
    let log = runs.clone();
    qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
        if argv.first().is_some_and(|a| a == "WallpaperSwitch") {
            log.borrow_mut().push(argv.to_vec());
        }
        Some(String::new())
    })));
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
    let page = rt
        .load_file(&r.join("modules/settings/QuickConfig.qml"))
        .unwrap();
    rt.poll_native();
    answer_runs(&mut rt);
    rt.tick(1000.0);
    // The type's properties: image, mode, schemeType, color, clearColor, pickColor, noSwitch.
    let run = |mode: &str| {
        [
            "WallpaperSwitch",
            "run",
            "",
            mode,
            "",
            "",
            "false",
            "false",
            "true",
        ]
        .map(String::from)
        .to_vec()
    };
    let pick = |rt: &mut Runtime, mode: &str| {
        rt.call_method(page, "setThemeMode", vec![Value::str(mode)])
            .unwrap()
            .unwrap();
    };
    // A double click: light, then dark while light still runs.
    pick(&mut rt, "light");
    pick(&mut rt, "dark");
    assert_eq!(*runs.borrow(), vec![run("light")], "dark waits for light");
    assert_eq!(rt.get(page, "pendingMode").to_js_string(), "dark");
    let mut turns = vec![];
    // Turns of the loop with the animation clock standing still: no time has to pass.
    for _ in 0..6 {
        rt.poll_native();
        answer_runs(&mut rt);
        rt.tick(1000.0);
        turns.push((
            runs.borrow().len(),
            rt.get(page, "pendingMode").to_js_string(),
        ));
    }
    assert_eq!(*runs.borrow(), vec![run("light"), run("dark")], "{turns:?}");
    // The dark tile stays picked while any run is pending, then follows the colors file.
    let settled = turns.iter().position(|(_, m)| m.is_empty());
    let settled = settled.unwrap_or_else(|| panic!("pendingMode never settled: {turns:?}"));
    assert!(
        turns[..settled].iter().all(|(_, m)| m == "dark"),
        "{turns:?}"
    );
    assert_eq!(
        turns[settled].0, 2,
        "settled before the last run: {turns:?}"
    );
    qml::native::set_spawn_hook(None);
}

const DARK: &str = r##"{"background": "#141218", "primary": "#d0bcff"}"##;
const LIGHT: &str = r##"{"background": "#fef7ff", "primary": "#6750a4"}"##;

/// A theme pick fades nothing by itself: the picked tile shows at once and nothing is armed
/// while switchwall.sh runs, so whatever changes meanwhile draws as it is. Every window fades
/// once, in the loop turn the new colors are applied, and a run that writes the colors already
/// shown fades nothing.
#[test]
fn theme_pick_fades_only_the_color_change() {
    const TEST: &str = concat!(module_path!(), "::theme_pick_fades_only_the_color_change");
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join(".config/rusty-impulse")).unwrap();
        std::fs::write(home.join(".config/rusty-impulse/config.json"), "{}\n").unwrap();
        testenv::run_in_own_process(TEST, testenv::home_vars(home));
        return;
    }
    let colors: Rc<RefCell<Option<PathBuf>>> = Rc::default();
    let target = colors.clone();
    // A run writes the colors of its mode and ends at once, as switchwall.sh does.
    qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
        if argv.first().is_some_and(|a| a == "WallpaperSwitch") {
            let dark = argv.get(2).is_some_and(|mode| mode == "dark");
            let path = target.borrow().clone().expect("the colors file's path");
            std::fs::write(path, if dark { DARK } else { LIGHT }).unwrap();
        }
        Some(String::new())
    })));
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
    let probe = rt
        .load_str(
            "import QtQuick\nimport ri.services\nimport ri.modules.common\nItem {\n property var loader: MaterialThemeLoader\n property string path: Directories.generatedMaterialThemePath\n property bool dark: Appearance.m3colors.darkmode\n}\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let path = PathBuf::from(rt.get(probe, "path").to_js_string());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, DARK).unwrap();
    *colors.borrow_mut() = Some(path);
    let Value::Item(loader) = rt.get(probe, "loader") else {
        panic!("no MaterialThemeLoader: {:?}", rt.errors())
    };
    rt.call_method(loader, "reapplyTheme", vec![])
        .unwrap()
        .unwrap();
    let page = rt
        .load_file(&r.join("modules/settings/QuickConfig.qml"))
        .unwrap();
    let turn = |rt: &mut Runtime| {
        rt.poll_native();
        answer_runs(rt);
        rt.tick(1000.0);
    };
    for _ in 0..6 {
        turn(&mut rt);
    }
    assert!(
        rt.get(loader, "loadedOnce").truthy() && rt.get(probe, "dark").truthy(),
        "the dark colors file was not applied: {:?}",
        rt.errors()
    );
    let unarmed = || -> Option<bool> { panic!("a frame was compared with no fade armed") };
    let mut window = qml::native::FadeArm::new();
    let gen0 = qml::native::crossfade_gen();
    let pick = |rt: &mut Runtime, mode: &str| {
        rt.call_method(page, "setThemeMode", vec![Value::str(mode)])
            .unwrap()
            .unwrap();
    };

    pick(&mut rt, "light");
    assert!(
        !rt.get(page, "shownDark").truthy(),
        "the tile shows the pick"
    );
    // The click's own turn redraws the tile: it shows at once.
    window.observe(true);
    assert_eq!(
        qml::native::crossfade_gen(),
        gen0,
        "the pick asked for a fade"
    );
    assert_eq!(window.take(unarmed), None);
    let mut turns = vec![];
    for _ in 0..8 {
        turn(&mut rt);
        let applied = !rt.get(probe, "dark").truthy();
        turns.push((applied, qml::native::crossfade_gen() - gen0));
        if !applied {
            // A turn before the colors arrive: whatever it redraws is not faded.
            window.observe(true);
            assert_eq!(window.take(unarmed), None, "{turns:?}");
        } else if turns.iter().filter(|t| t.0).count() == 1 {
            window.observe(true);
            assert_eq!(
                window.take(|| Some(true)),
                Some(std::time::Duration::from_millis(400)),
                "{turns:?}"
            );
        }
    }
    let applied = turns.iter().position(|t| t.0);
    let applied = applied.unwrap_or_else(|| panic!("light never applied: {turns:?}"));
    assert!(
        turns[..applied].iter().all(|t| t.1 == 0) && turns[applied..].iter().all(|t| t.1 == 1),
        "one fade, in the turn the colors change: {turns:?}"
    );
    assert_eq!(rt.get(page, "pendingMode").to_js_string(), "", "{turns:?}");

    // The same mode again: the run rewrites the colors already shown.
    let gen1 = qml::native::crossfade_gen();
    pick(&mut rt, "light");
    for _ in 0..8 {
        turn(&mut rt);
    }
    assert_eq!(rt.get(page, "pendingMode").to_js_string(), "");
    assert_eq!(qml::native::crossfade_gen(), gen1, "unchanged colors faded");
    window.observe(true);
    assert_eq!(window.take(unarmed), None);
    qml::native::set_spawn_hook(None);
}
