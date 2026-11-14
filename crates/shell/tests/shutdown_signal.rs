//! SIGTERM (session logout, `systemctl stop`) ends the native shell through its orderly exit:
//! its `Process` children are terminated, its instance directory is removed and it dies by the
//! signal, as its parent asked. A termination signal also ends a shell that is still starting
//! (its compositor does not answer), one whose Hyprland sends no events and one a reload has
//! just executed; and a launcher that ignores SIGCHLD does not cost the shell its children's
//! exit statuses.

use std::os::unix::process::{CommandExt, ExitStatusExt};
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
fn sigterm_exits_in_order() {
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
    let mut child = Command::new(NATIVE)
        .arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .env("WAYLAND_DISPLAY", &display)
        .env("XDG_RUNTIME_DIR", &run)
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    let log = testenv::Log::capture(rd);
    // The shell registers its instance before the QML runs, and the Process starts as its
    // component completes: once it logged its pid, `t` answers the first call.
    let started = |l: &str| {
        l.lines()
            .find_map(|l| l.split_once("STARTED ").map(|(_, p)| p.trim().to_string()))
    };
    log.wait(|l| started(l).is_some())
        .unwrap_or_else(|l| panic!("the Process never started:\n{l}"));
    let sleeper: i32 = started(&log.text()).unwrap().parse().unwrap();
    assert!(sleeper > 0);
    assert_eq!(
        call(&dir, &run, &display, "pid"),
        sleeper.to_string(),
        "the shell did not serve t:\n{}",
        log.text()
    );
    let by_id = run.join("rustshell/by-id");
    assert_eq!(std::fs::read_dir(&by_id).unwrap().count(), 1);
    // Opened while the sleeper is surely alive: the shell has not been signalled yet.
    let sleeper = testenv::PidFd::open(sleeper);

    // SAFETY: kill(2) of our own, not yet reaped child with a valid signal number.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let status = testenv::wait_exit(&mut child);
    let log = log.text();
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}\n{log}");
    assert_eq!(
        std::fs::read_dir(&by_id).unwrap().count(),
        0,
        "the instance directory stayed"
    );
    // `sleep infinity` ends only by the SIGTERM the shell's exit sends its Process groups.
    sleeper.wait_exit();
}

/// A compositor that accepts the connection and never answers (stopped, hung): the shell waits
/// in its startup for the globals, where no main loop reads signals yet. SIGTERM, SIGINT and
/// SIGHUP each still end it, by that signal.
#[test]
fn a_termination_signal_ends_a_shell_whose_compositor_is_silent() {
    use std::io::Read as _;
    for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let t = tempfile::tempdir().unwrap();
        let (dir, run) = (t.path().join("shell"), t.path().join("run"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(dir.join("shell.qml"), FIXTURE).unwrap();
        let display = run.join("wayland-silent");
        let compositor = std::os::unix::net::UnixListener::bind(&display).unwrap();
        let (rd, wr) = std::io::pipe().unwrap();
        let mut child = Command::new(NATIVE)
            .arg("-p")
            .arg(&dir)
            .env("RI_INTERP", "1")
            .env("WAYLAND_DISPLAY", &display)
            .env("XDG_RUNTIME_DIR", &run)
            .stdin(Stdio::null())
            .stdout(wr.try_clone().unwrap())
            .stderr(wr)
            .spawn()
            .unwrap();
        let log = testenv::Log::capture(rd);
        // The shell's first request (for the globals) arrived: it now waits for the answer.
        let (mut client, _) = compositor.accept().unwrap();
        let mut header = [0u8; 8];
        client.read_exact(&mut header).unwrap();
        // SAFETY: kill(2) of our own, not yet reaped child with a valid signal number.
        unsafe { libc::kill(child.id() as i32, sig) };
        // A shell the signal does not end stays (the test hangs to nextest's slow-timeout).
        let status = testenv::wait_exit(&mut child);
        assert_eq!(status.signal(), Some(sig), "{status:?}\n{}", log.text());
        // The line is the last thing the shell wrote: the reader may not have it yet.
        let line = ending_line(sig);
        log.wait(|l| l.lines().any(|l| l == line))
            .unwrap_or_else(|log| {
                panic!(
                    "no line {line:?} in the log of a starting shell ended by signal {sig}:\n{log}"
                )
            });
    }
}

/// The start of the log line of a shell that `sig`, sent by this test, ends.
fn ending_line(sig: i32) -> String {
    let name = match sig {
        libc::SIGTERM => "SIGTERM",
        libc::SIGINT => "SIGINT",
        libc::SIGHUP => "SIGHUP",
        _ => unreachable!(),
    };
    format!(
        "rusty-impulse: ending on signal {sig} ({name}) sent by pid {}",
        std::process::id()
    )
}

/// A shell that a termination signal ends says so in its log before it shuts down: the signal
/// by number and name and the process that sent it, by pid and name. Without the line a shell
/// ended from outside (`killall`, a session manager) and one the kernel killed leave the same
/// log, one that just stops.
#[test]
fn a_termination_signal_is_named_in_the_log() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let me = std::fs::read_to_string("/proc/self/comm").unwrap();
    for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let t = tempfile::tempdir().unwrap();
        let (dir, run) = (t.path().join("shell"), t.path().join("run"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(dir.join("shell.qml"), FIXTURE).unwrap();
        let display =
            std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
                .join(std::env::var_os("WAYLAND_DISPLAY").unwrap());
        let (rd, wr) = std::io::pipe().unwrap();
        let mut child = Command::new(NATIVE)
            .arg("-p")
            .arg(&dir)
            .env("RI_INTERP", "1")
            .env("WAYLAND_DISPLAY", &display)
            .env("XDG_RUNTIME_DIR", &run)
            .stdin(Stdio::null())
            .stdout(wr.try_clone().unwrap())
            .stderr(wr)
            .spawn()
            .unwrap();
        let log = testenv::Log::capture(rd);
        log.wait(|l| l.contains("STARTED "))
            .unwrap_or_else(|l| panic!("the Process never started:\n{l}"));
        // An answered call: the main loop runs and reads the signals.
        assert!(
            !call(&dir, &run, &display, "pid").is_empty(),
            "the shell did not serve t:\n{}",
            log.text()
        );
        // SAFETY: kill(2) of our own, not yet reaped child with a valid signal number.
        unsafe { libc::kill(child.id() as i32, sig) };
        let status = testenv::wait_exit(&mut child);
        assert_eq!(status.signal(), Some(sig), "{status:?}\n{}", log.text());
        let line = format!("{} ({})", ending_line(sig), me.trim_end());
        log.wait(|l| l.lines().any(|l| l == line))
            .unwrap_or_else(|log| {
                panic!("no line {line:?} in the log of a shell ended by signal {sig}:\n{log}")
            });
    }
}

/// Hyprland is up and sends no events: the shell's `hypr-events` thread stands in a read of the
/// event socket, and the shell's exit joins that thread. Only shutdown(2) of the connection
/// ends that read (a close(2) from another thread does not), so a shell that merely dropped
/// its end would never exit.
#[test]
fn sigterm_ends_a_shell_whose_hyprland_sends_no_events() {
    use std::io::{Read as _, Write as _};
    use std::os::unix::net::UnixListener;
    const HYPRLAND: &str = r#"import QtQuick
import Rustshell
import Rustshell.Hyprland
import Rustshell.Io
ShellRoot {
    // Read at load: the singleton exists, so the shell connects to Hyprland by itself.
    readonly property string events: Hyprland.eventSocketPath
    IpcHandler {
        target: "t"
        function events(): string { return Hyprland.eventSocketPath }
    }
}
"#;
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let (dir, run) = (t.path().join("shell"), t.path().join("run"));
    let hypr = run.join("hypr/silent");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&hypr).unwrap();
    std::fs::write(dir.join("shell.qml"), HYPRLAND).unwrap();
    let requests = UnixListener::bind(hypr.join(".socket.sock")).unwrap();
    let events = UnixListener::bind(hypr.join(".socket2.sock")).unwrap();
    // Hyprland writes its lock once its sockets listen.
    std::fs::write(hypr.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
    std::thread::spawn(move || {
        for mut s in requests.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"unknown request");
        }
    });
    // The run dir moves to the temp dir: keep the compositor's socket reachable.
    let display = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
        .join(std::env::var_os("WAYLAND_DISPLAY").unwrap());
    let (rd, wr) = std::io::pipe().unwrap();
    let mut child = Command::new(NATIVE)
        .arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .env("WAYLAND_DISPLAY", &display)
        .env("XDG_RUNTIME_DIR", &run)
        .env("HYPRLAND_INSTANCE_SIGNATURE", "silent")
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    let log = testenv::Log::capture(rd);
    // The thread connected; nothing is ever written to it.
    let (mut silent, _) = events.accept().unwrap();
    // The shell answers from its main loop, which is where it reads signals: one sent while
    // it still starts ends it at once, with no exit to wait in.
    assert_eq!(
        call(&dir, &run, &display, "events"),
        hypr.join(".socket2.sock").display().to_string(),
        "the shell did not serve t:\n{}",
        log.text()
    );

    // SAFETY: kill(2) of our own, not yet reaped child with a valid signal number.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    // A shell whose thread stays in its read stays (the test hangs to nextest's slow-timeout).
    let status = testenv::wait_exit(&mut child);
    let log = log.text();
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}\n{log}");
    assert_eq!(
        silent.read(&mut [0u8; 1]).unwrap(),
        0,
        "the connection ended"
    );
}

/// The state a reload's exec leaves when SIGTERM arrives before the next image has its
/// handlers: the signal blocked and pending (execve(2) keeps the mask and what is pending),
/// the previous image's `Process` group still running and its instance still registered. The
/// next image ends that group, removes that instance and dies by the signal, before it
/// connects to anything.
#[test]
fn sigterm_pending_at_the_reload_exec_ends_the_next_image_in_order() {
    let t = tempfile::tempdir().unwrap();
    let (dir, run) = (t.path().join("shell"), t.path().join("run"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shell.qml"), FIXTURE).unwrap();
    let instances = run.join("rustshell/by-id");
    std::fs::create_dir_all(&instances).unwrap();
    let (rd, wr) = std::io::pipe().unwrap();
    // bash stands for the previous image. It registers an instance as that image did (an id
    // that starts with the process id as `ipc::base36` writes it, lowest digit first, and a
    // lock nobody holds) and names it in `RI_RELOADED`. Its job control gives the background
    // `sleep` a process group of its own, as a `Process` child has; it is then a child of
    // the shell bash execs, handed over by name.
    let mut cmd = Command::new("bash");
    cmd.args([
        "-c",
        r#"set -em
        digits=0123456789abcdefghijklmnopqrstuvwxyz n=$$ id=
        while id=$id${digits:n%36:1}; n=$((n/36)); [ "$n" -gt 0 ]; do :; done
        id=${id}previous
        mkdir "$2/$id"
        : > "$2/$id/instance.lock"
        sleep infinity &
        RI_HANDED_OVER=$! RI_RELOADED=$id/0 exec "$0" -p "$1""#,
        NATIVE,
    ])
    .arg(&dir)
    .arg(&instances)
    .env("RI_INTERP", "1")
    .env("WAYLAND_DISPLAY", run.join("no-compositor"))
    .env("XDG_RUNTIME_DIR", &run)
    .stdin(Stdio::null())
    .stdout(wr.try_clone().unwrap())
    .stderr(wr);
    let sigterm_blocked_and_pending = || {
        // SAFETY: sigset_t is plain integers, valid all-zero; sigemptyset(3) initializes it.
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: `set` is a live sigset_t.
        unsafe { libc::sigemptyset(&mut set) };
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            // SAFETY: `set` is initialized; `sig` is a valid signal number.
            unsafe { libc::sigaddset(&mut set, sig) };
        }
        // SAFETY: `set` is initialized; a null old set is not written.
        unsafe { libc::sigprocmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
        // SAFETY: raise(3) with a valid signal number, blocked just above.
        unsafe { libc::raise(libc::SIGTERM) };
        Ok(())
    };
    // SAFETY: `sigterm_blocked_and_pending` runs in the forked child before exec and only
    // calls sigemptyset, sigaddset, sigprocmask and raise, which are async-signal-safe.
    unsafe { cmd.pre_exec(sigterm_blocked_and_pending) };
    let mut child = cmd.spawn().unwrap();
    drop(cmd);
    let log = testenv::Log::capture(rd);
    let status = testenv::wait_exit(&mut child);
    assert_eq!(
        status.signal(),
        Some(libc::SIGTERM),
        "{status:?}\n{}",
        log.text()
    );
    let left: Vec<_> = std::fs::read_dir(&instances)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "the instance stayed: {left:?}");
    // `sleep infinity` holds the log's pipe open until it ends, which only the SIGKILL the
    // next image sends the handed-over group does: the log ends once both are gone.
    let text = log.wait(|_| false).unwrap_err();
    assert!(
        !text.contains("cannot connect"),
        "the shell went on:\n{text}"
    );
    // The signal was sent to the previous image: this one names it without a sender.
    assert!(
        text.lines()
            .any(|l| l == "rusty-impulse: ending on signal 15 (SIGTERM)"),
        "the signal that ended the shell is not in its log:\n{text}"
    );
}

/// A launcher that ignores SIGCHLD leaves it ignored across the exec, and the kernel would
/// reap the shell's children itself: a `Process` still reports its child's exit code.
#[test]
fn a_shell_started_with_sigchld_ignored_reads_exit_codes() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    const EXITS: &str = r#"import QtQuick
import Rustshell
import Rustshell.Io
ShellRoot {
    Process {
        running: true
        command: ["sh", "-c", "exit 7"]
        onExited: (exitCode, exitStatus) => console.log("EXITED " + exitCode)
    }
}
"#;
    let t = tempfile::tempdir().unwrap();
    let (dir, run) = (t.path().join("shell"), t.path().join("run"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&run).unwrap();
    std::fs::write(dir.join("shell.qml"), EXITS).unwrap();
    // The run dir moves to the temp dir: keep the compositor's socket reachable.
    let display = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default())
        .join(std::env::var_os("WAYLAND_DISPLAY").unwrap());
    let (rd, wr) = std::io::pipe().unwrap();
    let mut cmd = Command::new(NATIVE);
    cmd.arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .env("WAYLAND_DISPLAY", &display)
        .env("XDG_RUNTIME_DIR", &run)
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr);
    let ignore_sigchld = || {
        // SAFETY: signal(2) with a valid signal number and SIG_IGN.
        unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };
        Ok(())
    };
    // SAFETY: `ignore_sigchld` runs in the forked child before exec and only calls signal(2),
    // which is async-signal-safe.
    unsafe { cmd.pre_exec(ignore_sigchld) };
    let mut child = cmd.spawn().unwrap();
    drop(cmd);
    let log = testenv::Log::capture(rd);
    let exited = |l: &str| {
        l.lines()
            .find_map(|l| l.split_once("EXITED ").map(|(_, c)| c.trim().to_string()))
    };
    log.wait(|l| exited(l).is_some())
        .unwrap_or_else(|l| panic!("the Process never reported its exit:\n{l}"));
    // SAFETY: kill(2) of our own, not yet reaped child with a valid signal number.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let status = testenv::wait_exit(&mut child);
    let log = log.text();
    assert_eq!(exited(&log).as_deref(), Some("7"), "{log}");
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}\n{log}");
}
