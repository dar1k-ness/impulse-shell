//! Orderly shutdown on SIGTERM/SIGINT/SIGHUP (session logout, `systemctl stop`, Ctrl+C).
//!
//! The handler only writes the signal number to a non-blocking self-pipe (write(2) is
//! async-signal-safe, signal-safety(7)); the main loop watches the pipe, ends like `Qt.quit()`
//! and the shell exits through its normal path (pending saves written, instance unregistered,
//! log flushed) before dying by the signal. The signals stay unblocked: a blocked mask (as a
//! signalfd needs) would be inherited by every child the shell or its libraries spawn, which
//! then could not be stopped with SIGTERM. A caught signal reverts to its default action in a
//! child at execve(2).
//!
//! The handler is installed only where the main loop starts to watch the pipe: a signal
//! during startup (which blocks on the compositor's answers) ends the shell at once, through
//! the handlers of `qml::native::install_shutdown_handlers`, instead of waiting in a pipe that
//! nothing reads yet.
//!
//! A reload's exec keeps the pipe's read end open ([`keep_across_exec`]): a signal written to
//! it after the main loop last read it is still there for the next image. The three signals
//! are also blocked for the exec ([`block`]): execve(2) resets a caught signal to its default
//! action but keeps the mask and what is pending, so one sent before the next image has its
//! handlers stays pending instead of killing it there. The next image reads both
//! ([`pending_after_exec`]) once the previous image's children are ended, shuts down if there
//! is one, and unblocks them ([`unblock`]) before it starts a thread or a child.

use qml::native::SignalSender;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicI32, Ordering};

/// The signals that request termination.
pub const SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

/// Write end of the self-pipe, -1 until [`SignalPipe::install`].
static WRITE_FD: AtomicI32 = AtomicI32::new(-1);
/// Read end of the self-pipe, -1 until [`SignalPipe::install`].
static READ_FD: AtomicI32 = AtomicI32::new(-1);

/// Names the previous image's read end, open across a reload's exec ([`keep_across_exec`]).
pub const INHERITED_ENV: &str = "RI_SIGNAL_PIPE";

/// Who sent the last termination signal the handler saw: a pid (0: a process of another pid
/// namespace), [`BY_KERNEL`], or [`NOT_SEEN`] before the first one.
static SENDER: AtomicI32 = AtomicI32::new(NOT_SEEN);
const NOT_SEEN: i32 = -1;
const BY_KERNEL: i32 = -2;

extern "C" fn on_signal(sig: libc::c_int, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
    // SAFETY: with SA_SIGINFO the kernel passes a valid siginfo_t (or null).
    let sender = match unsafe { qml::native::signal_sender(info) } {
        SignalSender::Process(pid) => pid,
        SignalSender::Kernel => BY_KERNEL,
    };
    SENDER.store(sender, Ordering::Relaxed);
    // Only async-signal-safe work (signal-safety(7)): errno is saved and restored around
    // write(2), which may set it.
    // SAFETY: __errno_location takes no arguments and returns this thread's errno slot.
    let errno = unsafe { libc::__errno_location() };
    // SAFETY: the slot is valid and aligned for the whole life of the thread.
    let saved = unsafe { *errno };
    let fd = WRITE_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        let b = sig as u8;
        // SAFETY: writes the one byte of `b`; `fd` is the self-pipe's write end, never closed.
        unsafe { libc::write(fd, (&raw const b).cast(), 1) };
    }
    // SAFETY: as above.
    unsafe { *errno = saved };
}

/// The read end of the self-pipe the termination signals are written to.
pub struct SignalPipe(OwnedFd);

impl SignalPipe {
    /// Creates the pipe and installs the handler for [`SIGNALS`] (except one inherited as
    /// ignored, as `nohup` sets it). Call once at startup, and only in the process that runs
    /// the main loop, right before the loop watches the pipe: any other would not end by these
    /// signals.
    pub fn install() -> std::io::Result<Self> {
        let mut fds = [0; 2];
        // SAFETY: `fds` has room for both ends.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: fresh descriptors we own; the write end lives for the whole process.
        let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        WRITE_FD.store(fds[1], Ordering::Relaxed);
        READ_FD.store(fds[0], Ordering::Relaxed);
        for sig in SIGNALS {
            // SAFETY: `sigaction` is a C struct of integers and a sigset_t, valid all-zero.
            let mut prev: libc::sigaction = unsafe { std::mem::zeroed() };
            // SAFETY: a null `act` only stores the current disposition in `prev`.
            if unsafe { libc::sigaction(sig, std::ptr::null(), &mut prev) } != 0
                || prev.sa_sigaction == libc::SIG_IGN
            {
                continue;
            }
            // SAFETY: as for `prev`.
            let mut act: libc::sigaction = unsafe { std::mem::zeroed() };
            act.sa_sigaction = on_signal as *const () as usize;
            act.sa_flags = libc::SA_RESTART | libc::SA_SIGINFO;
            // SAFETY: `act.sa_mask` is a valid sigset_t to initialize.
            unsafe { libc::sigemptyset(&mut act.sa_mask) };
            // SAFETY: installs `on_signal`, an `extern "C"` handler with the three arguments
            // of SA_SIGINFO doing only async-signal-safe work; a null `oldact` is allowed.
            unsafe { libc::sigaction(sig, &act, std::ptr::null_mut()) };
        }
        Ok(Self(read))
    }

    pub fn fd(&self) -> &OwnedFd {
        &self.0
    }

    /// The next signal received, `None` once none is pending.
    pub fn read(&self) -> Option<libc::c_int> {
        let mut b = 0u8;
        // SAFETY: reads at most one byte into `b`.
        let n = unsafe { libc::read(self.0.as_raw_fd(), (&raw mut b).cast(), 1) };
        (n == 1).then_some(libc::c_int::from(b))
    }
}

/// Writes the log line of a shell that `sig` ends ([`qml::native::log_ending_signal`]) with
/// the sender the handler saw and, while that process still exists, its name. A signal found
/// pending after a reload's exec was sent to the previous image: its sender is not known.
pub fn log_ending(sig: libc::c_int) {
    match SENDER.load(Ordering::Relaxed) {
        NOT_SEEN => qml::native::log_ending_signal(sig, None, None),
        BY_KERNEL => qml::native::log_ending_signal(sig, Some(SignalSender::Kernel), None),
        pid => {
            let name = std::fs::read(format!("/proc/{pid}/comm")).ok();
            let name = name.as_deref().map(<[u8]>::trim_ascii_end);
            qml::native::log_ending_signal(sig, Some(SignalSender::Process(pid)), name);
        }
    }
}

/// The read end [`INHERITED_ENV`] names, if it is an open pipe: taken over (closed on drop).
fn inherited() -> Option<OwnedFd> {
    let fd: i32 = std::env::var(INHERITED_ENV).ok()?.parse().ok()?;
    // SAFETY: `stat` is a C struct of integers, valid all-zero.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstat(2) of any number only fills `st` or fails with EBADF.
    let r = unsafe { libc::fstat(fd, &mut st) };
    if fd < 0 || r != 0 || st.st_mode & libc::S_IFMT != libc::S_IFIFO {
        return None;
    }
    // SAFETY: an open pipe the previous image left to this one; nothing else here owns it.
    Some(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Lets the pipe's read end, with the signals not read yet, survive the exec of a reload:
/// returns the variable that names it to the next image. [`close_on_exec`] undoes it when
/// the exec fails. `None` before [`SignalPipe::install`] or if fcntl(2) fails.
pub fn keep_across_exec() -> Option<(&'static str, String)> {
    let fd = READ_FD.load(Ordering::Relaxed);
    // SAFETY: fcntl(2) on the pipe's read end, open for the whole process.
    (fd >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFD, 0) } == 0)
        .then(|| (INHERITED_ENV, fd.to_string()))
}

/// Closes the pipe's read end on exec again, after [`keep_across_exec`] and a failed exec.
pub fn close_on_exec() {
    let fd = READ_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        // SAFETY: as in `keep_across_exec`.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
}

/// Puts SIGCHLD back to its default action. A launcher that ignores it leaves it ignored
/// across execve(2), and the kernel then reaps every child itself: wait(2) fails with
/// `ECHILD` and no exit status is ever read. Call at startup, before any child exists.
pub fn default_sigchld() {
    // SAFETY: signal(2) with a valid signal number and SIG_DFL.
    unsafe { libc::signal(libc::SIGCHLD, libc::SIG_DFL) };
}

/// [`SIGNALS`] as a signal set.
fn signal_set() -> libc::sigset_t {
    // SAFETY: sigset_t is plain integers, valid all-zero; sigemptyset(3) initializes it.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: `set` is a live sigset_t.
    unsafe { libc::sigemptyset(&mut set) };
    for sig in SIGNALS {
        // SAFETY: `set` is initialized and `sig` a valid signal number.
        unsafe { libc::sigaddset(&mut set, sig) };
    }
    set
}

fn set_mask(how: libc::c_int, set: &libc::sigset_t) -> libc::sigset_t {
    // SAFETY: as in `signal_set`; pthread_sigmask(3) fills it below.
    let mut old: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: both point at live signal sets; `how` is one of the SIG_* constants.
    unsafe { libc::pthread_sigmask(how, set, &mut old) };
    old
}

/// Blocks [`SIGNALS`] in the calling thread, for the exec of a reload on it: the mask and the
/// pending signals survive execve(2). Returns the previous mask, for [`restore`] when the
/// exec fails.
pub fn block() -> libc::sigset_t {
    set_mask(libc::SIG_BLOCK, &signal_set())
}

/// Puts back the mask [`block`] returned; a signal that became pending meanwhile is delivered.
pub fn restore(old: &libc::sigset_t) {
    set_mask(libc::SIG_SETMASK, old);
}

/// Unblocks [`SIGNALS`]: the image a reload executed, once it has seen that none is pending
/// ([`pending_after_exec`]). Before it starts a thread or a child, which would inherit the
/// blocked mask.
pub fn unblock() {
    set_mask(libc::SIG_UNBLOCK, &signal_set());
}

/// In the image a reload executed, while [`SIGNALS`] are still blocked: the termination signal
/// the previous image caught after its main loop last read its pipe ([`INHERITED_ENV`], which
/// the caller removes), or one sent since and still pending. One inherited as ignored is not
/// one: unblocked, it is discarded.
pub fn pending_after_exec() -> Option<libc::c_int> {
    if let Some(sig) = inherited().and_then(|old| SignalPipe(old).read()) {
        return Some(sig);
    }
    // SAFETY: as in `signal_set`; sigpending(2) fills it below.
    let mut pending: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: `pending` is a live sigset_t for sigpending(2) to fill.
    if unsafe { libc::sigpending(&mut pending) } != 0 {
        return None;
    }
    SIGNALS.into_iter().find(|&sig| {
        // SAFETY: `sigaction` is a C struct of integers and a sigset_t, valid all-zero.
        let mut act: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: `pending` was filled by sigpending(2); `sig` is a valid signal number.
        let is_pending = unsafe { libc::sigismember(&pending, sig) } == 1;
        // SAFETY: a null new action only stores the current disposition in `act`.
        let known = unsafe { libc::sigaction(sig, std::ptr::null(), &mut act) } == 0;
        is_pending && known && act.sa_sigaction != libc::SIG_IGN
    })
}

/// Ends the process by `sig` after the orderly shutdown, so the parent (systemd, the session)
/// sees the termination it asked for (`WIFSIGNALED`).
pub fn exit_by(sig: libc::c_int) -> ! {
    // SAFETY: restores the default disposition of a valid signal number.
    unsafe { libc::signal(sig, libc::SIG_DFL) };
    // Still blocked in an image a reload executed that shuts down at once: a blocked signal
    // would stay pending, and the process would exit with a status instead.
    unblock();
    // SAFETY: raise(3) of a valid signal number.
    unsafe { libc::raise(sig) };
    std::process::exit(128 + sig);
}

/// Whether a process with this pid exists (kill(2) with signal 0: only ESRCH says none does).
pub fn process_exists(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 sends nothing; it only checks that the pid exists.
    let sent = unsafe { libc::kill(pid, 0) } == 0;
    sent || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Whether the process runs a single thread (`/proc/self/task` has one entry): the condition
/// for `std::env::set_var`.
pub fn single_threaded() -> bool {
    std::fs::read_dir("/proc/self/task").is_ok_and(|d| d.count() == 1)
}

/// Whether the calling thread blocks `sig`.
#[cfg(test)]
pub(crate) fn blocked(sig: libc::c_int) -> bool {
    // SAFETY: sigset_t is plain integers, valid all-zero; pthread_sigmask(3) fills it below.
    let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: a null new set only stores the current mask in `mask`.
    unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut mask) };
    // SAFETY: `mask` was filled by pthread_sigmask(3); `sig` is a valid signal number.
    unsafe { libc::sigismember(&mask, sig) == 1 }
}

#[cfg(test)]
mod tests {
    /// Runs `f` in a forked child (signal dispositions must not leak into the test harness)
    /// and returns its wait status.
    fn in_child(f: impl FnOnce() -> i32) -> i32 {
        // SAFETY: the child only runs `f` and `_exit`s; it never returns into the test harness.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            let code = f();
            // SAFETY: ends the forked child without running the parent's exit handlers.
            unsafe { libc::_exit(code) };
        }
        let mut st = 0;
        // SAFETY: waits for our own child; `st` is a valid out pointer.
        assert_eq!(unsafe { libc::waitpid(pid, &mut st, 0) }, pid);
        st
    }

    /// SIGTERM reaches the pipe instead of killing the process, and a child spawned
    /// afterwards neither blocks nor catches it.
    #[test]
    fn signals_reach_the_pipe_and_children_start_clean() {
        let st = in_child(|| {
            let p = super::SignalPipe::install().unwrap();
            // SAFETY: raise(3) of a valid signal number, handled by the handler just installed.
            unsafe { libc::raise(libc::SIGTERM) };
            if p.read() != Some(libc::SIGTERM) || p.read().is_some() {
                return 1;
            }
            let out = std::process::Command::new("cat")
                .arg("/proc/self/status")
                .output()
                .unwrap();
            let status = String::from_utf8_lossy(&out.stdout).into_owned();
            let field = |name: &str| {
                status
                    .lines()
                    .find_map(|l| l.strip_prefix(name))
                    .map(|v| u64::from_str_radix(v.trim(), 16).unwrap())
                    .unwrap()
            };
            let term = 1 << (libc::SIGTERM - 1);
            if field("SigBlk:") & term != 0 || field("SigCgt:") & term != 0 {
                return 2;
            }
            0
        });
        assert!(
            libc::WIFEXITED(st) && libc::WEXITSTATUS(st) == 0,
            "status {st:#x}"
        );
    }

    use super::blocked as mask_has;

    /// A launcher that ignores SIGCHLD leaves it ignored across the exec; the shell puts the
    /// default action back, so wait(2) reads a child's exit status again.
    #[test]
    fn default_sigchld_lets_children_be_waited_for() {
        let st = in_child(|| {
            // SAFETY: signal(2) with a valid signal number and SIG_IGN.
            unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };
            super::default_sigchld();
            let st = std::process::Command::new("sh")
                .args(["-c", "exit 7"])
                .status();
            match st.map(|s| s.code()) {
                Ok(Some(7)) => 0,
                _ => 1,
            }
        });
        assert!(
            libc::WIFEXITED(st) && libc::WEXITSTATUS(st) == 0,
            "status {st:#x}"
        );
    }

    /// What a reload's exec leaves to the next image: a signal sent while the three are
    /// blocked stays pending (it would kill an image that has no handler yet), is found by
    /// `pending_after_exec`, and `exit_by` then ends the process by it, blocked as it is.
    #[test]
    fn a_signal_blocked_for_the_exec_is_pending_and_ends_the_next_image() {
        for sig in super::SIGNALS {
            let st = in_child(|| {
                let old = super::block();
                if super::pending_after_exec().is_some() {
                    return 1;
                }
                // SAFETY: raise(3) of a valid signal number, blocked: it stays pending.
                unsafe { libc::raise(sig) };
                if super::pending_after_exec() != Some(sig) {
                    return 2;
                }
                // SAFETY: `old` was filled by pthread_sigmask(3).
                if unsafe { libc::sigismember(&old, sig) } != 0 || !mask_has(sig) {
                    return 3;
                }
                super::exit_by(sig)
            });
            assert!(
                libc::WIFSIGNALED(st) && libc::WTERMSIG(st) == sig,
                "signal {sig}: status {st:#x}"
            );
        }
    }

    /// `unblock` and `restore` leave the three signals deliverable again, and a signal
    /// inherited as ignored (nohup) does not end the next image.
    #[test]
    fn unblocked_signals_are_delivered_and_ignored_ones_are_not_pending() {
        let st = in_child(|| {
            let old = super::block();
            super::restore(&old);
            if super::SIGNALS.into_iter().any(mask_has) {
                return 1;
            }
            super::block();
            super::unblock();
            if super::SIGNALS.into_iter().any(mask_has) {
                return 2;
            }
            super::block();
            // SAFETY: signal(2) with a valid signal number and SIG_IGN.
            unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
            // SAFETY: raise(3) of a valid signal number, blocked: it stays pending.
            unsafe { libc::raise(libc::SIGHUP) };
            if super::pending_after_exec().is_some() {
                return 3;
            }
            0
        });
        assert!(
            libc::WIFEXITED(st) && libc::WEXITSTATUS(st) == 0,
            "status {st:#x}"
        );
    }

    #[test]
    fn single_threaded_counts_threads() {
        let st = in_child(|| {
            if !super::single_threaded() {
                return 1;
            }
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let t = std::thread::spawn(move || rx.recv());
            let many = !super::single_threaded();
            drop(tx);
            let _ = t.join();
            if many { 0 } else { 2 }
        });
        assert!(
            libc::WIFEXITED(st) && libc::WEXITSTATUS(st) == 0,
            "status {st:#x}"
        );
    }

    #[test]
    fn exit_by_ends_with_the_signal() {
        let st = in_child(|| {
            let _p = super::SignalPipe::install().unwrap();
            super::exit_by(libc::SIGHUP)
        });
        assert!(libc::WIFSIGNALED(st) && libc::WTERMSIG(st) == libc::SIGHUP);
    }
}
