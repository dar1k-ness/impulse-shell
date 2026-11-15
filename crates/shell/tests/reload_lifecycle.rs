//! `Rustshell.reload()` re-executes the shell; what was running in the old image is not lost
//! on the way: a termination signal that arrived while the reload's turn ran still ends the
//! shell (the next image reads it), and an exec that fails leaves the `Process` children
//! running and guarded, as if the reload had not been asked for.

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use testenv::Log;

const NATIVE: &str = env!("CARGO_BIN_EXE_impulse-shell");

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
        function busyReload(ms: int): void {
            console.log("BUSY");
            var t = Date.now(); while (Date.now() - t < ms) {}
            Rustshell.reload(true);
        }
        function reload(): void { Rustshell.reload(true) }
        function pid(): int { return sleeper.processId }
    }
}
"#;

struct Shell {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    run: PathBuf,
    display: PathBuf,
    child: Child,
    log: std::sync::Arc<Log>,
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The fixture's shell with `env` added, once its Process logged its pid; None without a
/// compositor.
fn start(env: &[(&str, String)]) -> Option<(Shell, i32)> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return None;
    }
    let tmp = tempfile::tempdir().unwrap();
    let (dir, run) = (tmp.path().join("shell"), tmp.path().join("run"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&run).unwrap();
    std::fs::write(dir.join("shell.qml"), FIXTURE).unwrap();
    // The run dir moves to the temp dir: keep the compositor's socket reachable.
    let display = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
        .join(std::env::var_os("WAYLAND_DISPLAY").unwrap());
    let (rd, wr) = std::io::pipe().unwrap();
    let child = Command::new(NATIVE)
        .arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .env("WAYLAND_DISPLAY", &display)
        .env("XDG_RUNTIME_DIR", &run)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    let log = Log::capture(rd);
    let shell = Shell {
        _tmp: tmp,
        dir,
        run,
        display,
        child,
        log,
    };
    let started = |l: &str| {
        l.lines()
            .find_map(|l| l.split_once("STARTED ").map(|(_, p)| p.trim().to_string()))
    };
    shell
        .log
        .wait(|l| started(l).is_some())
        .unwrap_or_else(|l| panic!("the Process never started:\n{l}"));
    let sleeper = started(&shell.log.text()).unwrap().parse().unwrap();
    Some((shell, sleeper))
}

impl Shell {
    fn call(&self, args: &[&str]) -> Command {
        let mut c = Command::new(NATIVE);
        c.arg("-p")
            .arg(&self.dir)
            .args(["ipc", "call", "t"])
            .args(args)
            .env("WAYLAND_DISPLAY", &self.display)
            .env("XDG_RUNTIME_DIR", &self.run)
            .stdin(Stdio::null());
        c
    }

    fn answer(&self, args: &[&str]) -> String {
        let o = self.call(args).output().unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }
}

/// The state letter of `/proc/<pid>/stat` (`Z` for a zombie), None once it was reaped.
fn state(pid: i32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?.1.chars().next()
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn sigterm_during_the_reload_turn_ends_the_shell() {
    let Some((mut shell, sleeper)) = start(&[]) else {
        return;
    };
    let sleeper = testenv::PidFd::open(sleeper);
    // The handler busy-waits 1.5 s, then reloads in the same turn: the signal sent once it
    // runs reaches the self-pipe after the main loop last read it, before the exec.
    let mut busy = shell
        .call(&["busyReload", "1500"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    shell
        .log
        .wait(|l| l.contains("BUSY"))
        .unwrap_or_else(|l| panic!("the handler never ran:\n{l}"));
    // SAFETY: kill(2) of our own, not yet reaped child with a valid signal number.
    unsafe { libc::kill(shell.child.id() as i32, libc::SIGTERM) };
    // Without the signal carried to the next image, the reloaded shell runs on (and the test
    // hangs to nextest's slow-timeout).
    let status = testenv::wait_exit(&mut shell.child);
    assert_eq!(
        status.signal(),
        Some(libc::SIGTERM),
        "{status:?}\n{}",
        shell.log.text()
    );
    assert_eq!(
        std::fs::read_dir(shell.run.join("rustshell/by-id"))
            .unwrap()
            .count(),
        0,
        "the instance directory stayed"
    );
    sleeper.wait_exit();
    testenv::wait_exit(&mut busy);
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn failed_reload_exec_keeps_the_children() {
    // exec(2) fails with E2BIG once the arguments and environment exceed a quarter of the
    // stack limit (at least 128 KiB): two 120 kB variables, a 64 KiB limit set after start.
    let big = "x".repeat(120_000);
    let Some((mut shell, sleeper)) = start(&[("BIG1", big.clone()), ("BIG2", big)]) else {
        return;
    };
    let lim = libc::rlimit {
        rlim_cur: 65536,
        rlim_max: 65536,
    };
    // SAFETY: prlimit(2) of our own child with a valid resource and limit, no old value.
    let r = unsafe {
        libc::prlimit(
            shell.child.id() as i32,
            libc::RLIMIT_STACK,
            &lim,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(r, 0, "prlimit: {}", std::io::Error::last_os_error());
    let sleeper_fd = testenv::PidFd::open(sleeper);

    shell.answer(&["reload"]);
    shell
        .log
        .wait(|l| l.contains("ERROR: Cannot run "))
        .unwrap_or_else(|l| panic!("the reload neither ran nor failed:\n{l}"));
    let log = shell.log.text();
    assert!(log.contains("Argument list too long"), "{log}");
    assert_eq!(
        shell.answer(&["pid"]),
        sleeper.to_string(),
        "the Process lost its child:\n{log}"
    );
    assert!(
        matches!(state(sleeper), Some(s) if s != 'Z'),
        "the Process child was killed: {:?}",
        state(sleeper)
    );
    // Guarded again: the child guard ends it when the shell is killed outright.
    shell.child.kill().unwrap();
    shell.child.wait().unwrap();
    sleeper_fd.wait_exit();
}
