//! Signals to processes found by a scan of `/proc`, on real processes. `live::kill_exact_wait`: it signals only processes whose executable is
//! the name (never one that merely has it in its argv) and returns as soon as they exit.
//! `true` means it saw every target exit before it gave up; the calls that must return on the
//! targets' exit get a give-up event that never comes, so returning at all proves it.
//! A signal of any of them goes only to a process that is still the one the scan saw.
use colorgen::live::{Live, kill_exact_wait, kill_exact_wait_until, signal_scanned};
use colorgen::sys::Sys;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

/// A private executable called `name`: a symlink to bash (the kernel names a process after the
/// path it was exec'd by; a copy could hit ETXTBSY from a parallel test's fork).
fn exe(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kill-exact-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    std::os::unix::fs::symlink("/bin/bash", &p).unwrap();
    p
}

/// A bash script that runs until a signal ends it, the test writes a line to its stdin (a pipe
/// the test holds) or closes it. `ready` is printed once its traps are set.
struct Target {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// The script of a target. It answers a signal that has a trap whenever the signal comes: bash
/// runs a trap between two commands, and at once only while it waits for a background command
/// in the `wait` builtin (bash manual, Signals); in a builtin `read` it does not, a signal that
/// comes just before the read blocks is answered when the read ends. So the line is read by
/// `head` in the background and the script waits for it with `wait`; `$!` is that `head`.
/// `ready` comes from the background process once it is another program: until then it is a
/// process of the target's name with no traps.
///
/// A target is sent a signal that has a trap once: bash drops a signal that comes while its trap
/// for that signal still runs, and the line the trap prints comes before the trap is over.
fn script(traps: &str) -> String {
    format!(
        "{traps}\nexec 3<&0\nsh -c 'echo ready; exec head -n 1 >/dev/null' <&3 &\n\
         while kill -0 \"$!\" 2>/dev/null; do wait \"$!\"; done"
    )
}

impl Target {
    fn start(exe: &Path, args: &[&str], traps: &str) -> Target {
        let mut child = Command::new(exe)
            .args(["-c", &script(traps)])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert_eq!(line, "ready\n", "{} never started", exe.display());
        Target {
            child,
            stdin,
            stdout,
        }
    }

    fn line(&mut self) -> String {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        line
    }

    /// Not exited: the test reaps it only in `end`, so an exited target stays a zombie.
    fn alive(&self) -> bool {
        state(self.child.id()) != 'Z'
    }

    fn end(mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

/// `/proc/<pid>/stat`'s state: `Z` once the process exited and nobody reaped it yet.
fn state(pid: u32) -> char {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    stat[stat.rfind(')').unwrap() + 2..].chars().next().unwrap()
}

/// `kill_exact_wait_until` with a give-up event that never comes: the read end of a pipe whose
/// write end stays open in this process.
fn never(name: &str, sig: i32) -> bool {
    let (r, w) = std::io::pipe().unwrap();
    let saw = kill_exact_wait_until(name, sig, r.as_fd());
    drop(w);
    saw
}

#[test]
fn waits_for_exit_and_spares_argv_matches() {
    let name = format!("kx{}", std::process::id());
    let target = exe(&name);
    // On SIGTERM it says so and exits only once the test writes a line: the call must wait for
    // that exit, not return at once.
    let mut slow = Target::start(&target, &[], "trap 'echo term; wait \"$!\"; exit 0' TERM");
    let decoy = Target::start(Path::new("/bin/bash"), &[&name, &name], "");
    let pid = slow.child.id();

    let n = name.clone();
    let call = std::thread::spawn(move || {
        let saw = never(&n, libc::SIGTERM);
        (saw, state(pid))
    });
    assert_eq!(slow.line(), "term\n");
    writeln!(slow.stdin).unwrap();
    let (saw, at_return) = call.join().unwrap();
    assert!(saw, "did not see the exit");
    assert_eq!(at_return, 'Z', "returned before the target exited");
    assert!(!slow.alive(), "target still running");
    assert!(decoy.alive(), "argv-only match was signalled");
    decoy.end();
}

#[test]
fn sigkill_returns_promptly_and_nothing_to_kill_is_immediate() {
    let name = format!("ky{}", std::process::id());
    let target = exe(&name);
    let kids: Vec<Target> = (0..3)
        .map(|_| Target::start(&target, &[], "trap '' TERM"))
        .collect();
    assert!(never(&name, libc::SIGKILL), "did not see the exits");
    for k in &kids {
        assert!(!k.alive());
    }

    assert!(never(&name, libc::SIGKILL));
}

#[test]
fn gives_up_while_the_target_runs() {
    let name = format!("kz{}", std::process::id());
    let target = exe(&name);
    let mut stubborn = Target::start(&target, &[], "trap 'echo term' TERM");
    let (give_up, armed) = std::io::pipe().unwrap();
    let (tx, returned) = std::sync::mpsc::channel();
    let n = name.clone();
    std::thread::spawn(move || {
        let _ = tx.send(kill_exact_wait_until(&n, libc::SIGTERM, give_up.as_fd()));
    });
    assert_eq!(stubborn.line(), "term\n");
    assert_eq!(
        returned.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty),
        "returned with the target running and no give-up"
    );
    drop(armed);
    assert!(!returned.recv().unwrap(), "claimed the target exited");
    assert!(stubborn.alive());
    stubborn.end();

    // The deadline form: its timerfd fires (at once for a zero deadline) and it gives up the same.
    let mut stubborn = Target::start(&target, &[], "trap 'echo term' TERM");
    assert!(!kill_exact_wait(&name, libc::SIGTERM, Duration::ZERO));
    assert_eq!(stubborn.line(), "term\n");
    assert!(stubborn.alive());
    stubborn.end();
}

/// A target that answers `SIGUSR1` with a line.
fn answers_usr1(exe: &Path) -> Target {
    Target::start(exe, &[], "trap 'echo usr1' USR1")
}

/// Ends `t` by a line on its stdin; what it printed before it ended. A `SIGUSR1` sent before
/// this call shows here as its line.
fn printed_until_end(mut t: Target) -> String {
    writeln!(t.stdin).unwrap();
    let mut all = String::new();
    std::io::Read::read_to_string(&mut t.stdout, &mut all).unwrap();
    t.child.wait().unwrap();
    all
}

/// The scan is old when the signal goes out: a process of it may have exited and its pid gone
/// to another process, which then is not the one to signal. The scans here say what was seen
/// before the pid changed hands: the pid of `other` as a process of the name.
#[test]
fn a_pid_that_went_to_another_process_is_not_signalled() {
    let name = format!("kr{}", std::process::id());
    let target = exe(&name);
    let mut reloads = answers_usr1(&target);
    let other = answers_usr1(&exe(&format!("ko{}", std::process::id())));
    let scan = vec![
        (other.child.id() as i32, name.clone(), vec![name.clone()]),
        (reloads.child.id() as i32, name.clone(), vec![name.clone()]),
    ];
    let sent = signal_scanned(&scan, libc::SIGUSR1, &|comm, _| comm == name);
    assert_eq!(sent.len(), 1, "signalled the pid's new process");
    assert_eq!(reloads.line(), "usr1\n");

    // pywal's reload of polybar, by a part of the name.
    let polybar = format!("polybar{}", std::process::id());
    let mut bar = answers_usr1(&exe(&polybar));
    let scan = vec![
        (
            other.child.id() as i32,
            polybar.clone(),
            vec![polybar.clone()],
        ),
        (
            bar.child.id() as i32,
            polybar.clone(),
            vec![polybar.clone()],
        ),
    ];
    colorgen::kde::pywal::signal_polybar(&scan);
    assert_eq!(bar.line(), "usr1\n");

    // The reload of the shell's colours (kitty): by its own scan, to the process of the name.
    reloads.end();
    let mut reloads = answers_usr1(&target);
    Live.signal_named(&name, libc::SIGUSR1);
    assert_eq!(reloads.line(), "usr1\n");

    assert_eq!(printed_until_end(other), "", "a signal meant for another");
    reloads.end();
    bar.end();
}

/// Where a traced target stopped.
enum Stop {
    /// At the entry of a system call: its number and arguments.
    Entry(u64, [u64; 6]),
    Exit,
    /// A signal came for it; it gets it when it is let run with that signal.
    Signal(i32),
    Ended,
}

/// `ptrace(2)` on the child `pid` this thread traces.
fn ptrace(request: libc::c_uint, pid: i32, addr: usize, data: usize) -> i64 {
    // SAFETY: the requests used here take integers, or in `data` a struct that outlives the call.
    unsafe { libc::ptrace(request, pid, addr, data) }
}

/// Waits for the traced child to stop: the signal it stopped with, `None` once it ended.
fn stop_signal(pid: i32) -> Option<i32> {
    let mut status = 0;
    // SAFETY: `status` is a live integer.
    let got = unsafe { libc::waitpid(pid, &mut status, 0) };
    assert_eq!(got, pid);
    libc::WIFSTOPPED(status).then(|| libc::WSTOPSIG(status))
}

/// Lets the traced target run to its next stop, with `signal` delivered (0: none).
fn step(pid: i32, signal: i32) -> Stop {
    assert_eq!(ptrace(libc::PTRACE_SYSCALL, pid, 0, signal as usize), 0);
    match stop_signal(pid) {
        None => return Stop::Ended,
        Some(s) if s != libc::SIGTRAP | 0x80 => return Stop::Signal(s),
        Some(_) => {}
    }
    // SAFETY: a C struct of integers, all zero is a value of it.
    let mut info: libc::ptrace_syscall_info = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info);
    let filled = ptrace(
        libc::PTRACE_GET_SYSCALL_INFO,
        pid,
        size,
        &raw mut info as usize,
    );
    assert!(filled > 0);
    if info.op != libc::PTRACE_SYSCALL_INFO_ENTRY {
        return Stop::Exit;
    }
    // SAFETY: the kernel filled the entry member, `op` says so.
    let entry = unsafe { info.u.entry };
    Stop::Entry(entry.nr, entry.args)
}

/// Runs a target with a `SIGTERM` trap under ptrace up to the system call it waits in: a read
/// of its stdin or a `wait4` that blocks. `at` is the system call, counted from the one that
/// set the trap, at whose entry a `SIGTERM` is sent; the target then runs on to the next time
/// it goes to wait. The count of system calls up to the wait, and whether the target wrote
/// `term` to its stdout.
fn runs_to_its_wait(exe: &Path, at: Option<usize>) -> (usize, bool) {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new(exe);
    cmd.args(["-c", &script("trap 'echo term' TERM")])
        .stdin(Stdio::piped())
        .stdout(Stdio::null());
    // SAFETY: the closure makes one async-signal-safe system call between fork and exec.
    unsafe {
        cmd.pre_exec(|| match ptrace(libc::PTRACE_TRACEME, 0, 0, 0) {
            -1 => Err(std::io::Error::last_os_error()),
            _ => Ok(()),
        });
    }
    let mut child = cmd.spawn().unwrap();
    let pid = child.id() as i32;
    // It stops with SIGTRAP once it has exec'd.
    assert_eq!(stop_signal(pid), Some(libc::SIGTRAP));
    let options = libc::PTRACE_O_TRACESYSGOOD | libc::PTRACE_O_EXITKILL;
    assert_eq!(ptrace(libc::PTRACE_SETOPTIONS, pid, 0, options as usize), 0);

    let (mut trap_set, mut sent, mut answered, mut calls, mut signal) = (false, false, false, 0, 0);
    loop {
        let (nr, args) = match step(pid, std::mem::take(&mut signal)) {
            Stop::Entry(nr, args) => (nr as i64, args),
            Stop::Exit => continue,
            Stop::Signal(s) => {
                signal = s;
                continue;
            }
            Stop::Ended => panic!("the target ended"),
        };
        if !trap_set {
            // A handler, not the default action bash sets for a moment to learn the old one:
            // the first word of the `sigaction` the call is given.
            trap_set = nr == libc::SYS_rt_sigaction
                && args[0] == libc::SIGTERM as u64
                && args[1] != 0
                && ptrace(libc::PTRACE_PEEKDATA, pid, args[1] as usize, 0) > 1;
            continue;
        }
        calls += 1;
        if nr == libc::SYS_write && args[0] == 1 {
            let word = ptrace(libc::PTRACE_PEEKDATA, pid, args[1] as usize, 0);
            answered |= word.to_ne_bytes().starts_with(b"term\n");
        }
        let waits = (nr == libc::SYS_read && args[0] == 0)
            || (nr == libc::SYS_wait4 && args[2] & libc::WNOHANG as u64 == 0);
        if waits && (sent || at.is_none()) {
            break;
        }
        if at == Some(calls) {
            // SAFETY: a signal to the child.
            let rc = unsafe { libc::kill(pid, libc::SIGTERM) };
            assert_eq!(rc, 0);
            sent = true;
        }
    }
    child.kill().unwrap();
    child.wait().unwrap();
    (calls, answered)
}

/// A test that waits for the answer of a target that missed its signal waits for good. So: a
/// `SIGTERM` at the entry of each system call of the target, from the one that sets the trap to
/// the one it waits in, and the trap has run by the time the target goes to wait.
#[test]
fn a_signal_at_any_system_call_of_the_target_runs_its_trap() {
    let target = exe(&format!("kt{}", std::process::id()));
    let (calls, answered) = runs_to_its_wait(&target, None);
    assert!(calls > 0 && !answered, "{calls} {answered}");
    for at in 1..=calls {
        assert!(
            runs_to_its_wait(&target, Some(at)).1,
            "SIGTERM at system call {at} of {calls} after the trap was set: \
             the target went to wait with its trap not run"
        );
    }
}
