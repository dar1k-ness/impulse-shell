//! A handler reached through `ShellControl.ipcCall` runs once a window its caller closed is off
//! the screen. The original's overlay relies on it: the Recorder widget's region button closes the overlay
//! and calls `region screenshot`, whose selector copies the screen (`TempScreenshotProcess`) for
//! the content regions it offers. The original made the call with `qs ipc call`, a process of its own, and
//! under Quickshell 0.3.2 on Qt 6.12 the copy never has the closed window on it: Qt destroys
//! the window's surface as the caller returns, before the call arrives. Run in the caller's
//! turn, the handler came before the frame that destroys the surface, and the copy, made over
//! a Wayland connection of its own, had the overlay on it whenever the rest of the turn (the
//! selector's creation) took longer than the copy.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;

const MARK: &str = "SCREEN-COPY-ORDER ";

/// A bar that stays, a full-screen red window on the overlay layer, and `fx press`: the body of
/// Recorder.qml's region button (close the overlay, then `ShellControl.ipcCall("region",
/// "screenshot")`). The `region` handler stands for the selector's creation: it asks for the
/// screenshot (`TempScreenshotProcess`) and keeps the turn busy, as building the selector does.
fn shell_qml(shot: &Path) -> String {
    format!(
        r##"import QtQuick
import Rustshell
import Rustshell.Wayland
import Impulse
ShellRoot {{
    id: shell
    property bool overlayOpen: true
    PanelWindow {{
        anchors {{ top: true; left: true; right: true }}
        implicitHeight: 24
        exclusionMode: ExclusionMode.Ignore
        color: "#202020"
    }}
    PanelWindow {{
        visible: shell.overlayOpen
        anchors {{ top: true; bottom: true; left: true; right: true }}
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.layer: WlrLayer.Overlay
        color: "#ff0000"
    }}
    IpcHandler {{
        target: "fx"
        function press(): void {{
            shell.overlayOpen = false;
            ShellControl.ipcCall("region", "screenshot");
        }}
    }}
    IpcHandler {{
        target: "region"
        function screenshot(): void {{
            Screenshot.capture("HEADLESS-1", "{shot}");
            const until = Date.now() + 300;
            while (Date.now() < until) {{}}
        }}
    }}
    Connections {{
        target: Screenshot
        function onCaptured(path, ok) {{ console.log("{MARK}captured " + ok) }}
    }}
}}
"##,
        shot = shot.display()
    )
}

fn sway() -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("a headless sway (WAYLAND_DISPLAY)");
        return false;
    }
    true
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "screenshot_asked_for_after_a_window_closed_is_without_it",
        &[("WAYLAND_DISPLAY", None)],
    );
}

struct Shell {
    child: Child,
    dir: tempfile::TempDir,
    log: Arc<Log>,
}

impl Shell {
    /// rusty-impulse on `shell_qml()`.
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("shots/image-HEADLESS-1");
        std::fs::write(dir.path().join("shell.qml"), shell_qml(&shot)).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        let xdg = dir.path().join("xdg");
        let child = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("RI_LOG", "ri::frames=debug,ri::sched=debug")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdout(Stdio::null())
            .stderr(w)
            .spawn()
            .unwrap();
        Shell {
            child,
            dir,
            log: Log::capture(r),
        }
    }

    /// Waits for `f(log)` ([`Log::wait`] ends a wait the shell can no longer satisfy).
    fn wait(&self, what: &str, f: impl Fn(&str) -> bool) {
        if let Err(s) = self.log.wait(f) {
            let keep: Vec<&str> = s
                .lines()
                .filter(|l| {
                    l.contains("ri::frames")
                        || l.contains("qml:")
                        || l.contains(MARK)
                        || l.contains("layer_surface")
                        || l.contains("screencopy")
                })
                .collect();
            panic!(
                "rusty-impulse exited before {what}; its log's tail:\n{}",
                keep[keep.len().saturating_sub(40)..].join("\n")
            );
        }
    }

    fn ipc(&self, func: &str) {
        let st = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .arg("-p")
            .arg(self.dir.path())
            .args(["ipc", "call", "fx", func])
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "ipc call fx {func}");
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn screenshot_asked_for_after_a_window_closed_is_without_it() {
    if !sway() {
        return;
    }
    let sh = Shell::start();
    sh.wait("both windows drawn", |s| {
        s.contains("draw win0") && s.contains("draw win1")
    });
    // Mapped and shown: the loop has nothing left to do.
    sh.wait("the loop idle after the first frames", |s| {
        testenv::wayland::blocked_after(s, |l| l.contains("draw win"))
    });
    sh.ipc("press");
    sh.wait("the screenshot", |s| {
        s.contains(&format!("{MARK}captured "))
    });
    assert!(
        sh.log.text().contains(&format!("{MARK}captured true")),
        "the screenshot failed"
    );
    // No extension to tell the format by, as the original names it: the bytes are a PNG.
    let shot = std::fs::read(sh.dir.path().join("shots/image-HEADLESS-1")).unwrap();
    let shot = image::load_from_memory(&shot).expect("the screenshot is not an image");
    let shot = shot.to_rgb8();
    let (w, h) = shot.dimensions();
    let [r, g, b] = shot.get_pixel(w / 2, h / 2).0;
    assert!(
        r <= g.max(b).saturating_add(16),
        "the window closed before the screenshot was asked for is on it: rgb({r}, {g}, {b})"
    );
}
