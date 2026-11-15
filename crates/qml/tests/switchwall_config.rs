//! switchwall's config edits go through the running shell's Config singleton: written to
//! config.json directly (the original `jq … > tmp && mv`), Config's pending delayed save wrote
//! its stale `wallpaperPath` back over the new one.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

#[test]
fn switchwall_edit_survives_pending_config_save() {
    const TEST: &str = concat!(
        module_path!(),
        "::switchwall_edit_survives_pending_config_save"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let cfg_dir = home.join(".config/impulse-shell");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(
            cfg_dir.join("config.json"),
            r#"{
    "background": {
        "wallpaperPath": "/old.png"
    },
    "bar": {
        "weather": {
            "city": "Moscow"
        }
    }
}
"#,
        )
        .unwrap();
        testenv::run_in_own_process(TEST, testenv::home_vars(home));
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let cfg = home.join(".config/impulse-shell/config.json");
    let mut rt = Runtime::new(Some(ri()));
    let keep = ["Directories", "Config", "FileUtils", "StringUtils"];
    let dirs = ["modules/common", "modules/common/functions"].map(|d| ri().join(d));
    rt.load_singletons_where(&dirs, &|n| keep.contains(&n));
    let f = home.join("main.qml");
    std::fs::write(
        &f,
        "import QtQuick\nimport ri.modules.common\nItem { property string wp: Config.options.background.wallpaperPath; property string city: Config.options.bar.weather.city; function edit() { Config.options.bar.weather.city = \"Paris\" } }\n",
    )
    .unwrap();
    let root = rt.load_file(&f).unwrap();
    let mut lp = Loop::new(&mut rt);
    lp.advance(&mut rt, 300.0);
    // A user edit whose save is still pending in Config's write timer...
    rt.call_method(root, "edit", vec![]).unwrap().unwrap();
    // ...when switchwall (on its pool thread) sets the new wallpaper.
    let c = cfg.clone();
    std::thread::spawn(move || {
        qml::shell_config::sys().set_config(&c, &["background", "wallpaperPath"], "/new.png")
    })
    .join()
    .unwrap();
    lp.advance(&mut rt, 600.0);
    let read = |rt: &mut Runtime, k: &str| {
        rt.world
            .borrow_mut()
            .read(root, k)
            .unwrap()
            .to_js_string()
            .to_string()
    };
    assert_eq!(read(&mut rt, "wp"), "/new.png");
    assert_eq!(read(&mut rt, "city"), "Paris");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
    assert_eq!(v["background"]["wallpaperPath"], "/new.png");
    assert_eq!(v["bar"]["weather"]["city"], "Paris");
}

/// Config loaded from a file with a wallpaper and a palette type, a root item showing both and
/// a function that edits the palette type as a settings tile does.
fn two_process_setup(test: &str) -> Option<(Runtime, usize, PathBuf)> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let cfg_dir = home.join(".config/impulse-shell");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(
            cfg_dir.join("config.json"),
            r#"{
    "appearance": {
        "palette": {
            "type": "auto"
        }
    },
    "background": {
        "wallpaperPath": "/old.png"
    }
}
"#,
        )
        .unwrap();
        testenv::run_in_own_process(test, testenv::home_vars(home));
        return None;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let cfg = home.join(".config/impulse-shell/config.json");
    let mut rt = Runtime::new(Some(ri()));
    let keep = ["Directories", "Config", "FileUtils", "StringUtils"];
    let dirs = ["modules/common", "modules/common/functions"].map(|d| ri().join(d));
    rt.load_singletons_where(&dirs, &|n| keep.contains(&n));
    let f = home.join("main.qml");
    std::fs::write(
        &f,
        "import QtQuick\nimport ri.modules.common\nItem { property string wp: Config.options.background.wallpaperPath; property string type: Config.options.appearance.palette.type; function edit() { Config.options.appearance.palette.type = \"scheme-rainbow\" } }\n",
    )
    .unwrap();
    let root = rt.load_file(&f).unwrap();
    Some((rt, root, cfg))
}

/// What another shell process (the settings window has its own Config) does when it saves:
/// the whole file as that process holds it, replaced by rename.
fn other_process_saves(cfg: &std::path::Path, group: &str, sub: Option<&str>, key: &str, v: &str) {
    let mut j: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(cfg).unwrap()).unwrap();
    let o = match sub {
        Some(s) => &mut j[group][s],
        None => &mut j[group],
    };
    o[key] = v.into();
    let tmp = cfg.with_extension("other");
    std::fs::write(&tmp, serde_json::to_string_pretty(&j).unwrap()).unwrap();
    std::fs::rename(&tmp, cfg).unwrap();
}

fn assert_both(rt: &mut Runtime, root: usize, cfg: &std::path::Path) {
    let read = |k: &str| {
        rt.world
            .borrow_mut()
            .read(root, k)
            .unwrap()
            .to_js_string()
            .to_string()
    };
    let shown = (read("wp"), read("type"));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(cfg).unwrap()).unwrap();
    let file = (
        v["background"]["wallpaperPath"]
            .as_str()
            .unwrap()
            .to_string(),
        v["appearance"]["palette"]["type"]
            .as_str()
            .unwrap()
            .to_string(),
    );
    let want = ("/new.png".to_string(), "scheme-rainbow".to_string());
    assert_eq!((file, shown), (want.clone(), want));
}

/// The settings window saved a palette type the shell has not read yet (its reload waits in a
/// timer) when switchwall's new wallpaper reaches the shell's Config: the shell's save carried
/// its old palette type back over the one just saved.
#[test]
fn switchwall_edit_keeps_what_another_process_just_saved() {
    const TEST: &str = concat!(
        module_path!(),
        "::switchwall_edit_keeps_what_another_process_just_saved"
    );
    let Some((mut rt, root, cfg)) = two_process_setup(TEST) else {
        return;
    };
    let mut lp = Loop::new(&mut rt);
    lp.advance(&mut rt, 300.0);
    other_process_saves(
        &cfg,
        "appearance",
        Some("palette"),
        "type",
        "scheme-rainbow",
    );
    let c = cfg.clone();
    std::thread::spawn(move || {
        qml::shell_config::sys().set_config(&c, &["background", "wallpaperPath"], "/new.png")
    })
    .join()
    .unwrap();
    lp.advance(&mut rt, 600.0);
    assert_both(&mut rt, root, &cfg);
}

/// The same from the settings window's side: its own edit waits in the write timer when the
/// shell saves a new wallpaper.
#[test]
fn pending_edit_keeps_what_another_process_just_saved() {
    const TEST: &str = concat!(
        module_path!(),
        "::pending_edit_keeps_what_another_process_just_saved"
    );
    let Some((mut rt, root, cfg)) = two_process_setup(TEST) else {
        return;
    };
    let mut lp = Loop::new(&mut rt);
    lp.advance(&mut rt, 300.0);
    rt.call_method(root, "edit", vec![]).unwrap().unwrap();
    other_process_saves(&cfg, "background", None, "wallpaperPath", "/new.png");
    lp.advance(&mut rt, 600.0);
    assert_both(&mut rt, root, &cfg);
}
