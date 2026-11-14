//! Interrupts a JS evaluation that holds its thread too long, the way an embedder interrupts
//! QV4 with `QJSEngine::setInterrupted` from a watchdog thread.
//!
//! An evaluation is everything between a thread's outermost [`crate::eval::Interp::enter`] and
//! the drop of its frame guard: one binding, handler or callback together with whatever runs
//! synchronously inside it. Its start and end only bump an atomic sequence number. The timeout
//! counts the CPU time the evaluating thread consumed (its POSIX per-thread CPU clock), not wall
//! time: a runaway loop burns CPU, while a thread that waits for a core on a loaded machine does
//! not, so load never interrupts a long legitimate evaluation.
//!
//! Each thread owns a one-shot POSIX timer on the monotonic clock (`timer_create(2)` with
//! `SIGEV_THREAD_ID`) whose expiry signal goes to one shared watchdog thread, blocked in
//! `sigwaitinfo(2)` until then: the watchdog does not sample. An evaluation that starts on a
//! thread with no armed timer arms it for the timeout. When it expires and the evaluation
//! that armed it still runs, the watchdog reads the thread's CPU clock: an evaluation out of
//! its time is marked interrupted, one that is not gets the timer armed again for the rest
//! of it. When a later evaluation runs instead, the timer is re-armed for it, so an
//! evaluation is interrupted after at least one timeout and at most two of its own CPU time,
//! and a thread running only short evaluations costs one expiry per timeout of wall time,
//! not a system call per evaluation.
//! JS checks the mark at loop back-edges and calls and throws `Error: Interrupted`, again at
//! every later check of the same evaluation, so a `catch` cannot keep the loop alive (V4 keeps
//! throwing while `isInterrupted` is set).
//!
//! The timer is not one on the thread's CPU clock, which would expire exactly when the time
//! is consumed: whenever a thread's CPU-clock timer expires the kernel looks at the process's
//! `RLIMIT_RTTIME` and, where that is 0, the thread's realtime run time (0) has reached it:
//! the process is killed (SIGKILL at a hard limit of 0, else SIGXCPU), with no line anywhere
//! (`check_thread_timers` in the kernel's posix-cpu-timers.c). A library on any thread, or
//! `prlimit` from outside, can lower the limit to 0 at any moment, also after a timer was
//! armed (`libpipewire-module-rt` does with a portal that has no RTKit behind it).
//!
//! A thread can instead (or also) give each evaluation a step budget: the number of those
//! checks it may pass. Exhausting it interrupts the evaluation the same way, at a point that
//! depends only on the code that ran, never on the machine's load (the AOT check uses this).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

/// How much CPU time one evaluation may consume: the time after which compositors consider a
/// client hung (mutter's `check-alive-timeout`).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// The shortest time the timer is armed for again: an evaluation a moment short of
/// its CPU time, on a thread that waits for a core, does not keep the watchdog spinning.
const SHORTEST_REST: Duration = Duration::from_millis(1);

/// The watchdog state of one thread.
pub struct Slot {
    /// Odd while an evaluation runs on the owning thread: bumped at its start and at its end.
    seq: AtomicU64,
    /// `seq` of the evaluation to interrupt, 0 for none.
    interrupt: AtomicU64,
    /// Timeout in ms, 0 when off.
    timeout_ms: AtomicU64,
    /// The owning thread's monotonic timer; `None` when it could not be created, which
    /// leaves only the step budget.
    timer: Option<Timer>,
    /// The owning thread's CPU clock; `None` where the platform has none: the timeout then
    /// counts wall time.
    clock: Option<CpuClock>,
    /// The thread's CPU time in ns at which the evaluation the timer was armed for is out of
    /// time, 0 without a `clock`.
    until: AtomicU64,
    /// `seq` of the evaluation the timer was armed for, 0 while it is disarmed.
    armed: AtomicU64,
    /// Serialises arming the timer between the owning thread and the watchdog.
    arming: Mutex<()>,
    /// The owning thread's kernel thread id, for the log.
    tid: libc::pid_t,
    /// The kernel refused to arm the timer and has not armed it since: the log has the line.
    refused: AtomicBool,
    /// Checks one evaluation may pass, 0 when unlimited. Only the owning thread touches this
    /// and the two below.
    budget: AtomicU64,
    /// Checks the running evaluation has passed.
    steps: AtomicU64,
    /// Evaluations that exhausted the budget.
    exhausted: AtomicU64,
}

impl Slot {
    /// Arms the timer for the running evaluation unless it is armed already. `_lock` is
    /// `arming`, held.
    fn arm_locked(&self, _lock: &std::sync::MutexGuard<'_, ()>) {
        let Some(t) = &self.timer else {
            return;
        };
        let ms = self.timeout_ms.load(Ordering::SeqCst);
        let q = self.seq.load(Ordering::SeqCst);
        if ms == 0 || q.is_multiple_of(2) || self.armed.load(Ordering::SeqCst) != 0 {
            return;
        }
        let d = Duration::from_millis(ms);
        let now = self.clock.as_ref().and_then(CpuClock::now);
        let until = now.map_or(0, |now| (now + d).as_nanos() as u64);
        self.until.store(until, Ordering::SeqCst);
        if self.set(t, d, "its scripts run with no time limit") {
            self.armed.store(q, Ordering::SeqCst);
        }
    }

    /// Arms the timer for `d`; false when the kernel refused, which the log says with what
    /// follows from it (`then`), once until the kernel arms the timer again.
    fn set(&self, t: &Timer, d: Duration, then: &str) -> bool {
        match t.set(d) {
            Ok(()) => {
                if self.refused.load(Ordering::Relaxed) {
                    self.refused.store(false, Ordering::Relaxed);
                }
                true
            }
            Err(e) => {
                if !self.refused.swap(true, Ordering::Relaxed) {
                    elog::eprintln!(
                        "rusty-impulse: js watchdog: cannot arm the timer of thread {}, {then}: {e}",
                        self.tid
                    );
                }
                false
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.arming.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The watchdog thread's part, after an expiry signal: interrupts the evaluation the timer
    /// was armed for if it still runs, else re-arms the timer for the one running now.
    fn expired(&self) {
        let Some(t) = &self.timer else {
            return;
        };
        let g = self.lock();
        let a = self.armed.load(Ordering::SeqCst);
        let until = self.until.load(Ordering::SeqCst);
        // Disarmed, or re-armed since this signal was queued.
        if a == 0 || !t.expired() {
            return;
        }
        if self.seq.load(Ordering::SeqCst) == a {
            // The timer counted wall time: the evaluation goes on for the CPU time it has
            // left.
            let out_of_time = Duration::from_nanos(until);
            let left = self
                .clock
                .as_ref()
                .and_then(CpuClock::now)
                .and_then(|now| out_of_time.checked_sub(now))
                .filter(|left| until != 0 && !left.is_zero());
            if left.is_some_and(|left| {
                self.set(
                    t,
                    left.max(SHORTEST_REST),
                    "its script is interrupted before its time",
                )
            }) {
                return;
            }
            self.interrupt.store(a, Ordering::Relaxed);
        }
        // Cleared before `seq` is read again: an evaluation starting meanwhile either sees 0
        // and arms the timer itself, or is seen below.
        self.armed.store(0, Ordering::SeqCst);
        if self.seq.load(Ordering::SeqCst) != a {
            self.arm_locked(&g);
        }
    }

    /// The running evaluation was interrupted.
    #[inline]
    pub fn interrupted(&self) -> bool {
        let i = self.interrupt.load(Ordering::Relaxed);
        if i != 0 && self.current(i) {
            return true;
        }
        let b = self.budget.load(Ordering::Relaxed);
        b != 0 && self.spend(b)
    }

    #[inline]
    fn spend(&self, budget: u64) -> bool {
        let n = self.steps.load(Ordering::Relaxed) + 1;
        self.steps.store(n, Ordering::Relaxed);
        n > budget && self.exhaust()
    }

    #[cold]
    fn exhaust(&self) -> bool {
        let q = self.seq.load(Ordering::Relaxed);
        if q.is_multiple_of(2) {
            return false;
        }
        self.exhausted.fetch_add(1, Ordering::Relaxed);
        self.interrupt.store(q, Ordering::Relaxed);
        true
    }

    #[cold]
    fn current(&self, i: u64) -> bool {
        if i == self.seq.load(Ordering::Relaxed) {
            return true;
        }
        // A mark left from an evaluation that has ended since.
        let _ = self
            .interrupt
            .compare_exchange(i, 0, Ordering::Relaxed, Ordering::Relaxed);
        false
    }
}

/// A thread's CPU-time clock (`pthread_getcpuclockid`), readable from any thread while the
/// thread lives.
struct CpuClock(libc::clockid_t);

impl CpuClock {
    fn of_current_thread() -> Option<Self> {
        let mut id: libc::clockid_t = 0;
        // SAFETY: no preconditions; it names the calling thread.
        let me = unsafe { libc::pthread_self() };
        // SAFETY: `me` is the calling, live thread; `id` is a valid out pointer.
        let rc = unsafe { libc::pthread_getcpuclockid(me, &mut id) };
        (rc == 0).then_some(CpuClock(id))
    }

    fn now(&self) -> Option<Duration> {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `ts` is a valid out pointer; an invalid clock only returns an error.
        let rc = unsafe { libc::clock_gettime(self.0, &mut ts) };
        (rc == 0).then(|| Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32))
    }
}

/// A one-shot POSIX timer whose expiry sends the watchdog's signal to the watchdog thread.
struct Timer(libc::timer_t);

// SAFETY: a `timer_t` is a process-wide kernel timer id, usable from any thread; `Timer` owns it
// and deletes it once.
unsafe impl Send for Timer {}
// SAFETY: as above; timer_settime(2) and timer_gettime(2) may race, the kernel serialises them.
unsafe impl Sync for Timer {}

impl Timer {
    fn new(clock: libc::clockid_t, wd: &Watchdog) -> Option<Timer> {
        // SAFETY: an all-zero sigevent is valid; the fields set below are the ones
        // SIGEV_THREAD_ID reads (timer_create(2)).
        let mut ev: libc::sigevent = unsafe { std::mem::zeroed() };
        ev.sigev_notify = libc::SIGEV_THREAD_ID;
        ev.sigev_signo = wd.signal;
        ev.sigev_notify_thread_id = wd.tid;
        let mut id: libc::timer_t = std::ptr::null_mut();
        // SAFETY: `ev` and `id` are valid for the call; `wd.tid` is the live watchdog thread,
        // which blocks `wd.signal`.
        let rc = unsafe { libc::timer_create(clock, &mut ev, &mut id) };
        // Not `then_some`: a `Timer` made of a refused id would be dropped here, and its
        // timer_delete(2) would put its own error in place of the refusal's.
        (rc == 0).then(|| Timer(id))
    }

    /// Arms the timer to expire once after `d` of its clock, or disarms it with a `d` of zero.
    fn set(&self, d: Duration) -> std::io::Result<()> {
        let v = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: libc::timespec {
                tv_sec: d.as_secs() as libc::time_t,
                tv_nsec: d.subsec_nanos() as libc::c_long,
            },
        };
        // SAFETY: `self.0` is a live timer; `v` is valid and the old value is not asked for.
        let rc = unsafe { libc::timer_settime(self.0, 0, &v, std::ptr::null_mut()) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    /// The timer is not running: it expired or was never armed.
    fn expired(&self) -> bool {
        // SAFETY: an all-zero itimerspec is valid; timer_gettime(2) fills it.
        let mut v: libc::itimerspec = unsafe { std::mem::zeroed() };
        // SAFETY: `self.0` is a live timer and `v` a valid out pointer.
        let rc = unsafe { libc::timer_gettime(self.0, &mut v) };
        rc == 0 && v.it_value.tv_sec == 0 && v.it_value.tv_nsec == 0
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a live timer this value owns; nothing uses it afterwards.
        unsafe { libc::timer_delete(self.0) };
    }
}

struct Watchdog {
    slots: Arc<Mutex<Vec<Weak<Slot>>>>,
    /// The signal the timers send, blocked on the watchdog thread and taken by `sigwaitinfo`.
    signal: libc::c_int,
    /// The watchdog thread's kernel thread id, the timers' `sigev_notify_thread_id`.
    tid: libc::pid_t,
}

/// `None` where the system refused the watchdog thread.
static WATCHDOG: OnceLock<Option<Watchdog>> = OnceLock::new();

thread_local! {
    static SLOT: Arc<Slot> = register();
}

/// The shared watchdog, started by the first thread that evaluates JS. `None` where the system
/// refused its thread (`EAGAIN`): no timer can be made then, which leaves only the step budget.
fn watchdog() -> Option<&'static Watchdog> {
    let wd = WATCHDOG.get_or_init(|| {
        let signal = libc::SIGRTMIN();
        let slots: Arc<Mutex<Vec<Weak<Slot>>>> = Arc::default();
        let watched = slots.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("js-watchdog".into())
            .spawn(move || {
                let set = signal_set(signal);
                // SAFETY: `set` is a valid sigset; only the calling thread's mask changes.
                unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
                // Sent once the signal is blocked: no timer can target this thread earlier.
                // SAFETY: gettid(2) has no preconditions.
                let _ = tx.send(unsafe { libc::gettid() });
                #[cfg(test)]
                tests::before_first_wait();
                run(&set, &watched);
            });
        if let Err(e) = spawned {
            elog::eprintln!("rusty-impulse: js watchdog: cannot start its thread: {e}");
            return None;
        }
        let tid = rx.recv().ok()?;
        Some(Watchdog { slots, signal, tid })
    });
    wd.as_ref()
}

fn signal_set(signal: libc::c_int) -> libc::sigset_t {
    // SAFETY: an all-zero sigset_t is a valid value for sigemptyset(3) to initialise.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: `set` is a valid, writable sigset_t.
    unsafe { libc::sigemptyset(&mut set) };
    // SAFETY: `set` is initialised and `signal` is a valid real-time signal number.
    unsafe { libc::sigaddset(&mut set, signal) };
    set
}

fn register() -> Arc<Slot> {
    let wd = watchdog();
    // SAFETY: gettid(2) has no preconditions.
    let tid = unsafe { libc::gettid() };
    let timer = wd.and_then(|wd| {
        let t = Timer::new(libc::CLOCK_MONOTONIC, wd);
        if t.is_none() {
            elog::eprintln!(
                "rusty-impulse: js watchdog: cannot make the timer of thread {tid}, its scripts run \
                 with no time limit: {}",
                std::io::Error::last_os_error()
            );
        }
        t
    });
    let s = Arc::new(Slot {
        seq: AtomicU64::new(0),
        interrupt: AtomicU64::new(0),
        timeout_ms: AtomicU64::new(DEFAULT_TIMEOUT.as_millis() as u64),
        timer,
        clock: CpuClock::of_current_thread(),
        until: AtomicU64::new(0),
        armed: AtomicU64::new(0),
        arming: Mutex::new(()),
        tid,
        refused: AtomicBool::new(false),
        budget: AtomicU64::new(0),
        steps: AtomicU64::new(0),
        exhausted: AtomicU64::new(0),
    });
    let Some(wd) = wd else {
        return s;
    };
    wd.slots
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Arc::downgrade(&s));
    s
}

/// This thread's slot, for the interpreter's checks.
pub fn slot() -> Arc<Slot> {
    SLOT.with(Arc::clone)
}

/// Sets how long one evaluation on this thread may run before it is interrupted (`None`: never).
pub fn set_timeout(t: Option<Duration>) {
    let ms = t.map_or(0, |t| (t.as_millis() as u64).max(1));
    SLOT.with(|s| {
        let g = s.lock();
        s.timeout_ms.store(ms, Ordering::SeqCst);
        if let Some(t) = &s.timer
            && s.armed.swap(0, Ordering::SeqCst) != 0
        {
            // A refusal leaves at most an expiry that finds the timer disarmed.
            let _ = t.set(Duration::ZERO);
        }
        // An evaluation running now gets the new timeout.
        s.arm_locked(&g);
    });
}

/// Sets how many interrupt checks (loop back-edges and calls) one evaluation on this thread may
/// pass before it is interrupted (`None`: unlimited).
pub fn set_step_budget(steps: Option<u64>) {
    SLOT.with(|s| s.budget.store(steps.unwrap_or(0), Ordering::Relaxed));
}

/// Evaluations on this thread that exhausted their step budget since the last call.
pub fn take_exhausted() -> u64 {
    SLOT.with(|s| s.exhausted.swap(0, Ordering::Relaxed))
}

/// The outermost JS frame of this thread was entered.
pub(crate) fn begin() {
    SLOT.with(|s| {
        let q = s.seq.load(Ordering::Relaxed) + 1;
        s.steps.store(0, Ordering::Relaxed);
        s.seq.store(q, Ordering::SeqCst);
        // Pairs with the watchdog's clearing of `armed` before it reads `seq`: one of the two
        // sides sees the other, so an evaluation never starts unwatched.
        if s.armed.load(Ordering::SeqCst) == 0 && s.timeout_ms.load(Ordering::Relaxed) != 0 {
            s.arm_locked(&s.lock());
        }
    });
}

/// The outermost JS frame of this thread was left.
pub(crate) fn end() {
    SLOT.with(|s| {
        let q = s.seq.load(Ordering::Relaxed) + 1;
        s.seq.store(q, Ordering::Release);
    });
}

/// Times the watchdog thread's wait ended, for whatever reason.
#[cfg(test)]
static WAKEUPS: AtomicU64 = AtomicU64::new(0);

/// The watchdog thread: sleeps in `sigwaitinfo` until a timer expires, then handles every
/// expired timer (expiries of several timers may arrive as one wakeup).
fn run(set: &libc::sigset_t, slots: &Mutex<Vec<Weak<Slot>>>) {
    loop {
        // SAFETY: `set` is a valid sigset of a signal this thread blocks; the info is not
        // asked for.
        let woken = unsafe { libc::sigwaitinfo(set, std::ptr::null_mut()) };
        #[cfg(test)]
        WAKEUPS.fetch_add(1, Ordering::SeqCst);
        if woken < 0 {
            continue;
        }
        let live: Vec<Arc<Slot>> = {
            let mut slots = slots.lock().unwrap_or_else(|e| e.into_inner());
            slots.retain(|w| w.strong_count() > 0);
            slots.iter().filter_map(Weak::upgrade).collect()
        };
        for s in live {
            s.expired();
        }
    }
}

/// CPU time the calling thread has consumed (its POSIX per-thread CPU clock), the time an
/// evaluation's timeout counts.
#[cfg(test)]
pub(crate) fn thread_cpu_time() -> Duration {
    CpuClock::of_current_thread()
        .and_then(|c| c.now())
        .expect("the thread's CPU clock")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blocks the calling thread (off the CPU) until another thread has consumed `cpu` of its
    /// own CPU time: wall time passes for this thread while its CPU clock stands still.
    fn off_cpu_while_another_thread_burns(cpu: Duration) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let t0 = thread_cpu_time();
            while thread_cpu_time() - t0 < cpu {
                std::hint::spin_loop();
            }
            let _ = tx.send(());
        });
        rx.recv().unwrap();
    }

    /// The watchdog thread's way to its first wait, which a test may close.
    struct FirstWait {
        closed: bool,
        /// The watchdog thread has gone through, to its first wait.
        passed: bool,
    }

    static FIRST_WAIT: Mutex<FirstWait> = Mutex::new(FirstWait {
        closed: false,
        passed: false,
    });
    static FIRST_WAIT_CHANGED: std::sync::Condvar = std::sync::Condvar::new();

    /// The watchdog thread calls this once, before its first wait: it stays here while a
    /// test keeps the way closed.
    pub(super) fn before_first_wait() {
        let mut way = FIRST_WAIT.lock().unwrap();
        while way.closed {
            way = FIRST_WAIT_CHANGED.wait(way).unwrap();
        }
        way.passed = true;
        FIRST_WAIT_CHANGED.notify_all();
    }

    /// The timeout counts the evaluating thread's CPU time: time it spends off the CPU (as
    /// when it waits for a core on a loaded machine) does not bring the interrupt closer.
    #[test]
    fn timeout_counts_the_threads_cpu_time_not_wall_time() {
        std::thread::spawn(|| {
            set_timeout(Some(Duration::from_millis(100)));
            let s = slot();
            // Read before the timer is armed: the timeout counts all of the evaluation's CPU
            // time, the little it spends before going off the CPU too.
            let t0 = thread_cpu_time();
            begin();
            off_cpu_while_another_thread_burns(Duration::from_millis(400));
            assert!(!s.interrupted(), "interrupted while off the CPU");
            while !s.interrupted() {
                std::hint::spin_loop();
            }
            let spent = thread_cpu_time() - t0;
            end();
            assert!(!s.interrupted());
            assert!(
                spent >= Duration::from_millis(100),
                "interrupted after {spent:?}"
            );
        })
        .join()
        .unwrap();
    }

    /// Nothing wakes the watchdog before the timer armed for an evaluation expires: it waits
    /// for the timer instead of sampling the clock. The process has one watchdog
    /// thread, which other tests' expiring timers wake too, so under `cargo test` (tests as
    /// threads of one process) the count is taken in a process of its own.
    #[test]
    fn the_watchdog_sleeps_while_an_evaluation_is_within_its_time() {
        const ALONE: &str = "QML_WATCHDOG_TEST_ALONE";
        if std::env::var_os(ALONE).is_none() {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "--test-threads=1", "--nocapture"])
                .arg("watchdog::tests::the_watchdog_sleeps_while_an_evaluation_is_within_its_time")
                .env(ALONE, "1")
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success() && text.contains("1 passed"),
                "{text}{}",
                String::from_utf8_lossy(&out.stderr)
            );
            return;
        }
        std::thread::spawn(|| {
            // The watchdog thread, which `set_timeout` starts, is kept from its first wait
            // until the count has begun: the order in which a count of the thread's context
            // switches read its first going to sleep as a wakeup. It came by chance on a
            // loaded machine; here every run has it.
            FIRST_WAIT.lock().unwrap().closed = true;
            // A timeout no run of this test reaches.
            set_timeout(Some(Duration::from_secs(3600)));
            let s = slot();
            begin();
            // Counted where the watchdog's wait returns, not as the thread's context switches.
            let before = WAKEUPS.load(Ordering::SeqCst);
            let mut way = FIRST_WAIT.lock().unwrap();
            assert!(
                !way.passed,
                "the watchdog reached its first wait before the count"
            );
            way.closed = false;
            FIRST_WAIT_CHANGED.notify_all();
            while !way.passed {
                way = FIRST_WAIT_CHANGED.wait(way).unwrap();
            }
            drop(way);
            off_cpu_while_another_thread_burns(Duration::from_millis(400));
            let woken = WAKEUPS.load(Ordering::SeqCst) - before;
            assert_eq!(woken, 0, "the watchdog woke {woken} times");
            // The count does see a wakeup: the timer's expiry is one, and one more follows
            // when the thread had consumed less CPU time than the wall time that passed.
            set_timeout(Some(Duration::from_millis(100)));
            while !s.interrupted() {
                std::hint::spin_loop();
            }
            end();
            assert_ne!(WAKEUPS.load(Ordering::SeqCst) - before, 0);
        })
        .join()
        .unwrap();
    }

    /// Short evaluations that together outlast the timeout are not interrupted, the next one
    /// that alone runs out of time is, and changing the timeout mid-evaluation applies to it.
    #[test]
    fn only_an_evaluation_that_alone_runs_out_of_time_is_interrupted() {
        std::thread::spawn(|| {
            set_timeout(Some(Duration::from_millis(50)));
            let s = slot();
            let t0 = thread_cpu_time();
            while thread_cpu_time() - t0 < Duration::from_millis(300) {
                begin();
                let t = thread_cpu_time();
                while thread_cpu_time() - t < Duration::from_millis(5) {}
                assert!(!s.interrupted(), "a 5 ms evaluation was interrupted");
                end();
            }
            begin();
            set_timeout(None);
            let t = thread_cpu_time();
            while thread_cpu_time() - t < Duration::from_millis(300) {}
            assert!(!s.interrupted(), "interrupted with the timeout off");
            set_timeout(Some(Duration::from_millis(50)));
            while !s.interrupted() {
                std::hint::spin_loop();
            }
            end();
        })
        .join()
        .unwrap();
    }

    /// A realtime budget (`RLIMIT_RTTIME`) of 0, which libpipewire's realtime module left the
    /// shell with, does not make the watchdog's timer the end of the process: the kernel sent
    /// SIGKILL when a thread's CPU-clock timer expired, 5 s of the main thread's CPU time
    /// after the shell's start. The timeout counts CPU time and interrupts. In a process of
    /// its own: the limit cannot be raised again.
    #[test]
    fn a_realtime_budget_of_zero_leaves_the_process_alive() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_realtime_budget_of_zero_leaves_the_process_alive"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        leave_no(libc::RLIMIT_RTTIME);
        std::thread::spawn(|| {
            set_timeout(Some(Duration::from_millis(50)));
            let s = slot();
            // Short evaluations that together outlast the timeout: a CPU-clock timer armed by
            // the first one expired among them.
            let t0 = thread_cpu_time();
            while thread_cpu_time() - t0 < Duration::from_millis(300) {
                begin();
                let t = thread_cpu_time();
                while thread_cpu_time() - t < Duration::from_millis(5) {}
                assert!(!s.interrupted(), "a 5 ms evaluation was interrupted");
                end();
            }
            let t0 = thread_cpu_time();
            begin();
            off_cpu_while_another_thread_burns(Duration::from_millis(200));
            assert!(!s.interrupted(), "interrupted while off the CPU");
            while !s.interrupted() {
                std::hint::spin_loop();
            }
            let spent = thread_cpu_time() - t0;
            end();
            assert!(
                spent >= Duration::from_millis(50),
                "interrupted after {spent:?}"
            );
        })
        .join()
        .unwrap();
    }

    /// The process's limits fall to nothing: no realtime time, no further timer.
    fn leave_no(resource: libc::__rlimit_resource_t) {
        let none = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: setrlimit(2) reads the rlimit it is given.
        let rc = unsafe { libc::setrlimit(resource, &none) };
        assert_eq!(rc, 0, "{}", std::io::Error::last_os_error());
    }

    /// The realtime budget falls to 0 while an evaluation runs, after its timer was armed (a
    /// library on another thread, or `prlimit` from outside, lowers it at any moment): the
    /// process lives and the evaluation is interrupted once it is out of time. With a
    /// CPU-clock timer armed the kernel sent SIGKILL at its expiry. In a process of its own:
    /// the limit cannot be raised again.
    #[test]
    fn a_realtime_budget_that_falls_to_zero_under_an_evaluation_leaves_the_process_alive() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_realtime_budget_that_falls_to_zero_under_an_evaluation_leaves_the_process_alive"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        std::thread::spawn(|| {
            set_timeout(Some(Duration::from_millis(50)));
            let s = slot();
            let t0 = thread_cpu_time();
            begin();
            leave_no(libc::RLIMIT_RTTIME);
            while !s.interrupted() {
                std::hint::spin_loop();
            }
            let spent = thread_cpu_time() - t0;
            end();
            assert!(
                spent >= Duration::from_millis(50),
                "interrupted after {spent:?}"
            );
        })
        .join()
        .unwrap();
    }

    /// A thread whose slot has its timer is watched whatever the limits become later: with a
    /// realtime budget of 0 and no further timer to be had (`RLIMIT_SIGPENDING` counts
    /// timers), an evaluation that does not return is still interrupted. A second timer,
    /// made when the budget was first seen at 0, could be refused, which left the
    /// evaluations unwatched with no line in the log. In a process of its own: the limits
    /// cannot be raised again.
    #[test]
    fn an_evaluation_is_watched_when_no_further_timer_can_be_made() {
        const TEST: &str = concat!(
            module_path!(),
            "::an_evaluation_is_watched_when_no_further_timer_can_be_made"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        std::thread::spawn(|| {
            set_timeout(Some(Duration::from_millis(50)));
            let s = slot();
            assert!(s.timer.is_some());
            leave_no(libc::RLIMIT_SIGPENDING);
            leave_no(libc::RLIMIT_RTTIME);
            begin();
            assert_ne!(
                s.armed.load(Ordering::SeqCst),
                0,
                "the evaluation runs unwatched"
            );
            while !s.interrupted() {
                std::hint::spin_loop();
            }
            end();
        })
        .join()
        .unwrap();
    }

    /// The system refuses a thread's timer (`RLIMIT_SIGPENDING` counts timers): the thread
    /// gets a slot without one and its evaluations run unwatched by time, which the log says
    /// with the reason. In a process of its own: the limit cannot be raised again, and the
    /// test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_timer_is_in_the_log() {
        const TEST: &str = concat!(module_path!(), "::a_refused_timer_is_in_the_log");
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        leave_no(libc::RLIMIT_SIGPENDING);
        let (s, log) = testenv::stderr_with(slot);
        // SAFETY: gettid(2) has no preconditions.
        let tid = unsafe { libc::gettid() };
        let line = format!(
            "rusty-impulse: js watchdog: cannot make the timer of thread {tid}, its scripts run with no \
             time limit: Resource temporarily unavailable (os error 11)"
        );
        assert!(log.lines().any(|l| l == line), "stderr: {log:?}");
        assert!(s.timer.is_none());
        begin();
        assert_eq!(s.armed.load(Ordering::SeqCst), 0);
        end();
    }

    /// The kernel refuses to arm a thread's timer (the timer is deleted under its slot here:
    /// timer_settime(2) then fails with `EINVAL`): the thread's evaluations run unwatched by
    /// time, which the log says with the reason, once and not at every evaluation. In a
    /// process of its own: the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_arming_is_in_the_log() {
        const TEST: &str = concat!(module_path!(), "::a_refused_arming_is_in_the_log");
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        set_timeout(Some(Duration::from_millis(50)));
        let s = slot();
        let t = s.timer.as_ref().expect("the thread has its timer");
        // SAFETY: `t.0` is a live timer; nothing but timer_settime(2) and the slot's own
        // timer_delete(2) is given the id afterwards, and both only fail on it.
        assert_eq!(unsafe { libc::timer_delete(t.0) }, 0);
        let ((), log) = testenv::stderr_with(|| {
            for _ in 0..2 {
                begin();
                assert_eq!(s.armed.load(Ordering::SeqCst), 0);
                end();
            }
        });
        // SAFETY: gettid(2) has no preconditions.
        let tid = unsafe { libc::gettid() };
        let line = format!(
            "rusty-impulse: js watchdog: cannot arm the timer of thread {tid}, its scripts run with no \
             time limit: Invalid argument (os error 22)"
        );
        assert_eq!(
            log.lines().filter(|l| *l == line).count(),
            1,
            "stderr: {log:?}"
        );
    }

    /// The system refuses the watchdog thread (the limit on threads is reached when the first
    /// thread evaluates JS): that thread gets a slot without a timer, instead of a panic, and
    /// its evaluations run unwatched by time; the step budget, which needs no thread, still
    /// ends one; the refusal is in the log. In a process of its own: the watchdog is started
    /// once per process, and the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_watchdog_thread_leaves_the_step_budget() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_watchdog_thread_leaves_the_step_budget"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        testenv::refuse_new_threads();
        let (s, log) = testenv::stderr_with(|| {
            set_timeout(Some(Duration::from_millis(1)));
            slot()
        });
        assert!(
            log.contains("rusty-impulse: js watchdog: cannot start its thread:"),
            "stderr: {log:?}"
        );
        assert!(s.timer.is_none());
        begin();
        end();
        set_step_budget(Some(10_000));
        let mut rt = crate::runtime::Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem {\n property int n: 0\n onNChanged: { for (;;) {} }\n}\n",
            )
            .unwrap();
        rt.settle();
        rt.set(root, "n", crate::value::Value::Num(1.0));
        let errs = rt.errors();
        assert!(
            errs.iter().any(|e| e.ends_with("onNChanged: Interrupted")),
            "{errs:?}"
        );
        assert_eq!(take_exhausted(), 1);
    }
}
