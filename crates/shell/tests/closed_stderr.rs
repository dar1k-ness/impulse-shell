//! A shell whose stderr reader went away (the terminal it was started from closed, `rusty-impulse 2>&1
//! | head`) keeps running: its log lines are dropped, as Quickshell's are, instead of the
//! write's EPIPE ending it with std's panic (exit 101).

use std::io::BufRead;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Stdio};

const NATIVE: &str = env!("CARGO_BIN_EXE_rusty-impulse");

const FIXTURE: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
ShellRoot {
    Process {
        id: sleeper
        running: true
        command: ["sleep", "infinity"]
        onStarted: console.log("STARTED " + processId)
    }
    IpcHandler {
        target: "t"
        function say(): void { console.log("a line nobody reads") }
        function pid(): int { return sleeper.processId }
    }
}
"#;

fn call(dir: &Path, run: &Path, display: &Path, f: &str) -> String {
    let o = Command::new(NATIVE)
        .arg("-p")
        .arg(dir)
        .args(["ipc", "call", "t", f])
        .env("WAYLAND_DISPLAY", display)
        .env("XDG_RUNTIME_DIR", run)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn shell_outlives_the_reader_of_its_stderr() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let (dir, run) = (t.path().join("shell"), t.path().join("run"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&run).unwrap();
    std::fs::write(dir.join("shell.qml"), FIXTURE).unwrap();
    // The run dir moves to the temp dir: keep the compositor's socket reachable.
    let display = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
        .join(std::env::var_os("WAYLAND_DISPLAY").unwrap());
    let (rd, wr) = std::io::pipe().unwrap();
    // The write end goes with the Command: once it is dropped, only the shell holds it.
    let mut child = Command::new(NATIVE)
        .arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .env("WAYLAND_DISPLAY", &display)
        .env("XDG_RUNTIME_DIR", &run)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(wr)
        .spawn()
        .unwrap();
    let mut lines = std::io::BufReader::new(rd).lines();
    let sleeper: i32 = lines
        .by_ref()
        .map_while(Result::ok)
        .find_map(|l| {
            l.split_once("STARTED ")
                .map(|(_, p)| p.trim().parse().unwrap())
        })
        .expect("the Process never started");
    // The reader leaves: every write to the shell's stderr fails with EPIPE from here on.
    drop(lines);

    call(&dir, &run, &display, "say");
    assert_eq!(
        call(&dir, &run, &display, "pid"),
        sleeper.to_string(),
        "the shell stopped serving after a log line to a closed stderr"
    );
    // SAFETY: kill(2) of our own, not yet reaped child with a valid signal number.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let status = testenv::wait_exit(&mut child);
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
}
