use super::run;
use crate::svc::Stop;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;

fn sh(script: &str) -> Command {
    let mut c = Command::new("sh");
    c.args(["-c", script]);
    c
}

#[test]
fn collects_both_pipes_and_status() {
    let o = run(
        &mut sh("printf out; printf err >&2; exit 3"),
        &Default::default(),
    )
    .unwrap();
    assert_eq!((&o.out[..], &o.err[..]), (&b"out"[..], &b"err"[..]));
    assert_eq!(o.status.and_then(|s| s.code()), Some(3));
    // Output after the child exits, from a descendant still holding the pipe, is kept: the
    // descendant prints once the FIFO's only writer, the child, has exited.
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("gone");
    crate::testenv::fifo(&gone);
    let mut c = sh("(cat \"$0\" >/dev/null; echo late) & exec 3>\"$0\"; echo early");
    let o = run(c.arg(&gone), &Default::default()).unwrap();
    assert_eq!(String::from_utf8_lossy(&o.out), "early\nlate\n");
    let big = run(&mut sh("head -c 300000 /dev/zero"), &Default::default()).unwrap();
    assert_eq!(big.out.len(), 300000);
}

/// A stop ends the child's whole process group and returns at once (the old loop killed
/// only the direct child and then waited on EOF that a surviving descendant held off).
#[test]
fn stop_kills_the_group_and_returns_promptly() {
    let dir = tempfile::tempdir().unwrap();
    let pidf = dir.path().join("pid");
    crate::testenv::fifo(&pidf);
    let stop: Arc<Stop> = Default::default();
    let s = stop.clone();
    let p2 = pidf.clone();
    // The descendant never exits on its own: a run that waited for it would never return.
    let h = std::thread::spawn(move || {
        run(sh("sleep infinity & echo $! > \"$0\"; wait").arg(&p2), &s).unwrap()
    });
    let grandchild: i32 = std::fs::read_to_string(&pidf)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    stop.store(true, Ordering::Relaxed);
    let o = h.join().unwrap();
    assert!(o.status.is_none());
    // Only the stop's SIGTERM to the group ends it.
    crate::testenv::wait_gone(grandchild);
}

/// A descendant that ignores SIGTERM and keeps stdout open does not hold a stopped run.
#[test]
fn stopped_run_does_not_wait_for_pipe_eof() {
    let dir = tempfile::tempdir().unwrap();
    let ready = dir.path().join("ready");
    crate::testenv::fifo(&ready);
    let stop: Arc<Stop> = Default::default();
    let s = stop.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let r2 = ready.clone();
    let h = std::thread::spawn(move || {
        let mut c = sh("(trap '' TERM; echo > \"$0\"; exec sleep infinity) & exec sleep infinity");
        let ch = c
            .arg(&r2)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        tx.send(ch.id() as i32).unwrap();
        super::collect(ch, &s).unwrap()
    });
    use std::os::unix::process::CommandExt;
    let pgid = rx.recv().unwrap();
    // The descendant ignores SIGTERM from here on and never closes the pipes: a run waiting
    // for their EOF would never return.
    std::fs::read_to_string(&ready).unwrap();
    stop.store(true, Ordering::Relaxed);
    let o = h.join().unwrap();
    assert!(o.status.is_none());
    // SAFETY: kill(2) takes plain integers and touches no memory of ours.
    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

/// A filter on the calling thread that turns its poll(2)/ppoll(2) calls, and those of the
/// threads and processes it starts, into seccomp user notifications on the returned listener
/// (seccomp_unotify(2)), which the test answers with `SECCOMP_USER_NOTIF_FLAG_CONTINUE`: it
/// observes the calls, never changes them.
#[cfg(target_arch = "x86_64")]
fn notify_polls() -> std::os::fd::OwnedFd {
    use std::os::fd::FromRawFd;
    let insn = |code: u32, jt: u8, k: u32| libc::sock_filter {
        code: code as u16,
        jt,
        jf: 0,
        k,
    };
    let jeq = libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K;
    let ret = libc::BPF_RET | libc::BPF_K;
    let mut prog = [
        insn(
            libc::BPF_LD | libc::BPF_W | libc::BPF_ABS,
            0,
            std::mem::offset_of!(libc::seccomp_data, nr) as u32,
        ),
        insn(jeq, 2, libc::SYS_poll as u32),
        insn(jeq, 1, libc::SYS_ppoll as u32),
        insn(ret, 0, libc::SECCOMP_RET_ALLOW),
        insn(ret, 0, libc::SECCOMP_RET_USER_NOTIF),
    ];
    let fprog = libc::sock_fprog {
        len: prog.len() as u16,
        filter: prog.as_mut_ptr(),
    };
    // SAFETY: PR_SET_NO_NEW_PRIVS takes plain integers; it only affects this thread.
    let rc = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    assert_eq!(rc, 0, "{}", std::io::Error::last_os_error());
    // SAFETY: seccomp(2) reads the program `fprog` points to, which lives for the call; without
    // SECCOMP_FILTER_FLAG_TSYNC the filter applies to this thread only.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER,
            libc::SECCOMP_FILTER_FLAG_NEW_LISTENER,
            &fprog,
        )
    };
    assert!(fd >= 0, "seccomp: {}", std::io::Error::last_os_error());
    // SAFETY: seccomp just returned this listener descriptor, owned by nothing else.
    unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) }
}

/// Waiting on a child is one blocking poll, with no timeout: no wakeups while it runs, and
/// after it only one poll per readiness event (each pipe's EOF and the pidfd), so neither a
/// timeout loop nor a POLLHUP hot loop passes. The waiting thread's polls are seen as they are
/// made, as seccomp user notifications; the child runs until the test closes its stdin.
#[cfg(target_arch = "x86_64")]
#[test]
fn no_idle_wakeups_while_waiting() {
    use std::os::fd::AsRawFd;
    const EVENTS: usize = 3;
    let (held, release) = std::io::pipe().unwrap();
    let mut release = Some(release);
    let (tx, rx) = std::sync::mpsc::channel();
    let h = std::thread::spawn(move || {
        let listener = notify_polls();
        // SAFETY: gettid(2) takes no arguments and always succeeds.
        tx.send((unsafe { libc::gettid() }, listener)).unwrap();
        let o = run(Command::new("cat").stdin(held), &Default::default()).unwrap();
        // The end mark for the test: a poll of no fds.
        // SAFETY: poll(2) with no fds reads and writes no memory.
        unsafe { libc::poll(std::ptr::null_mut(), 0, 0) };
        o
    });
    let (tid, listener) = rx.recv().unwrap();
    let mut polls = 0;
    loop {
        // SAFETY: all-zero is a valid seccomp_notif, and SECCOMP_IOCTL_NOTIF_RECV requires it.
        let mut n: libc::seccomp_notif = unsafe { std::mem::zeroed() };
        // SAFETY: the ioctl writes one seccomp_notif into `n`.
        if unsafe { libc::ioctl(listener.as_raw_fd(), libc::SECCOMP_IOCTL_NOTIF_RECV, &mut n) } < 0
        {
            let e = std::io::Error::last_os_error();
            assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "NOTIF_RECV: {e}");
            continue;
        }
        let resp = libc::seccomp_notif_resp {
            id: n.id,
            val: 0,
            error: 0,
            flags: libc::SECCOMP_USER_NOTIF_FLAG_CONTINUE as u32,
        };
        // SAFETY: the ioctl reads the one seccomp_notif_resp `resp`. ENOENT (the caller died
        // meanwhile) needs no answer.
        if unsafe { libc::ioctl(listener.as_raw_fd(), libc::SECCOMP_IOCTL_NOTIF_SEND, &resp) } < 0 {
            let e = std::io::Error::last_os_error();
            assert_eq!(e.raw_os_error(), Some(libc::ENOENT), "NOTIF_SEND: {e}");
        }
        if n.pid as i32 != tid {
            continue;
        }
        let (nfds, timeout) = (n.data.args[1], n.data.args[2]);
        if nfds == 0 {
            break;
        }
        let forever = if i64::from(n.data.nr) == libc::SYS_poll {
            timeout as i32 == -1
        } else {
            timeout == 0
        };
        assert!(forever, "poll with a timeout: {:?}", n.data.args);
        polls += 1;
        assert!(polls <= 1 + EVENTS, "{polls} polls for {EVENTS} events");
        if polls == 1 {
            drop(release.take());
        }
    }
    let o = h.join().unwrap();
    assert_eq!(o.status.map(|s| s.signal()), Some(None));
}

#[test]
fn failed_poll_kills_the_group_instead_of_waiting_on_it() {
    // poll(2) failing for good (EINVAL, ENOMEM) leaves nothing to wait on: blocking in wait()
    // would hang on a child that never exits by itself.
    super::POLL_FAULT.with(|f| f.set(Some(libc::EINVAL)));
    let e = run(
        &mut sh("trap '' TERM; exec sleep infinity"),
        &Default::default(),
    )
    .unwrap_err();
    assert_eq!(e.raw_os_error(), Some(libc::EINVAL));
}

#[test]
fn killed_job_kills_its_groups_that_handle_sigterm() {
    // QProcess::kill: SIGKILL for what a job spawned, also a program ignoring SIGTERM.
    let stop = Arc::new(Stop::default());
    let g = super::with_stop(&stop, || {
        super::spawn(
            sh("trap '' TERM; exec sleep infinity")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null()),
        )
        .unwrap()
    });
    let run = {
        let stop = stop.clone();
        std::thread::spawn(move || run(&mut sh("trap '' TERM; echo; exec sleep infinity"), &stop))
    };
    stop.kill();
    assert_eq!(g.wait().unwrap().signal(), Some(libc::SIGKILL));
    assert!(run.join().unwrap().unwrap().status.is_none());
}

/// With SIGCHLD ignored (inherited from a launcher that ignores it) the kernel reaps the child
/// itself and wait(2) fails with `ECHILD`: the run ends with that error instead of polling
/// the exited child's pidfd, readable for good, without end. The child runs until the stop's
/// SIGTERM, which the run sends once it watches the child: one that exited on its own could
/// be reaped before the run opened its pidfd.
#[test]
fn failed_wait_ends_the_run() {
    const TEST: &str = concat!(module_path!(), "::failed_wait_ends_the_run");
    if !crate::testenv::in_own_process(TEST) {
        crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
        return;
    }
    // SAFETY: signal(2) with a valid signal number and SIG_IGN.
    unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };
    let stop: Arc<Stop> = Default::default();
    stop.store(true, Ordering::Relaxed);
    let e = run(Command::new("sleep").arg("infinity"), &stop).unwrap_err();
    assert_eq!(e.raw_os_error(), Some(libc::ECHILD));
}
