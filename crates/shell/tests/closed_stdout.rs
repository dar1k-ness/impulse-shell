//! A command that prints its answer (`-V`, `--help`, `--check-tree`, `ipc call`, `ipc prop
//! get`, `ipc listen`) to a stdout nobody reads any more (`impulse-shell -V | true`, `impulse-shell ipc listen
//! t sig | head -1`) ends by SIGPIPE, as `qs` and every program with the signal's default
//! action do, instead of std's `print!` panic (exit 101); a full disk is an error message.

use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};

use testenv::Log;

const NATIVE: &str = env!("CARGO_BIN_EXE_impulse-shell");

/// A pipe's write end whose read end is closed: every write fails with EPIPE.
fn closed() -> Stdio {
    let (rd, wr) = std::io::pipe().unwrap();
    drop(rd);
    wr.into()
}

fn assert_ended_by_sigpipe(what: &str, o: &Output) {
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(
        (o.status.signal(), &*err),
        (Some(libc::SIGPIPE), ""),
        "{what}: {:?}",
        o.status
    );
}

#[test]
fn version_help_and_check_tree_end_by_sigpipe() {
    let commands = [
        vec!["-V"],
        vec!["--help"],
        vec!["--check-tree", "-p", ricompiled::ROOT],
    ];
    for args in commands {
        let run = |stdout: Stdio| {
            Command::new(NATIVE)
                .args(&args)
                .env_remove("RUST_BACKTRACE")
                .stdin(Stdio::null())
                .stdout(stdout)
                .stderr(Stdio::piped())
                .output()
                .unwrap()
        };
        // The command does print: the closed stdout below is written to.
        let o = run(Stdio::piped());
        assert!(
            o.status.success() && !o.stdout.is_empty(),
            "{args:?}: {o:?}"
        );
        assert_ended_by_sigpipe(&format!("{args:?}"), &run(closed()));
    }
}

#[test]
fn full_disk_is_an_error_message() {
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let o = Command::new(NATIVE)
        .arg("--help")
        .env_remove("RUST_BACKTRACE")
        .stdin(Stdio::null())
        .stdout(full)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(255), "{err}");
    assert!(
        err.starts_with("impulse-shell: cannot write to the standard output: ")
            && err.lines().count() == 1,
        "{err}"
    );
}

const FIXTURE: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
ShellRoot {
    IpcHandler {
        target: "t"
        property int count: 3
        signal ping(msg: string)
        function add(a: int, b: int): int { return a + b }
        function fire(m: string): void { ping(m) }
    }
    Component.onCompleted: console.log("LOADED")
}
"#;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The shell's log line for a registered `ipc listen` (under `RI_LOG=ri::ipc=debug`).
const LISTENER_CREATED: &str = "Remote listener created for";

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn ipc_answers_end_by_sigpipe() {
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
    let display = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
        .join(std::env::var_os("WAYLAND_DISPLAY").unwrap());
    let (rd, wr) = std::io::pipe().unwrap();
    let _shell = Kill(
        Command::new(NATIVE)
            .arg("-p")
            .arg(&dir)
            .env("RI_INTERP", "1")
            .env("RI_LOG", "ri::ipc=debug")
            .env("WAYLAND_DISPLAY", &display)
            .env("XDG_RUNTIME_DIR", &run)
            .stdin(Stdio::null())
            .stdout(wr.try_clone().unwrap())
            .stderr(wr)
            .spawn()
            .unwrap(),
    );
    let log = Log::capture(rd);
    log.wait(|l| l.contains("LOADED"))
        .unwrap_or_else(|l| panic!("the shell never loaded the fixture:\n{l}"));
    let ipc = |args: &[&str], stdout: Stdio| {
        let mut c = Command::new(NATIVE);
        c.arg("-p")
            .arg(&dir)
            .arg("ipc")
            .args(args)
            .env("WAYLAND_DISPLAY", &display)
            .env("XDG_RUNTIME_DIR", &run)
            .env_remove("RUST_BACKTRACE")
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(Stdio::piped());
        c
    };
    for (args, answer) in [
        (&["call", "t", "add", "1", "2"][..], "3\n"),
        (&["prop", "get", "t", "count"][..], "3\n"),
    ] {
        let o = ipc(args, Stdio::piped()).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&o.stdout),
            answer,
            "{args:?}: {o:?}"
        );
        let o = ipc(args, closed()).output().unwrap();
        assert_ended_by_sigpipe(&format!("{args:?}"), &o);
    }

    // `ipc listen t ping | head -1`: the reader is gone when the signal's value is written.
    let listener = ipc(&["listen", "t", "ping"], closed()).spawn().unwrap();
    log.wait(|l| l.contains(LISTENER_CREATED))
        .unwrap_or_else(|l| panic!("no listener registered:\n{l}"));
    let o = ipc(&["call", "t", "fire", "x"], Stdio::null())
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    // On a listener that ignored the failed write this waits to nextest's slow-timeout.
    let o = listener.wait_with_output().unwrap();
    assert_ended_by_sigpipe("ipc listen", &o);
}
