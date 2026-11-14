//! A password check that has just started is answered before any window but a lock surface
//! draws again. `PamContext.start()` runs a helper that asks for the password a few ms later,
//! and the lock screen answers in `onPamMessage` (`respond(currentText)`), in the turn that
//! reads the request. The loop reads nothing while it draws, so a frame drawn between
//! `start()` and that turn starts the check as much later as the frame takes: in a Hyprland
//! guest without a GPU the blurred wallpaper behind the lock screen took 0.8 s a frame, the
//! refusal of a wrong password (PAM's own 1 to 3 s after the check) came 4 s or more after its
//! Return, and cleared the next password, typed meanwhile: its Return sent an empty one, in 3
//! of 50 rounds, and in none of 50 with Quickshell, which draws on another thread.
//!
//! Read from the requests on the wire (`WAYLAND_DEBUG`) and what QML logs: the handler that
//! starts the check changes the panel as well, and the panel's frame comes after the helper's
//! first message. A helper that sends none (a module waiting for a finger on a reader) is
//! waited for 100 ms at most, and the panel draws then.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;

fn shell_qml(pam: &Path) -> String {
    format!(
        r##"import QtQuick
import Rustshell
import Rustshell.Io
import Rustshell.Services.Pam
ShellRoot {{
    id: root
    property int checks: 0
    PanelWindow {{
        anchors {{ top: true; left: true }}
        implicitWidth: 64; implicitHeight: 32
        color: root.checks % 2 ? "#00ff00" : "#ff0000"
    }}
    PamContext {{
        id: pam
        configDirectory: "{}"
        config: "t"
        onPamMessage: console.log("PAM message " + message)
        onCompleted: result => console.log("PAM completed " + result)
    }}
    IpcHandler {{
        target: "pam"
        function check(): void {{ console.log("CHECK"); root.checks++; pam.start() }}
        function abort(): void {{ pam.abort(); console.log("ABORTED") }}
    }}
}}
"##,
        pam.display()
    )
}

fn sway() -> Option<testenv::Sway> {
    testenv::require_all(&["sway"]).then(testenv::Sway::start)
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "no_window_draws_between_a_password_check_and_its_helpers_first_message",
        &[("PATH", Some("/nonexistent"))],
    );
}

/// rusty-impulse running [`shell_qml`] on `sway` with its Wayland traffic logged.
struct Shell<'a> {
    child: Child,
    dir: tempfile::TempDir,
    log: Arc<Log>,
    sway: &'a testenv::Sway,
}

impl<'a> Shell<'a> {
    /// A PAM service `t` of the lines `conf`; `HANG` in them is a program that never returns
    /// and says nothing ([`Shell::hung_pid`]).
    fn start(sway: &'a testenv::Sway, conf: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let pam = dir.path().join("pam");
        std::fs::create_dir(&pam).unwrap();
        let pidfile = dir.path().join("pid");
        testenv::fifo(&pidfile);
        let hang = dir.path().join("hang.sh");
        testenv::install_script(
            &hang,
            &format!(
                "#!/bin/sh\necho $$ > {}\nexec sleep infinity\n",
                pidfile.display()
            ),
        );
        std::fs::write(pam.join("t"), conf.replace("HANG", &hang.to_string_lossy())).unwrap();
        std::fs::write(dir.path().join("shell.qml"), shell_qml(&pam)).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        let xdg = dir.path().join("xdg");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
        let child = sway
            .env(&mut cmd)
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("RI_LOG", "ri::sched")
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
            sway,
        }
    }

    /// Waits for `f(log)`; panics with the log's tail if rusty-impulse exits first.
    fn wait(&self, what: &str, f: impl Fn(&str) -> bool) -> String {
        if let Err(s) = self.log.wait(&f) {
            panic!("rusty-impulse exited before {what}:\n{}", tail(&s));
        }
        self.log.text()
    }

    fn ipc(&self, func: &str) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
        let st = self
            .sway
            .env(&mut cmd)
            .arg("-p")
            .arg(self.dir.path())
            .args(["ipc", "call", "pam", func])
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "ipc call pam {func}");
    }

    /// The pid of the `HANG` program the check started; blocks until it runs.
    fn hung_pid(&self) -> i32 {
        std::fs::read_to_string(self.dir.path().join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }
}

impl Drop for Shell<'_> {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tail(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(60)..].join("\n")
}

/// The lines of the commits that show a new buffer (a frame), from line `from` on.
fn frames(lines: &[&str], from: usize) -> Vec<usize> {
    let mut attached = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate().skip(from) {
        // wayland-backend logs `iface#id`, its discarded-message lines `iface@id`.
        let l = l.replace('@', "#");
        let Some(at) = l.find("-> wl_surface#") else {
            continue;
        };
        let Some((surface, call)) = l[at + 3..].split_once('.') else {
            continue;
        };
        if call.starts_with("attach(wl_buffer#") {
            attached.insert(surface.to_string());
        } else if call.starts_with("commit(") && attached.remove(surface) {
            out.push(i);
        }
    }
    out
}

fn line(lines: &[&str], what: &str, from: usize) -> Option<usize> {
    (from..lines.len()).find(|&i| lines[i].contains(what))
}

/// The shell has nothing left to draw: sway answered every frame callback it asked for, and
/// after its last frame and what QML last logged the loop went to wait for events
/// (`RI_LOG=ri::sched`: a turn with nothing due). A frame asked for then is drawn at once, not
/// when the callback of the one before comes.
fn idle(log: &str) -> bool {
    let log = log.replace('@', "#");
    let answered = (log.split(".frame(new id wl_callback#").skip(1)).all(|r| {
        let id = r.split(')').next().unwrap_or_default();
        r.contains(&format!("wl_callback#{id}.done("))
    });
    answered
        && testenv::wayland::blocked_after(&log, |l| {
            l.contains(".commit(")
                || l.contains(".done(")
                || l.contains("CHECK")
                || l.contains("PAM ")
        })
}

/// The panel has its first frame and the shell is [`idle`].
fn settled(sh: &Shell) {
    sh.wait("the panel's first frame", |s| {
        !frames(&s.lines().collect::<Vec<_>>(), 0).is_empty() && idle(s)
    });
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn no_window_draws_between_a_password_check_and_its_helpers_first_message() {
    let Some(sway) = sway() else {
        return;
    };
    let sh = Shell::start(
        &sway,
        "auth optional pam_echo.so hello\nauth required pam_permit.so\n",
    );
    settled(&sh);
    for round in 1..=5 {
        let done = |s: &str| {
            let lines: Vec<&str> = s.lines().collect();
            let checks: Vec<usize> = (0..lines.len())
                .filter(|&i| lines[i].contains("CHECK"))
                .collect();
            checks.len() == round
                && line(&lines, "PAM completed", checks[round - 1]).is_some()
                && !frames(&lines, checks[round - 1]).is_empty()
                && idle(s)
        };
        sh.ipc("check");
        let log = sh.wait("the check's end, the panel's frame and an idle shell", done);
        let lines: Vec<&str> = log.lines().collect();
        let check = (0..lines.len())
            .filter(|&i| lines[i].contains("CHECK"))
            .nth(round - 1)
            .unwrap();
        let heard = line(&lines, "PAM message hello", check).unwrap();
        let frame = frames(&lines, check)[0];
        assert!(
            heard < frame,
            "round {round}: the panel drew (line {frame}) before the helper was heard (line \
             {heard}) after the check started (line {check}):\n{}",
            lines[check..=heard.max(frame)].join("\n")
        );
    }
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_helper_that_sends_nothing_holds_the_frame_for_a_moment_only() {
    let Some(sway) = sway() else {
        return;
    };
    let sh = Shell::start(&sway, "auth required pam_exec.so quiet HANG\n");
    settled(&sh);
    let started = std::time::Instant::now();
    sh.ipc("check");
    let log = sh.wait("the panel's frame", |s| {
        let lines: Vec<&str> = s.lines().collect();
        line(&lines, "CHECK", 0).is_some_and(|c| !frames(&lines, c).is_empty())
    });
    let took = started.elapsed();
    let hung = sh.hung_pid();
    sh.ipc("abort");
    let end = sh.wait("the abort", |s| s.contains("ABORTED"));
    // pam_exec setsid()s its program: the abort does not end it.
    // SAFETY: kill(2) takes plain integers and touches no memory of ours.
    unsafe { libc::kill(hung, libc::SIGKILL) };
    assert!(
        !end.contains("PAM "),
        "the helper was heard:\n{}",
        tail(&end)
    );
    let lines: Vec<&str> = log.lines().collect();
    assert!(
        line(&lines, "PAM ", 0).is_none(),
        "the helper was heard before the frame"
    );
    // The call itself (a second rusty-impulse started) is in `took`.
    assert!(
        took < std::time::Duration::from_secs(5),
        "the panel drew {took:?} after the check started"
    );
}
