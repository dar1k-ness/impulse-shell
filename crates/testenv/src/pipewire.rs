//! Helpers of the media tests (PipeWire).

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// The sockets `pipewire.socket` lists, in its order.
pub const PIPEWIRE_SOCKETS: [&str; 2] = ["pipewire-0", "pipewire-0-manager"];

/// `pipewire` socket-activated on [`PIPEWIRE_SOCKETS`] in `dir` (sd_listen_fds(3), what the
/// `pipewire.socket` unit does). The sockets are bound and listening once this returns, so a
/// client connects at once and its requests wait for the daemon instead of the client
/// waiting for the socket file.
pub fn socket_activated_pipewire(dir: &Path) -> Command {
    let fds: Vec<OwnedFd> = PIPEWIRE_SOCKETS
        .iter()
        .map(|n| {
            let l = UnixListener::bind(dir.join(n)).unwrap();
            // Above the fds the child gets them at, so the dup2s below do not clobber them.
            // SAFETY: F_DUPFD_CLOEXEC duplicates the listener's open descriptor; it reads no memory.
            let fd = unsafe {
                libc::fcntl(
                    l.as_raw_fd(),
                    libc::F_DUPFD_CLOEXEC,
                    3 + PIPEWIRE_SOCKETS.len() as i32,
                )
            };
            assert!(fd >= 0, "{}", std::io::Error::last_os_error());
            // SAFETY: `fd` is the new descriptor fcntl returned; nothing else owns it.
            unsafe { OwnedFd::from_raw_fd(fd) }
        })
        .collect();
    let mut c = Command::new("sh");
    c.args(["-c", "LISTEN_PID=$$ exec pipewire"])
        .env("LISTEN_FDS", PIPEWIRE_SOCKETS.len().to_string())
        .env("LISTEN_FDNAMES", PIPEWIRE_SOCKETS.join(":"));
    let dup = move || {
        for (i, fd) in fds.iter().enumerate() {
            // SAFETY: dup2(2) is async-signal-safe and only takes descriptors; the sources sit above
            // the targets (F_DUPFD_CLOEXEC from 3 + len), so no target closes a later source.
            if unsafe { libc::dup2(fd.as_raw_fd(), 3 + i as i32) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    };
    // SAFETY: `dup` only calls dup2(2), async-signal-safe, and allocates nothing.
    unsafe { c.pre_exec(dup) };
    c
}
