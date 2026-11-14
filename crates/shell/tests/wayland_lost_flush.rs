//! A shell whose Wayland connection broke (the compositor died) exits as Qt's
//! QWaylandDisplay::checkWaylandError does, with a warning and status 255, but first writes out
//! what the exit would discard: an edit of `Config.options` still in Config.qml's delayed save
//! (the original waits 50 ms before it writes config.json).

use std::process::{Command, Stdio};

use testenv::Log;

const NATIVE: &str = env!("CARGO_BIN_EXE_rusty-impulse");

/// The original's Config.qml in small: the adapter's edits are saved on a Timer after
/// `onAdapterUpdated`. Here the Timer never comes due during the test, so the edit is still
/// pending when the compositor dies.
const CONFIG: &str = r#"pragma Singleton
import QtQuick
import Rustshell
import Rustshell.Io
Singleton {
    id: root
    property string filePath: "@PATH@"
    property alias options: adapter
    Timer {
        id: fileWriteTimer
        interval: 86400000
        onTriggered: view.writeAdapter()
    }
    FileView {
        id: view
        path: root.filePath
        onAdapterUpdated: fileWriteTimer.restart()
        onLoaded: console.log("LOADED mode=" + adapter.mode)
        JsonAdapter {
            id: adapter
            property string mode: "unset"
        }
    }
}
"#;

const SHELL: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
ShellRoot {
    property string mode: Config.options.mode
    IpcHandler {
        target: "t"
        function set(v: string): string {
            Config.options.mode = v;
            return "SET";
        }
    }
}
"#;

#[test]
fn lost_compositor_writes_the_pending_config_save() {
    if !testenv::require("sway") {
        return;
    }
    let sway = testenv::Sway::start();
    let t = tempfile::tempdir().unwrap();
    let (dir, run) = (t.path().join("shell"), t.path().join("run"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&run).unwrap();
    let config = t.path().join("config.json");
    std::fs::write(&config, r#"{"mode": "before"}"#).unwrap();
    std::fs::write(
        dir.join("Config.qml"),
        CONFIG.replace("@PATH@", config.to_str().unwrap()),
    )
    .unwrap();
    std::fs::write(dir.join("shell.qml"), SHELL).unwrap();
    let (rd, wr) = std::io::pipe().unwrap();
    // The shell's own run dir (its IPC instance), the private sway's socket by absolute path.
    let mut child = sway
        .env(&mut Command::new(NATIVE))
        .arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .env("XDG_RUNTIME_DIR", &run)
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    let log = Log::capture(rd);
    log.wait(|l| l.contains("LOADED mode=before"))
        .unwrap_or_else(|l| panic!("the shell never loaded config.json:\n{l}"));
    let call = sway
        .env(&mut Command::new(NATIVE))
        .arg("-p")
        .arg(&dir)
        .args(["ipc", "call", "t", "set", "after"])
        .env("XDG_RUNTIME_DIR", &run)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(call.status.success(), "{call:?}");
    assert_eq!(String::from_utf8_lossy(&call.stdout).trim(), "SET");
    // The edit is in the delayed save, not in the file.
    let saved = std::fs::read_to_string(&config).unwrap();
    assert!(saved.contains("before"), "{saved}");

    drop(sway);
    let status = testenv::wait_exit(&mut child);
    // All the shell wrote, not what was read of it when the shell exited.
    let text = log.until_closed();
    assert_eq!(status.code(), Some(255), "{status:?}\n{text}");
    assert!(
        text.contains("The Wayland connection broke"),
        "no warning:\n{text}"
    );
    let saved = std::fs::read_to_string(&config).unwrap();
    let v: serde_json::Value = serde_json::from_str(&saved)
        .unwrap_or_else(|e| panic!("config.json is no JSON ({e}): {saved}\n{text}"));
    assert_eq!(
        v["mode"], "after",
        "the pending save was lost:\n{saved}\n{text}"
    );
}
