//! What a call costs, counted instead of timed: the bytes the calling thread allocated and
//! the bytes of the strings the engine read from end to end ([`qml::utf16::read`]). The
//! thread's CPU clock is no measure of it: on a virtual machine it also counts the time the
//! hypervisor ran something else on the thread's CPU (`steal` of proc_stat(5)), which a
//! call of a millisecond falls between and a call of 50 ms does not.
//!
//! Shared by the tests of both paths: `js_long_strings.rs` (interpreter) and
//! `crates/rigen/check/tests/long_strings.rs` (compiled).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCATED: Cell<u64> = const { Cell::new(0) };
}

/// [`System`], adding up the bytes each thread asks for.
struct Counting;

fn count(bytes: usize) {
    ALLOCATED.set(ALLOCATED.get() + bytes as u64);
}

// SAFETY: every call goes to `System` with the caller's arguments; the count is a thread
// local without a destructor, which allocates nothing.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count(l.size());
        // SAFETY: the caller's contract, passed on.
        unsafe { System.alloc(l) }
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        count(l.size());
        // SAFETY: the caller's contract, passed on.
        unsafe { System.alloc_zeroed(l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        count(size);
        // SAFETY: the caller's contract, passed on.
        unsafe { System.realloc(p, l, size) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: the caller's contract, passed on.
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// The work of the calling thread so far.
#[derive(Clone, Copy)]
pub struct Work {
    /// Bytes allocated: a copy of a string is its length of them.
    pub allocated: u64,
    /// Bytes of strings read from end to end to count or encode their UTF-16 code units.
    pub read: u64,
}

impl Work {
    pub fn now() -> Self {
        Work {
            allocated: ALLOCATED.get(),
            read: qml::utf16::read(),
        }
    }

    pub fn since(self, start: Work) -> Self {
        Work {
            allocated: self.allocated - start.allocated,
            read: self.read - start.read,
        }
    }

    /// `"allocated"` or `"read"` when `self`, the work on a text `growth` times as long as
    /// that of `short`, is not under four times as much per byte: a call that copies or
    /// reads the whole text on each turn of its loop makes it `growth` times as much.
    pub fn grew_faster_than(self, short: Work, growth: usize) -> Option<&'static str> {
        let limit = 4 * growth as u64;
        if self.allocated >= limit * short.allocated && self.allocated > 0 {
            Some("allocated")
        } else if self.read >= limit * short.read && self.read > 0 {
            Some("read")
        } else {
            None
        }
    }
}

impl std::fmt::Display for Work {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} allocated, {} read", self.allocated, self.read)
    }
}
