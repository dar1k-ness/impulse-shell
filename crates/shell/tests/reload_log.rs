//! `Rustshell.reload()` re-executes the shell, and the new image goes on as the same instance
//! with the same log, as a Quickshell reload stays one process, instance and log: `rusty-impulse log`
//! still shows what was logged before the reload (why it was asked for), and `rusty-impulse log -f`
//! keeps following.

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

const SHELL: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
ShellRoot {
    IpcHandler {
        target: "t"
        function reload(): void {
            console.log("BEFORE THE RELOAD");
            Rustshell.reload(true);
        }
    }
    Component.onCompleted: console.log("LOADED")
}
"#;

fn loads(log: &str) -> usize {
    log.lines().filter(|l| l.ends_with("LOADED")).count()
}

/// How many images finished loading: the line the shell logs once the load is over, after
/// everything the QML logged while loading (`LOADED`).
fn loaded(log: &str) -> usize {
    let done = |l: &&str| l.ends_with("INFO: Configuration Loaded");
    log.lines().filter(done).count()
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn reload_continues_the_instance_log() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let (dir, run) = (t.path().join("shell"), t.path().join("run"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&run).unwrap();
    std::fs::write(dir.join("shell.qml"), SHELL).unwrap();
    // The run dir moves to the temp dir: keep the compositor's socket reachable.
    let display = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
        .join(std::env::var_os("WAYLAND_DISPLAY").unwrap());
    let ri = |args: &[&str]| {
        let mut c = Command::new(NATIVE);
        c.arg("-p")
            .arg(&dir)
            .args(args)
            .env("RI_INTERP", "1")
            .env("NO_COLOR", "1")
            .env("WAYLAND_DISPLAY", &display)
            .env("XDG_RUNTIME_DIR", &run)
            .stdin(Stdio::null());
        c
    };
    let (rd, wr) = std::io::pipe().unwrap();
    let _shell = Kill(
        ri(&[])
            .stdout(wr.try_clone().unwrap())
            .stderr(wr)
            .spawn()
            .unwrap(),
    );
    let console = Log::capture(rd);
    console
        .wait(|l| loads(l) == 1)
        .unwrap_or_else(|l| panic!("the shell never loaded:\n{l}"));
    let instances = || -> Vec<_> {
        std::fs::read_dir(run.join("rustshell/by-id"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect()
    };
    let before = instances();
    assert_eq!(before.len(), 1, "{before:?}");

    let (rd, wr) = std::io::pipe().unwrap();
    let _follower = Kill(ri(&["log", "-f"]).stdout(wr).spawn().unwrap());
    let followed = Log::capture(rd);
    followed
        .wait(|l| loads(l) == 1)
        .unwrap_or_else(|l| panic!("`log -f` did not show the log:\n{l}"));

    let call = ri(&["ipc", "call", "t", "reload"]).output().unwrap();
    assert!(call.status.success(), "{call:?}");
    console
        .wait(|l| loads(l) == 2)
        .unwrap_or_else(|l| panic!("the re-executed shell never loaded:\n{l}"));

    // `log -f` goes on with what the next image logs; it ends if the instance's log does.
    // Up to the last line of that image's load: `log` below shows the log up to there, and
    // `LOADED` is not its end.
    followed
        .wait(|l| loaded(l) == 2)
        .unwrap_or_else(|l| panic!("`log -f` ended with the reload:\n{l}"));
    let followed = followed.text();
    let once = |text: &str, line: &str| text.matches(line).count() == 1;
    assert!(once(&followed, "BEFORE THE RELOAD"), "{followed}");
    assert!(once(&followed, "Launching config"), "{followed}");
    assert!(once(&followed, "Reloading configuration..."), "{followed}");

    // The same instance, and one log with both images' lines in it.
    assert_eq!(instances(), before);
    let shown = ri(&["log"]).output().unwrap();
    let shown = String::from_utf8_lossy(&shown.stdout);
    assert_eq!(shown, followed, "`log` and `log -f` differ");
    let text = std::fs::read_to_string(before[0].join("log.log")).unwrap();
    assert!(once(&text, "BEFORE THE RELOAD"), "{text}");
    assert_eq!(loads(&text), 2, "{text}");
}
