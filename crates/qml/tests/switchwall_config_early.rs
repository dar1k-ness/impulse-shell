//! A switchwall config edit made while Config is still loading its file (first run applies the
//! default wallpaper right at startup) must not be replaced by that load.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

#[test]
fn switchwall_edit_before_config_loaded_survives() {
    const TEST: &str = concat!(
        module_path!(),
        "::switchwall_edit_before_config_loaded_survives"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let cfg_dir = home.join(".config/rusty-impulse");
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
    let cfg = home.join(".config/rusty-impulse/config.json");
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
    // The running shell already routes switchwall's edits to Config...
    rt.poll_config_edits();
    // ...when switchwall (on its pool thread) sets the new wallpaper before Config has loaded.
    let c = cfg.clone();
    std::thread::spawn(move || {
        qml::shell_config::sys().set_config(&c, &["background", "wallpaperPath"], "/new.png")
    })
    .join()
    .unwrap();
    let mut lp = Loop::new(&mut rt);
    // The edit reaches the runtime before Config's FileView has loaded the file.
    rt.poll_config_edits();
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
    assert_eq!(read(&mut rt, "city"), "Moscow");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
    assert_eq!(v["background"]["wallpaperPath"], "/new.png");
    assert_eq!(v["bar"]["weather"]["city"], "Moscow");
}
