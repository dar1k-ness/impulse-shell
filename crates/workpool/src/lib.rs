//! Bounded workers for one-off blocking actions (a D-Bus call, a logind request) and one
//! reaper thread for detached children, instead of a fresh thread per action or per child.

// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

pub mod userenv;

use std::collections::BinaryHeap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Worker threads: one-off jobs queue behind each other instead of spawning a thread each.
pub const WORKERS: usize = 4;

type Job = Box<dyn FnOnce() + Send + 'static>;

struct Due {
    at: Instant,
    seq: u64,
    job: Job,
}

impl PartialEq for Due {
    fn eq(&self, o: &Self) -> bool {
        (self.at, self.seq) == (o.at, o.seq)
    }
}
impl Eq for Due {}
impl PartialOrd for Due {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Due {
    /// Earliest deadline first (BinaryHeap is a max-heap), FIFO among equal deadlines.
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (o.at, o.seq).cmp(&(self.at, self.seq))
    }
}

struct Queue {
    /// Jobs by deadline.
    due: BinaryHeap<Due>,
    /// The last sequence number.
    seq: u64,
    /// Jobs running now.
    running: usize,
    /// Worker threads started.
    workers: usize,
    /// The time while a test holds the clock; it then moves only by the test's advance.
    frozen: Option<Instant>,
    /// Clock advances so far, and the last one a worker has looked at.
    advanced: u64,
    seen: u64,
    /// Whether the pool is in [`WAITING`], and whether it counts in [`HELD`].
    listed: bool,
    held: bool,
}

impl Queue {
    fn now(&self) -> Instant {
        self.frozen.unwrap_or_else(Instant::now)
    }

    /// Jobs no worker was to be had for do not count: nothing runs them until one starts.
    fn idle(&self) -> bool {
        let now = self.now();
        self.running == 0 && (self.workers == 0 || self.due.peek().is_none_or(|d| d.at > now))
    }
}

type Guard = std::sync::MutexGuard<'static, Queue>;

struct Pool {
    q: Mutex<Queue>,
    cv: Condvar,
    /// Signalled whenever a job finishes ([`wait_idle`]).
    done: Condvar,
}

fn pool() -> &'static Pool {
    static P: OnceLock<&'static Pool> = OnceLock::new();
    P.get_or_init(|| {
        let p: &'static Pool = Box::leak(Box::new(Pool {
            q: Mutex::new(Queue {
                due: BinaryHeap::new(),
                seq: 0,
                running: 0,
                workers: 0,
                frozen: None,
                advanced: 0,
                seen: 0,
                listed: false,
                held: false,
            }),
            cv: Condvar::new(),
            done: Condvar::new(),
        }));
        let mut g = p.q.lock().unwrap();
        for _ in 0..WORKERS {
            if let Err(e) = p.spawn_worker(&mut g) {
                elog::eprintln!("impulse-shell: cannot start a pool worker: {e}");
            }
        }
        drop(g);
        p
    })
}

impl Pool {
    fn spawn_worker(&'static self, g: &mut Queue) -> std::io::Result<()> {
        #[cfg(test)]
        if tests::POOL_SPAWN_FAILS.get() {
            return Err(std::io::Error::from_raw_os_error(libc::EAGAIN));
        }
        std::thread::Builder::new()
            .name(format!("pool{}", g.workers))
            .spawn(move || worker(self))?;
        g.workers += 1;
        Ok(())
    }

    /// With no worker started (`EAGAIN` under `RLIMIT_NPROC` or `threads-max`), tries to start
    /// one again. Failing that the jobs stay queued, never run by the caller, and the pool
    /// waits for a thread ([`WAITING`]). No delayed job waits here: [`after`] refuses one
    /// while no worker runs. `Err` is the refusal, for the caller to log.
    fn without_workers(&'static self, g: &mut Queue) -> std::io::Result<()> {
        if g.workers > 0 || g.due.is_empty() {
            return Ok(());
        }
        let started = self.spawn_worker(g);
        match started {
            Ok(()) => self.cv.notify_one(),
            Err(_) if !g.listed => {
                g.listed = true;
                waits(Waiting::Pool(self));
                if !g.held {
                    g.held = true;
                    holds(true);
                }
                wake_idle_long();
            }
            Err(_) => {}
        }
        started
    }

    /// [`ask_waiting`]: the thread asked for again.
    fn ask(&'static self) {
        let mut g = self.q.lock().unwrap();
        g.listed = false;
        let _ = self.without_workers(&mut g);
    }

    /// [`serve_waiting`]: the calling thread, its own work done, is the worker the pool was
    /// refused. A worker never ends, so this does not return then.
    fn serve(&'static self) {
        let mut g = self.q.lock().unwrap();
        g.listed = false;
        if g.workers == 0 && !g.due.is_empty() {
            g.workers += 1;
            drop(g);
            lane_thread_done();
            worker(self);
        }
    }

    /// Runs the earliest job, which is due, without holding the queue.
    fn run_one(&'static self, mut g: Guard) -> Guard {
        let d = g.due.pop().unwrap();
        g.running += 1;
        // Another due job may wait behind this one.
        self.cv.notify_one();
        drop(g);
        caught(d.job);
        let mut g = self.q.lock().unwrap();
        g.running -= 1;
        if g.held && g.idle() {
            g.held = false;
            holds(false);
        }
        self.done.notify_all();
        g
    }
}

fn worker(p: &'static Pool) {
    let mut g = p.q.lock().unwrap();
    loop {
        if g.seen != g.advanced {
            // The test's advance returns once a worker has looked at the new time.
            g.seen = g.advanced;
            p.done.notify_all();
        }
        let now = g.now();
        match g.due.peek().map(|d| d.at) {
            Some(at) if at <= now => g = p.run_one(g),
            Some(_) if g.frozen.is_some() => g = p.cv.wait(g).unwrap(),
            Some(at) => g = p.cv.wait_timeout(g, at - now).unwrap().0,
            None => g = p.cv.wait(g).unwrap(),
        }
    }
}

/// Runs one job; a panic is logged and the worker lives on, so panicking jobs cannot use up
/// the [`WORKERS`] and leave the pool dead.
fn caught(job: Job) {
    caught_as("pool job", job);
}

/// Runs `f`; a panic is logged, naming `what` panicked, instead of ending the calling thread.
fn caught_as(what: &str, f: impl FnOnce()) {
    if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        let msg = e
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| e.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        elog::eprintln!("impulse-shell: {what} panicked: {msg}");
    }
}

/// Run `f` on a pool worker as soon as one is free. The caller never runs it: with no worker
/// to be had (`EAGAIN`) `f` stays queued, the refusal is logged, and a worker is asked for
/// again as [`WAITING`] tells.
pub fn run(f: impl FnOnce() + Send + 'static) {
    rewatch();
    let p = pool();
    let mut g = p.q.lock().unwrap();
    let now = g.now();
    push(&mut g, now, Box::new(f));
    p.cv.notify_one();
    let refused = p.without_workers(&mut g);
    let held = g.due.len();
    drop(g);
    if let Err(e) = refused {
        elog::eprintln!(
            "impulse-shell: cannot start a pool worker: {e}; {held} queued until one starts"
        );
    }
}

/// Run `f` on a pool worker once `d` has passed; waiting does not hold a worker. With no
/// worker running and none to be started (`EAGAIN` under `RLIMIT_NPROC` or `threads-max`), a
/// nonzero delay has nobody to wait it out: `f` is dropped and the error returned, so the
/// caller picks its own way rather than a job that never runs.
pub fn after(d: Duration, f: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    if d.is_zero() {
        run(f);
        return Ok(());
    }
    rewatch();
    let p = pool();
    let mut g = p.q.lock().unwrap();
    if g.workers == 0 {
        p.spawn_worker(&mut g)?;
    }
    let at = g.now() + d;
    push(&mut g, at, Box::new(f));
    p.cv.notify_one();
    Ok(())
}

fn push(q: &mut Queue, at: Instant, job: Job) {
    q.seq += 1;
    q.due.push(Due {
        at,
        seq: q.seq,
        job,
    });
}

/// Most [`long`] jobs that run at once when submitted from outside the pool; more queue.
pub const LONG_MAX: usize = 16;

/// How long an idle [`long`] thread waits for the next job before it exits: long enough that
/// jobs arriving every few seconds (hyprctl requests, downloads) reuse a thread.
pub const LONG_IDLE: Duration = Duration::from_secs(10);

#[derive(Default)]
struct LongQ {
    /// Jobs submitted from a [`long`] job: they never wait for a slot, as their submitter may
    /// be waiting for them (`http::cancellable`), and run before queued outside jobs.
    nested: std::collections::VecDeque<Job>,
    outer: std::collections::VecDeque<Job>,
    threads: usize,
    idle: usize,
    /// Outer jobs running now (at most `max`).
    running: usize,
    /// Jobs running now, nested ones included.
    busy: usize,
    /// The time while a test holds the clock; it then moves only by the test's advance.
    frozen: Option<Instant>,
    /// Clock advances so far, and the idle threads that have not looked at the last one yet.
    advanced: u64,
    behind: usize,
    /// Whether the pool is in [`WAITING`], and whether it counts in [`HELD`].
    listed: bool,
    held: bool,
}

impl LongQ {
    fn now(&self) -> Instant {
        self.frozen.unwrap_or_else(Instant::now)
    }
}

struct Elastic {
    q: Mutex<LongQ>,
    cv: Condvar,
    /// Signalled whenever a job finishes ([`wait_idle`]), a thread goes idle or exits.
    done: Condvar,
    max: usize,
    idle: Duration,
}

thread_local! {
    static IN_LONG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

static ELASTIC: OnceLock<Elastic> = OnceLock::new();

fn elastic() -> &'static Elastic {
    ELASTIC.get_or_init(|| Elastic::new(LONG_MAX, LONG_IDLE))
}

/// A queue other than [`long`]'s was refused its thread: an idle `long` thread, which would
/// stay for [`LONG_IDLE`], ends now and is that thread ([`serve_waiting`]). Called with that
/// queue's lock held, after it is in [`WAITING`].
fn wake_idle_long() {
    if let Some(e) = ELASTIC.get() {
        // An idle thread is either before its look at `WAITING_LEN` or waiting already.
        drop(e.q.lock().unwrap());
        e.cv.notify_one();
    }
}

/// A job that blocks for as long as its work runs (a child process, a D-Bus wait, a
/// transfer), off the bounded workers of [`run`]. At most [`LONG_MAX`] such jobs run at once;
/// a burst beyond that queues instead of starting a thread per job. Jobs submitted from a
/// running `long` job always get a thread. Threads idle for [`LONG_IDLE`] exit.
///
/// The caller never runs `f`, with one exception. When no thread runs and the system refuses
/// one (`EAGAIN`), `f` stays queued, the refusal is logged, and the thread is asked for again
/// as [`WAITING`] tells. The exception is a job submitted from a running `long` job and
/// refused its thread: its submitter, which is no main loop and may wait for it, runs it.
pub fn long(f: impl FnOnce() + Send + 'static) {
    let _ = elastic().take(Box::new(f), false);
}

/// [`long`] for a caller that has its own answer to a refused thread (a request that fails
/// instead of waiting): where `long` would keep `f` queued with no thread to run it, or run
/// it on its submitter, `f` is dropped unrun and the refusal returned.
pub fn try_long(f: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    elastic().take(Box::new(f), true)
}

fn spawn_long(name: String, f: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    #[cfg(test)]
    if tests::LONG_SPAWN_FAILS.get() {
        return Err(std::io::Error::from_raw_os_error(libc::EAGAIN));
    }
    std::thread::Builder::new().name(name).spawn(f).map(drop)
}

impl Elastic {
    fn new(max: usize, idle: Duration) -> Elastic {
        Elastic {
            q: Mutex::new(LongQ::default()),
            cv: Condvar::new(),
            done: Condvar::new(),
            max,
            idle,
        }
    }

    #[cfg(test)]
    fn submit(&'static self, job: Job) {
        let _ = self.take(job, false);
    }

    /// [`long`], or [`try_long`] with `or_refuse`.
    fn take(&'static self, job: Job, or_refuse: bool) -> std::io::Result<()> {
        rewatch();
        let mut g = self.q.lock().unwrap();
        let nested = IN_LONG.with(|c| c.get());
        if nested {
            g.nested.push_back(job);
        } else {
            g.outer.push_back(job);
        }
        let Err(e) = self.start(&mut g, nested) else {
            drop(g);
            self.cv.notify_one();
            return Ok(());
        };
        if g.threads > 0 && !nested {
            drop(g);
            // The threads there are take it once one of them is free.
            elog::eprintln!(
                "impulse-shell: cannot start another pool thread ({e}): the job waits for a running one"
            );
            self.cv.notify_one();
            return Ok(());
        }
        // No thread to be had and none that would take it.
        if or_refuse || nested {
            let job = if nested {
                g.nested.pop_back()
            } else {
                g.outer.pop_back()
            };
            if or_refuse {
                drop(g);
                return Err(e);
            }
            // The submitter is a `long` job itself: it runs the job rather than wait for it
            // with nobody to run it.
            g.busy += 1;
            drop(g);
            elog::eprintln!(
                "impulse-shell: cannot start a pool thread: {e}; its job runs on its submitter"
            );
            caught(job.unwrap());
            self.finished(&mut self.q.lock().unwrap());
            return Ok(());
        }
        let held = g.outer.len();
        if !g.listed {
            g.listed = true;
            waits(Waiting::Long(self));
        }
        if !g.held {
            g.held = true;
            holds(true);
        }
        drop(g);
        elog::eprintln!(
            "impulse-shell: cannot start a pool thread: {e}; {held} queued until one starts"
        );
        Ok(())
    }

    /// Starts a thread when a queued job needs one.
    fn start(&'static self, g: &mut LongQ, nested: bool) -> std::io::Result<()> {
        // Jobs an idle thread will not pick up: every nested one, outer ones within the cap.
        let runnable = g.nested.len() + g.outer.len().min(self.max.saturating_sub(g.running));
        if runnable > g.idle && (nested || g.threads < self.max) {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            spawn_long(format!("long{n}"), move || {
                self.worker();
                serve_waiting();
            })?;
            g.threads += 1;
        }
        Ok(())
    }

    /// [`ask_waiting`]: the thread asked for again.
    fn ask(&'static self) {
        let mut g = self.q.lock().unwrap();
        g.listed = false;
        if g.threads > 0 || g.outer.is_empty() {
            return;
        }
        match self.start(&mut g, false) {
            Ok(()) => self.cv.notify_one(),
            Err(_) => {
                g.listed = true;
                waits(Waiting::Long(self));
            }
        }
    }

    /// [`serve_waiting`]: the calling thread, its own work done, is the thread the jobs were
    /// refused, until it has been idle as long as any.
    fn serve(&'static self) {
        let mut g = self.q.lock().unwrap();
        g.listed = false;
        if g.threads == 0 && !g.outer.is_empty() {
            g.threads += 1;
            drop(g);
            lane_thread_done();
            self.worker();
        }
    }

    fn finished(&self, g: &mut LongQ) {
        g.busy -= 1;
        if g.held && g.busy == 0 && g.nested.is_empty() && g.outer.is_empty() {
            g.held = false;
            holds(false);
        }
        self.done.notify_all();
    }

    /// Jobs no thread was to be had for do not count: nothing runs them until one starts.
    fn idle(g: &LongQ) -> bool {
        g.busy == 0 && (g.threads == 0 || (g.nested.is_empty() && g.outer.is_empty()))
    }

    fn worker(&'static self) {
        IN_LONG.with(|c| c.set(true));
        let mut g = self.q.lock().unwrap();
        // Since when this thread has had no job.
        let mut since = None;
        loop {
            if let Some(job) = g.nested.pop_front() {
                since = None;
                g.busy += 1;
                drop(g);
                caught(job);
                g = self.q.lock().unwrap();
                self.finished(&mut g);
                continue;
            }
            if g.running < self.max
                && let Some(job) = g.outer.pop_front()
            {
                since = None;
                g.running += 1;
                g.busy += 1;
                drop(g);
                caught(job);
                g = self.q.lock().unwrap();
                g.running -= 1;
                self.finished(&mut g);
                // A queued outer job may have waited for this slot.
                if !g.outer.is_empty() {
                    self.cv.notify_one();
                }
                continue;
            }
            // Threads started for nested jobs beyond the cap do not linger, and none does
            // while another queue waits for a thread: this one ends and is it.
            if g.threads > self.max || WAITING_LEN.load(std::sync::atomic::Ordering::Relaxed) != 0 {
                break;
            }
            let now = g.now();
            let idled = now.saturating_duration_since(*since.get_or_insert(now));
            if idled >= self.idle {
                break;
            }
            let seen = g.advanced;
            g.idle += 1;
            self.done.notify_all();
            g = if g.frozen.is_some() {
                self.cv.wait(g).unwrap()
            } else {
                self.cv.wait_timeout(g, self.idle - idled).unwrap().0
            };
            g.idle -= 1;
            if seen != g.advanced {
                // The test's advance returns once every idle thread has looked at the new time.
                g.behind -= 1;
                if g.behind == 0 {
                    self.done.notify_all();
                }
            }
        }
        g.threads -= 1;
        self.done.notify_all();
        drop(g);
        // The thread may go on as another queue's ([`serve_waiting`]).
        IN_LONG.with(|c| c.set(false));
    }
}

/// `pidfd_open(2)`: readable once the process has exited. Linux 5.3 is required (checked once
/// at startup by [`require_pidfd`]), so an error here is a real one: the process is gone, or
/// the fd table is full.
pub fn pidfd_open(pid: i32) -> std::io::Result<OwnedFd> {
    // SAFETY: pidfd_open(2) takes a pid and flags and returns a new fd or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `fd` was just returned by pidfd_open and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Fails unless the kernel has `pidfd_open(2)` (Linux 5.3+), which every child the shell
/// starts is watched and signalled through.
pub fn require_pidfd() -> Result<(), String> {
    match pidfd_open(std::process::id() as i32) {
        Ok(_) => Ok(()),
        Err(e) if e.raw_os_error() == Some(libc::ENOSYS) => {
            Err("impulse-shell needs Linux 5.3 or later (pidfd_open(2) is missing)".into())
        }
        Err(e) => Err(format!("impulse-shell: pidfd_open(2): {e}")),
    }
}

/// Called once the child is gone, with its exit status: `None` when wait(2) failed, so the
/// status is unknown (`ECHILD`: something else reaped it, or SIGCHLD is ignored).
type OnExit = Box<dyn FnOnce(Option<std::process::ExitStatus>) + Send + 'static>;

/// A watched child; its pidfd stays registered with the reaper's epoll set while it is open.
struct Kid {
    child: Waited,
    pidfd: OwnedFd,
    on_exit: Option<OnExit>,
}

/// A child to reap: one this image spawned, or one it inherited across execve(2), for which
/// std has no `Child`.
enum Waited {
    Spawned(std::process::Child),
    Adopted(i32),
}

impl Waited {
    fn pid(&self) -> i32 {
        match self {
            Waited::Spawned(ch) => ch.id() as i32,
            Waited::Adopted(pid) => *pid,
        }
    }

    /// Reaps the child if it exited: its status, `None` while it runs.
    fn try_wait(&mut self, pidfd: &OwnedFd) -> std::io::Result<Option<std::process::ExitStatus>> {
        if let Waited::Spawned(ch) = self {
            return ch.try_wait();
        }
        // SAFETY: an all-zero siginfo_t is a valid value for waitid(2) to fill.
        let mut si: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `pidfd` is an open pidfd and `si` a writable siginfo_t (waitid(2), P_PIDFD).
        let r = unsafe {
            libc::waitid(
                libc::P_PIDFD,
                pidfd.as_raw_fd() as libc::id_t,
                &mut si,
                libc::WEXITED | libc::WNOHANG,
            )
        };
        if r < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: waitid filled `si` for a SIGCHLD; si_pid is 0 if the child still runs.
        if unsafe { si.si_pid() } == 0 {
            return Ok(None);
        }
        // SAFETY: as above, si_status is set together with si_pid.
        let st = unsafe { si.si_status() };
        Ok(Some(wait_status(si.si_code, st)))
    }

    /// Blocks until the child exits and reaps it.
    fn wait(self) -> std::io::Result<std::process::ExitStatus> {
        match self {
            Waited::Spawned(mut ch) => ch.wait(),
            Waited::Adopted(pid) => {
                let mut st = 0;
                loop {
                    // SAFETY: waitpid(2) writes the status to the live `st`.
                    if unsafe { libc::waitpid(pid, &mut st, 0) } == pid {
                        use std::os::unix::process::ExitStatusExt;
                        return Ok(std::process::ExitStatus::from_raw(st));
                    }
                    let e = std::io::Error::last_os_error();
                    if e.kind() != std::io::ErrorKind::Interrupted {
                        return Err(e);
                    }
                }
            }
        }
    }
}

/// The wait(2) status word that waitid(2)'s `si_code` and `si_status` describe.
fn wait_status(code: libc::c_int, status: libc::c_int) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    let raw = match code {
        libc::CLD_EXITED => (status & 0xff) << 8,
        libc::CLD_DUMPED => (status & 0x7f) | 0x80,
        _ => status & 0x7f,
    };
    std::process::ExitStatus::from_raw(raw)
}

struct Reaper {
    ep: OwnedFd,
    kids: Mutex<std::collections::HashMap<i32, Kid>>,
    /// Reaped children whose exit callback still runs (changed under `kids`' lock).
    exiting: std::sync::atomic::AtomicUsize,
    /// Signalled whenever a reaped child's callback has run ([`wait_idle`]).
    gone: Condvar,
}

impl Reaper {
    /// No child left to reap and no exit callback running; `kids` is the locked map.
    fn idle(&self, kids: &std::collections::HashMap<i32, Kid>) -> bool {
        kids.is_empty() && self.exiting.load(std::sync::atomic::Ordering::Relaxed) == 0
    }
}

static REAPER: OnceLock<std::sync::Arc<Reaper>> = OnceLock::new();

/// The reaper, started by the first child. One that cannot start (no epoll fd, no thread) is
/// logged and tried again for the next child.
fn reaper() -> Option<&'static Reaper> {
    reaper_for(true)
}

/// [`reaper`]; with `first` false (a child from [`UNWATCHED`]) a refusal is not logged again.
fn reaper_for(first: bool) -> Option<&'static Reaper> {
    static STARTING: Mutex<()> = Mutex::new(());
    if REAPER.get().is_none() {
        let _starting = STARTING.lock().unwrap_or_else(|e| e.into_inner());
        if REAPER.get().is_none() {
            match start_reaper() {
                Ok(r) => drop(REAPER.set(r)),
                Err(e) if first => {
                    elog::eprintln!("impulse-shell: cannot start the reaper thread: {e}")
                }
                Err(_) => {}
            }
        }
    }
    REAPER.get().map(|r| &**r)
}

fn start_reaper() -> std::io::Result<std::sync::Arc<Reaper>> {
    // SAFETY: epoll_create1(2) takes only flags.
    let ep = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    if ep < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Refused its thread, the structure is dropped here, and its epoll fd closed with it.
    let r = std::sync::Arc::new(Reaper {
        // SAFETY: `ep` was just returned by epoll_create1 and nothing else owns it.
        ep: unsafe { OwnedFd::from_raw_fd(ep) },
        kids: Mutex::default(),
        exiting: Default::default(),
        gone: Condvar::new(),
    });
    #[cfg(test)]
    if tests::REAPER_SPAWN_FAILS.get() {
        return Err(std::io::Error::from_raw_os_error(libc::EAGAIN));
    }
    let on_thread = r.clone();
    std::thread::Builder::new()
        .name("reaper".into())
        .spawn(move || reap_loop(&on_thread))?;
    Ok(r)
}

fn reap_loop(r: &Reaper) {
    let mut evs = [libc::epoll_event { events: 0, u64: 0 }; 16];
    loop {
        // SAFETY: `evs` has room for the 16 events epoll_wait may write; `r` keeps `r.ep` open.
        let n = unsafe { libc::epoll_wait(r.ep.as_raw_fd(), evs.as_mut_ptr(), 16, -1) };
        for e in evs.iter().take(n.max(0) as usize) {
            let pid = e.u64 as i32;
            let mut kids = r.kids.lock().unwrap();
            let Some(Kid { child, pidfd, .. }) = kids.get_mut(&pid) else {
                continue;
            };
            let st = match child.try_wait(pidfd) {
                Ok(None) => continue,
                Ok(Some(st)) => Some(st),
                // The child is gone all the same (its pidfd is readable): its exit is
                // reported, the status unknown.
                Err(e) => {
                    elog::eprintln!("impulse-shell: cannot wait for child {pid}: {e}");
                    None
                }
            };
            // Dropping the pidfd also removes it from the epoll set.
            let on_exit = kids.remove(&pid).and_then(|k| k.on_exit);
            r.exiting.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            drop(kids);
            if let Some(f) = on_exit {
                // This is the one thread that reaps: a callback that panics must not end it,
                // or every later child stays a zombie and its exit is never reported.
                caught_as("child exit callback", move || f(st));
            }
            // The child reaped gave a process slot back: what waits for a thread asks again.
            rewatch();
            let _kids = r.kids.lock().unwrap();
            r.exiting.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            r.gone.notify_all();
        }
    }
}

/// Reap a detached child when it exits (no zombie), without a thread waiting on it.
pub fn reap(ch: std::process::Child) {
    watch(ch, None);
}

/// [`reap`], then call `f` with the child's exit status on the reaper thread (keep it short:
/// hand longer work to [`run`]). The status is `None` when wait(2) failed: the child is gone,
/// how it ended is unknown.
pub fn reap_then(
    ch: std::process::Child,
    f: impl FnOnce(Option<std::process::ExitStatus>) + Send + 'static,
) {
    watch(ch, Some(Box::new(f)));
}

/// Reap `pid`, a child this image inherited across execve(2) from the image that spawned it
/// (a reload): std has no `Child` for it, and nothing else waits for it.
pub fn adopt(pid: i32) {
    adopt_with(pid, None);
}

/// [`adopt`], then call `f` with the child's exit status on the reaper thread.
pub fn adopt_then(pid: i32, f: impl FnOnce(Option<std::process::ExitStatus>) + Send + 'static) {
    adopt_with(pid, Some(Box::new(f)));
}

fn adopt_with(pid: i32, on_exit: Option<OnExit>) {
    watch_with(Waited::Adopted(pid), on_exit, pidfd_open(pid));
}

/// The processes whose parent is this one (every thread's children), from `/proc`.
pub fn children() -> Vec<i32> {
    let me = std::process::id() as i32;
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            // pid (comm) state ppid ...
            std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
                s.rsplit_once(')')
                    .and_then(|(_, r)| r.split_whitespace().nth(1)?.parse::<i32>().ok())
                    == Some(me)
            })
        })
        .collect()
}

fn watch(ch: std::process::Child, on_exit: Option<OnExit>) {
    let pidfd = pidfd_open(ch.id() as i32);
    watch_with(Waited::Spawned(ch), on_exit, pidfd);
}

/// Children that got neither the reaper nor a thread of their own: [`rewatch`] tries them
/// again before the next job or child, so each is still reaped and its exit reported.
static UNWATCHED: Mutex<Vec<(Waited, Option<OnExit>)>> = Mutex::new(Vec::new());

/// How many children [`UNWATCHED`] holds (written under its lock): none unless the system is
/// out of threads, so a job submitted reads this and takes no lock.
static UNWATCHED_LEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Whose jobs wait for a thread the system refused (`EAGAIN`).
enum Waiting {
    Lane(&'static Lane),
    Long(&'static Elastic),
    Pool(&'static Pool),
}

/// The queues whose jobs wait for a thread the system refused: each is here once (its own
/// `listed`, under its own lock, which is taken before this one and never after).
///
/// Nothing tells a process that the system has a thread to give again, so the thread is asked
/// for when something of ours has ended or is about to need one anyway, never on a timer:
/// - a thread of this crate whose own work is done (a lane that emptied, a [`long`] thread
///   with no job, the thread that waited for one child) does the waiting work itself before
///   it ends ([`serve_waiting`]): it is the thread that was asked for;
/// - the reaper asks once a child has been reaped, which gave a process slot back
///   (`RLIMIT_NPROC` counts threads and processes together);
/// - every job and child taken in asks first ([`rewatch`]), whichever queue it is for;
/// - [`ask_again`] asks, for a caller about to end the process.
static WAITING: Mutex<Vec<Waiting>> = Mutex::new(Vec::new());

/// How many queues [`WAITING`] holds (written under its lock): none unless the system is out
/// of threads, so a job submitted reads this and takes no lock.
static WAITING_LEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many queues hold jobs the system refused a thread, or run them now that it gave one:
/// each from its refusal until it has run what it held. [`finish_held`] waits for none.
static HELD: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// A queue starts (`true`) or stops holding jobs refused their thread ([`HELD`]). Called with
/// the queue's lock held.
fn holds(more: bool) {
    let mut held = HELD.0.lock().unwrap_or_else(|e| e.into_inner());
    if more {
        *held += 1;
    } else {
        *held -= 1;
        if *held == 0 {
            HELD.1.notify_all();
        }
    }
}

/// For a caller about to end the process: asks again for every thread the system refused
/// ([`ask_again`]) and waits until the jobs that waited for one have run, `limit` at most.
/// `false`: some had not, the system still refusing or the jobs taking longer. With nothing
/// refused, which is the usual case, it returns at once.
pub fn finish_held(limit: Duration) -> bool {
    ask_again();
    let held = HELD.0.lock().unwrap_or_else(|e| e.into_inner());
    let (_held, limit) = HELD
        .1
        .wait_timeout_while(held, limit, |n| *n > 0)
        .unwrap_or_else(|e| e.into_inner());
    !limit.timed_out()
}

thread_local! {
    /// Whether this thread, a lane's, still counts in [`LANES`].
    static LANE_COUNTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A lane's thread stops counting as a lane at work: when it ends, or before it goes on as a
/// worker of a pool, which counts its own.
fn lane_thread_done() {
    if LANE_COUNTED.with(|c| c.replace(false)) {
        lanes_busy(-1);
    }
}

fn waits(w: Waiting) {
    let mut waiting = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    waiting.push(w);
    WAITING_LEN.store(waiting.len(), std::sync::atomic::Ordering::Relaxed);
}

/// Asks again for the thread of every queue in [`WAITING`]; one refused again goes back there,
/// unlogged: its refusal was logged when its job was taken in.
#[cold]
#[inline(never)]
fn ask_waiting() {
    let all = {
        let mut waiting = WAITING.lock().unwrap_or_else(|e| e.into_inner());
        WAITING_LEN.store(0, std::sync::atomic::Ordering::Relaxed);
        std::mem::take(&mut *waiting)
    };
    for w in all {
        match w {
            Waiting::Lane(l) => l.ask(),
            Waiting::Long(e) => e.ask(),
            Waiting::Pool(p) => p.ask(),
        }
    }
}

/// Called by a thread of this crate whose own work is done, before it ends: it does the work
/// that waits for a thread ([`WAITING`], [`UNWATCHED`]), being one. It cannot ask for another
/// instead: the system counts it until it is gone.
fn serve_waiting() {
    loop {
        let w = {
            let mut waiting = WAITING.lock().unwrap_or_else(|e| e.into_inner());
            // The bounded pool last: its worker never ends, so it serves nothing after.
            let at = waiting
                .iter()
                .position(|w| !matches!(w, Waiting::Pool(_)))
                .or(waiting.len().checked_sub(1));
            let w = at.map(|at| waiting.swap_remove(at));
            WAITING_LEN.store(waiting.len(), std::sync::atomic::Ordering::Relaxed);
            w
        };
        match w {
            Some(Waiting::Lane(l)) => l.serve(),
            Some(Waiting::Long(e)) => e.serve(),
            Some(Waiting::Pool(p)) => p.serve(),
            None => {
                // Jobs first: a child may live as long as the shell.
                let kid = {
                    let mut unwatched = UNWATCHED.lock().unwrap();
                    let kid = unwatched.pop();
                    UNWATCHED_LEN.store(unwatched.len(), std::sync::atomic::Ordering::Relaxed);
                    kid
                };
                let Some((ch, on_exit)) = kid else {
                    return;
                };
                lane_thread_done();
                wait_here(ch, on_exit);
            }
        }
    }
}

/// Waits for `ch` on the calling thread, which has nothing else to do, and reports its exit.
fn wait_here(ch: Waited, on_exit: Option<OnExit>) {
    let pid = ch.pid();
    let st = ch
        .wait()
        .map_err(|e| elog::eprintln!("impulse-shell: cannot wait for child {pid}: {e}"))
        .ok();
    if let Some(f) = on_exit {
        caught_as("child exit callback", move || f(st));
    }
}

/// Asks the system again for every thread it refused: the ones of [`Lane::run`], [`long`] and
/// [`run`] jobs that are still queued, and of children nothing waits for yet. For a caller
/// that is about to end the process and waits for some of those jobs first; otherwise the
/// threads are asked for without it ([`WAITING`]).
pub fn ask_again() {
    rewatch();
}

/// Called before every job and child is taken in: the children in [`UNWATCHED`] go to the
/// reaper or to a thread of their own, or back to the list, and the queues in [`WAITING`] ask
/// for their thread again. No job or exit callback runs here, and the only locks taken are the
/// reaper's and the queues' own, which no caller holds.
#[inline]
fn rewatch() {
    if UNWATCHED_LEN.load(std::sync::atomic::Ordering::Relaxed) != 0 {
        rewatch_unwatched();
    }
    if WAITING_LEN.load(std::sync::atomic::Ordering::Relaxed) != 0 {
        ask_waiting();
    }
}

#[cold]
#[inline(never)]
fn rewatch_unwatched() {
    let left = {
        let mut unwatched = UNWATCHED.lock().unwrap();
        UNWATCHED_LEN.store(0, std::sync::atomic::Ordering::Relaxed);
        std::mem::take(&mut *unwatched)
    };
    for (ch, on_exit) in left {
        // A child nothing waited for keeps its pid, as a zombie once it exited.
        let pidfd = pidfd_open(ch.pid());
        watch_one(ch, on_exit, pidfd, false);
    }
}

fn watch_with(ch: Waited, on_exit: Option<OnExit>, pidfd: std::io::Result<OwnedFd>) {
    rewatch();
    watch_one(ch, on_exit, pidfd, true);
}

fn spawn_reap1(f: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    #[cfg(test)]
    if tests::REAP1_SPAWN_FAILS.get() {
        return Err(std::io::Error::from_raw_os_error(libc::EAGAIN));
    }
    std::thread::Builder::new()
        .name("reap1".into())
        .spawn(f)
        .map(drop)
}

/// `first` is false for a child from [`UNWATCHED`]: its refusal was logged when it got there,
/// and is not logged again for every job submitted while no thread is to be had.
fn watch_one(ch: Waited, on_exit: Option<OnExit>, pidfd: std::io::Result<OwnedFd>, first: bool) {
    let pid = ch.pid();
    let err: std::io::Error = match (reaper_for(first), pidfd) {
        (Some(r), Ok(fd)) => {
            let mut ev = libc::epoll_event {
                events: libc::EPOLLIN as u32,
                u64: pid as u64,
            };
            let mut kids = r.kids.lock().unwrap();
            // SAFETY: both fds are open (`fd` is moved into `kids` below, the epoll fd is
            // 'static) and `ev` is a live epoll_event.
            let added = unsafe {
                libc::epoll_ctl(
                    r.ep.as_raw_fd(),
                    libc::EPOLL_CTL_ADD,
                    fd.as_raw_fd(),
                    &mut ev,
                )
            };
            if added == 0 {
                kids.insert(
                    pid,
                    Kid {
                        child: ch,
                        pidfd: fd,
                        on_exit,
                    },
                );
                return;
            }
            std::io::Error::last_os_error()
        }
        (None, _) => std::io::Error::other("no reaper thread"),
        (_, Err(e)) => e,
    };
    // Out of fds or threads: a child must still be reaped (and its exit reported), so it gets
    // a thread of its own.
    if first {
        elog::eprintln!(
            "impulse-shell: cannot watch child {pid} ({err}): waiting for it on a thread"
        );
    }
    // A refused thread drops what it was given, so the child stays here until one starts.
    let kid = std::sync::Arc::new(Mutex::new(Some((ch, on_exit))));
    let on_thread = kid.clone();
    let waited = spawn_reap1(move || {
        let Some((ch, on_exit)) = on_thread.lock().unwrap().take() else {
            return;
        };
        wait_here(ch, on_exit);
        serve_waiting();
    });
    if let Err(e) = waited {
        if first {
            elog::eprintln!(
                "impulse-shell: cannot start a thread to wait for child {pid} ({e}): it is tried again \
                 with the next job or child"
            );
        }
        let mut unwatched = UNWATCHED.lock().unwrap();
        unwatched.extend(kid.lock().unwrap().take());
        UNWATCHED_LEN.store(unwatched.len(), std::sync::atomic::Ordering::Relaxed);
    }
}

/// Blocks until the pool has no work left: no [`run`] job due or running, no [`long`] job
/// queued or running, no [`Lane`] job queued or running, and no child [`reap`] still waits for. Jobs whose [`after`] delay has not
/// passed yet do not count, nor do the jobs that wait for a thread the system refused
/// ([`WAITING`]): nothing runs them before one starts. Work a job hands on (a `long` job's `run`, a child it detaches) is
/// waited for as well, since every stage is checked again until all are idle at once.
pub fn wait_idle() {
    let (p, e) = (pool(), elastic());
    loop {
        let mut g = p.q.lock().unwrap();
        while !g.idle() {
            g = p.done.wait(g).unwrap();
        }
        drop(g);
        let mut g = e.q.lock().unwrap();
        while !Elastic::idle(&g) {
            g = e.done.wait(g).unwrap();
        }
        drop(g);
        let mut n = LANES.0.lock().unwrap_or_else(|e| e.into_inner());
        while *n != 0 {
            n = LANES.1.wait(n).unwrap_or_else(|e| e.into_inner());
        }
        drop(n);
        // Started by the first `reap`, which may have come from a job waited for above.
        if let Some(r) = REAPER.get() {
            let mut k = r.kids.lock().unwrap();
            while !Reaper::idle(r, &k) {
                k = r.gone.wait(k).unwrap();
            }
        }
        if p.q.lock().unwrap().idle()
            && Elastic::idle(&e.q.lock().unwrap())
            && lanes_idle()
            && REAPER
                .get()
                .is_none_or(|r| Reaper::idle(r, &r.kids.lock().unwrap()))
        {
            return;
        }
    }
}

/// A lane's queued jobs, whether a thread is draining them, whether the lane is in
/// [`WAITING`], and whether it counts in [`HELD`].
type LaneQ = (std::collections::VecDeque<Job>, bool, bool, bool);

/// Jobs that run one at a time, in the order submitted, on a thread of the lane's own that ends
/// once the lane is empty. For a peer that serves requests one at a time anyway (a compositor
/// answering IPC on its main loop, a daemon applying requests in arrival order): a burst holds
/// one thread instead of one per request, and a peer that never answers holds only its own
/// lane, never the bounded workers of [`run`] nor a slot of [`long`] (whose outside jobs queue
/// once [`LONG_MAX`] run: a lane drained there would wait behind unrelated transfers).
pub struct Lane {
    q: Mutex<LaneQ>,
}

impl Lane {
    pub const fn new() -> Lane {
        Lane {
            q: Mutex::new((std::collections::VecDeque::new(), false, false, false)),
        }
    }

    /// Run `f` once every job submitted to this lane before it has finished. The caller never
    /// runs a job itself: the jobs wait on a peer, the caller is a main loop.
    ///
    /// When the lane needs a thread and the system refuses one (`EAGAIN`), `f` stays queued and
    /// the refusal is logged; the thread is asked for again as [`WAITING`] tells, and the one
    /// the lane gets runs everything queued, in order.
    pub fn run(&'static self, f: impl FnOnce() + Send + 'static) {
        rewatch();
        let mut q = self.lock();
        q.0.push_back(Box::new(f));
        if q.1 {
            return;
        }
        if let Err(e) = self.start(&mut q) {
            let held = q.0.len();
            drop(q);
            elog::eprintln!(
                "impulse-shell: cannot start a lane thread: {e}; {held} queued until one starts"
            );
        }
    }

    /// Starts the lane's thread, which the caller found missing; refused, the lane waits for
    /// it in [`WAITING`].
    fn start(&'static self, q: &mut LaneQ) -> std::io::Result<()> {
        // Started under the lock, which its first look at the queue waits for.
        let started = spawn_lane(move || self.drain_and_serve());
        match started {
            Ok(()) => {
                q.1 = true;
                // Counted before the lane unlocks, so [`wait_idle`] never sees a queued job
                // and no draining lane.
                lanes_busy(1);
            }
            Err(_) if !q.2 => {
                q.2 = true;
                waits(Waiting::Lane(self));
                if !q.3 {
                    q.3 = true;
                    holds(true);
                }
                wake_idle_long();
            }
            Err(_) => {}
        }
        started
    }

    /// [`ask_waiting`]: the thread asked for again.
    fn ask(&'static self) {
        let mut q = self.lock();
        q.2 = false;
        if !q.1 && !q.0.is_empty() {
            let _ = self.start(&mut q);
        }
    }

    /// [`serve_waiting`]: the calling thread, its own work done, is the thread the lane was
    /// refused.
    fn serve(&'static self) {
        let mut q = self.lock();
        q.2 = false;
        if q.1 || q.0.is_empty() {
            return;
        }
        q.1 = true;
        lanes_busy(1);
        drop(q);
        self.drain();
        lanes_busy(-1);
    }

    /// What the lane's own thread does. It counts as a lane at work until it has also run the
    /// jobs of the lanes that waited for a thread, so [`wait_idle`] waits for those too.
    fn drain_and_serve(&self) {
        LANE_COUNTED.with(|c| c.set(true));
        self.drain();
        serve_waiting();
        lane_thread_done();
    }

    /// [`Lane::run`] for a lane shared by its users rather than static (one per peer object
    /// whose requests must stay in order), and for a caller that must never run a job itself:
    /// the jobs wait on the peer, the caller is a main loop. The draining thread holds the lane
    /// until it is empty.
    ///
    /// `Err` when the lane needs a thread and the system refuses one (`EAGAIN`): `f` is dropped
    /// unrun and the lane is left as it was, idle, for the caller to ask again later.
    pub fn try_run_shared(
        self: &std::sync::Arc<Self>,
        f: impl FnOnce() + Send + 'static,
    ) -> std::io::Result<()> {
        rewatch();
        let mut q = self.lock();
        if !q.1 {
            let me = self.clone();
            // Started under the lock, which its first look at the queue waits for: it finds
            // `f`, and a refusal leaves nothing queued behind a drainer that does not exist.
            spawn_lane(move || me.drain_and_serve())?;
            q.1 = true;
            // Counted before the lane unlocks, as in `run`.
            lanes_busy(1);
        }
        q.0.push_back(Box::new(f));
        Ok(())
    }

    fn drain(&self) {
        while let Some(f) = {
            let mut q = self.lock();
            let f = q.0.pop_front();
            q.1 = f.is_some();
            if f.is_none() && q.3 {
                q.3 = false;
                holds(false);
            }
            f
        } {
            caught(f);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LaneQ> {
        self.q.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn spawn_lane(f: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    #[cfg(test)]
    if tests::LANE_SPAWN_FAILS.get() {
        return Err(std::io::Error::from_raw_os_error(libc::EAGAIN));
    }
    std::thread::Builder::new()
        .name("lane".into())
        .spawn(f)
        .map(drop)
}

/// Lanes draining now, and the signal that one finished ([`wait_idle`]).
static LANES: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

fn lanes_busy(d: isize) {
    let mut n = LANES.0.lock().unwrap_or_else(|e| e.into_inner());
    *n = n.saturating_add_signed(d);
    if *n == 0 {
        LANES.1.notify_all();
    }
}

fn lanes_idle() -> bool {
    *LANES.0.lock().unwrap_or_else(|e| e.into_inner()) == 0
}

impl Default for Lane {
    fn default() -> Lane {
        Lane::new()
    }
}

/// Children the reaper still waits for (tests, stats).
pub fn reaping() -> usize {
    reaper().map_or(0, |r| r.kids.lock().unwrap().len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    thread_local! {
        /// Makes this thread fail to start a bounded pool worker, as on `EAGAIN`.
        pub(super) static POOL_SPAWN_FAILS: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
        /// Makes this thread's [`Lane::run`] and [`Lane::try_run_shared`] fail to start a
        /// thread, as on `EAGAIN`.
        pub(super) static LANE_SPAWN_FAILS: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
        /// Makes this thread fail to start a [`long`] thread, as on `EAGAIN`.
        pub(super) static LONG_SPAWN_FAILS: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
        /// Makes this thread fail to start the reaper thread, as on `EAGAIN`.
        pub(super) static REAPER_SPAWN_FAILS: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
        /// Makes this thread fail to start a child's own `reap1` thread, as on `EAGAIN`.
        pub(super) static REAP1_SPAWN_FAILS: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
    }

    fn threads() -> usize {
        std::fs::read_dir("/proc/self/task").unwrap().count()
    }

    /// Runs `body` alone in a re-exec'd test process. The pool and the thread count are
    /// process-wide: next to other tests, timed jobs queue behind their bursts and libtest
    /// starts their threads in the middle of a count.
    fn alone(test: &str, body: impl FnOnce()) {
        if std::env::var("WORKPOOL_TEST").is_ok_and(|t| t == test) {
            return body();
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &format!("tests::{test}"), "--nocapture"])
            .args(["--test-threads=1"])
            .env("WORKPOOL_TEST", test)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "{stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn small_pool(max: usize, idle: Duration) -> &'static Elastic {
        Box::leak(Box::new(Elastic::new(max, idle)))
    }

    /// Jobs block in [`Gate::pass`] until the test calls [`Gate::open`].
    #[derive(Clone, Default)]
    struct Gate(Arc<(Mutex<bool>, Condvar)>);

    impl Gate {
        fn pass(&self) {
            let mut open = self.0.0.lock().unwrap();
            while !*open {
                open = self.0.1.wait(open).unwrap();
            }
        }

        fn open(&self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }
    }

    thread_local! {
        static RAN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Whether this is the first job the calling thread runs.
    fn first_job_here() -> bool {
        !RAN.replace(true)
    }

    /// From now on the pool's clock moves only by [`advance_pool`].
    fn freeze_pool() {
        pool()
            .q
            .lock()
            .unwrap()
            .frozen
            .get_or_insert_with(Instant::now);
    }

    /// Moves the frozen pool clock by `d`; returns once a worker has looked at the new time, so
    /// the earliest job due by then has been taken and every job not yet due is still queued.
    fn advance_pool(d: Duration) {
        let p = pool();
        let mut g = p.q.lock().unwrap();
        let t = g.frozen.expect("freeze_pool first");
        g.frozen = Some(t + d);
        g.advanced += 1;
        let n = g.advanced;
        p.cv.notify_all();
        while g.workers > 0 && g.seen < n {
            g = p.done.wait(g).unwrap();
        }
    }

    /// From now on `e`'s clock moves only by [`advance_long`].
    fn freeze_long(e: &Elastic) {
        e.q.lock().unwrap().frozen.get_or_insert_with(Instant::now);
    }

    /// Moves `e`'s frozen clock by `d`; returns once every idle thread has looked at the new
    /// time (and exited if its idle time is up).
    fn advance_long(e: &Elastic, d: Duration) {
        let mut g = e.q.lock().unwrap();
        let t = g.frozen.expect("freeze_long first");
        g.frozen = Some(t + d);
        g.advanced += 1;
        g.behind = g.idle;
        e.cv.notify_all();
        while g.behind > 0 {
            g = e.done.wait(g).unwrap();
        }
    }

    #[test]
    fn long_jobs_beyond_the_cap_queue_instead_of_starting_threads() {
        let e = small_pool(3, Duration::from_millis(200));
        // No thread idles out mid-test: only the jobs move the pool.
        freeze_long(e);
        let (now, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let gate = Gate::default();
        let (started_tx, started) = std::sync::mpsc::channel();
        let (tx, rx) = std::sync::mpsc::channel();
        for _ in 0..10 {
            let (now, peak, gate) = (now.clone(), peak.clone(), gate.clone());
            let (started_tx, tx) = (started_tx.clone(), tx.clone());
            e.submit(Box::new(move || {
                let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(n, Ordering::SeqCst);
                started_tx.send(()).unwrap();
                gate.pass();
                now.fetch_sub(1, Ordering::SeqCst);
                tx.send(()).unwrap();
            }));
            assert!(e.q.lock().unwrap().threads <= 3);
        }
        // The cap is used: three jobs run at once, each holding its slot until the gate opens,
        // and the other seven wait in the queue, not on threads of their own.
        for _ in 0..3 {
            started.recv().unwrap();
        }
        {
            let g = e.q.lock().unwrap();
            assert_eq!((g.running, g.threads, g.outer.len()), (3, 3, 7));
        }
        gate.open();
        for _ in 0..10 {
            rx.recv().unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 3, "the cap was passed");
    }

    #[test]
    fn nested_long_jobs_run_although_the_cap_is_reached() {
        // Every outer job waits for a job it submits (as `http::cancellable` does): with nested
        // jobs queued behind outer ones this would deadlock.
        let e = small_pool(2, Duration::from_millis(200));
        // No thread idles out mid-test: only the jobs move the pool.
        freeze_long(e);
        let (tx, rx) = std::sync::mpsc::channel();
        for k in 0..6 {
            let tx = tx.clone();
            e.submit(Box::new(move || {
                let (itx, irx) = std::sync::mpsc::channel();
                e.submit(Box::new(move || itx.send(k).unwrap()));
                tx.send(irx.recv().unwrap()).unwrap();
            }));
        }
        let mut got: Vec<i32> = (0..6).map(|_| rx.recv().unwrap()).collect();
        got.sort();
        assert_eq!(got, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn long_threads_are_reused_then_exit_when_idle() {
        alone("long_threads_are_reused_then_exit_when_idle", || {
            let idle = Duration::from_millis(300);
            let e = small_pool(4, idle);
            freeze_long(e);
            let before = threads();
            let spawned = Arc::new(AtomicUsize::new(0));
            let b = Arc::new(std::sync::Barrier::new(5));
            for _ in 0..4 {
                let (b, spawned) = (b.clone(), spawned.clone());
                e.submit(Box::new(move || {
                    b.wait();
                    if first_job_here() {
                        spawned.fetch_add(1, Ordering::SeqCst);
                    }
                }));
            }
            b.wait();
            assert!(threads() <= before + 4);
            // A job queued right after the burst reuses an idle thread.
            let (tx, rx) = std::sync::mpsc::channel();
            let spawned2 = spawned.clone();
            e.submit(Box::new(move || {
                let n = threads();
                if first_job_here() {
                    spawned2.fetch_add(1, Ordering::SeqCst);
                }
                tx.send(n).unwrap();
            }));
            assert!(
                rx.recv().unwrap() <= before + 4,
                "an idle thread was not reused"
            );
            let (tx, rx) = std::sync::mpsc::channel();
            let spawned3 = spawned.clone();
            e.submit(Box::new(move || {
                if first_job_here() {
                    spawned3.fetch_add(1, Ordering::SeqCst);
                }
                tx.send(()).unwrap();
                panic!("a panicking long job does not take its peers down")
            }));
            // Every job has run its `first_job_here`: no thread is started after this.
            rx.recv().unwrap();
            let n = spawned.load(Ordering::SeqCst);
            assert!((1..=4).contains(&n), "{n} threads ran the jobs");
            // Every thread waits idle, those started for a job another one took first too.
            let mut g = e.q.lock().unwrap();
            while g.busy != 0 || g.idle != g.threads {
                g = e.done.wait(g).unwrap();
            }
            let started = g.threads;
            assert!(started >= n);
            drop(g);
            advance_long(e, idle - Duration::from_nanos(1));
            assert_eq!(
                e.q.lock().unwrap().threads,
                started,
                "a thread exited before its idle time was up"
            );
            advance_long(e, Duration::from_nanos(1));
            assert_eq!(e.q.lock().unwrap().threads, 0, "an idle thread lingered");
            // And it starts again from nothing.
            let (tx, rx) = std::sync::mpsc::channel();
            e.submit(Box::new(move || tx.send(()).unwrap()));
            rx.recv().unwrap();
            // The process-wide pool runs jobs too.
            let (tx, rx) = std::sync::mpsc::channel();
            long(move || {
                tx.send(std::thread::current().name().map(str::to_owned))
                    .unwrap()
            });
            let name = rx.recv().unwrap().unwrap();
            assert!(name.starts_with("long"), "{name}");
        });
    }

    #[test]
    fn runs_jobs_on_bounded_workers_in_deadline_order() {
        alone("runs_jobs_on_bounded_workers_in_deadline_order", || {
            let p = pool();
            reaper();
            freeze_pool();
            let t0 = threads();
            let (tx, rx) = std::sync::mpsc::channel();
            let (early_tx, early_ran) = std::sync::mpsc::channel();
            let t = p.q.lock().unwrap().now();
            let (early, late) = (t + Duration::from_millis(40), t + Duration::from_millis(80));
            {
                // One critical section, so no worker pops a job before the whole batch is queued.
                let mut g = p.q.lock().unwrap();
                push(
                    &mut g,
                    late,
                    Box::new(move || {
                        // Popped only once it was the earliest job: none due before it is left.
                        let earlier = pool()
                            .q
                            .lock()
                            .unwrap()
                            .due
                            .iter()
                            .filter(|d| d.at < late)
                            .count();
                        tx.send(earlier).unwrap()
                    }),
                );
                for _ in 0..200 {
                    let early_tx = early_tx.clone();
                    push(&mut g, early, Box::new(move || early_tx.send(()).unwrap()));
                }
                p.cv.notify_all();
            }
            advance_pool(early - t - Duration::from_nanos(1));
            assert_eq!(
                p.q.lock().unwrap().due.len(),
                201,
                "a delayed job ran early"
            );
            // Every job falls due at once: the later deadline, queued first, runs last.
            advance_pool(late - early + Duration::from_nanos(1));
            for _ in 0..200 {
                early_ran.recv().unwrap();
            }
            assert_eq!(
                rx.recv().unwrap(),
                0,
                "the later deadline ran before earlier ones, despite queuing first"
            );

            let (tx, rx) = std::sync::mpsc::channel();
            for _ in 0..200 {
                let tx = tx.clone();
                run(move || tx.send(()).unwrap());
            }
            assert!(threads() <= t0, "a burst must not spawn threads");
            for _ in 0..200 {
                rx.recv().unwrap();
            }
        });
    }

    #[test]
    fn delayed_job_waits_without_blocking_others() {
        alone("delayed_job_waits_without_blocking_others", || {
            freeze_pool();
            let (tx, rx) = std::sync::mpsc::channel();
            let delay = Duration::from_millis(300);
            for _ in 0..(WORKERS * 2) {
                let tx = tx.clone();
                after(delay, move || tx.send(()).unwrap()).unwrap();
            }
            // Every worker runs a job at once while the delayed jobs wait: none of them holds
            // a worker (else the barrier would never fill).
            let b = Arc::new(std::sync::Barrier::new(WORKERS + 1));
            for _ in 0..WORKERS {
                let b = b.clone();
                run(move || {
                    b.wait();
                });
            }
            b.wait();
            advance_pool(delay - Duration::from_nanos(1));
            assert_eq!(
                pool().q.lock().unwrap().due.len(),
                WORKERS * 2,
                "a delayed job ran early"
            );
            advance_pool(Duration::from_nanos(1));
            for _ in 0..WORKERS * 2 {
                rx.recv().unwrap();
            }
        });
    }

    #[test]
    fn panicking_jobs_do_not_kill_the_workers() {
        alone("panicking_jobs_do_not_kill_the_workers", || {
            pool();
            freeze_pool();
            let t0 = threads();
            let (tx, rx) = std::sync::mpsc::channel();
            for k in 0..WORKERS * 3 {
                let tx = tx.clone();
                run(move || {
                    tx.send(()).unwrap();
                    panic!("job {k}");
                });
            }
            for _ in 0..WORKERS * 3 {
                rx.recv().unwrap();
            }
            let (tx, rx) = std::sync::mpsc::channel();
            for k in 0..WORKERS * 2 {
                let tx = tx.clone();
                after(Duration::from_millis(10), move || tx.send(k).unwrap()).unwrap();
            }
            advance_pool(Duration::from_millis(10));
            for _ in 0..WORKERS * 2 {
                rx.recv().expect("the pool died after its jobs panicked");
            }
            assert_eq!(threads(), t0, "workers must live on, not be replaced");
        });
    }

    fn zombie(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
            s.rsplit(')')
                .next()
                .is_some_and(|r| r.trim_start().starts_with('Z'))
        })
    }

    #[test]
    fn a_lane_runs_in_order_and_a_stuck_job_holds_only_its_lane() {
        static LANE: Lane = Lane::new();
        let (go_tx, go) = std::sync::mpsc::channel::<()>();
        let (tx, rx) = std::sync::mpsc::channel::<usize>();
        LANE.run(move || go.recv().unwrap());
        for i in 0..3 {
            let tx = tx.clone();
            LANE.run(move || tx.send(i).unwrap());
        }
        drop(tx);
        // Every bounded worker is free while the lane's first job waits.
        let (ptx, prx) = std::sync::mpsc::channel();
        for _ in 0..=WORKERS {
            let ptx = ptx.clone();
            run(move || ptx.send(()).unwrap());
        }
        for _ in 0..=WORKERS {
            prx.recv().unwrap();
        }
        go_tx.send(()).unwrap();
        assert_eq!(rx.iter().collect::<Vec<_>>(), [0, 1, 2]);
    }

    /// With no thread to be had, the submitter does not run the lane's jobs itself: they stay
    /// queued, in order, with no drainer, and the first submission that is given a thread
    /// starts the one that runs them all, the earlier ones first.
    #[test]
    fn a_lane_whose_thread_fails_to_start_keeps_its_jobs_for_the_next_submission() {
        alone(
            "a_lane_whose_thread_fails_to_start_keeps_its_jobs_for_the_next_submission",
            lane_keeps_its_jobs,
        );
    }

    fn lane_keeps_its_jobs() {
        static LANE: Lane = Lane::new();
        let (tx, rx) = std::sync::mpsc::channel::<(usize, std::thread::ThreadId)>();
        let job = |i| {
            let tx = tx.clone();
            move || tx.send((i, std::thread::current().id())).unwrap()
        };
        LANE_SPAWN_FAILS.set(true);
        LANE.run(job(0));
        LANE.run(job(1));
        LANE_SPAWN_FAILS.set(false);
        assert!(rx.try_recv().is_err(), "a refused job ran on its submitter");
        {
            let q = LANE.lock();
            assert!(
                q.0.len() == 2 && !q.1,
                "the lane lost a job or has a drainer"
            );
        }
        LANE.run(job(2));
        let ran: Vec<_> = (0..3).map(|_| rx.recv().unwrap()).collect();
        assert_eq!(ran.iter().map(|r| r.0).collect::<Vec<_>>(), [0, 1, 2]);
        let caller = std::thread::current().id();
        assert!(ran.iter().all(|r| r.1 == ran[0].1 && r.1 != caller));
    }

    /// A lane job refused its thread does not wait for the lane's next job: a job taken in by
    /// any other queue asks for the lane's thread again.
    #[test]
    fn a_lane_refused_its_thread_is_given_it_when_another_queue_takes_a_job() {
        alone(
            "a_lane_refused_its_thread_is_given_it_when_another_queue_takes_a_job",
            || {
                static REFUSED: Lane = Lane::new();
                static OTHER: Lane = Lane::new();
                let ran = Arc::new(AtomicUsize::new(0));
                let r = ran.clone();
                LANE_SPAWN_FAILS.set(true);
                REFUSED.run(move || {
                    r.fetch_add(1, Ordering::SeqCst);
                });
                LANE_SPAWN_FAILS.set(false);
                assert_eq!(WAITING_LEN.load(Ordering::SeqCst), 1);
                let log = logged(|| OTHER.run(|| ()));
                assert_eq!(log, "");
                wait_idle();
                assert_eq!(ran.load(Ordering::SeqCst), 1, "the refused job did not run");
                assert_eq!(WAITING_LEN.load(Ordering::SeqCst), 0);
            },
        );
    }

    /// While the system still refuses threads, a lane's thread whose own lane has emptied runs
    /// the jobs of a lane that was refused its thread before it ends: it is a thread, and it
    /// could not be asked for again before it is gone.
    #[test]
    fn a_lane_refused_its_thread_is_drained_by_a_lane_thread_that_ends() {
        alone(
            "a_lane_refused_its_thread_is_drained_by_a_lane_thread_that_ends",
            || {
                static REFUSED: Lane = Lane::new();
                static BUSY: Lane = Lane::new();
                let gate = Gate::default();
                let (tx, rx) = std::sync::mpsc::channel();
                let (g, t) = (gate.clone(), tx.clone());
                BUSY.run(move || {
                    t.send(("busy", std::thread::current().id())).unwrap();
                    g.pass();
                });
                let busy = rx.recv().unwrap();
                LANE_SPAWN_FAILS.set(true);
                for i in ["first", "second"] {
                    let t = tx.clone();
                    REFUSED.run(move || t.send((i, std::thread::current().id())).unwrap());
                }
                assert!(rx.try_recv().is_err(), "a refused job ran on its submitter");
                gate.open();
                wait_idle();
                let ran: Vec<_> = rx.try_iter().collect();
                assert_eq!(ran, [("first", busy.1), ("second", busy.1)]);
                assert_eq!(WAITING_LEN.load(Ordering::SeqCst), 0);
            },
        );
    }

    /// While the calling thread is still refused threads, a child that ends gives a process
    /// slot back: the reaper, having reaped it, asks for the refused lane's thread again.
    #[test]
    fn a_lane_refused_its_thread_is_given_it_when_a_child_is_reaped() {
        alone(
            "a_lane_refused_its_thread_is_given_it_when_a_child_is_reaped",
            || {
                static REFUSED: Lane = Lane::new();
                let ran = Arc::new(AtomicUsize::new(0));
                let r = ran.clone();
                LANE_SPAWN_FAILS.set(true);
                REFUSED.run(move || {
                    r.fetch_add(1, Ordering::SeqCst);
                });
                let ch = std::process::Command::new("true").spawn().unwrap();
                reap(ch);
                wait_idle();
                assert_eq!(ran.load(Ordering::SeqCst), 1, "the refused job did not run");
            },
        );
    }

    /// With no [`long`] thread and none to be had, the job stays queued and is logged: its
    /// caller (a main loop) never runs it. [`ask_again`] starts the thread once the system
    /// gives one. [`try_long`] drops such a job and returns the refusal instead.
    #[test]
    fn a_long_job_refused_its_thread_waits_for_one_and_try_long_refuses_it() {
        alone(
            "a_long_job_refused_its_thread_waits_for_one_and_try_long_refuses_it",
            || {
                let e = small_pool(2, Duration::from_millis(200));
                freeze_long(e);
                let (tx, rx) = std::sync::mpsc::channel();
                let caller = std::thread::current().id();
                LONG_SPAWN_FAILS.set(true);
                let t = tx.clone();
                let log = logged(|| {
                    e.submit(Box::new(move || {
                        t.send(std::thread::current().id()).unwrap()
                    }))
                });
                assert_eq!(
                    log.matches("impulse-shell: cannot start a pool thread: ")
                        .count(),
                    1,
                    "log: {log:?}"
                );
                assert!(log.contains("; 1 queued until one starts"), "log: {log:?}");
                assert!(rx.try_recv().is_err(), "the caller ran the job");
                let refused = e
                    .take(Box::new(move || tx.send(caller).unwrap()), true)
                    .unwrap_err();
                assert_eq!(refused.raw_os_error(), Some(libc::EAGAIN));
                assert_eq!(e.q.lock().unwrap().outer.len(), 1);
                ask_again();
                assert_eq!(e.q.lock().unwrap().threads, 0);
                LONG_SPAWN_FAILS.set(false);
                ask_again();
                assert_ne!(rx.recv().unwrap(), caller);
                // The refused job was dropped unrun, and with it the last sender.
                assert!(rx.recv().is_err());
            },
        );
    }

    /// Before the process ends, [`finish_held`] asks for the refused threads again and waits
    /// for the jobs that waited for them: it reports `false` at its limit while the system
    /// still refuses, and returns once they have run when it no longer does.
    #[test]
    fn finish_held_asks_again_and_waits_for_the_jobs_refused_their_threads() {
        alone(
            "finish_held_asks_again_and_waits_for_the_jobs_refused_their_threads",
            || {
                static REFUSED: Lane = Lane::new();
                assert!(finish_held(Duration::ZERO), "nothing was refused");
                let ran = Arc::new(AtomicUsize::new(0));
                let job = || {
                    let r = ran.clone();
                    move || {
                        r.fetch_add(1, Ordering::SeqCst);
                    }
                };
                for fails in [&LANE_SPAWN_FAILS, &LONG_SPAWN_FAILS, &POOL_SPAWN_FAILS] {
                    fails.set(true);
                }
                REFUSED.run(job());
                long(job());
                run(job());
                assert!(!finish_held(Duration::ZERO), "still refused");
                assert_eq!(ran.load(Ordering::SeqCst), 0);
                for fails in [&LANE_SPAWN_FAILS, &LONG_SPAWN_FAILS, &POOL_SPAWN_FAILS] {
                    fails.set(false);
                }
                assert!(finish_held(Duration::from_secs(3600)));
                assert_eq!(ran.load(Ordering::SeqCst), 3);
            },
        );
    }

    /// While the system still refuses threads, a lane's thread that ends becomes the thread
    /// [`long`] and [`run`] were refused: their jobs run on it, [`wait_idle`] waits for them,
    /// and it stays the pool's worker.
    #[test]
    fn a_lane_thread_that_ends_runs_the_pool_jobs_refused_their_threads() {
        alone(
            "a_lane_thread_that_ends_runs_the_pool_jobs_refused_their_threads",
            || {
                static BUSY: Lane = Lane::new();
                let gate = Gate::default();
                let (tx, rx) = std::sync::mpsc::channel();
                let (g, t) = (gate.clone(), tx.clone());
                BUSY.run(move || {
                    t.send(("lane", std::thread::current().id())).unwrap();
                    g.pass();
                });
                let lane = rx.recv().unwrap().1;
                LONG_SPAWN_FAILS.set(true);
                POOL_SPAWN_FAILS.set(true);
                LANE_SPAWN_FAILS.set(true);
                let t = tx.clone();
                long(move || t.send(("long", std::thread::current().id())).unwrap());
                let t = tx.clone();
                run(move || t.send(("run", std::thread::current().id())).unwrap());
                assert!(rx.try_recv().is_err(), "a refused job ran on its submitter");
                gate.open();
                // `long` first: its thread ends once it has no job and another queue waits,
                // while a worker of the bounded pool never ends.
                assert_eq!(rx.recv().unwrap(), ("long", lane));
                assert_eq!(rx.recv().unwrap(), ("run", lane));
                wait_idle();
                assert_eq!(pool().q.lock().unwrap().workers, 1);
                assert_eq!(WAITING_LEN.load(Ordering::SeqCst), 0);
                // The pool has its worker now: the next job needs no new thread.
                run(move || tx.send(("next", std::thread::current().id())).unwrap());
                assert_eq!(rx.recv().unwrap(), ("next", lane));
            },
        );
    }

    /// A lane refused its thread while a [`long`] thread sits idle is given that thread at
    /// once: the idle thread ends and runs the lane's job, without waiting out its idle time.
    #[test]
    fn a_lane_refused_its_thread_is_given_an_idle_long_thread() {
        alone(
            "a_lane_refused_its_thread_is_given_an_idle_long_thread",
            || {
                static REFUSED: Lane = Lane::new();
                let (tx, rx) = std::sync::mpsc::channel();
                let t = tx.clone();
                long(move || t.send(std::thread::current().id()).unwrap());
                let idle = rx.recv().unwrap();
                wait_idle();
                LANE_SPAWN_FAILS.set(true);
                REFUSED.run(move || tx.send(std::thread::current().id()).unwrap());
                assert_eq!(rx.recv().unwrap(), idle);
                wait_idle();
                assert_eq!(elastic().q.lock().unwrap().threads, 0);
            },
        );
    }

    /// A shared lane never runs a job on its caller: with no thread to be had the job is
    /// refused and dropped, the lane stays idle and holds nothing, and the next submission
    /// (threads available again) starts its thread and runs in order.
    #[test]
    fn a_shared_lane_whose_thread_fails_to_start_refuses_the_job() {
        let lane = Arc::new(Lane::new());
        let (tx, rx) = std::sync::mpsc::channel::<usize>();
        let dropped = Arc::new(());
        LANE_SPAWN_FAILS.set(true);
        let (t, d) = (tx.clone(), dropped.clone());
        let e = lane
            .try_run_shared(move || {
                let _d = d;
                t.send(0).unwrap();
            })
            .unwrap_err();
        LANE_SPAWN_FAILS.set(false);
        assert_eq!(e.raw_os_error(), Some(libc::EAGAIN));
        assert_eq!(Arc::strong_count(&dropped), 1, "the refused job was kept");
        {
            let q = lane.lock();
            assert!(q.0.is_empty() && !q.1, "the lane kept a job or a drainer");
        }
        assert_eq!(Arc::strong_count(&lane), 1, "a drainer holds the lane");

        let caller = std::thread::current().id();
        let (go_tx, go) = std::sync::mpsc::channel::<()>();
        lane.try_run_shared(move || {
            assert_ne!(std::thread::current().id(), caller);
            go.recv().unwrap();
        })
        .unwrap();
        // Queued behind a job that is running: no thread is asked for, so none is refused.
        LANE_SPAWN_FAILS.set(true);
        for i in 1..3 {
            let t = tx.clone();
            lane.try_run_shared(move || t.send(i).unwrap()).unwrap();
        }
        LANE_SPAWN_FAILS.set(false);
        drop(tx);
        assert!(
            rx.try_recv().is_err(),
            "a job ran ahead of the lane's first"
        );
        go_tx.send(()).unwrap();
        assert_eq!(rx.iter().collect::<Vec<_>>(), [1, 2]);
    }

    /// With no worker to be had, [`run`] keeps the job queued and logs the refusal: the caller
    /// (a main loop) never runs it, and [`wait_idle`] does not wait for a job nothing runs. The
    /// next submission once threads are available again starts a worker, which runs both jobs
    /// in the order submitted.
    #[test]
    fn a_pool_whose_workers_fail_to_start_keeps_its_jobs_for_a_worker() {
        alone(
            "a_pool_whose_workers_fail_to_start_keeps_its_jobs_for_a_worker",
            || {
                POOL_SPAWN_FAILS.set(true);
                assert_eq!(pool().q.lock().unwrap().workers, 0);
                let (tx, rx) = std::sync::mpsc::channel();
                let name = || std::thread::current().name().map(str::to_owned);
                let t = tx.clone();
                let log = logged(|| run(move || t.send((0, name())).unwrap()));
                assert_eq!(
                    log.matches("impulse-shell: cannot start a pool worker: ")
                        .count(),
                    1,
                    "log: {log:?}"
                );
                assert!(log.contains("; 1 queued until one starts"), "log: {log:?}");
                assert!(rx.try_recv().is_err(), "the caller ran the job");
                wait_idle();
                assert_eq!(pool().q.lock().unwrap().due.len(), 1);
                POOL_SPAWN_FAILS.set(false);
                let log = logged(|| run(move || tx.send((1, name())).unwrap()));
                assert_eq!(log, "");
                let worker = Some("pool0".to_owned());
                assert_eq!(rx.recv().unwrap(), (0, worker.clone()));
                assert_eq!(rx.recv().unwrap(), (1, worker));
                wait_idle();
                assert_eq!(pool().q.lock().unwrap().workers, 1);
            },
        );
    }

    /// With no worker to be had, [`after`] refuses a delayed job instead of queueing it with
    /// nobody to wait out its delay (a deadline that would never fire); a zero delay is a
    /// [`run`], kept for a worker, and once threads are available again a delayed job starts
    /// the worker that runs both.
    #[test]
    fn a_delayed_job_with_no_worker_to_wait_for_it_is_refused() {
        alone(
            "a_delayed_job_with_no_worker_to_wait_for_it_is_refused",
            || {
                POOL_SPAWN_FAILS.set(true);
                freeze_pool();
                assert_eq!(pool().q.lock().unwrap().workers, 0);
                let (tx, rx) = std::sync::mpsc::channel();
                let t = tx.clone();
                let e = after(Duration::from_millis(20), move || t.send(0).unwrap()).unwrap_err();
                assert_eq!(e.raw_os_error(), Some(libc::EAGAIN));
                assert!(
                    pool().q.lock().unwrap().due.is_empty(),
                    "a delayed job was queued with no worker"
                );
                let t = tx.clone();
                after(Duration::ZERO, move || t.send(1).unwrap()).unwrap();
                assert!(rx.try_recv().is_err(), "the caller ran the job");
                POOL_SPAWN_FAILS.set(false);
                after(Duration::from_millis(20), move || tx.send(2).unwrap()).unwrap();
                assert_eq!(pool().q.lock().unwrap().workers, 1);
                assert_eq!(rx.recv().unwrap(), 1);
                advance_pool(Duration::from_millis(20));
                assert_eq!(rx.recv().unwrap(), 2);
            },
        );
    }

    /// [`wait_idle`] does not wait for the job of a lane whose thread failed to start (nothing
    /// would run it), and does wait for it once the lane's next job has started the thread.
    #[test]
    fn wait_idle_waits_for_a_refused_lane_job_once_its_thread_starts() {
        alone(
            "wait_idle_waits_for_a_refused_lane_job_once_its_thread_starts",
            || {
                static LANE: Lane = Lane::new();
                let ran = Arc::new(AtomicUsize::new(0));
                let job = || {
                    let r = ran.clone();
                    move || {
                        r.fetch_add(1, Ordering::SeqCst);
                    }
                };
                LANE_SPAWN_FAILS.set(true);
                LANE.run(job());
                LANE_SPAWN_FAILS.set(false);
                wait_idle();
                assert_eq!(ran.load(Ordering::SeqCst), 0);
                assert_eq!(LANE.lock().0.len(), 1, "the refused job was not kept");
                LANE.run(job());
                wait_idle();
                assert!(
                    LANE.lock().0.is_empty(),
                    "wait_idle returned with a queued lane job"
                );
                assert_eq!(ran.load(Ordering::SeqCst), 2);
                assert!(lanes_idle());
            },
        );
    }

    /// A lane drains on a thread of its own: with every [`long`] slot taken by jobs that wait
    /// for it, its job still runs instead of queueing behind them.
    #[test]
    fn a_lane_runs_while_every_long_slot_is_taken() {
        alone("a_lane_runs_while_every_long_slot_is_taken", || {
            static LANE: Lane = Lane::new();
            let (started_tx, started) = std::sync::mpsc::channel::<()>();
            let lane_ran = Arc::new((Mutex::new(false), Condvar::new()));
            for _ in 0..LONG_MAX {
                let (started_tx, lane_ran) = (started_tx.clone(), lane_ran.clone());
                long(move || {
                    started_tx.send(()).unwrap();
                    let mut ran = lane_ran.0.lock().unwrap();
                    while !*ran {
                        ran = lane_ran.1.wait(ran).unwrap();
                    }
                });
            }
            for _ in 0..LONG_MAX {
                started.recv().unwrap();
            }
            let l = lane_ran.clone();
            LANE.run(move || {
                *l.0.lock().unwrap() = true;
                l.1.notify_all();
            });
            assert!(
                elastic().q.lock().unwrap().outer.is_empty(),
                "the lane queued for a long slot"
            );
            let mut ran = lane_ran.0.lock().unwrap();
            while !*ran {
                ran = lane_ran.1.wait(ran).unwrap();
            }
            drop(ran);
            wait_idle();
        });
    }

    #[test]
    fn wait_idle_waits_for_work_handed_on() {
        alone("wait_idle_waits_for_work_handed_on", || {
            let (tx, rx) = std::sync::mpsc::channel::<&str>();
            let (go_tx, go) = std::sync::mpsc::channel::<()>();
            let done = Arc::new(AtomicUsize::new(0));
            let d = done.clone();
            long(move || {
                go.recv().unwrap();
                let d2 = d.clone();
                run(move || {
                    let ch = std::process::Command::new("true").spawn().unwrap();
                    reap_then(ch, move |_| {
                        d2.fetch_add(1, Ordering::SeqCst);
                    });
                });
                d.fetch_add(1, Ordering::SeqCst);
            });
            let waiter = std::thread::spawn(move || {
                wait_idle();
                tx.send("idle").unwrap();
            });
            go_tx.send(()).unwrap();
            assert_eq!(rx.recv().unwrap(), "idle");
            waiter.join().unwrap();
            assert_eq!(
                done.load(Ordering::SeqCst),
                2,
                "returned before the handed-on work"
            );
        });
    }

    #[test]
    fn reaper_leaves_no_zombies_and_no_thread_per_child() {
        alone("reaper_leaves_no_zombies_and_no_thread_per_child", || {
            pool();
            reaper();
            let t0 = threads();
            let mut pids = vec![];
            let mut stdins = vec![];
            for _ in 0..20 {
                // Runs until its stdin closes.
                let mut ch = std::process::Command::new("cat")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                stdins.push(ch.stdin.take().unwrap());
                pids.push(ch.id());
                reap(ch);
            }
            assert!(threads() <= t0, "one reaper thread for every child");
            assert_eq!(reaping(), 20);
            drop(stdins);
            // Returns once the reaper has waited for every child.
            wait_idle();
            assert_eq!(reaping(), 0);
            assert!(
                pids.iter().all(|&p| !zombie(p)),
                "a detached child stayed a zombie"
            );
        });
    }

    #[test]
    fn reap_then_reports_the_exit_status() {
        let (tx, rx) = std::sync::mpsc::channel();
        for code in [0, 3] {
            let ch = std::process::Command::new("sh")
                .args(["-c", &format!("exit {code}")])
                .spawn()
                .unwrap();
            let tx = tx.clone();
            reap_then(ch, move |st| tx.send((code, st.unwrap().code())).unwrap());
        }
        let mut got = [rx.recv().unwrap(), rx.recv().unwrap()];
        got.sort();
        assert_eq!(got, [(0, Some(0)), (3, Some(3))]);
    }

    /// A panic in an exit callback does not end the reaper thread: a child that exits after
    /// it is reaped and reported. The second child starts once the first callback runs, and
    /// the reaper thread's end is an event too (a thread-local the callback leaves there is
    /// dropped with the thread), so the test waits for whichever of the two happens.
    #[test]
    fn a_panicking_exit_callback_leaves_the_reaper_running() {
        struct Ended(std::sync::mpsc::Sender<&'static str>);
        impl Drop for Ended {
            fn drop(&mut self) {
                let _ = self.0.send("the reaper thread ended");
            }
        }
        thread_local! {
            static ENDED: std::cell::RefCell<Option<Ended>> =
                const { std::cell::RefCell::new(None) };
        }
        let exits = |code: i32| {
            std::process::Command::new("sh")
                .args(["-c", &format!("exit {code}")])
                .spawn()
                .unwrap()
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let (first, t) = (exits(0), tx.clone());
        let first_pid = first.id();
        reap_then(first, move |_| {
            ENDED.with(|e| *e.borrow_mut() = Some(Ended(t.clone())));
            t.send("the first callback runs").unwrap();
            panic!("an exit callback panics");
        });
        assert_eq!(rx.recv().unwrap(), "the first callback runs");
        let second = exits(5);
        let second_pid = second.id();
        reap_then(second, move |st| {
            assert_eq!(st.unwrap().code(), Some(5));
            tx.send("the second child was reported").unwrap();
        });
        assert_eq!(rx.recv().unwrap(), "the second child was reported");
        assert!(!zombie(first_pid) && !zombie(second_pid));
    }

    #[test]
    fn a_child_without_a_pidfd_is_still_reaped_and_reported() {
        let ch = std::process::Command::new("sh")
            .args(["-c", "exit 4"])
            .spawn()
            .unwrap();
        let pid = ch.id();
        let (tx, rx) = std::sync::mpsc::channel();
        let emfile = std::io::Error::from_raw_os_error(libc::EMFILE);
        watch_with(
            Waited::Spawned(ch),
            Some(Box::new(move |st| tx.send(st.unwrap().code()).unwrap())),
            Err(emfile),
        );
        assert_eq!(rx.recv().unwrap(), Some(4));
        assert!(!zombie(pid));
    }

    /// A child the previous image spawned and watched (a detached program, the child guard)
    /// is reaped by the image a reload execs into, whenever it exits: no zombie is left.
    #[test]
    fn children_inherited_across_exec_are_reaped() {
        const STAGE: &str = "WORKPOOL_EXEC";
        let me = |stage: &str| {
            let mut c = std::process::Command::new(std::env::current_exe().unwrap());
            c.args([
                "--exact",
                "tests::children_inherited_across_exec_are_reaped",
            ])
            .args(["--nocapture", "--test-threads=1"])
            .env(STAGE, stage);
            c
        };
        match std::env::var(STAGE).as_deref() {
            Ok("before") => {
                use std::os::fd::IntoRawFd;
                use std::os::unix::process::CommandExt;
                // Runs until its stdin reaches EOF, which the image after the exec decides.
                let mut kid = std::process::Command::new("sh")
                    .args(["-c", "read -r _; exit 3"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdin = kid.stdin.take().unwrap().into_raw_fd();
                // SAFETY: fcntl(2) on the pipe's write end, which this image owns; clearing
                // FD_CLOEXEC hands it to the image the exec below starts.
                assert_eq!(unsafe { libc::fcntl(stdin, libc::F_SETFD, 0) }, 0);
                let pid = kid.id();
                reap(kid);
                let e = me("after")
                    .env("WORKPOOL_KID", pid.to_string())
                    .env("WORKPOOL_KID_STDIN", stdin.to_string())
                    .exec();
                panic!("exec: {e}");
            }
            Ok("after") => {
                let kid: i32 = std::env::var("WORKPOOL_KID").unwrap().parse().unwrap();
                let stdin: i32 = std::env::var("WORKPOOL_KID_STDIN")
                    .unwrap()
                    .parse()
                    .unwrap();
                // SAFETY: the write end the image before the exec left open for this one;
                // nothing else here owns it.
                let stdin = unsafe { OwnedFd::from_raw_fd(stdin) };
                let inherited = children();
                assert!(inherited.contains(&kid), "{inherited:?}");
                let (tx, rx) = std::sync::mpsc::channel();
                for pid in inherited {
                    if pid == kid {
                        let tx = tx.clone();
                        adopt_then(pid, move |st| tx.send(st).unwrap());
                    } else {
                        adopt(pid);
                    }
                }
                // The child still runs: it exits once adopted, as its stdin closes.
                drop(stdin);
                let st = rx.recv().unwrap();
                assert_eq!(st.unwrap().code(), Some(3));
                assert!(!children().contains(&kid), "{kid} left a zombie");
            }
            _ => {
                let out = me("before").output().unwrap();
                let stdout = String::from_utf8_lossy(&out.stdout);
                assert!(
                    out.status.success() && stdout.contains("1 passed"),
                    "{stdout}{}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    }

    /// What `body` writes to stderr (the log). Call it [`alone`]: fd 2 is the process's.
    fn logged(body: impl FnOnce()) -> String {
        use std::io::{Read as _, Seek as _};
        // SAFETY: memfd_create(2) with a NUL-terminated name and no flags.
        let log = unsafe { libc::memfd_create(c"log".as_ptr(), 0) };
        assert!(log >= 0);
        // SAFETY: memfd_create just returned this descriptor, owned by nothing else.
        let mut log = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(log) });
        // SAFETY: dup(2) of this process's open stderr.
        let stderr = unsafe { libc::dup(2) };
        assert!(stderr >= 0);
        // SAFETY: dup2(2) between two open descriptors; `log` stays open for the call.
        assert_eq!(unsafe { libc::dup2(log.as_raw_fd(), 2) }, 2);
        body();
        // SAFETY: dup2(2) between two open descriptors, putting stderr back.
        assert_eq!(unsafe { libc::dup2(stderr, 2) }, 2);
        // SAFETY: close(2) of the copy made above, used by nothing else.
        unsafe { libc::close(stderr) };
        let mut text = String::new();
        log.rewind().unwrap();
        log.read_to_string(&mut text).unwrap();
        text
    }

    /// The kernel reaps this process's children itself from here on, so wait(2) for one fails
    /// with `ECHILD`: what a launcher that ignores SIGCHLD leaves to the program it starts.
    fn ignore_sigchld() {
        // SAFETY: signal(2) with a valid signal number and SIG_IGN.
        unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };
    }

    fn exits(code: i32) -> std::process::Child {
        std::process::Command::new("sh")
            .args(["-c", &format!("exit {code}")])
            .spawn()
            .unwrap()
    }

    /// A refused extra `long` thread is logged; its job waits for a running thread and runs.
    #[test]
    fn a_refused_extra_long_thread_is_logged() {
        alone("a_refused_extra_long_thread_is_logged", || {
            let e = small_pool(2, Duration::from_millis(200));
            freeze_long(e);
            let gate = Gate::default();
            let (tx, rx) = std::sync::mpsc::channel();
            let (g, t) = (gate.clone(), tx.clone());
            e.submit(Box::new(move || {
                t.send("first started").unwrap();
                g.pass();
                t.send("first done").unwrap();
            }));
            assert_eq!(rx.recv().unwrap(), "first started");
            let log = logged(|| {
                LONG_SPAWN_FAILS.set(true);
                e.submit(Box::new(move || tx.send("second ran").unwrap()));
                LONG_SPAWN_FAILS.set(false);
            });
            assert_eq!(e.q.lock().unwrap().threads, 1);
            assert!(
                log.contains("impulse-shell: cannot start another pool thread (")
                    && log.contains("the job waits for a running one"),
                "log: {log:?}"
            );
            gate.open();
            assert_eq!(rx.recv().unwrap(), "first done");
            assert_eq!(rx.recv().unwrap(), "second ran");
        });
    }

    /// A refused reaper thread is logged and the child waited for on a thread of its own; the
    /// next child starts the reaper, which then reaps without a thread per child.
    #[test]
    fn a_refused_reaper_thread_is_logged_and_tried_again() {
        alone("a_refused_reaper_thread_is_logged_and_tried_again", || {
            let (tx, rx) = std::sync::mpsc::channel();
            let report = move |st: Option<std::process::ExitStatus>| {
                let on = std::thread::current().name().map(str::to_owned);
                tx.send((on, st.and_then(|s| s.code()))).unwrap();
            };
            let fds = || std::fs::read_dir("/proc/self/fd").unwrap().count();
            let before = fds();
            let (first, r) = (exits(4), report.clone());
            let log = logged(|| {
                REAPER_SPAWN_FAILS.set(true);
                reap_then(first, r);
                REAPER_SPAWN_FAILS.set(false);
                assert_eq!(rx.recv().unwrap(), (Some("reap1".into()), Some(4)));
            });
            assert!(
                log.contains("impulse-shell: cannot start the reaper thread: "),
                "log: {log:?}"
            );
            assert!(REAPER.get().is_none());
            assert_eq!(fds(), before, "the refused reaper left a descriptor open");
            let log = logged(|| {
                reap_then(exits(5), report);
                assert_eq!(rx.recv().unwrap(), (Some("reaper".into()), Some(5)));
            });
            assert_eq!(log, "", "the second child was not watched by the reaper");
        });
    }

    /// A child refused both the reaper and a thread of its own is logged and kept: the next
    /// child brings it to the reaper, which reaps it and runs its exit callback.
    #[test]
    fn a_child_refused_its_own_thread_is_reaped_with_the_next_one() {
        alone(
            "a_child_refused_its_own_thread_is_reaped_with_the_next_one",
            || {
                let (tx, rx) = std::sync::mpsc::channel();
                let report = move |st: Option<std::process::ExitStatus>| {
                    let on = std::thread::current().name().map(str::to_owned);
                    tx.send((on, st.and_then(|s| s.code()))).unwrap();
                };
                let (first, r) = (exits(4), report.clone());
                let pid = first.id();
                let log = logged(|| {
                    REAPER_SPAWN_FAILS.set(true);
                    REAP1_SPAWN_FAILS.set(true);
                    reap_then(first, r);
                    REAPER_SPAWN_FAILS.set(false);
                    REAP1_SPAWN_FAILS.set(false);
                });
                let line =
                    format!("impulse-shell: cannot start a thread to wait for child {pid} (");
                assert!(log.contains(&line), "log: {log:?}");
                assert!(!log.contains("left unreaped"), "log: {log:?}");
                assert!(rx.try_recv().is_err(), "nothing waited for the child yet");
                reap_then(exits(5), report);
                let mut exits = [rx.recv().unwrap(), rx.recv().unwrap()];
                exits.sort();
                let reaper = || Some("reaper".to_owned());
                assert_eq!(exits, [(reaper(), Some(4)), (reaper(), Some(5))]);
                assert!(UNWATCHED.lock().unwrap().is_empty());
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
                assert!(stat.is_err(), "the first child is a zombie: {stat:?}");
            },
        );
    }

    /// A child refused both the reaper and a thread of its own, with no child after it, is
    /// reaped with the next job, whichever way the job is submitted. The job's submitter does
    /// not run the exit callback: the reaper thread does. While no thread is to be had, a job
    /// leaves the child where it is and logs nothing more.
    #[test]
    fn a_child_refused_its_own_thread_is_reaped_with_the_next_job() {
        alone(
            "a_child_refused_its_own_thread_is_reaped_with_the_next_job",
            || {
                static LANE: Lane = Lane::new();
                let shared = Arc::new(Lane::new());
                let job = || {};
                type Submit = Box<dyn Fn()>;
                let submits: [(&str, Submit); 5] = [
                    ("run", Box::new(move || run(job))),
                    ("long", Box::new(move || long(job))),
                    ("Lane::run", Box::new(move || LANE.run(job))),
                    (
                        "Lane::try_run_shared",
                        Box::new(move || shared.try_run_shared(job).unwrap()),
                    ),
                    (
                        "after",
                        Box::new(move || {
                            freeze_pool();
                            after(Duration::from_secs(1), job).unwrap();
                        }),
                    ),
                ];
                let (tx, rx) = std::sync::mpsc::channel();
                for (code, (name, submit)) in submits.iter().enumerate() {
                    let tx = tx.clone();
                    let report = move |st: Option<std::process::ExitStatus>| {
                        let on = std::thread::current().name().map(str::to_owned);
                        tx.send((on, st.and_then(|s| s.code()))).unwrap();
                    };
                    let child = exits(code as i32);
                    let pid = child.id();
                    // The first child finds no reaper thread; the later ones no descriptor.
                    let first = REAPER.get().is_none();
                    let log = logged(|| {
                        REAPER_SPAWN_FAILS.set(first);
                        REAP1_SPAWN_FAILS.set(true);
                        let emfile = std::io::Error::from_raw_os_error(libc::EMFILE);
                        watch_with(Waited::Spawned(child), Some(Box::new(report)), Err(emfile));
                        if first {
                            run(job);
                            wait_idle();
                        }
                        REAPER_SPAWN_FAILS.set(false);
                        REAP1_SPAWN_FAILS.set(false);
                    });
                    let line =
                        format!("impulse-shell: cannot start a thread to wait for child {pid} (");
                    assert_eq!(log.matches(&line).count(), 1, "{name}: log: {log:?}");
                    let reaper_refused = log.matches("cannot start the reaper thread").count();
                    assert_eq!(reaper_refused, first as usize, "{name}: log: {log:?}");
                    assert_eq!(UNWATCHED_LEN.load(Ordering::Relaxed), 1, "{name}");
                    assert!(
                        rx.try_recv().is_err(),
                        "{name}: nothing waited for the child"
                    );
                    let log = logged(|| {
                        submit();
                        wait_idle();
                    });
                    assert_eq!(log, "", "{name}");
                    let exit = rx.try_recv().ok();
                    let reaper = Some("reaper".to_owned());
                    assert_eq!(exit, Some((reaper, Some(code as i32))), "{name}");
                    assert_eq!(UNWATCHED_LEN.load(Ordering::Relaxed), 0, "{name}");
                    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
                    assert!(stat.is_err(), "{name}: the child is a zombie: {stat:?}");
                }
            },
        );
    }

    /// With SIGCHLD ignored wait(2) fails (`ECHILD`): the exit callback still runs, with no
    /// status, and the failure is logged. Both on the reaper thread and on a child's own.
    #[test]
    fn a_failed_wait_is_logged_and_still_runs_the_exit_callback() {
        alone(
            "a_failed_wait_is_logged_and_still_runs_the_exit_callback",
            || {
                ignore_sigchld();
                let (tx, rx) = std::sync::mpsc::channel();
                let report = move |st: Option<std::process::ExitStatus>| {
                    let on = std::thread::current().name().map(str::to_owned);
                    tx.send((on, st)).unwrap();
                };
                // The child exits at the end of its input, closed once the reaper watches it:
                // one that exited on its own could be reaped before its pidfd is opened.
                let mut ch = std::process::Command::new("cat")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                let (pid, input, r) = (ch.id(), ch.stdin.take(), report.clone());
                let log = logged(|| {
                    reap_then(ch, r);
                    drop(input);
                    assert_eq!(rx.recv().unwrap(), (Some("reaper".into()), None));
                });
                assert!(
                    log.contains(&format!("impulse-shell: cannot wait for child {pid}: ")),
                    "log: {log:?}"
                );
                let ch = exits(0);
                let pid = ch.id();
                let log = logged(|| {
                    let emfile = std::io::Error::from_raw_os_error(libc::EMFILE);
                    watch_with(Waited::Spawned(ch), Some(Box::new(report)), Err(emfile));
                    assert_eq!(rx.recv().unwrap(), (Some("reap1".into()), None));
                });
                assert!(
                    log.contains(&format!("impulse-shell: cannot wait for child {pid}: ")),
                    "log: {log:?}"
                );
            },
        );
    }

    #[test]
    fn require_pidfd_passes_on_this_kernel() {
        require_pidfd().unwrap();
    }
}
