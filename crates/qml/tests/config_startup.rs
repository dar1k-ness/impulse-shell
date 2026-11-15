//! The original's Config singleton leaves an existing config.json alone at startup, as Quickshell does.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

// User values for literal (`dock.enable: false`) and bound (`screenRecord.savePath:
// Directories.videos...`) initializers, nested sections, and a key Config.qml lacks.
const TEXT: &str = r#"{
    "background": {
        "wallpaperPath": "/home/u/Pictures/wall.png",
        "widgets": {
            "weather": {
                "enable": true
            }
        }
    },
    "bar": {
        "bottom": true,
        "weather": {
            "enable": true,
            "city": "Moscow",
            "enableGPS": false
        },
        "utilButtons": {
            "showScreenRecord": true
        }
    },
    "dock": {
        "enable": true,
        "height": 61,
        "hoverToReveal": false
    },
    "screenRecord": {
        "savePath": "/home/u/Clips"
    },
    "time": {
        "format": "h:mm AP"
    },
    "custom": 1
}
"#;

#[test]
fn startup_keeps_existing_config() {
    const TEST: &str = concat!(module_path!(), "::startup_keeps_existing_config");
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let cfg_dir = home.join(".config/impulse-shell");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(cfg_dir.join("config.json"), TEXT).unwrap();
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
        "import QtQuick\nimport ri.modules.common\nItem { property string wp: Config.options.background.wallpaperPath; property bool ready: Config.ready; property string city: Config.options.bar.weather.city; property bool gps: Config.options.bar.weather.enableGPS }\n",
    )
    .unwrap();
    let root = rt.load_file(&f).unwrap();
    let mut lp = Loop::new(&mut rt);
    lp.advance(&mut rt, 300.0);
    let wp = rt
        .world
        .borrow_mut()
        .read(root, "wp")
        .unwrap()
        .to_js_string();
    let ready = rt
        .world
        .borrow_mut()
        .read(root, "ready")
        .unwrap()
        .to_js_string();
    eprintln!("wp={wp} ready={ready}");
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), TEXT);
    assert_eq!(wp, "/home/u/Pictures/wall.png");
    let city = rt
        .world
        .borrow_mut()
        .read(root, "city")
        .unwrap()
        .to_js_string();
    let gps = rt
        .world
        .borrow_mut()
        .read(root, "gps")
        .unwrap()
        .to_js_string();
    assert_eq!((city.as_str(), gps.as_str()), ("Moscow", "false"));
}
