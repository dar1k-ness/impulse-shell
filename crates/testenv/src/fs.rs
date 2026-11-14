//! Waits on file system events (inotify(7)) and FIFO handshakes, never on a timer.

use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

/// Blocks until `path` exists, reading inotify events on its directory (which must exist) for
/// entries created or renamed into it. No deadline of its own: a hang is nextest's
/// `slow-timeout` to end.
pub fn wait_exists(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let dir = path.parent().expect("a path inside a directory");
    let cdir = std::ffi::CString::new(dir.as_os_str().as_bytes()).unwrap();
    // SAFETY: inotify_init1(2) takes only flags.
    let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
    assert!(
        fd >= 0,
        "inotify_init1: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: `fd` is the new descriptor inotify_init1 returned; nothing else owns it.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mask = libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_ONLYDIR;
    // SAFETY: `fd` is a live inotify descriptor and `cdir` a NUL-terminated path that outlives
    // the call.
    let wd = unsafe { libc::inotify_add_watch(fd.as_raw_fd(), cdir.as_ptr(), mask) };
    assert!(
        wd >= 0,
        "inotify_add_watch({}): {}",
        dir.display(),
        std::io::Error::last_os_error()
    );
    let mut events = std::fs::File::from(fd);
    let mut buf = [0u8; 4096];
    // Armed before the first look, so an entry made in between is not missed.
    while !path.exists() {
        match events.read(&mut buf) {
            Ok(n) => assert!(n > 0, "inotify closed"),
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "inotify: {e}"),
        }
    }
}

/// inotify(7) watch of one directory's entries: [`DirWatch::wait`] blocks until a file in it is
/// created, written or moved in since the last wait.
pub struct DirWatch(OwnedFd);

impl DirWatch {
    pub fn new(dir: &Path) -> DirWatch {
        // SAFETY: inotify_init1(2) takes flags and returns a new descriptor or -1.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        assert!(
            fd >= 0,
            "inotify_init1: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: `fd` is the new descriptor inotify_init1 returned; nothing else owns it.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let c = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()).unwrap();
        let mask = libc::IN_CREATE | libc::IN_MODIFY | libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO;
        // SAFETY: `c` is a NUL-terminated path that outlives the call.
        let wd = unsafe { libc::inotify_add_watch(fd.as_raw_fd(), c.as_ptr(), mask) };
        assert!(
            wd >= 0,
            "inotify_add_watch({}): {}",
            dir.display(),
            std::io::Error::last_os_error()
        );
        DirWatch(fd)
    }

    /// Blocks until a change arrived since the last wait, and consumes the queued events.
    pub fn wait(&self) {
        crate::proc::wait_readable(&self.0);
        let mut buf = [0u8; 4096];
        // SAFETY: read(2) into `buf`, which is valid for its length; the fd is nonblocking, so
        // the loop ends once the queue is empty (-1, EAGAIN).
        while unsafe { libc::read(self.0.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
    }

    /// Waits for changes until `done()` holds; checks it first, so a change made before the
    /// watch existed counts only when `done` sees it.
    pub fn until(&self, done: impl Fn() -> bool) {
        while !done() {
            self.wait();
        }
    }
}

/// Makes a FIFO at `path` (mkfifo(3)) for a handshake with a script: opening it blocks until
/// the other end is opened too, and a read returns what the script wrote once it closed its end.
pub fn fifo(path: &std::path::Path) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).expect("path without NUL");
    // SAFETY: `c` is a NUL-terminated path that lives for the call.
    let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
    assert_eq!(
        rc,
        0,
        "mkfifo {}: {}",
        path.display(),
        std::io::Error::last_os_error()
    );
}

/// Lets a script held reading the FIFO at `path` ([`fifo`]) go: opens it for writing, which
/// waits for the script to have it open, and closes it (EOF for the script).
#[track_caller]
pub fn release_fifo(path: &Path) {
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap_or_else(|e| panic!("open {} for writing: {e}", path.display()));
    drop(f);
}
