//! Run a QML file in the native runtime with the service pollers (PAM, polkit, processes) for
//! manual tests. Each stdin line is written to the root's `input` property and bumps its
//! `inputSeq` (repeated lines still notify); the root's
//! `quit: true` ends the run.
//! usage: authcheck FILE.qml
//!
//! The loop is the shell's, without windows: it sleeps in poll(2) on the runtime's wakeups and
//! fds, on stdin and on a timerfd armed for the next timer or animation frame, and runs timers
//! and animations at wall-clock ms.
use qml::value::Value;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Instant;

/// One frame at 60 Hz, while animations run.
const FRAME: f64 = 1000.0 / 60.0;

/// A CLOCK_MONOTONIC timerfd: readable once the time [`Deadline::arm`] set has passed.
struct Deadline(OwnedFd);

impl Deadline {
    fn new() -> Self {
        // SAFETY: timerfd_create(2) takes a clock id and flags and returns a new fd or -1.
        let fd = unsafe {
            libc::timerfd_create(
                libc::CLOCK_MONOTONIC,
                libc::TFD_CLOEXEC | libc::TFD_NONBLOCK,
            )
        };
        assert!(
            fd >= 0,
            "timerfd_create: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: `fd` was just returned by timerfd_create and nothing else owns it.
        Self(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    /// Expires `ms` from now, or never for `None` (a zero it_value disarms it,
    /// timerfd_settime(2)). Setting it also clears an expiry not read yet.
    fn arm(&self, ms: Option<f64>) {
        let ns = ms.map_or(0, |ms| (ms * 1e6).ceil().max(1.0) as i64);
        let spec = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: libc::timespec {
                tv_sec: ns / 1_000_000_000,
                tv_nsec: ns % 1_000_000_000,
            },
        };
        // SAFETY: the fd is the open timerfd and `spec` a live itimerspec it only reads.
        let r =
            unsafe { libc::timerfd_settime(self.0.as_raw_fd(), 0, &spec, std::ptr::null_mut()) };
        assert!(
            r == 0,
            "timerfd_settime: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Whether `fd` is readable or hung up now (poll(2) without waiting, after the runtime's wait
/// returned for one of the fds).
fn readable(fd: i32) -> bool {
    let mut p = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `p` is one live pollfd for the duration of the call.
    (unsafe { libc::poll(&mut p, 1, 0) }) == 1
}

/// The complete lines stdin has for one read(2) once it is readable, keeping a partial line in
/// `buf`; `None` at end of file.
fn read_lines(buf: &mut Vec<u8>) -> Option<Vec<String>> {
    let mut chunk = [0u8; 4096];
    // SAFETY: reads at most `chunk.len()` bytes into the live buffer.
    let n = unsafe { libc::read(0, chunk.as_mut_ptr().cast(), chunk.len()) };
    if n == 0 {
        return None;
    }
    if n < 0 {
        let e = std::io::Error::last_os_error();
        assert!(e.kind() == std::io::ErrorKind::Interrupted, "stdin: {e}");
        return Some(vec![]);
    }
    buf.extend_from_slice(&chunk[..n as usize]);
    let mut lines = vec![];
    while let Some(i) = buf.iter().position(|&b| b == b'\n') {
        let l: Vec<u8> = buf.drain(..=i).collect();
        lines.push(String::from_utf8_lossy(&l[..i]).into_owned());
    }
    Some(lines)
}

fn main() {
    let file = std::env::args().nth(1).expect("usage: authcheck FILE.qml");
    let mut rt = qml::runtime::Runtime::new(None);
    let root = rt
        .load_file(std::path::Path::new(&file))
        .expect("load failed");
    rt.settle();
    let mut seen = std::collections::HashSet::new();
    let start = Instant::now();
    let deadline = Deadline::new();
    let mut stdin_open = true;
    let mut buf = Vec::new();
    loop {
        rt.poll_native();
        let now = start.elapsed().as_secs_f64() * 1000.0;
        let busy = rt.tick(now);
        for e in rt.errors() {
            if seen.insert(e.clone()) {
                elog::eprintln!("qml error: {e}");
            }
        }
        if rt.get(root, "quit").truthy() {
            break;
        }
        let mut next = rt.next_deadline(now);
        if busy {
            next = next.min(FRAME);
        }
        if next <= 0.0 {
            continue;
        }
        deadline.arm(next.is_finite().then_some(next));
        let mut fds = vec![deadline.0.as_raw_fd()];
        if stdin_open {
            fds.push(0);
        }
        rt.wait_native_or(&fds);
        if stdin_open && readable(0) {
            match read_lines(&mut buf) {
                Some(lines) => {
                    for l in lines {
                        let n = rt.get(root, "inputSeq").to_number();
                        rt.world.borrow_mut().write(root, "input", Value::str(&l));
                        rt.world
                            .borrow_mut()
                            .write(root, "inputSeq", Value::Num(n + 1.0));
                        rt.settle();
                    }
                }
                None => stdin_open = false,
            }
        }
    }
}
