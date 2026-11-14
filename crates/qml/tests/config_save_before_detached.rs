//! An option set right before `Rustshell.execDetached` is in config.json when the command
//! starts: QuickConfig.qml sets `palette.type` and runs `switchwall.sh --noswitch`, which reads
//! the type from the file. Config.qml saves on a timer, so the scheme switch applied the
//! previous type and only a second click took the new one.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

#[test]
fn detached_command_sees_option_set_before_it() {
    const TEST: &str = concat!(
        module_path!(),
        "::detached_command_sees_option_set_before_it"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let cfg_dir = home.join(".config/rusty-impulse");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(
            cfg_dir.join("config.json"),
            "{\n    \"appearance\": {\n        \"palette\": {\n            \"type\": \"auto\"\n        }\n    }\n}\n",
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
        "import QtQuick\nimport Quickshell\nimport ri.modules.common\nItem {\n function set(v) { Config.options.appearance.palette.type = v }\n function pick(v) { Config.options.appearance.palette.type = v; Rustshell.execDetached([\"true\"]) }\n}\n",
    )
    .unwrap();
    let root = rt.load_file(&f).unwrap();
    let mut lp = Loop::new(&mut rt);
    lp.advance(&mut rt, 300.0);
    let on_disk = || -> String {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        v["appearance"]["palette"]["type"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // Without a command, the save still waits for Config's write timer.
    rt.call_method(root, "set", vec![qml::value::Value::str("scheme-content")])
        .unwrap()
        .unwrap();
    assert_eq!(on_disk(), "auto");
    lp.advance(&mut rt, 300.0);
    assert_eq!(on_disk(), "scheme-content");
    // A command started in the same handler finds the new value already saved.
    rt.call_method(root, "pick", vec![qml::value::Value::str("scheme-neutral")])
        .unwrap()
        .unwrap();
    assert_eq!(on_disk(), "scheme-neutral");
    lp.advance(&mut rt, 300.0);
    assert_eq!(on_disk(), "scheme-neutral");
}
