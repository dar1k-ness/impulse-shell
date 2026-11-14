//! The original's `services/Idle.qml` writes `inhibit: false` through `property alias inhibit:
//! idleInhibitor.enabled`, the IdleInhibitor declared below it. Started as after a login
//! (`states.json` kept from the previous session, with another `hyprlandInstanceSignature`),
//! the shell must keep that false: the "Keep awake" tile reads `Idle.inhibit`, `Idle` stores it
//! as `idle.inhibit` once `Persistent` is ready, and a true one makes a real
//! `zwp_idle_inhibitor_v1`, so the screen never locks. The original leaves all three off.
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use testenv::{DirWatch, Log};

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The tree's own singletons under a shell that reads what the tile reads
/// (`IdleInhibitorToggle.toggled: Idle.inhibit`).
const SHELL: &str = r#"import QtQuick
import Rustshell
import ri.services

ShellRoot {
    readonly property bool keepAwake: Idle.inhibit
    onKeepAwakeChanged: console.log("KEEPAWAKE " + keepAwake)
    Component.onCompleted: console.log("KEEPAWAKE " + keepAwake)
}
"#;

const PREVIOUS: &str = "previous-login";
const CURRENT: &str = "this-login";

fn tree(dir: &Path) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = dir.join("rusty-impulse");
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        if e.file_name() != "shell.qml" {
            std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
        }
    }
    std::fs::write(d.join("shell.qml"), SHELL).unwrap();
    d
}

fn states(file: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn keep_awake_stays_off_after_a_login() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let home = base.path().join("home");
    let state = home.join(".local/state/rustshell");
    std::fs::create_dir_all(&state).unwrap();
    let file = state.join("states.json");
    let before = serde_json::json!({
        "hyprlandInstanceSignature": PREVIOUS,
        "idle": { "inhibit": false },
    });
    std::fs::write(&file, before.to_string()).unwrap();
    let watch = DirWatch::new(&state);

    let (r, w) = std::io::pipe().unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
    c.arg("-p")
        .arg(tree(base.path()))
        .env("RI_INTERP", "1")
        .env("RI_LOG", "ri::stats=debug")
        .env("HYPRLAND_INSTANCE_SIGNATURE", CURRENT)
        .envs(testenv::home_vars(&home))
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w);
    let shell = Kill(c.spawn().unwrap());
    drop(c);
    let log = Log::capture(r);

    // `Persistent` stores this session's signature once it is ready, after `Idle` stored its
    // `inhibit` (the same `readyChanged`); the file is written 100 ms later.
    let stored = || states(&file).is_some_and(|s| s["hyprlandInstanceSignature"] == CURRENT);
    watch.until(|| stored() || log.closed());
    assert!(
        stored(),
        "rusty-impulse exited before it stored its state; log:\n{}",
        log.text()
    );
    // The shell printed the tile's lines before it stored the state, but what `log` has read
    // of them by now may be less: the shell is ended and its output read to the end.
    drop(shell);
    let text = log.until_closed();
    let tile = text
        .lines()
        .rev()
        .find_map(|l| l.find("KEEPAWAKE ").map(|i| l[i..].trim().to_string()));
    let saved = states(&file).unwrap()["idle"]["inhibit"].clone();
    let inhibitors = text.matches("idle inhibitor on").count();
    assert_eq!(
        (tile.as_deref(), saved, inhibitors),
        (Some("KEEPAWAKE false"), serde_json::Value::Bool(false), 0),
        "tile, idle.inhibit in states.json, inhibitors created; log:\n{text}"
    );
}
