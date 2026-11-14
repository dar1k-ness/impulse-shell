//! Parallel image work runs on threads that exist only while there is work. rayon's global
//! pool would keep one parked thread per CPU, each with its stack and malloc arena, for the rest
//! of the shell's life after the first wallpaper analysis or thumbnail run.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long the pool outlives its last job, so a burst (a wallpaper's least-busy-region,
/// regions and a grid of thumbnails) shares one set of threads.
pub const IDLE: Duration = Duration::from_secs(3);

#[derive(Default)]
struct Shared {
    pool: Option<Arc<rayon::ThreadPool>>,
    users: usize,
    last: Option<Instant>,
    reaper: bool,
    /// The time while [`test_clock`] holds the clock; it then moves only by `advance`.
    frozen: Option<Instant>,
    /// `advance` calls so far, and the last one the reaper has looked at.
    advanced: u64,
    seen: u64,
}

impl Shared {
    fn now(&self) -> Instant {
        self.frozen.unwrap_or_else(Instant::now)
    }
}

static S: Mutex<Shared> = Mutex::new(Shared {
    pool: None,
    users: 0,
    last: None,
    reaper: false,
    frozen: None,
    advanced: 0,
    seen: 0,
});
static CV: Condvar = Condvar::new();

struct Use;

impl Drop for Use {
    fn drop(&mut self) {
        let mut s = S.lock().unwrap();
        s.users -= 1;
        s.last = Some(s.now());
        if s.users == 0 && !s.reaper {
            s.reaper = std::thread::Builder::new()
                .name("img-idle".into())
                .spawn(reap)
                .is_ok();
        }
        CV.notify_all();
    }
}

/// Drops the pool once it has had no user for [`IDLE`]; its threads then exit.
fn reap() {
    let mut s = S.lock().unwrap();
    loop {
        s.seen = s.advanced;
        CV.notify_all();
        let now = s.now();
        match s.last.map(|t| now.saturating_duration_since(t)) {
            _ if s.users > 0 => s = CV.wait(s).unwrap(),
            Some(e) if e < IDLE && s.frozen.is_some() => s = CV.wait(s).unwrap(),
            Some(e) if e < IDLE => s = CV.wait_timeout(s, IDLE - e).unwrap().0,
            _ => break,
        }
    }
    let pool = s.pool.take();
    s.reaper = false;
    CV.notify_all();
    drop(s);
    drop(pool);
}

/// Whether the pool's threads are running (started and not yet wound down).
#[doc(hidden)]
pub fn alive() -> bool {
    S.lock().unwrap().pool.is_some()
}

/// A manual clock for the pool's idle timeout, for tests of when it winds down.
#[doc(hidden)]
pub mod test_clock {
    use super::{CV, S};
    use std::time::{Duration, Instant};

    /// From now on the pool's clock moves only by [`advance`].
    pub fn freeze() {
        let mut s = S.lock().unwrap();
        if s.frozen.is_none() {
            s.frozen = Some(Instant::now());
        }
    }

    /// Moves the frozen clock by `d`; returns once an idle pool's reaper has looked at the new
    /// time (and wound the pool down if its idle time is up).
    pub fn advance(d: Duration) {
        let mut s = S.lock().unwrap();
        let t = s.frozen.expect("test_clock::freeze first");
        s.frozen = Some(t + d);
        s.advanced += 1;
        let n = s.advanced;
        CV.notify_all();
        while s.reaper && s.seen < n {
            s = CV.wait(s).unwrap();
        }
    }
}

/// Run `f` (and every rayon call inside it) on the shared pool of one thread per CPU, started
/// on demand and wound down [`IDLE`] after its last job. Inside the pool already, `f` runs inline.
/// Where the system refuses the pool's threads, `f` runs on the calling thread alone.
pub fn scoped<R: Send>(f: impl FnOnce() -> R + Send) -> R {
    if rayon::current_thread_index().is_some() {
        return f();
    }
    let pool = {
        let mut s = S.lock().unwrap();
        if s.pool.is_none() {
            s.pool = rayon::ThreadPoolBuilder::new()
                .thread_name(|i| format!("img{i}"))
                .build()
                .ok()
                .map(Arc::new);
        }
        s.users += 1;
        s.pool.clone()
    };
    let _use = Use;
    match pool {
        Some(p) => p.install(f),
        None => alone(f),
    }
}

/// Runs `f` with the calling thread as the one worker of a pool that lasts for this call: what
/// [`scoped`] does where the system refuses the pool's threads (`EAGAIN`). A rayon call made
/// off a pool goes to rayon's global one, which panics when its threads are refused too. Once
/// `f` has returned the thread is no pool's worker, so its next call can have the shared pool.
fn alone<R: Send>(f: impl FnOnce() -> R + Send) -> R {
    // The pool's one thread is not started: rayon hands over what would run on it.
    let mut worker = None;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .spawn_handler(|thread| {
            worker = Some(thread);
            Ok(())
        })
        .build();
    let (Ok(pool), Some(worker)) = (pool, worker) else {
        return f();
    };
    let mut out = None;
    let job: Box<dyn FnOnce() + Send + '_> = Box::new(|| {
        out = Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)));
    });
    // SAFETY: `spawn` wants a `'static` job and this one borrows `out` and whatever `f`
    // borrows. The pool has no thread, so the job stays in its queue until `worker.run()`
    // takes it on this thread, and `run` returns only when the pool has ended, which a queued
    // job holds off until it has run: the job is over before anything it borrows ends.
    // Should this thread unwind before `run`, the job is never run.
    let job: Box<dyn FnOnce() + Send + 'static> = unsafe { std::mem::transmute(job) };
    pool.spawn(job);
    // The queued job now keeps the pool: it ends, and `run` returns, when the job is done.
    drop(pool);
    worker.run();
    match out {
        Some(Ok(r)) => r,
        Some(Err(panic)) => std::panic::resume_unwind(panic),
        None => unreachable!("the pool ended before its job ran"),
    }
}
