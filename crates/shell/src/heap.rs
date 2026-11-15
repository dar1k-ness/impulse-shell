//! The process allocator: jemalloc, linked unprefixed so the C libraries in the process (Mesa,
//! PipeWire, fontconfig, …) allocate from it too. Freed memory goes back to the kernel by
//! jemalloc's decay, which its background thread purges on its own clock, so the peak of a theme
//! switch, a wallpaper decode or a panel teardown leaves RSS within about a second with no work
//! on the main loop (jemalloc TUNING.md: `background_thread`, shorter decay for memory-sensitive
//! programs). The shell starts that thread with [`start_background_thread`] once its startup no
//! longer needs to be single-threaded, not at the allocator's initialisation before `main`.

use std::ffi::c_char;

#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// `const char *`, the type jemalloc declares `malloc_conf` with.
#[repr(transparent)]
pub struct Conf(*const c_char);

// SAFETY: points at a 'static string literal that nothing writes.
unsafe impl Sync for Conf {}

/// Compiled-in options, read by jemalloc at its initialisation (jemalloc(3), "TUNING"): the
/// `MALLOC_CONF` environment variable still overrides them. They are TUNING.md's "low resource
/// consumption application" set. Dirty pages are handed back (`MADV_DONTNEED`) after about 1 s
/// unused instead of jemalloc's 10 s default; with `muzzy_decay_ms:0` there is no second, lazily
/// reclaimed (`MADV_FREE`) stage that would keep the pages counted in RSS. One arena: the main
/// thread does nearly all allocating, and every arena keeps its own partly used pages. Thread
/// caches hold only blocks up to 1 KiB: each of the process's threads (Mesa, PipeWire, Rust
/// workers) otherwise keeps a cache of freed blocks up to 32 KiB that counts as allocated.
/// `thp:never` marks the heap's mappings `MADV_NOHUGEPAGE`: where the kernel's
/// `transparent_hugepage/enabled` is `always` (Arch's default) khugepaged otherwise backs the
/// heap with 2 MiB pages, which fills in the pages jemalloc purged and keeps a whole huge page
/// resident for any live block in it (madvise(2), `MADV_HUGEPAGE`).
/// `background_thread` is not among them: [`start_background_thread`] enables it.
#[unsafe(export_name = "malloc_conf")]
pub static MALLOC_CONF: Conf =
    Conf(c"dirty_decay_ms:1000,muzzy_decay_ms:0,narenas:1,tcache_max:1024,thp:never".as_ptr());

/// Starts jemalloc's purging thread (mallctl `background_thread`, jemalloc(3)), unless
/// `MALLOC_CONF` chose `background_thread` itself. As a `malloc_conf` option jemalloc would
/// start it at its initialisation, before `main`, and the shell could not apply its
/// `//@ pragma Env` lines with no other thread running.
pub fn start_background_thread() {
    let chosen = std::env::var("MALLOC_CONF").is_ok_and(|c| {
        c.split(',').any(|o| {
            o.split_once(':')
                .is_some_and(|(k, _)| k == "background_thread")
        })
    });
    if chosen {
        return;
    }
    if let Err(e) = tikv_jemalloc_ctl::background_thread::write(true) {
        elog::eprintln!("impulse-shell: cannot start jemalloc's background thread: {e}");
    }
}

/// Allocator statistics in bytes (jemalloc(3) `stats.*`).
#[derive(Clone, Copy, Debug)]
pub struct Stats {
    /// Held by the application.
    pub allocated: usize,
    /// In pages with live allocations.
    pub active: usize,
    /// Physically resident, metadata and not yet purged dirty pages included.
    pub resident: usize,
    /// Virtual memory kept mapped for reuse but returned to the kernel.
    pub retained: usize,
}

pub fn stats() -> Option<Stats> {
    use tikv_jemalloc_ctl::{epoch, stats};
    epoch::advance().ok()?;
    Some(Stats {
        allocated: stats::allocated::read().ok()?,
        active: stats::active::read().ok()?,
        resident: stats::resident::read().ok()?,
        retained: stats::retained::read().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Condvar, Mutex, OnceLock};

    const M: u64 = 1 << 20;

    /// jemalloc's `extent_hooks_t` (jemalloc(3), `arena.<i>.extent_hooks`): nine function
    /// pointers, in this order since jemalloc 5.0.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ExtentHooks {
        alloc: *const (),
        dalloc: Option<HookDalloc>,
        destroy: *const (),
        commit: *const (),
        decommit: Option<HookRange>,
        purge_lazy: Option<HookRange>,
        purge_forced: Option<HookRange>,
        split: *const (),
        merge: *const (),
    }

    type HookDalloc = unsafe extern "C" fn(*mut ExtentHooks, *mut u8, usize, bool, u32) -> bool;
    type HookRange =
        unsafe extern "C" fn(*mut ExtentHooks, *mut u8, usize, usize, usize, u32) -> bool;

    /// Holds plain function pointers and nulls only.
    struct Hooks(ExtentHooks);
    // SAFETY: the table is never written after it is built; jemalloc calls through it from any
    // thread, as through its default one.
    unsafe impl Sync for Hooks {}
    // SAFETY: as above.
    unsafe impl Send for Hooks {}

    /// The arena's own hooks, and the table installed in their place.
    static DEFAULT: OnceLock<(Hooks, usize)> = OnceLock::new();
    static MINE: OnceLock<Hooks> = OnceLock::new();
    /// Pages handed back so far (each decommit, purge or unmap that succeeded), and whether a
    /// thread other than the test's did one.
    static RETURNS: Mutex<u64> = Mutex::new(0);
    static RETURNED: Condvar = Condvar::new();
    static TEST_TID: AtomicU64 = AtomicU64::new(0);
    static ELSEWHERE: AtomicBool = AtomicBool::new(false);

    fn default_hooks() -> (ExtentHooks, *mut ExtentHooks) {
        let (h, p) = DEFAULT.get().expect("hooks installed");
        (h.0, *p as *mut ExtentHooks)
    }

    /// Called by jemalloc with its locks released; touches only atomics, a futex mutex and a
    /// condvar, so it never allocates.
    fn note_return(failed: bool) -> bool {
        if !failed {
            // SAFETY: gettid(2) takes no arguments.
            let tid = unsafe { libc::gettid() } as u64;
            if tid != TEST_TID.load(Ordering::Acquire) {
                ELSEWHERE.store(true, Ordering::Release);
            }
            *RETURNS.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            RETURNED.notify_all();
        }
        failed
    }

    unsafe extern "C" fn dalloc(
        _: *mut ExtentHooks,
        a: *mut u8,
        n: usize,
        c: bool,
        i: u32,
    ) -> bool {
        let (h, p) = default_hooks();
        // SAFETY: jemalloc's own dalloc hook, called with the arguments jemalloc passed.
        note_return(h.dalloc.is_none_or(|f| unsafe { f(p, a, n, c, i) }))
    }

    macro_rules! range_hook {
        ($name:ident) => {
            unsafe extern "C" fn $name(
                _: *mut ExtentHooks,
                a: *mut u8,
                n: usize,
                off: usize,
                len: usize,
                i: u32,
            ) -> bool {
                let (h, p) = default_hooks();
                // SAFETY: jemalloc's own hook, called with the arguments jemalloc passed.
                note_return(h.$name.is_none_or(|f| unsafe { f(p, a, n, off, len, i) }))
            }
        };
    }
    range_hook!(decommit);
    range_hook!(purge_lazy);
    range_hook!(purge_forced);

    /// Wraps the calling thread's arena's hooks so every page range jemalloc hands back to the
    /// kernel is counted and wakes [`wait_return`].
    fn watch_returns() {
        use tikv_jemalloc_ctl::raw;
        // SAFETY: gettid(2) takes no arguments.
        TEST_TID.store(unsafe { libc::gettid() } as u64, Ordering::Release);
        // SAFETY: a mallctl name of the type read (jemalloc(3), MALLCTL NAMESPACE).
        let arena = unsafe { raw::read::<u32>(b"thread.arena\0") }.unwrap();
        let name = format!("arena.{arena}.extent_hooks\0");
        // SAFETY: as above; the pointer names jemalloc's static default table.
        let p = unsafe { raw::read::<*mut ExtentHooks>(name.as_bytes()) }.unwrap();
        // SAFETY: `p` points at a live `extent_hooks_t`, which jemalloc never frees.
        let h = unsafe { *p };
        let _ = DEFAULT.set((Hooks(h), p as usize));
        let mine = MINE.get_or_init(|| {
            Hooks(ExtentHooks {
                dalloc: Some(dalloc),
                decommit: Some(decommit),
                purge_lazy: Some(purge_lazy),
                purge_forced: Some(purge_forced),
                ..h
            })
        });
        // SAFETY: as above; the table is 'static and its other entries are jemalloc's own.
        unsafe {
            raw::write(
                name.as_bytes(),
                &mine.0 as *const ExtentHooks as *mut ExtentHooks,
            )
        }
        .unwrap();
    }

    /// Blocks until jemalloc handed pages back after the `seen`-th time; returns the new count.
    fn wait_return(seen: u64) -> u64 {
        let g = RETURNS.lock().unwrap();
        *RETURNED.wait_while(g, |n| *n <= seen).unwrap()
    }

    fn anon_rss() -> u64 {
        let s = std::fs::read_to_string("/proc/self/statm").unwrap();
        let f: Vec<u64> = s.split_whitespace().map(|v| v.parse().unwrap()).collect();
        // SAFETY: sysconf takes no pointers.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
        (f[1] - f[2]) * page
    }

    #[test]
    fn options_are_in_effect() {
        use tikv_jemalloc_ctl::raw;
        // SAFETY: a mallctl name of the type read (jemalloc(3), MALLCTL NAMESPACE).
        assert!(!unsafe { raw::read::<bool>(b"opt.background_thread\0") }.unwrap());
        super::start_background_thread();
        // SAFETY: as above.
        assert!(unsafe { raw::read::<bool>(b"background_thread\0") }.unwrap());
        // SAFETY: as above.
        let dirty = unsafe { raw::read::<isize>(b"opt.dirty_decay_ms\0") };
        assert_eq!(dirty.unwrap(), 1000);
        // SAFETY: as above.
        let muzzy = unsafe { raw::read::<isize>(b"opt.muzzy_decay_ms\0") };
        assert_eq!(muzzy.unwrap(), 0);
        // SAFETY: as above.
        assert_eq!(unsafe { raw::read::<u32>(b"opt.narenas\0") }.unwrap(), 1);
        // SAFETY: as above.
        let tcache_max = unsafe { raw::read::<usize>(b"opt.tcache_max\0") };
        assert_eq!(tcache_max.unwrap(), 1024);
        // SAFETY: as above; `opt.thp` is a `const char *` naming a static string.
        let thp = unsafe { raw::read::<*const std::ffi::c_char>(b"opt.thp\0") }.unwrap();
        // SAFETY: jemalloc's NUL-terminated name of the mode, never freed.
        assert_eq!(unsafe { std::ffi::CStr::from_ptr(thp) }, c"never");
        // C code in the process allocates from jemalloc too (`thread.allocated`: bytes this
        // thread allocated, unaffected by tests running in parallel).
        // SAFETY: a mallctl name of the type read (jemalloc(3), MALLCTL NAMESPACE).
        let mine = || unsafe { raw::read::<u64>(b"thread.allocated\0").unwrap() };
        let before = mine();
        // black_box: LLVM may delete a malloc/free pair whose memory is never used, as it
        // does in release builds.
        // SAFETY: malloc of a nonzero size; the block is freed below.
        let p = std::hint::black_box(unsafe { libc::malloc(16 << 20) });
        assert!(!p.is_null());
        let during = mine();
        // SAFETY: `p` is the live block malloc returned above, freed once.
        unsafe { libc::free(std::hint::black_box(p)) };
        assert!(during >= before + (16 << 20), "{before} -> {during}");
    }

    /// Freed heap leaves RSS with nothing calling into the allocator: the background thread
    /// purges it once the decay time is over.
    #[test]
    fn freed_heap_returns_to_the_kernel_by_itself() {
        // Process-wide RSS: measure in a child running only this test, away from the
        // allocations of tests running in parallel.
        const CHILD: &str = "RI_HEAP_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "heap::tests::freed_heap_returns_to_the_kernel_by_itself",
                    "--exact",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            return;
        }
        super::start_background_thread();
        watch_returns();
        // Interleaved survivors: the freed half is scattered between live blocks, the case
        // glibc only gave back on malloc_trim.
        let n = 2048;
        let mut v: Vec<Box<[u8]>> = (0..n)
            .map(|_| vec![1u8; 64 << 10].into_boxed_slice())
            .collect();
        let mut keep = Vec::with_capacity(n / 2);
        let before = anon_rss();
        let mut seen = *RETURNS.lock().unwrap();
        for (k, b) in v.drain(..).enumerate() {
            if k % 2 == 0 {
                keep.push(b);
            }
        }
        drop(v);
        // Woken by each page range jemalloc hands back, measured then; no clock of the test's.
        let mut after = anon_rss();
        while before.saturating_sub(after) < 48 * M {
            seen = wait_return(seen);
            after = anon_rss();
        }
        assert!(
            ELSEWHERE.load(Ordering::Acquire),
            "only the test thread handed pages back: the background thread never purged"
        );
        drop(keep);
    }
}
