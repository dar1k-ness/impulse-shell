//! `eprintln!` and `eprint!` that drop a failed write instead of panicking.
//!
//! std's macros panic when stderr's write fails (EPIPE once the reader of a pipe is gone, as
//! a closed terminal or `rusty-impulse 2>&1 | head` leaves it; SIGPIPE is ignored in Rust programs),
//! and the shell would exit with 101 on its next log line. A log line nobody can read is lost
//! instead, as Qt's message handler loses it. The workspace lints forbid std's macros outside
//! tests (`clippy::print_stderr`).
//!
//! A line reaches stderr whole. Its text and its newline are formatted first and written with
//! one write(2), so the lines of another thread or process that writes to the same pipe,
//! terminal or file (libwayland's `WAYLAND_DEBUG` lines, a child's stderr) come before it or
//! after it, never inside it. The kernel keeps a write to a pipe in one piece up to `PIPE_BUF`
//! bytes (POSIX; 4096 on Linux). A longer line is still written in full, never cut, but another
//! writer's bytes can land inside it: on a pipe that is full the kernel takes it in parts.

#[doc(hidden)]
pub fn write(args: std::fmt::Arguments) {
    match args.as_str() {
        Some(text) => write_whole(text.as_bytes()),
        None => {
            // What a failing `Display` had written so far is logged, as `write_fmt` did.
            let mut text = String::new();
            let _ = std::fmt::write(&mut text, args);
            write_whole(text.as_bytes());
        }
    }
}

/// Writes all of `text` to stderr, with one write(2) when stderr takes it at once. What a
/// short write left is written next, a write a signal interrupted (`EINTR`) is repeated, and
/// on a stderr that does not block and takes no more (`EAGAIN`) the rest waits until stderr is
/// writable, as the whole write waits on a stderr that blocks. Any other error (`EPIPE`: the
/// reader is gone) drops the rest. Other threads of the process that log wait meanwhile.
pub fn write_whole(text: &[u8]) {
    let _others_wait = std::io::stderr().lock();
    write_whole_unlocked(text);
}

/// [`write_whole`] without std's lock on stderr, for a panic hook: the message of a panic is
/// written even while another thread holds that lock (and waits for the thread that
/// panicked). The cost: a text longer than `PIPE_BUF` can mix with a long line another thread
/// of this process logs at the same moment.
pub fn write_whole_unlocked(mut text: &[u8]) {
    while !text.is_empty() {
        // SAFETY: `text` is a valid buffer of `text.len()` bytes for the length of the call.
        let n = unsafe { libc::write(libc::STDERR_FILENO, text.as_ptr().cast(), text.len()) };
        match usize::try_from(n) {
            Ok(0) => return,
            Ok(n) => text = &text[n..],
            Err(_) => match std::io::Error::last_os_error().kind() {
                std::io::ErrorKind::Interrupted => {}
                std::io::ErrorKind::WouldBlock if writable() => {}
                _ => return,
            },
        }
    }
}

/// Waits until stderr takes output again, or reports an error or hangup to the next write.
/// `false` when the wait itself failed.
fn writable() -> bool {
    let mut poll = libc::pollfd {
        fd: libc::STDERR_FILENO,
        events: libc::POLLOUT,
        revents: 0,
    };
    loop {
        // SAFETY: `poll` is one valid pollfd that outlives the call.
        let n = unsafe { libc::poll(&mut poll, 1, -1) };
        if n >= 0 {
            return true;
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return false;
        }
    }
}

/// Stderr as a writer for whole lines: each `write` takes all it is given, as
/// [`write_whole`] writes it, and never fails.
pub struct Stderr;

impl std::io::Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        write_whole(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `std::eprintln!`, a failed write ignored.
#[macro_export]
macro_rules! eprintln {
    () => {
        $crate::write(::std::format_args!("\n"))
    };
    ($($arg:tt)*) => {
        $crate::write(::std::format_args!("{}\n", ::std::format_args!($($arg)*)))
    };
}

/// `std::eprint!`, a failed write ignored.
#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => {
        $crate::write(::std::format_args!($($arg)*))
    };
}
