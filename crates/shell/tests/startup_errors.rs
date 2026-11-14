//! Startup failures the user can act on end with a one-line message and exit status 1, not a
//! panic and backtrace, and leave no instance registered.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What makes an instance findable and look alive or dead to `qs list` / `qs ipc` under the
/// run dir `run` (`$XDG_RUNTIME_DIR/rustshell`): its lock, its socket, its `by-pid` link and
/// its link under `by-shell/<path id>`. The logs and the shared `by-path` link are not.
fn registration(run: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut dirs = vec![run.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(run).unwrap();
            let kind = e.file_type().unwrap();
            if kind.is_dir() {
                dirs.push(p);
            } else if rel.starts_with("by-pid")
                || rel.starts_with("by-shell")
                || matches!(
                    p.file_name().and_then(|n| n.to_str()),
                    Some("instance.lock" | "ipc.sock")
                )
            {
                found.push(rel.to_path_buf());
            }
        }
    }
    found.sort();
    found
}

#[cfg(feature = "interp")]
#[test]
fn no_compositor_is_a_clean_error_not_a_panic() {
    let dir = std::env::temp_dir().join(format!("ri-startup-errors-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shell.qml"), "import QtQuick\nItem {}\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
        .arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("WAYLAND_DISPLAY", "no-such-compositor")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("WAYLAND_SOCKET")
        .env_remove("RUST_BACKTRACE")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!err.contains("panicked"), "startup panicked:\n{err}");
    assert!(
        err.contains("rusty-impulse: cannot connect to the Wayland compositor"),
        "no startup error message:\n{err}"
    );
    assert_eq!(out.status.code(), Some(1), "stderr:\n{err}");
}

/// The exit on a startup error ends the log thread wherever it is: what was logged until
/// then, and the error itself, are in the instance's log first, where `rusty-impulse log` shows why
/// the shell did not start.
#[cfg(feature = "interp")]
#[test]
fn startup_error_is_in_the_instance_log() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    std::fs::write(dir.join("shell.qml"), "import QtQuick\nItem {}\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
        .arg("-p")
        .arg(dir)
        .env("RI_INTERP", "1")
        .env("XDG_RUNTIME_DIR", dir)
        .env("WAYLAND_DISPLAY", "no-such-compositor")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("WAYLAND_SOCKET")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let instances: Vec<_> = std::fs::read_dir(dir.join("rustshell/by-id"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(instances.len(), 1, "{instances:?}");
    let text = std::fs::read_to_string(instances[0].join("log.log")).unwrap();
    assert!(text.contains("Launching config"), "log.log: {text:?}");
    assert!(
        text.contains("ERROR: cannot connect to the Wayland compositor"),
        "log.log: {text:?}"
    );
    let detailed = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
        .arg("log")
        .arg(instances[0].join("log.qslog"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let detailed = String::from_utf8_lossy(&detailed.stdout);
    assert!(
        detailed.contains("cannot connect to the Wayland compositor"),
        "log.qslog: {detailed:?}"
    );
}

/// Runs `rusty-impulse -p <dir>` on `shell.qml` (`src`) in a fresh `dir` that is also its runtime dir,
/// with no compositor. Returns its exit code, its stderr, whether it registered the instance
/// and the [`registration`] it left behind.
fn failed_start(test: &str, src: &str) -> (Option<i32>, String, bool, Vec<PathBuf>) {
    let dir = std::env::temp_dir().join(format!("ri-startup-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shell.qml"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
        .arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("WAYLAND_DISPLAY", "no-such-compositor")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("WAYLAND_SOCKET")
        .output()
        .unwrap();
    let run = dir.join("rustshell");
    // `by-path/<path id>` is linked last when the instance registers, and stays.
    let registered = std::fs::read_dir(run.join("by-path")).is_ok_and(|mut d| d.next().is_some());
    let left = registration(&run);
    let _ = std::fs::remove_dir_all(&dir);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    (out.status.code(), err, registered, left)
}

/// A start that fails after the instance registered (here: no compositor) unregisters it
/// before exiting: no dead instance stays behind for `qs list` and `qs ipc`.
#[cfg(feature = "interp")]
#[test]
fn failed_start_leaves_no_instance_registered() {
    let (code, err, registered, left) = failed_start("no-compositor", "import QtQuick\nItem {}\n");
    assert_eq!(code, Some(1), "stderr:\n{err}");
    assert!(
        err.contains("rusty-impulse: cannot connect to the Wayland compositor"),
        "did not fail at the Wayland connection:\n{err}"
    );
    assert!(registered, "the instance never registered:\n{err}");
    assert!(left.is_empty(), "left registered: {left:?}");
}

/// The AOT-only shell refuses a tree it was not compiled from after the instance registered,
/// and unregisters it before exiting.
#[cfg(not(feature = "interp"))]
#[test]
fn refused_tree_leaves_no_instance_registered() {
    let (code, err, registered, left) = failed_start(
        "refused-tree",
        "import QtQuick\nItem { property int refused: 1 + 2 }\n",
    );
    assert_eq!(code, Some(1), "stderr:\n{err}");
    assert!(
        err.contains("differs from the tree this binary was compiled from"),
        "the tree was not refused:\n{err}"
    );
    assert!(registered, "the instance never registered:\n{err}");
    assert!(left.is_empty(), "left registered: {left:?}");
}

/// A copy of the tree the binary was compiled from, without `gone`.
#[cfg(not(feature = "interp"))]
fn tree_without(test: &str, gone: &str) -> tempfile::TempDir {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let target = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &target);
            } else {
                std::fs::copy(e.path(), target).unwrap();
            }
        }
    }
    let dir = tempfile::Builder::new()
        .prefix(&format!("ri-startup-{test}-"))
        .tempdir()
        .unwrap();
    copy(
        Path::new(ricompiled::ROOT),
        &dir.path().join("rusty-impulse"),
    );
    std::fs::remove_file(dir.path().join("rusty-impulse").join(gone)).unwrap();
    dir
}

/// The AOT-only shell on its tree with one compiled file removed says what `--check-tree`
/// says of it and ends with status 1: it does not start without its bar
/// (`modules/rusty/bar/Bar.qml`) or with no layer at all (`ReloadPopup.qml`, which the root
/// needs).
#[cfg(not(feature = "interp"))]
#[test]
fn a_tree_that_lacks_a_compiled_file_is_refused() {
    for gone in ["modules/rusty/bar/Bar.qml", "ReloadPopup.qml"] {
        let dir = tree_without("lacks", gone);
        let tree = dir.path().join("rusty-impulse");
        let ri = |args: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
                .args(args)
                .arg("-p")
                .arg(&tree)
                .env("XDG_RUNTIME_DIR", dir.path())
                .env("WAYLAND_DISPLAY", "no-such-compositor")
                .env("XDG_CONFIG_HOME", dir.path().join("config"))
                .env("XDG_STATE_HOME", dir.path().join("state"))
                .env("XDG_CACHE_HOME", dir.path().join("cache"))
                .env_remove("WAYLAND_SOCKET")
                .env_remove("RI_INTERP")
                .output()
                .unwrap()
        };
        let checked = ri(&["--check-tree"]);
        let said = String::from_utf8_lossy(&checked.stderr).into_owned();
        assert_eq!(
            checked.status.code(),
            Some(1),
            "--check-tree without {gone}:\n{said}"
        );
        let line = format!("  {gone}: missing");
        assert!(
            said.lines().any(|l| l == line),
            "--check-tree without {gone}:\n{said}"
        );
        let started = ri(&[]);
        let err = String::from_utf8_lossy(&started.stderr).into_owned();
        assert_eq!(
            started.status.code(),
            Some(1),
            "started without {gone}:\n{err}"
        );
        assert!(
            err.lines().any(|l| l == line) && err.contains("rusty-impulse: install again: "),
            "started without {gone}, the tree was not refused:\n{err}"
        );
        assert!(
            registration(&dir.path().join("rustshell")).is_empty(),
            "started without {gone}, left registered"
        );
    }
}

fn ptrace(req: libc::c_uint, tid: i32, data: usize) {
    // SAFETY: requests on our own tracee whose `data` is an integer or a valid pointer.
    let r = unsafe { libc::ptrace(req, tid, 0usize, data) };
    assert_ne!(r, -1, "ptrace {req}: {}", std::io::Error::last_os_error());
}

/// waitpid(2) on one thread of the tracee: its status once it changed.
fn wait(tid: i32, flags: i32) -> Option<i32> {
    let mut st = 0;
    // SAFETY: `st` is a valid out pointer.
    let r = unsafe { libc::waitpid(tid, &mut st, flags | libc::__WALL) };
    assert_ne!(r, -1, "waitpid {tid}: {}", std::io::Error::last_os_error());
    (r == tid).then_some(st)
}

/// waitpid(2) on every thread of the tracee whose process group is `pgid`, blocking until one
/// changes: its id and status.
fn wait_group(pgid: i32) -> (i32, i32) {
    let mut st = 0;
    // SAFETY: `st` is a valid out pointer.
    let r = unsafe { libc::waitpid(-pgid, &mut st, libc::__WALL) };
    assert_ne!(
        r,
        -1,
        "waitpid -{pgid}: {}",
        std::io::Error::last_os_error()
    );
    (r, st)
}

/// `//@ pragma Env` is applied with `setenv`, which is sound only while no other thread exists.
/// A start must not depend on how soon the scheduler runs a thread started before it: the
/// first thread rusty-impulse starts is held, as a busy CPU would, never running a single instruction,
/// until the main thread exits (jemalloc's purging thread, started at the allocator's
/// initialisation before `main`, then keeps the main thread's name and is counted). A start that
/// waits on that thread hangs, and nextest's slow-timeout (.config/nextest.toml) ends it.
#[test]
fn env_pragmas_apply_before_any_thread() {
    use std::os::unix::process::CommandExt;
    let dir = std::env::temp_dir().join(format!("ri-startup-pragmas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("shell.qml"),
        "//@ pragma Env RI_TEST_PRAGMA=1\nimport QtQuick\nItem {}\n",
    )
    .unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
    cmd.arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("WAYLAND_DISPLAY", "no-such-compositor")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("WAYLAND_SOCKET")
        .env_remove("MALLOC_CONF")
        .env_remove("RUST_BACKTRACE")
        .stderr(std::process::Stdio::piped())
        // Its own process group, which every tracee (thread) shares and no other child has.
        .process_group(0);
    let traceme = || {
        // SAFETY: PTRACE_TRACEME reads no pointers.
        if unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0, 0usize, 0usize) } == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    };
    // SAFETY: the child only calls ptrace(2), which is async-signal-safe, before exec.
    unsafe { cmd.pre_exec(traceme) };
    let mut child = cmd.spawn().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut stderr, &mut s).unwrap();
        s
    });
    let pid = child.id() as i32;
    let st = wait(pid, 0).unwrap();
    assert!(
        libc::WIFSTOPPED(st) && libc::WSTOPSIG(st) == libc::SIGTRAP,
        "{st:#x}"
    );
    let opts = (libc::PTRACE_O_EXITKILL | libc::PTRACE_O_TRACEEXIT) as usize;
    ptrace(
        libc::PTRACE_SETOPTIONS,
        pid,
        opts | libc::PTRACE_O_TRACECLONE as usize,
    );
    ptrace(libc::PTRACE_CONT, pid, 0);
    let event = |st: i32, ev: i32| st >> 8 == libc::SIGTRAP | (ev << 8);
    let mut held = None;
    // Every change of a tracee, blocking: the main thread's exit ends it, reported only once
    // every other thread is gone (ptrace(2), "the death of the thread group leader").
    let status = loop {
        let (t, st) = wait_group(pid);
        if libc::WIFEXITED(st) || libc::WIFSIGNALED(st) {
            if t == pid {
                break st;
            }
            if held == Some(t) {
                // Ended by the process's exit while held.
                held = None;
            }
        } else if t == pid && held.is_none() && event(st, libc::PTRACE_EVENT_CLONE) {
            let mut tid: libc::c_ulong = 0;
            ptrace(libc::PTRACE_GETEVENTMSG, pid, (&raw mut tid) as usize);
            let tid = tid as i32;
            // A new tracee stops before its first instruction.
            assert!(wait(tid, 0).is_some_and(|st| libc::WIFSTOPPED(st)));
            held = Some(tid);
            ptrace(libc::PTRACE_SETOPTIONS, pid, opts);
            ptrace(libc::PTRACE_CONT, pid, 0);
        } else if t == pid && event(st, libc::PTRACE_EVENT_EXIT) {
            if let Some(h) = held.take() {
                // SAFETY: PTRACE_DETACH of our tracee reads no pointers. It fails (ESRCH) only
                // when the process's exit already killed it.
                unsafe { libc::ptrace(libc::PTRACE_DETACH, h, 0usize, 0usize) };
            }
            ptrace(libc::PTRACE_CONT, pid, 0);
        } else if st >> 16 != 0 {
            // Another ptrace event (the held thread's own exit): no signal to deliver.
            ptrace(libc::PTRACE_CONT, t, 0);
        } else {
            ptrace(libc::PTRACE_CONT, t, libc::WSTOPSIG(st) as usize);
        }
    };
    // Reaped above; `child` has nothing left to wait for.
    drop(child);
    let err = err.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!err.contains("panicked"), "startup panicked:\n{err}");
    assert!(
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 1,
        "status {status:#x}, stderr:\n{err}"
    );
}
