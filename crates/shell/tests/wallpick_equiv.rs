//! services/Wallpick.qml `proc`: the in-process job leaves the service in the state the
//! `rusty-impulse wallpick` command, run as a real process, would leave it in: the last line
//! the command writes as the status, a non-zero exit as a failure.
#![cfg(feature = "interp")]
use qml::runtime::Runtime;
use qml::value::Value;
#[path = "support/frames.rs"]
mod frames;
use std::path::{Path, PathBuf};

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// A fresh session under `home`, this process's own (see the test).
fn session(home: &Path) {
    let _ = std::fs::remove_dir_all(home);
    std::fs::create_dir_all(home).unwrap();
}

/// The state `rusty-impulse wallpick ARGS`, run in this session, leaves a service that reads
/// it as Wallpick.qml reads its job.
fn command(args: &[&str]) -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
        .arg("wallpick")
        .args(args)
        .output()
        .expect("run rusty-impulse wallpick");
    let last = |b: &[u8]| {
        String::from_utf8_lossy(b)
            .lines()
            .last()
            .map(str::to_string)
    };
    let status = last(&out.stderr)
        .or_else(|| last(&out.stdout))
        .unwrap_or_default();
    serde_json::json!([false, status, !out.status.success()]).to_string()
}

fn state(rt: &mut Runtime, root: usize) -> String {
    rt.call_method(root, "state", vec![])
        .unwrap()
        .unwrap()
        .to_js_string()
        .to_string()
}

/// Runs `action` through the service and waits for its Process to exit.
fn run(rt: &mut Runtime, frames: &mut frames::Frames, root: usize, action: &str) -> String {
    rt.call_method(root, "run", vec![Value::Str(action.into())]);
    rt.settle();
    frames.until(rt, &mut |_| {}, |rt| !state(rt, root).starts_with("[true"));
    state(rt, root)
}

#[test]
fn wallpick_proc_matches_the_rusty_impulse_wallpick_process() {
    // Both sides read HOME and the XDG directories: the test runs in a process of its own
    // with them set to a directory of its own.
    const TEST: &str = concat!(
        module_path!(),
        "::wallpick_proc_matches_the_rusty_impulse_wallpick_process"
    );
    if !testenv::in_own_process(TEST) {
        let home = std::env::temp_dir().join(format!("wallpick-equiv-{}", std::process::id()));
        testenv::run_in_own_process(
            TEST,
            [
                ("HOME", home.clone()),
                ("XDG_CONFIG_HOME", home.join(".config")),
                ("XDG_STATE_HOME", home.join(".local/state")),
                ("XDG_CACHE_HOME", home.join(".cache")),
            ],
        );
        let _ = std::fs::remove_dir_all(&home);
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let config = home.join(".config/rusty-impulse/config.json");
    let mut got = vec![];
    let write_config = || {
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, "{\"background\": {}}}").unwrap();
    };
    session(&home);
    let mut out = vec![command(&["reset-taste"]), command(&["like"])];
    write_config();
    out.push(command(&["pick"]));
    got.push(out);
    {
        session(&home);
        let mut rt = Runtime::new(Some(ri()));
        let mut frames = frames::Frames::new(&mut rt);
        rt.load_singletons_where(&[ri().join("services")], &|n| n == "Wallpick");
        rt.settle();
        let f = home.join("t/shell.qml");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(
            &f,
            "import QtQuick\nimport ri.services\nItem {\n\
             function run(a) { if (a === \"reset\") Wallpick.resetTaste(); \
             else if (a === \"like\") Wallpick.like(); else Wallpick.pick() }\n\
             function state() { return JSON.stringify([Wallpick.busy, Wallpick.status, Wallpick.failed]) }\n}\n",
        )
        .unwrap();
        let root = rt.load_file(&f).unwrap();
        rt.settle();
        let mut out = vec![
            run(&mut rt, &mut frames, root, "reset"),
            run(&mut rt, &mut frames, root, "like"),
        ];
        write_config();
        out.push(run(&mut rt, &mut frames, root, "pick"));
        got.push(out);
    }
    assert!(got[0][1].ends_with("true]"), "{:?}", got[0]);
    assert!(got[0][2].contains("config.json"), "{:?}", got[0]);
    assert_eq!(got[0], got[1]);
}
