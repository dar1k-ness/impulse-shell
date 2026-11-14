//! The shell's panic hook: a panic is one record, whole on stderr and in the instance log.
//!
//! std's hook writes a message longer than 512 bytes in pieces and its `RUST_BACKTRACE` note
//! in a write of its own, loses all of it on a stderr that does not block and is full, and
//! knows nothing of the instance log (`log.log`, `log.qslog`), where a panic left no trace.
//!
//! The hook runs on the thread that panicked, before it unwinds, so whatever that thread
//! holds is still held. The hook takes none of it: no std lock on stderr
//! ([`elog::write_whole_unlocked`]), and the instance log is reached through its channel
//! (`mpsc`, no mutex of ours). It waits for one thing, the `qs-log` thread writing the record
//! out, and that thread shares no lock with the rest of the shell; when it is the `qs-log`
//! thread that panicked, the hook does not wait ([`crate::rilog::flush_in_panic`]).
//! Nothing here can panic: the text is put together from strings and numbers only.

use std::backtrace::{Backtrace, BacktraceStatus};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};

/// Installs the hook; call it first in `main`.
pub fn log_whole() {
    std::panic::set_hook(Box::new(|info| {
        let backtrace = Backtrace::capture();
        let traced = backtrace.status() == BacktraceStatus::Captured;
        let record = record(info, traced.then_some(&backtrace));
        let mut text = format!("\n{record}\n");
        if !traced && !NOTED.swap(true, Ordering::Relaxed) {
            text.push_str(NOTE);
        }
        elog::write_whole_unlocked(text.as_bytes());
        crate::rilog::log(crate::rilog::Level::Critical, "default", &record);
        crate::rilog::flush_in_panic();
    }));
}

/// std's hint, once per process, when no backtrace was asked for.
const NOTE: &str =
    "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n";
static NOTED: AtomicBool = AtomicBool::new(false);

/// What std's hook prints, as one text without the newlines around it: the thread, the place,
/// the message, and `backtrace` when `RUST_BACKTRACE` asks for one.
fn record(info: &std::panic::PanicHookInfo, backtrace: Option<&Backtrace>) -> String {
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("<unnamed>");
    // SAFETY: gettid(2) takes no arguments and cannot fail.
    let tid = unsafe { libc::gettid() };
    let message = info.payload_as_str().unwrap_or("Box<dyn Any>");
    let mut record = format!("thread '{name}' ({tid}) panicked at ");
    // A `String` takes whatever is written to it: these writes cannot fail.
    let _ = match info.location() {
        Some(at) => write!(record, "{at}:\n{message}"),
        None => write!(record, "an unknown place:\n{message}"),
    };
    if let Some(backtrace) = backtrace {
        let _ = write!(record, "\nstack backtrace:\n{backtrace}");
        record.truncate(record.trim_end().len());
    }
    record
}
