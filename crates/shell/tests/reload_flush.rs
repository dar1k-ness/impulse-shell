//! `Rustshell.reload()` re-executes the shell, which discards the process image as an exit
//! does: an edit of `Config.options` still in Config.qml's delayed save must be written to
//! config.json first, so the re-executed shell loads it instead of the edit before.

use std::path::Path;
use std::process::{Child, Command, Stdio};

use testenv::Log;

const NATIVE: &str = env!("CARGO_BIN_EXE_rusty-impulse");

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The original's Config.qml in small: the adapter's edits are saved on a Timer after
/// `onAdapterUpdated` (the original waits 50 ms). Here the Timer never comes due during the test, so the
/// edit is still pending whenever the reload runs.
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
        function setAndReload(v: string): void {
            Config.options.mode = v;
            Rustshell.reload(true);
        }
    }
}
"#;

fn loaded(log: &str) -> Vec<&str> {
    log.lines()
        .filter_map(|l| l.split("LOADED mode=").nth(1))
        .collect()
}

#[test]
fn reload_writes_the_pending_config_save_first() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
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
    // The run dir moves to the temp dir: keep the compositor's socket reachable.
    let display = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
        .join(std::env::var_os("WAYLAND_DISPLAY").unwrap());
    let (rd, wr) = std::io::pipe().unwrap();
    let _shell = Kill(
        Command::new(NATIVE)
            .arg("-p")
            .arg(&dir)
            .env("RI_INTERP", "1")
            .env("WAYLAND_DISPLAY", &display)
            .env("XDG_RUNTIME_DIR", &run)
            .stdin(Stdio::null())
            .stdout(wr.try_clone().unwrap())
            .stderr(wr)
            .spawn()
            .unwrap(),
    );
    let log = Log::capture(rd);
    log.wait(|l| !loaded(l).is_empty())
        .unwrap_or_else(|l| panic!("the shell never loaded config.json:\n{l}"));
    assert_eq!(loaded(&log.text()), ["before"]);

    // Its answer comes before the reload: the shell re-executes after the turn that ran it.
    let call = Command::new(NATIVE)
        .arg("-p")
        .arg(&dir)
        .args(["ipc", "call", "t", "setAndReload", "after"])
        .env("WAYLAND_DISPLAY", &display)
        .env("XDG_RUNTIME_DIR", &run)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(call.status.success(), "{call:?}");
    log.wait(|l| loaded(l).len() == 2)
        .unwrap_or_else(|l| panic!("the re-executed shell never loaded config.json:\n{l}"));
    let text = log.text();
    assert_eq!(
        loaded(&text),
        ["before", "after"],
        "the reload lost the pending save:\n{text}"
    );
    let saved = std::fs::read_to_string(&config).unwrap();
    let v: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(v["mode"], "after", "{saved}");
}
