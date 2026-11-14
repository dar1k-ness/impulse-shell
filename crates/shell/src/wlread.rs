//! Reads the Wayland socket on its own thread and wakes the main loop only for the shell's
//! events, like QtWaylandClient's `QWaylandDisplay::EventThread`.
//!
//! Mesa's EGL platform keeps its buffer releases and per-swap `wl_display.sync` throttle on a
//! private `wl_event_queue` it dispatches itself inside `eglSwapBuffers`. Polling the socket on
//! the main loop woke it for each of those (one turn per window per frame on top of the frame
//! callbacks); here they are read into Mesa's queue without a main-thread wakeup. The read uses
//! libwayland's multi-reader protocol (`wl_display_prepare_read_queue` / `wl_display_read_events`),
//! so Mesa's own reads on the main thread interleave with this one.

use std::os::fd::AsRawFd;
use std::sync::{Arc, Mutex};

use wayland_client::Connection;
use wayland_client::backend::WaylandError;

/// The error that stopped the reader, for the main loop to report.
pub type Failure = Arc<Mutex<Option<WaylandError>>>;

/// Start the reader. The events it reads for the shell's objects wake the shell's `EventQueue`
/// waker (`EventQueue::poll_dispatch_pending`); `failed` runs once if the connection breaks,
/// with the error in the returned slot.
pub fn spawn(conn: Connection, failed: impl FnOnce() + Send + 'static) -> std::io::Result<Failure> {
    let failure = Failure::default();
    let slot = failure.clone();
    std::thread::Builder::new()
        .name("wayland-read".into())
        .spawn(move || {
            let e = read_loop(&conn);
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
            failed();
        })?;
    Ok(failure)
}

fn read_loop(conn: &Connection) -> WaylandError {
    loop {
        // `None`: events for the shell were read by another thread (Mesa reading for its own
        // queue on the main thread); hand them to the shell's queue.
        let Some(guard) = conn.prepare_read() else {
            if let Err(e) = conn.backend().dispatch_inner_queue() {
                return e;
            }
            continue;
        };
        let mut fd = libc::pollfd {
            fd: guard.connection_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd; the guard keeps the socket open.
        if unsafe { libc::poll(&mut fd, 1, -1) } < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return WaylandError::Io(e);
        }
        match guard.read() {
            Ok(_) => {}
            Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return e,
        }
    }
}
