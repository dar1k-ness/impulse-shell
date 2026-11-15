//! The main loop's sync passes (popups, session lock, keyboard modes, focus grabs, idle
//! inhibitors, shortcuts, screencopy) run on the changes they read, checked against a headless
//! sway: none while the loop is awake with nothing changed, and a compositor event that changes
//! what they read (a surface configured) still takes effect with nothing else waking the loop.
//!
//! Each test runs its own sway: the shared one (WAYLAND_DISPLAY) also serves the test binaries
//! nextest runs meanwhile, whose output hotplug, scale and power changes reach every client and
//! are real sync-pass triggers, and whose frame and wakeup counts this test's panel would skew.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;

fn sway() -> bool {
    testenv::require("sway")
}

#[test]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "awake_loop_without_changes_runs_no_sync_passes",
        &[("PATH", Some("/nonexistent"))],
    );
}

/// impulse-shell running `shell.qml` with its Wayland traffic and `ri::sync` passes logged.
struct Shell {
    child: Child,
    _sway: testenv::Sway,
    _dir: tempfile::TempDir,
    log: Arc<Log>,
}

impl Shell {
    fn start(qml: &str) -> Self {
        let sway = testenv::Sway::start();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("shell.qml"), qml).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        let xdg = dir.path().join("xdg");
        let child = Command::new(env!("CARGO_BIN_EXE_impulse-shell"))
            .arg("-p")
            .arg(dir.path())
            .env("WAYLAND_DISPLAY", sway.display())
            .env("XDG_RUNTIME_DIR", sway.runtime_dir())
            .env_remove("SWAYSOCK")
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("RI_LOG", "ri::sync")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdout(Stdio::null())
            .stderr(w)
            .spawn()
            .unwrap();
        Shell {
            child,
            _sway: sway,
            _dir: dir,
            log: Log::capture(r),
        }
    }

    fn text(&self) -> String {
        self.log.text().replace('@', "#")
    }

    /// Blocks until `f(log)` holds, on each line impulse-shell prints; the log if it exits first.
    fn wait(&self, what: &str, f: impl Fn(&str) -> bool) -> String {
        match self.log.wait(|s| f(&s.replace('@', "#"))) {
            Ok(()) => self.text(),
            Err(s) => panic!(
                "impulse-shell exited before {what}; its log tail:\n{}",
                tail(&s.replace('@', "#"))
            ),
        }
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tail(s: &str) -> String {
    let keep = [
        "sync pass",
        "TICK",
        "qml:",
        "get_popup",
        "get_layer_surface",
        "create_inhibitor",
        "get_xdg_surface",
        "configure(",
    ];
    let lines: Vec<&str> = s
        .lines()
        .filter(|l| keep.iter().any(|k| l.contains(k)))
        .collect();
    lines[lines.len().saturating_sub(60)..].join("\n")
}

fn count(s: &str, pat: &str) -> usize {
    s.lines().filter(|l| l.contains(pat)).count()
}

/// The argument lists of every `-> iface#N.request(...)` the client sent.
fn requests<'a>(log: &'a str, req: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    log.lines().filter_map(move |l| {
        let (_, rest) = l.split_once(" -> ")?;
        let (_, call) = rest.split_once('.')?;
        call.strip_prefix(req)?.strip_prefix('(')
    })
}

/// A Timer keeps the loop running full turns while nothing it reads changes (its handler
/// writes no property): the sync passes must not run at all, not even as a periodic pass.
#[test]
fn awake_loop_without_changes_runs_no_sync_passes() {
    if !sway() {
        return;
    }
    let sh = Shell::start(
        r#"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 200
        implicitHeight: 40
        color: "green"
    }
    Timer {
        interval: 40
        repeat: true
        running: true
        onTriggered: console.log("TICK")
    }
}
"#,
    );
    // The window is counted in timer turns after the panel's configure: startup writes (layout,
    // the first frame) settle within the first SETTLE turns, then WINDOW turns (1.5 s at 40 ms)
    // must run no sync pass.
    const SETTLE: usize = 12;
    const WINDOW: usize = 37;
    // Byte offsets just past each TICK line after the panel's layer-surface configure.
    let ticks_after = |s: &str| -> Vec<usize> {
        let mut off = 0;
        let mut configured = false;
        let mut ends = vec![];
        for l in s.split_inclusive('\n') {
            off += l.len();
            configured |= l.contains("zwlr_layer_surface_v1#") && l.contains(".configure(");
            if configured && l.contains("TICK") && l.ends_with('\n') {
                ends.push(off);
            }
        }
        ends
    };
    let log = sh.wait(
        "the panel configured and SETTLE + WINDOW timer turns run",
        |s| ticks_after(s).len() > SETTLE + WINDOW,
    );
    let ticks = ticks_after(&log);
    let window = &log[ticks[SETTLE]..ticks[SETTLE + WINDOW]];
    let passes = count(window, "sync pass");
    assert_eq!(
        passes,
        0,
        "{passes} sync passes in {WINDOW} timer turns with nothing changed:\n{}",
        tail(window)
    );
}

/// Quickshell `ScreencopyView.paintCursor` is the `overlay_cursor` of the capture request
/// (WlrScreencopyContext: `capture_output(paintCursors ? 1 : 0, output)`); it is off by default.
#[test]
fn screencopy_view_asks_for_the_cursor_when_paint_cursor_is_set() {
    if !sway() {
        return;
    }
    let sh = Shell::start(
        r#"import QtQuick
import Rustshell
import Rustshell.Wayland
ShellRoot {
    PanelWindow {
        id: panel
        anchors { top: true; left: true }
        implicitWidth: 300
        implicitHeight: 60
        ScreencopyView {
            width: 100; height: 60
            captureSource: panel.screen
            paintCursor: true
        }
        ScreencopyView {
            x: 100; width: 100; height: 60
            captureSource: panel.screen
        }
    }
}
"#,
    );
    let overlay = |s: &str| -> Vec<String> {
        let mut v: Vec<String> = requests(s, "capture_output")
            .filter_map(|a| a.split(", ").nth(1))
            .map(str::to_owned)
            .collect();
        v.sort();
        v
    };
    let s = sh.wait("the capture of both views", |s| overlay(s).len() == 2);
    assert_eq!(
        overlay(&s),
        ["0", "1"],
        "overlay_cursor of the two capture_output requests:\n{}",
        requests(&s, "capture_output")
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// An `IdleInhibitor` whose window is a popup has no inhibitor until the popup is configured,
/// then gets one on the popup's surface: the configure alone (no timer, nothing else changing)
/// re-runs the sync.
#[test]
fn inhibitor_moves_to_its_window_once_configured() {
    if !sway() {
        return;
    }
    let sh = Shell::start(
        r#"import QtQuick
import Rustshell
import Rustshell.Wayland
ShellRoot {
    PanelWindow {
        id: panel
        anchors { top: true; left: true }
        implicitWidth: 300
        implicitHeight: 60
        color: "green"
        PopupWindow {
            id: pop
            anchor.window: panel
            anchor.rect.y: 60
            visible: true
            implicitWidth: 120
            implicitHeight: 80
            color: "yellow"
        }
    }
    IdleInhibitor { window: pop; enabled: true }
}
"#,
    );
    // wl_surface#N of the popup, from `get_xdg_surface(new id xdg_surface#M, wl_surface#N)`
    // and `xdg_surface#M.get_popup(...)`.
    let popup_surface = |s: &str| -> Option<String> {
        let xs = s.lines().find_map(|l| {
            let (_, rest) = l.split_once(" -> ")?;
            let (obj, call) = rest.split_once('.')?;
            call.starts_with("get_popup(")
                .then(|| obj.trim().to_string())
                .filter(|o| o.starts_with("xdg_surface#"))
        })?;
        requests(s, "get_xdg_surface").find_map(|a| {
            let (id, surf) = a.split_once(", ")?;
            (id.trim_start_matches("new id ") == xs).then(|| surf.trim_end_matches(')').to_string())
        })
    };
    let last_inhibited = |s: &str| -> Option<String> {
        requests(s, "create_inhibitor")
            .last()
            .and_then(|a| a.split(", ").nth(1))
            .map(|x| x.trim_end_matches(')').to_string())
    };
    let s = sh.wait("an inhibitor on the popup's surface", |s| {
        popup_surface(s).is_some_and(|p| last_inhibited(s).as_deref() == Some(p.as_str()))
    });
    assert!(
        s.lines()
            .any(|l| l.contains("xdg_popup#") && l.contains(".configure(")),
        "no popup configure:\n{}",
        tail(&s)
    );
}
