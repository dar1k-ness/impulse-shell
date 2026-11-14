//! Nothing runs frame by frame while nothing the shell draws can be seen.
//!
//! A running `CavaVisualizer`: its capture (a thread, an audio stream and 60 frames a second
//! handed to the main loop) stops when the compositor confirms the session lock or powers
//! the last output off, and starts again with the unlock or the output's power. The media
//! controls keep their visualizer running for as long as the panel is open, also behind a
//! lock and on a dark screen.
//!
//! A running `FrameAnimation` (the wave of the media controls' progress bar): it is triggered
//! at every step of the animation clock, shown or not, and in a window out of sight (any
//! behind a lock but the lock surfaces, every one on a dark screen) it would keep that clock
//! stepping on its timer. It rests there, and steps again with the unlock or the output.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;

const SHELL: &str = r##"import QtQuick
import Rustshell
import Rustshell.Wayland
import Impulse
ShellRoot {
    id: shell
    property bool locked: false
    WlSessionLock {
        locked: shell.locked
        surface: Component {
            WlSessionLockSurface { color: "black" }
        }
    }
    PanelWindow {
        anchors { top: true; left: true; right: true }
        implicitHeight: 24
        color: "#202020"
        CavaVisualizer { running: true }
    }
    IpcHandler {
        target: "fx"
        function lock(): void { shell.locked = true }
        function unlock(): void { shell.locked = false }
    }
}
"##;

/// A FrameAnimation in a panel counts its triggers; `fx.mark` prints the count now (`TXT mark`)
/// and a second later (`TXT later`).
const FRAMES: &str = r##"import QtQuick
import Rustshell
import Rustshell.Wayland
ShellRoot {
    id: shell
    property bool locked: false
    property int frames: 0
    WlSessionLock {
        locked: shell.locked
        surface: Component {
            WlSessionLockSurface { color: "black" }
        }
    }
    PanelWindow {
        anchors { top: true; left: true; right: true }
        implicitHeight: 24
        color: "#202020"
        FrameAnimation { running: true; onTriggered: shell.frames++ }
    }
    Timer { id: later; interval: 1000; onTriggered: console.log("TXT later " + shell.frames) }
    IpcHandler {
        target: "fx"
        function lock(): void { shell.locked = true }
        function unlock(): void { shell.locked = false }
        function mark(): void { console.log("TXT mark " + shell.frames); later.restart() }
    }
}
"##;

const SCREEN: &str = "HEADLESS-1";
const STARTED: &str = "cava: capture started";
const STOPPED: &str = "cava: capture stopped";

struct Shell {
    child: Child,
    dir: tempfile::TempDir,
    log: Arc<Log>,
    display: std::path::PathBuf,
}

impl Shell {
    /// rusty-impulse on [`SHELL`] at the Wayland socket `display`, with its visualizer capturing.
    fn start(display: &Path) -> Self {
        let sh = Shell::spawn(display, SHELL);
        sh.wait("the visualizer capturing", |s| captures(s) == (1, 0));
        sh
    }

    /// rusty-impulse on the shell `qml` at the Wayland socket `display`, once it knows the output and
    /// its power: what follows is a change of them.
    fn spawn(display: &Path, qml: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("shell.qml"), qml).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        let xdg = dir.path().join("xdg");
        let child = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("RI_LOG", "ri::cava=debug,ri::sched=debug")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .env("WAYLAND_DISPLAY", display)
            .stdout(w.try_clone().unwrap())
            .stderr(w)
            .spawn()
            .unwrap();
        let sh = Shell {
            child,
            dir,
            log: Log::capture(r),
            display: display.to_path_buf(),
        };
        sh.wait("the output's power mode", |s| {
            s.lines()
                .any(|l| l.contains("zwlr_output_power_v1#") && l.contains(".mode(1)"))
        });
        sh
    }

    fn wait(&self, what: &str, f: impl Fn(&str) -> bool) {
        if let Err(s) = self.log.wait(f) {
            let keep: Vec<&str> = s
                .lines()
                .filter(|l| {
                    l.contains("cava:")
                        || l.contains("qml:")
                        || l.contains("output_power")
                        || l.contains("session_lock")
                })
                .collect();
            panic!(
                "rusty-impulse exited before {what}; rusty-impulse log tail:\n{}",
                keep[keep.len().saturating_sub(40)..].join("\n")
            );
        }
    }

    fn ipc(&self, func: &str) {
        let st = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .arg("-p")
            .arg(self.dir.path())
            .args(["ipc", "call", "fx", func])
            .env("WAYLAND_DISPLAY", &self.display)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "ipc call fx {func}");
    }

    /// Has [`FRAMES`] count for a second: how many times its FrameAnimation was triggered in
    /// it, and how many of the loop's turns then went to sleep until the frame clock.
    fn second(&self) -> (usize, usize) {
        let laters = |s: &str| s.matches("TXT later ").count();
        let n = laters(&self.log.text());
        self.ipc("mark");
        self.wait("the count a second after the mark", |s| laters(s) > n);
        let log = self.log.text();
        let (from, to) = (
            log.rfind("TXT mark ").unwrap(),
            log.rfind("TXT later ").unwrap(),
        );
        let count = |at: usize, mark: &str| -> usize {
            let rest = &log[at + mark.len()..];
            rest[..rest.find('\n').unwrap()].trim().parse().unwrap()
        };
        let timed = log[from..to]
            .lines()
            .filter(|l| l.trim_start().starts_with("t=") && l.contains(", Frame))"))
            .count();
        (count(to, "TXT later ") - count(from, "TXT mark "), timed)
    }

    /// The ids of the shell's threads named `cava`, the captures'. A new thread carries the
    /// shell's name until it names itself, and a capture says [`STARTED`] from its own thread
    /// after that; a stopped capture's thread ends in its own time.
    fn capture_threads(&self) -> Vec<String> {
        let tasks = format!("/proc/{}/task", self.child.id());
        std::fs::read_dir(tasks)
            .unwrap()
            .filter_map(|t| {
                let t = t.ok()?;
                let comm = std::fs::read_to_string(t.path().join("comm")).ok()?;
                (comm.trim() == "cava").then(|| t.file_name().to_string_lossy().into_owned())
            })
            .collect()
    }

    /// The thread of the one capture started so far.
    fn first_capture(&self) -> String {
        let t = self.capture_threads();
        assert_eq!(t.len(), 1, "threads named cava at the start: {t:?}");
        t.into_iter().next().unwrap()
    }

    /// Asserts that one capture runs besides the stopped one of the thread `old`.
    fn assert_new_capture(&self, old: &str, when: &str) {
        let t = self.capture_threads();
        assert_eq!(
            t.iter().filter(|t| *t != old).count(),
            1,
            "threads named cava {when}, the stopped capture's being {old}: {t:?}"
        );
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How many times the shell started its capture and stopped it for an unseen screen.
fn captures(log: &str) -> (usize, usize) {
    let n = |what| log.lines().filter(|l| l.contains(what)).count();
    (n(STARTED), n(STOPPED))
}

/// What the compositor said last about the output's power (`Some(true)`: on) in the lines of
/// the shell's log after the first one that ends in `after`; `None` without such a line.
fn power_after(log: &str, after: &str) -> Option<Option<bool>> {
    let mut lines = log.lines().skip_while(|l| !l.ends_with(after));
    lines.next()?;
    Some(
        lines
            .filter(|l| l.contains("zwlr_output_power_v1#"))
            .filter_map(|l| Some(l.split_once(".mode(")?.1.starts_with('1')))
            .last(),
    )
}

fn swaymsg(sock: &Path, args: &[&str]) {
    let st = Command::new("swaymsg")
        .env("SWAYSOCK", sock)
        .args(args)
        .stdout(Stdio::null())
        .status()
        .expect("swaymsg");
    assert!(st.success(), "swaymsg {args:?}");
}

#[test]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "the_capture_stops_while_the_session_is_locked",
        &[("PATH", Some("/nonexistent"))],
    );
}

/// The compositor confirms the lock: the capture stops, though the panel's visualizer still
/// runs. The unlock starts it again.
#[test]
fn the_capture_stops_while_the_session_is_locked() {
    if !testenv::require_all(&["sway"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let sh = Shell::start(sway.display());
    let first = sh.first_capture();
    sh.ipc("lock");
    sh.wait("the capture stopped by the lock", |s| captures(s) == (1, 1));
    let log = sh.log.text();
    let at = |what: &str| log.lines().position(|l| l.contains(what));
    let locked = log.lines().position(|l| l.ends_with(".locked()"));
    assert!(
        locked.is_some() && locked < at(STOPPED),
        "the capture stopped before the compositor locked the session"
    );
    sh.ipc("unlock");
    sh.wait("the capture started by the unlock", |s| {
        captures(s) == (2, 1)
    });
    sh.assert_new_capture(&first, "after the unlock");
}

/// The only output is powered off: the capture stops, and its thread ends. The output's power
/// starts it again. Locking a session whose screen is dark, and unlocking it there, changes
/// nothing: the capture waits for both the unlock and the output.
#[test]
fn the_capture_stops_while_no_output_is_on() {
    if !testenv::require_all(&["sway", "swaymsg"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let sock = sway.ipc_socket();
    let sh = Shell::start(sway.display());
    let first = sh.first_capture();
    swaymsg(&sock, &["output", SCREEN, "power", "off"]);
    sh.wait("the capture stopped by the output's power", |s| {
        captures(s) == (1, 1)
    });
    sh.ipc("lock");
    sh.wait("the session locked", |s| {
        power_after(s, ".locked()") == Some(None)
    });
    swaymsg(&sock, &["output", SCREEN, "power", "on"]);
    sh.wait("the output on again", |s| {
        power_after(s, ".locked()") == Some(Some(true))
    });
    swaymsg(&sock, &["output", SCREEN, "power", "off"]);
    sh.wait("the output off again", |s| {
        power_after(s, ".locked()") == Some(Some(false))
    });
    sh.ipc("unlock");
    sh.wait("the session unlocked", |s| {
        power_after(s, ".unlock_and_destroy()") == Some(None)
    });
    swaymsg(&sock, &["output", SCREEN, "power", "on"]);
    // One start more and no stop: the capture stayed away from the first power off to here.
    sh.wait("the capture started by the output's power", |s| {
        captures(s) == (2, 1)
    });
    sh.assert_new_capture(&first, "with the screen on");
}

/// A FrameAnimation of a panel is triggered at every frame while the panel is shown, and not
/// behind a lock or on a dark screen, where a second passes without the loop waking for the
/// frame clock: only the call that asks for the count and the Timer that prints it step it.
/// It runs again with the unlock on a screen that is on.
#[test]
fn frame_animations_rest_out_of_sight() {
    if !testenv::require_all(&["sway", "swaymsg"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let sock = sway.ipc_socket();
    let sh = Shell::spawn(sway.display(), FRAMES);
    let runs = |when: &str| {
        let (frames, _) = sh.second();
        assert!(
            frames >= 30,
            "{when}: the FrameAnimation was triggered {frames} times in a second"
        );
    };
    let rests = |when: &str| {
        let (frames, timed) = sh.second();
        assert!(
            frames < 10 && timed < 10,
            "{when}: the FrameAnimation was triggered {frames} times in a second, and {timed} \
             turns of the loop waited for the frame clock"
        );
    };
    runs("shown");
    sh.ipc("lock");
    sh.wait("the session locked", |s| {
        power_after(s, ".locked()").is_some()
    });
    rests("behind the lock");
    swaymsg(&sock, &["output", SCREEN, "power", "off"]);
    sh.wait("the output off", |s| {
        power_after(s, ".locked()") == Some(Some(false))
    });
    rests("behind the lock, the screen dark");
    sh.ipc("unlock");
    sh.wait("the session unlocked", |s| {
        power_after(s, ".unlock_and_destroy()").is_some()
    });
    rests("the screen dark");
    swaymsg(&sock, &["output", SCREEN, "power", "on"]);
    sh.wait("the output on again", |s| {
        power_after(s, ".unlock_and_destroy()") == Some(Some(true))
    });
    runs("shown again");
}
