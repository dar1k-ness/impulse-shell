//! Ends the process groups of running `Process` children when the shell dies without cleaning
//! up after them (SIGKILL, the OOM killer), which no signal handler of its own can catch.
//!
//! A small guard process (the shell binary re-executed with [`ARG`]) reads the groups to end
//! from a pipe whose only write end the shell holds (`O_CLOEXEC`, so no child inherits it).
//! The kernel closes that end however the shell dies; on EOF the guard sends SIGTERM to every
//! group still registered and SIGKILL to those left after [`GRACE`]. Detached children
//! (`execDetached`, app windows) are never registered and outlive the shell, as they should.
//! `PR_SET_PDEATHSIG` would need a `pre_exec` hook (a fork of the whole shell instead of
//! posix_spawn), follows the spawning *thread* rather than the process (prctl(2)) and reaches
//! only the leader, not a pipeline's other members.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::Mutex;
use std::time::Duration;

/// The argument that makes the shell binary run [`serve`].
pub const ARG: &str = "__ri-child-guard";

/// How long the guard waits for SIGTERMed groups before it sends SIGKILL.
pub const GRACE: Duration = Duration::from_secs(2);

/// Builds the command that runs the guard; set once by the embedding binary ([`enable`]).
type Launcher = fn() -> std::process::Command;

#[derive(Default)]
struct Guard {
    launch: Option<Launcher>,
    /// Write end of the guard's pipe, while a guard runs.
    pipe: Option<std::fs::File>,
    /// Groups registered now: sent again to a guard started after the previous one died.
    groups: HashSet<i32>,
}

static GUARD: Mutex<Option<Guard>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Guard) -> T) -> T {
    let mut g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Guard::default))
}

/// Guard the shell's `Process` children; `launch` builds the command that runs [`serve`] (the
/// shell binary: `/proc/self/exe` [`ARG`]). Without this call registration does nothing.
pub fn enable(launch: Launcher) {
    with(|g| g.launch = Some(launch));
}

/// The command [`enable`] takes in the shell binary: this executable, run with [`ARG`]. It
/// reads nothing from its environment, so it keeps this process's.
pub fn self_command() -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut c = std::process::Command::new("/proc/self/exe");
    c.arg0("ri-childguard").arg(ARG);
    c
}

/// Registers the process group `pgid` (a child that leads its own group, not yet reaped).
pub fn add(pgid: i32) {
    if pgid > 0 {
        with(|g| {
            if g.launch.is_some() && g.groups.insert(pgid) {
                g.send(pgid);
            }
        });
    }
}

/// Unregisters `pgid`; call before its leader is reaped, as its id may be reused after.
pub fn remove(pgid: i32) {
    with(|g| {
        if g.groups.remove(&pgid) {
            g.send(-pgid);
        }
    });
}

impl Guard {
    fn send(&mut self, msg: i32) {
        if self.pipe.is_none() {
            self.start();
            return;
        }
        let pipe = self.pipe.as_mut().unwrap();
        match pipe.write_all(&msg.to_le_bytes()) {
            Ok(()) => {}
            // The guard died (killed on its own): start another with the current groups.
            Err(e) if e.raw_os_error() == Some(libc::EPIPE) => {
                elog::eprintln!("impulse-shell: child guard exited, starting it again");
                self.pipe = None;
                self.start();
            }
            Err(e) => elog::eprintln!("impulse-shell: child guard: {e}"),
        }
    }

    /// Starts a guard and sends it every registered group.
    fn start(&mut self) {
        let Some(launch) = self.launch else { return };
        let mut fds = [0; 2];
        // SAFETY: `fds` is the writable array of two descriptors pipe2(2) fills.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            elog::eprintln!(
                "impulse-shell: child guard: pipe: {}",
                std::io::Error::last_os_error()
            );
            return;
        }
        // SAFETY: pipe2 just returned this read end, owned by nothing else.
        let r = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        // SAFETY: pipe2 just returned this write end, owned by nothing else.
        let w = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        let mut c = launch();
        {
            use std::os::unix::process::CommandExt;
            // Its own group: a terminal's ^C to the shell's group does not end it first.
            c.process_group(0);
        }
        c.stdin(std::process::Stdio::from(r))
            .stdout(std::process::Stdio::null());
        match c.spawn() {
            Ok(ch) => workpool::reap(ch),
            Err(e) => {
                elog::eprintln!("impulse-shell: cannot start the child guard: {e}");
                return;
            }
        }
        let mut pipe = std::fs::File::from(w);
        let mut buf = Vec::with_capacity(self.groups.len() * 4);
        for g in &self.groups {
            buf.extend_from_slice(&g.to_le_bytes());
        }
        if let Err(e) = pipe.write_all(&buf) {
            elog::eprintln!("impulse-shell: child guard: {e}");
            return;
        }
        self.pipe = Some(pipe);
    }
}

/// The guard process: reads groups from stdin until EOF, then ends those still registered.
/// Returns the exit status.
pub fn serve() -> i32 {
    // SAFETY: PR_SET_NAME reads a NUL-terminated name (a static C string here) and copies it.
    unsafe { libc::prctl(libc::PR_SET_NAME, c"ri-childguard".as_ptr()) };
    // Hangups and ^C reach the whole session, and a SIGTERM the whole unit or process group
    // of the shell; the guard leaves only after the shell.
    for sig in [libc::SIGHUP, libc::SIGINT, libc::SIGTERM] {
        // SAFETY: SIG_IGN installs no handler, so no code of ours runs in signal context.
        unsafe { libc::signal(sig, libc::SIG_IGN) };
    }
    let mut groups = HashSet::new();
    let mut stdin = std::io::stdin().lock();
    let mut msg = [0u8; 4];
    loop {
        match stdin.read_exact(&mut msg) {
            Ok(()) => {
                let g = i32::from_le_bytes(msg);
                if g > 0 {
                    groups.insert(g);
                } else {
                    groups.remove(&-g);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    let grace = timer(GRACE);
    end_groups(
        groups.into_iter().collect(),
        grace.as_ref().map(|t| t.as_fd()),
    );
    0
}

/// A timerfd(2) on `CLOCK_MONOTONIC` that turns readable once `d` has passed; `None` when the
/// kernel cannot make one.
fn timer(d: Duration) -> Option<OwnedFd> {
    // SAFETY: timerfd_create(2) takes plain integers and returns a new descriptor or -1.
    let fd = unsafe { libc::timerfd_create(libc::CLOCK_MONOTONIC, libc::TFD_CLOEXEC) };
    if fd < 0 {
        return None;
    }
    // SAFETY: timerfd_create just returned this descriptor, owned by nothing else.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let zero = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let spec = libc::itimerspec {
        it_interval: zero,
        // A zero value would disarm the timer instead of expiring at once.
        it_value: libc::timespec {
            tv_sec: d.as_secs() as libc::time_t,
            tv_nsec: d.subsec_nanos().max(u32::from(d.is_zero())) as libc::c_long,
        },
    };
    // SAFETY: `spec` is a valid itimerspec for the call; a null old value is allowed.
    let rc = unsafe { libc::timerfd_settime(fd.as_raw_fd(), 0, &spec, std::ptr::null_mut()) };
    (rc == 0).then_some(fd)
}

/// SIGTERM each group, then SIGKILL the ones with members left once the grace is over:
/// `grace_over` turned readable (a [`timer`]; `None`: over at once). Each member's exit is an
/// event on its pidfd (pidfd_open(2)), so the guard sleeps in poll(2) until the last of them
/// exits or the grace ends. A member may fork after the scan (a TERM trap starting cleanup, a
/// supervisor respawning): once the known members exited the groups are scanned again, and
/// their new members are waited for in turn.
fn end_groups(groups: Vec<i32>, grace_over: Option<BorrowedFd<'_>>) {
    // Opened before the SIGTERM, so a member that exits right away is still waited for (a
    // pidfd stays valid, and readable, once its process exited).
    let mut waiting = pidfds(&groups);
    for &g in &groups {
        // SAFETY: kill(2) takes plain integers and touches no memory of ours.
        unsafe { libc::kill(-g, libc::SIGTERM) };
    }
    while let Some(over) = grace_over
        && !waiting.is_empty()
        && wait_exits(&waiting, over)
    {
        waiting = pidfds(&groups);
    }
    for g in groups {
        // A group exists while any process is in it (kill(2) with signal 0), including
        // members the last scan could not see.
        // SAFETY: kill(2) takes plain integers and touches no memory of ours.
        if unsafe { libc::kill(-g, 0) } == 0 {
            // SAFETY: as above.
            unsafe { libc::kill(-g, libc::SIGKILL) };
        }
    }
}

/// Pidfds of the running members of `groups`.
fn pidfds(groups: &[i32]) -> Vec<OwnedFd> {
    members(groups)
        .into_iter()
        .filter_map(|pid| workpool::pidfd_open(pid).ok())
        .collect()
}

/// Waits until every process behind `pidfds` exited (true) or `grace_over` turned readable
/// (false).
fn wait_exits(pidfds: &[OwnedFd], grace_over: BorrowedFd<'_>) -> bool {
    let poll = |fd: i32| libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let mut fds: Vec<libc::pollfd> = std::iter::once(poll(grace_over.as_raw_fd()))
        .chain(pidfds.iter().map(|fd| poll(fd.as_raw_fd())))
        .collect();
    while fds.len() > 1 {
        // SAFETY: `fds` holds `fds.len()` initialized pollfds over open descriptors.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return false;
        }
        if fds[0].revents != 0 {
            return false;
        }
        let mut first = true;
        fds.retain(|p| std::mem::take(&mut first) || p.revents == 0);
    }
    true
}

/// Pids of the running (neither zombie nor dead) members of `groups`.
fn members(groups: &[i32]) -> Vec<i32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| {
            let pid = e.file_name().to_str()?.parse::<i32>().ok()?;
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            // pid (comm) state ppid pgrp ...
            let mut f = stat.rsplit_once(')')?.1.split_whitespace();
            let state = f.next()?;
            let pgrp = f.nth(1)?.parse::<i32>().ok()?;
            (!matches!(state, "Z" | "X") && groups.contains(&pgrp)).then_some(pid)
        })
        .collect()
}

/// Fd of the guard pipe's write end (tests).
#[cfg(test)]
fn pipe_fd() -> Option<i32> {
    use std::os::fd::AsRawFd;
    with(|g| g.pipe.as_ref().map(|p| p.as_raw_fd()))
}

#[cfg(test)]
mod tests;
