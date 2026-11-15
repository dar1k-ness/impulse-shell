//! A child process run to completion on the calling (pool) thread with its stdout and
//! stderr collected, and nothing polled: one `poll(2)` waits on both pipes, the child's
//! pidfd and a stop eventfd. The child leads its own process group, so a stop ends it and
//! every descendant at once, and a stopped run returns when the child has exited, not when
//! some descendant that kept a pipe open lets go of it.
use crate::svc::Stop;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[derive(Debug, Default)]
pub struct Output {
    pub out: Vec<u8>,
    pub err: Vec<u8>,
    /// `None` when the run was stopped.
    pub status: Option<ExitStatus>,
}

/// Spawns `cmd` (stdout/stderr piped, its own process group, tracked as a [`Group`]) and
/// collects it. On `stop` the group gets SIGTERM and the run returns as soon as the child has
/// exited.
pub fn run(cmd: &mut Command, stop: &Arc<Stop>) -> std::io::Result<Output> {
    let ch = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    collect(ch, stop)
}

/// A child leading its own process group, tracked as the shell's `Process` children are: a
/// reload ([`crate::native::end_handed_over`]) SIGKILLs its group, the shell's exit SIGTERMs it,
/// and the child guard ends it when the shell is killed outright. Dropped before [`wait`] it
/// is SIGKILLed with its group, as QProcess::~QProcess kills its child.
///
/// [`wait`]: Group::wait
pub struct Group {
    ch: Option<Child>,
    /// The running job's kill (see [`with_stop`]) SIGKILLs the group.
    kill: Option<Arc<crate::svc::Target>>,
}

thread_local! {
    static JOB: std::cell::RefCell<Option<Arc<Stop>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` (an in-process job) with every [`Group`] it spawns on this thread bound to `stop`:
/// [`Stop::kill`], QProcess::kill on the job's `Process`, SIGKILLs their groups.
pub fn with_stop<R>(stop: &Arc<Stop>, f: impl FnOnce() -> R) -> R {
    struct Reset(Option<Arc<Stop>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            let prev = self.0.take();
            JOB.with(|j| *j.borrow_mut() = prev);
        }
    }
    let _reset = Reset(JOB.with(|j| j.replace(Some(stop.clone()))));
    f()
}

/// Spawns `cmd` leading its own process group, tracked as a [`Group`]: what an in-process
/// job starts in place of the script's program (kdialog, cliphist, songrec, wf-recorder).
pub fn spawn(cmd: &mut Command) -> std::io::Result<Group> {
    let ch = cmd.process_group(0).spawn()?;
    let pid = ch.id() as i32;
    track(pid);
    let kill = JOB.with(|j| j.borrow().as_ref().map(|s| s.target(-pid, None)));
    Ok(Group { ch: Some(ch), kill })
}

fn track(pid: i32) {
    crate::native::children::add(pid);
    crate::childguard::add(pid);
}

/// Call before reaping `pid`: once reaped its (group) id may be reused.
fn untrack(pid: i32) {
    crate::native::children::remove(pid);
    crate::childguard::remove(pid);
}

impl Group {
    pub fn child(&self) -> &Child {
        self.ch.as_ref().expect("a live Group has its child")
    }

    pub fn child_mut(&mut self) -> &mut Child {
        self.ch.as_mut().expect("a live Group has its child")
    }

    pub fn id(&self) -> u32 {
        self.child().id()
    }

    /// Waits for the child to exit (waitid(2) `WNOWAIT`), untracks it, then reaps it.
    pub fn wait(mut self) -> std::io::Result<ExitStatus> {
        let mut ch = self.ch.take().expect("a live Group has its child");
        let pid = ch.id() as i32;
        // SAFETY: siginfo_t is integers and pointers, for which all zeros (null) is valid.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `info` is a live siginfo_t for the kernel to fill; the child is ours and
        // unreaped, and WNOWAIT leaves it so.
        while unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        } < 0
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
        {}
        if let Some(k) = self.kill.take() {
            k.disarm();
        }
        untrack(pid);
        ch.wait()
    }

    /// Reads stdout to its end (when piped), then [`wait`](Group::wait)s.
    pub fn wait_with_output(mut self) -> std::io::Result<(ExitStatus, Vec<u8>)> {
        let mut out = vec![];
        if let Some(mut o) = self.child_mut().stdout.take() {
            o.read_to_end(&mut out)?;
        }
        Ok((self.wait()?, out))
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if let Some(ch) = self.ch.take() {
            let pid = ch.id() as i32;
            // SAFETY: kill(2) takes plain integers. Unreaped, so the group id is still the
            // child's.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            if let Some(k) = self.kill.take() {
                k.disarm();
            }
            untrack(pid);
            workpool::reap(ch);
        }
    }
}

fn eventfd() -> Option<Arc<OwnedFd>> {
    // SAFETY: eventfd(2) takes plain integers and touches no memory of ours.
    let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    // SAFETY: eventfd just returned this descriptor, owned by nothing else.
    (fd >= 0).then(|| Arc::new(unsafe { OwnedFd::from_raw_fd(fd) }))
}

/// Reads what is there (the fd polled readable); `false` once the pipe is closed.
fn drain(r: &mut impl Read, buf: &mut Vec<u8>) -> bool {
    let mut b = [0u8; 16384];
    loop {
        match r.read(&mut b) {
            Ok(0) => return false,
            Ok(n) => {
                buf.extend_from_slice(&b[..n]);
                return true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
}

fn collect(mut ch: Child, stop: &Arc<Stop>) -> std::io::Result<Output> {
    let pid = ch.id() as i32;
    let mut so = ch.stdout.take();
    let mut se = ch.stderr.take();
    let pidfd = match workpool::pidfd_open(pid) {
        Ok(fd) => fd,
        Err(e) => {
            // SAFETY: kill(2) takes plain integers. Unreaped, so the group id is still the
            // child's.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            let _ = ch.wait();
            return Err(e);
        }
    };
    track(pid);
    let wake = eventfd();
    let _wake_on_stop = wake.as_ref().map(|w| {
        let w = Arc::downgrade(w);
        stop.on_stop(move || {
            if let Some(w) = w.upgrade() {
                let one = 1u64;
                // SAFETY: `one` is the 8 readable bytes an eventfd write takes; `w` keeps the
                // descriptor open for the call.
                unsafe { libc::write(w.as_raw_fd(), (&one as *const u64).cast(), 8) };
            }
        })
    });
    let mut watch = Some(crate::ports::watch_kill(-pid, stop.clone()));
    let mut o = Output::default();
    let mut status = None;
    let mut stop_seen = false;
    loop {
        let stopped = stop.load(Ordering::Acquire);
        if status.is_some() && (stopped || (so.is_none() && se.is_none())) {
            break;
        }
        let mut fds: Vec<(libc::pollfd, u8)> = vec![];
        let mut add = |fd: RawFd, tag: u8| {
            fds.push((
                libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                tag,
            ))
        };
        if let Some(p) = &so {
            add(p.as_raw_fd(), 0);
        }
        if let Some(p) = &se {
            add(p.as_raw_fd(), 1);
        }
        if status.is_none() {
            add(pidfd.as_raw_fd(), 2);
        }
        if !stop_seen && let Some(w) = &wake {
            add(w.as_raw_fd(), 3);
        }
        let mut pfds: Vec<libc::pollfd> = fds.iter().map(|f| f.0).collect();
        let n = poll(&mut pfds);
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            // Nothing can be waited for any more: end the group rather than block on a child
            // that may never exit, as QProcess::kill.
            if status.is_none() {
                // SAFETY: kill(2) takes plain integers. Unreaped, so the group id is still
                // the child's.
                unsafe { libc::kill(-pid, libc::SIGKILL) };
                drop(watch.take());
                untrack(pid);
                let _ = ch.wait();
            }
            return Err(e);
        }
        for (p, &(_, tag)) in pfds.iter().zip(&fds) {
            if p.revents == 0 {
                continue;
            }
            match tag {
                0 => {
                    if !so.as_mut().is_some_and(|r| drain(r, &mut o.out)) {
                        so = None;
                    }
                }
                1 => {
                    if !se.as_mut().is_some_and(|r| drain(r, &mut o.err)) {
                        se = None;
                    }
                }
                2 => {
                    // Before the reap: a later stop must not signal a reused process group id.
                    watch = None;
                    untrack(pid);
                    match ch.wait() {
                        Ok(st) => status = Some(st),
                        // The child is gone all the same (its pidfd is readable), how it
                        // ended is unknown: `ECHILD` when something else reaped it, or
                        // SIGCHLD is ignored. Polling its pidfd again would never block.
                        Err(e) => {
                            elog::eprintln!("impulse-shell: cannot wait for child {pid}: {e}");
                            return Err(e);
                        }
                    }
                }
                _ => stop_seen = true,
            }
        }
    }
    drop(watch);
    if stop.load(Ordering::Acquire) {
        o.status = None;
    } else {
        o.status = status;
    }
    Ok(o)
}

/// poll(2) over `pfds`, no timeout.
fn poll(pfds: &mut [libc::pollfd]) -> libc::c_int {
    #[cfg(test)]
    if let Some(e) = POLL_FAULT.with(|f| f.take()) {
        // SAFETY: __errno_location(3) takes nothing and returns this thread's errno.
        let errno = unsafe { libc::__errno_location() };
        // SAFETY: `errno` points at this thread's errno; setting it is what a failing poll does.
        unsafe { *errno = e };
        return -1;
    }
    // SAFETY: `pfds` is a writable array of `pfds.len()` pollfds over open descriptors.
    unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, -1) }
}

#[cfg(test)]
thread_local! {
    /// The errno the next [`poll`] on this thread fails with (tests).
    static POLL_FAULT: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
mod tests;
