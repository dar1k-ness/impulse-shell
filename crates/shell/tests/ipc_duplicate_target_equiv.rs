//! Two panels of a family declare `IpcHandler { target: "search" }` (the original's Waffle family: the
//! Start menu and the Task View, `panelFamilies/WaffleFamily.qml`). Quickshell uses the handler
//! registered first and its LazyLoaders, all active from the start, load last declared first,
//! so `ipc call search toggle` reaches the panel declared last (the Task View). The real
//! quickshell running the fixture vs the native shell (`rusty-impulse -p`): the same panel must answer.
use std::path::Path;
use std::process::{Child, Command, Stdio};

use testenv::Log;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The shape of the original's shell.qml: a family loader that turns on when the config is ready, its
/// panels each behind a `PanelLoader` whose `active` is already true when the family is made.
const SHELL: &str = r#"import QtQuick
import Quickshell
import Quickshell.Io

ShellRoot {
    id: root
    property bool ready: false
    component PanelLoader: LazyLoader {
        property bool extraCondition: true
        active: root.ready && extraCondition
    }
    component Panel: Scope {
        id: panel
        required property string name
        Component.onCompleted: console.log("PANEL " + name)
        IpcHandler {
            target: "search"
            function toggle(): void { console.log("IPCGOT " + panel.name) }
        }
    }
    component Family: Scope {
        PanelLoader { component: Panel { name: "startMenu" } }
        PanelLoader { component: Panel { name: "sessionScreen" } }
        PanelLoader { component: Panel { name: "taskView" } }
    }
    LazyLoader {
        active: root.ready
        component: Family {}
    }
    IpcHandler {
        target: "end"
        function mark(): void { console.log("IPCEND") }
    }
    Timer {
        interval: 50
        running: true
        onTriggered: root.ready = true
    }
}
"#;

/// The lines of `log` carrying `tag`, from the tag on, in order.
fn tagged(log: &str, tag: &str) -> Vec<String> {
    log.lines()
        .filter_map(|l| l.find(tag).map(|i| l[i..].trim().to_string()))
        .collect()
}

/// Runs the fixture with `bin`, calls `search toggle` once its three panels are loaded and
/// returns the order they loaded in followed by the panel that answered.
fn answered(bin: &str, dir: &Path) -> Vec<String> {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("shell.qml"), SHELL).unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let mut c = Command::new(bin);
    c.arg("-p")
        .arg(dir)
        .env("RI_INTERP", "1")
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w);
    let _k = Kill(c.spawn().unwrap());
    drop(c);
    let log = Log::capture(r);
    if let Err(s) = log.wait(|s| tagged(s, "PANEL ").len() >= 3) {
        panic!("{bin}: exited before the panels loaded; log:\n{s}");
    }
    // The second call is answered after the first: every line of the first call's handlers
    // stands in the log before `IPCEND`, one handler or two.
    for call in [["search", "toggle"], ["end", "mark"]] {
        let st = Command::new(bin)
            .arg("-p")
            .arg(dir)
            .args(["ipc", "call"])
            .args(call)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .expect("ipc call");
        assert!(st.success(), "{bin}: ipc call {call:?} failed ({st})");
    }
    if let Err(s) = log.wait(|s| s.contains("IPCEND")) {
        panic!("{bin}: exited before the calls were answered; log:\n{s}");
    }
    let text = log.text();
    assert!(
        !tagged(&text, "IPCGOT ").is_empty(),
        "{bin}: no handler ran; log:\n{text}"
    );
    let mut got = tagged(&text, "PANEL ");
    got.extend(tagged(&text, "IPCGOT "));
    got
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn duplicate_ipc_target_reaches_the_same_panel_as_quickshell() {
    if !testenv::require("qs") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let orig = answered("qs", &base.path().join("orig"));
    let native = answered(
        env!("CARGO_BIN_EXE_rusty-impulse"),
        &base.path().join("native"),
    );
    assert_eq!(native, orig);
    assert_eq!(orig.last().map(String::as_str), Some("IPCGOT taskView"));
}
