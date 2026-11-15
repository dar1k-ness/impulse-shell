//! The shell binary runs the child guard (`qml::childguard`) when started with its argument:
//! groups written to it are ended once its stdin closes, as when the shell dies by SIGKILL.

use std::io::Write;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Stdio};

/// `pid` is alive (not a zombie) and no signal is pending for it: nothing ended it, nor is
/// about to.
fn untouched(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/status")).is_ok_and(|s| {
        let field = |k: &str| {
            s.lines()
                .find_map(|l| l.strip_prefix(k))
                .map(str::trim)
                .unwrap_or_default()
                .to_owned()
        };
        !field("State:").starts_with('Z')
            && field("SigPnd:").bytes().all(|b| b == b'0')
            && field("ShdPnd:").bytes().all(|b| b == b'0')
    })
}

#[test]
fn guard_mode_ends_registered_groups_when_its_pipe_closes() {
    let spawn_group = || {
        Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap()
    };
    let (mut kept, mut ended) = (spawn_group(), spawn_group());
    let mut guard = Command::new(env!("CARGO_BIN_EXE_impulse-shell"))
        .arg(qml::childguard::ARG)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = guard.stdin.take().unwrap();
    for msg in [kept.id() as i32, ended.id() as i32, -(kept.id() as i32)] {
        pipe.write_all(&msg.to_le_bytes()).unwrap();
    }
    drop(pipe);
    let s = testenv::wait_exit(&mut guard);
    assert!(s.success(), "guard: {s}");
    let s = testenv::wait_exit(&mut ended);
    assert!(
        s.signal().is_some(),
        "the registered group was not ended: {s}"
    );
    assert!(
        untouched(kept.id() as i32),
        "an unregistered group was ended"
    );
    let _ = kept.kill();
    let _ = kept.wait();
}

/// The guard outlives a SIGTERM (a `kill` of the shell's process group, a stopped unit): the
/// shell is still shutting down then, and its groups are the guard's to end once it is gone.
/// The signal is sent once the guard reads its pipe, which it does only after it set up its
/// signals: the pipe is first filled with no-op messages (group 0) past its capacity, so the
/// write returns only when the guard took some of them.
#[test]
fn guard_mode_outlives_sigterm() {
    use std::os::fd::AsRawFd;
    let mut ended = Command::new("sleep")
        .arg("300")
        .process_group(0)
        .spawn()
        .unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_impulse-shell"));
    c.arg(qml::childguard::ARG).stdin(Stdio::piped());
    // SAFETY: signal(2) is async-signal-safe; the closure touches nothing else. A test runner
    // that ignores SIGTERM would hand that on to the guard.
    let default_action = || {
        // SAFETY: signal(2) with SIG_DFL installs no handler and is async-signal-safe.
        unsafe { libc::signal(libc::SIGTERM, libc::SIG_DFL) };
        Ok(())
    };
    // SAFETY: `default_action` runs in the forked child before exec and only calls signal(2),
    // which is async-signal-safe.
    unsafe { c.pre_exec(default_action) };
    let mut guard = c.spawn().unwrap();
    let mut pipe = guard.stdin.take().unwrap();
    // SAFETY: F_GETPIPE_SZ on the pipe's open write end takes no argument.
    // SAFETY: fcntl(2) F_GETPIPE_SZ takes the descriptor `pipe` owns and no pointer.
    let capacity = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETPIPE_SZ) };
    assert!(capacity > 0, "{}", std::io::Error::last_os_error());
    pipe.write_all(&vec![0u8; capacity as usize + 4]).unwrap();
    // SAFETY: kill(2) takes plain integers; the guard is not reaped yet, so the pid is its.
    // SAFETY: kill(2) takes plain integers; `guard` is this test's unreaped child.
    let sent = unsafe { libc::kill(guard.id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(sent, 0, "{}", std::io::Error::last_os_error());
    pipe.write_all(&(ended.id() as i32).to_le_bytes()).unwrap();
    drop(pipe);
    let s = testenv::wait_exit(&mut guard);
    assert!(s.success(), "guard after SIGTERM: {s}");
    let s = testenv::wait_exit(&mut ended);
    assert!(
        s.signal().is_some(),
        "the registered group was not ended: {s}"
    );
}
