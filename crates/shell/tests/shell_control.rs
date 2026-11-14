//! `Impulse/ShellControl` in the running shell (`rusty-impulse -p`), on a fixture shell with a
//! `settings.qml` next to it, in the headless sway:
//!
//! - `ShellControl.ipcCall` in the shell runs the shell's `IpcHandler` function;
//! - `ShellControl.openWindow` shows the file's window in a process of this binary, one however
//!   often it is asked for;
//! - `ShellControl.ipcCall` in that window's process runs the handler in the shell.
//!
//! No `qs` is run for any of it: the one on PATH here only records being run.
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::{Log, SwayWindows};

const MARK: &str = "SHELLCONTROL ";
const TITLE: &str = "shell control settings fixture";

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("shell.qml"),
        format!(
            r#"import QtQuick
import Rustshell
import Rustshell.Io
import Impulse

ShellRoot {{
    IpcHandler {{
        target: "fixture"
        function ipc(): void {{ ShellControl.ipcCall("sidebarLeft", "toggle") }}
        function window(): void {{ ShellControl.openWindow(Rustshell.shellPath("settings.qml")) }}
    }}
    IpcHandler {{
        target: "sidebarLeft"
        function toggle(): void {{ console.log("{MARK}sidebarLeft.toggle") }}
    }}
    IpcHandler {{
        target: "cheatsheet"
        function toggle(): void {{ console.log("{MARK}cheatsheet.toggle") }}
    }}
    Component.onCompleted: console.log("{MARK}ready")
}}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("settings.qml"),
        format!(
            r#"import QtQuick
import QtQuick.Controls
import Impulse

ApplicationWindow {{
    visible: true
    width: 240
    height: 120
    title: "{TITLE}"
    Component.onCompleted: ShellControl.ipcCall("cheatsheet", "toggle")
}}
"#
        ),
    )
    .unwrap();
    dir
}

/// A running shell whose marked log lines are read as they arrive.
struct Shell {
    child: Child,
    log: Arc<Log>,
    /// How much of the log [`Shell::wait_for`] consumed.
    taken: usize,
}

/// The marked lines of `log`.
fn marked(log: &str) -> impl Iterator<Item = String> + '_ {
    log.lines().filter_map(|l| {
        l.find(MARK)
            .map(|i| l[i + MARK.len()..].trim_end().to_string())
    })
}

impl Shell {
    fn spawn(mut cmd: Command) -> Self {
        let (r, w) = std::io::pipe().unwrap();
        let child = cmd
            .stdin(Stdio::null())
            .stdout(w.try_clone().unwrap())
            .stderr(w)
            .spawn()
            .expect("spawn shell");
        drop(cmd);
        Self {
            child,
            log: Log::capture(r),
            taken: 0,
        }
    }

    /// Blocks until the shell prints `what` marked; the marked lines since the last wait.
    fn wait_for(&mut self, what: &str) -> Vec<String> {
        let from = self.taken;
        if let Err(log) = self.log.wait(|s| marked(&s[from..]).any(|l| l == what)) {
            panic!("shell exited waiting for {what}:\n{log}");
        }
        let log = self.log.text();
        let upto = log.rfind('\n').map_or(from, |i| i + 1).max(from);
        self.taken = upto;
        marked(&log[from..upto]).collect()
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Pids whose command line mentions `needle` (the fixture's own temp path).
fn pids_with(needle: &str) -> Vec<i32> {
    let me = std::process::id() as i32;
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter(|&p| p != me)
        .filter(|p| {
            std::fs::read(format!("/proc/{p}/cmdline"))
                .is_ok_and(|c| String::from_utf8_lossy(&c).contains(needle))
        })
        .collect()
}

fn sway_sock() -> Option<PathBuf> {
    if let Some(s) = std::env::var_os("SWAYSOCK") {
        return Some(s.into());
    }
    let run = std::env::var_os("XDG_RUNTIME_DIR")?;
    std::fs::read_dir(run)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("sway-ipc.") && n.ends_with(".sock"))
        })
}

/// How many toplevels titled [`TITLE`] the compositor shows.
fn windows(sock: &Path) -> usize {
    let out = Command::new("swaymsg")
        .env("SWAYSOCK", sock)
        .args(["-t", "get_tree", "-r"])
        .output()
        .expect("swaymsg");
    String::from_utf8_lossy(&out.stdout)
        .matches(&format!("\"name\": \"{TITLE}\""))
        .count()
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn shell_control_calls_reach_the_shell_and_open_one_window() {
    if !testenv::require("swaymsg") {
        return;
    }
    let Some(sock) = sway_sock() else {
        return testenv::unavailable("a running sway (the headless compositor of tools/testenv)");
    };
    assert_eq!(windows(&sock), 0, "a stale fixture window is open");

    let dir = fixture();
    let needle = dir.path().to_string_lossy().into_owned();
    let settings = format!("{needle}/settings.qml");
    // A `qs` that only records being run.
    let fake = dir.path().join("bin");
    std::fs::create_dir_all(&fake).unwrap();
    let marker = dir.path().join("spawned-qs");
    testenv::install_script(
        &fake.join("qs"),
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\n", marker.display()),
    );
    let path = std::env::var("PATH").unwrap_or_default();
    let exe = Path::new(env!("CARGO_BIN_EXE_rusty-impulse"));
    let mut rs = Command::new(exe);
    rs.arg("-p")
        .arg(dir.path())
        .env("RI_INTERP", "1")
        .env("LANG", "C.UTF-8")
        .env("PATH", format!("{}:{path}", fake.display()));
    let win = SwayWindows::subscribe(&sock);
    let mut sh = Shell::spawn(rs);
    sh.wait_for("ready");
    let click = |function: &str| {
        let st = Command::new(exe)
            .arg("-p")
            .arg(dir.path())
            .args(["ipc", "call", "fixture", function])
            .env("LANG", "C.UTF-8")
            .stdout(Stdio::null())
            .status()
            .expect("ipc call");
        assert!(st.success(), "ipc call fixture {function} failed ({st})");
    };

    // In the shell: its own handler runs.
    click("ipc");
    assert_eq!(sh.wait_for("sidebarLeft.toggle"), ["sidebarLeft.toggle"]);

    // The window opens in its own process, and its call runs the handler in the shell.
    click("window");
    assert_eq!(sh.wait_for("cheatsheet.toggle"), ["cheatsheet.toggle"]);
    win.wait_until(|| windows(&sock) >= 1);
    let opened = pids_with(&settings);
    assert_eq!(opened.len(), 1, "{opened:?}");

    // Asked for again while it is open: the same window, no second process and no second
    // call from one. The shell handled this click before the next one.
    click("window");
    click("ipc");
    assert_eq!(sh.wait_for("sidebarLeft.toggle"), ["sidebarLeft.toggle"]);
    assert_eq!(pids_with(&settings), opened);
    assert_eq!(windows(&sock), 1);

    drop(sh);
    for p in pids_with(&needle) {
        testenv::kill_pid_and_wait(p);
    }
    win.wait_until(|| windows(&sock) == 0);
    assert!(
        !marker.exists(),
        "qs was run: {:?}",
        std::fs::read_to_string(&marker)
    );
}
