//! Selection data written to the reader's pipe (`send` events) and read from the owner's pipe
//! (`receive`) from the event loop.

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::rc::Rc;
use std::sync::Arc;

use calloop::generic::Generic;
use calloop::{Interest, LoopHandle, Mode, PostAction, RegistrationToken};

/// The transfers in flight for one selection source. As wl-clipboard and GTK do, each pipe is
/// made non-blocking and written whenever it is writable, so a reader that stops reading holds
/// only its pipe, never a thread or the loop. A transfer ends once all is written or the reader
/// closes its end (EPIPE); dropping this (the source was cancelled or replaced) ends the rest
/// and closes their pipes.
pub struct Transfers<D: 'static> {
    handle: LoopHandle<'static, D>,
    live: Rc<RefCell<Vec<RegistrationToken>>>,
}

impl<D> Transfers<D> {
    pub fn new(handle: LoopHandle<'static, D>) -> Self {
        Self {
            handle,
            live: Rc::default(),
        }
    }

    /// Writes `data` to `pipe`, the fd of a `send` event, from the loop.
    pub fn send(&self, data: Arc<str>, pipe: OwnedFd) -> io::Result<()> {
        set_nonblocking(&pipe)?;
        let token = Rc::new(Cell::new(None::<RegistrationToken>));
        let mine = token.clone();
        let live = self.live.clone();
        let mut written = 0;
        let source = Generic::new(File::from(pipe), Interest::WRITE, Mode::Level);
        let t = self
            .handle
            .insert_source(source, move |_, pipe, _| {
                if !write_some(pipe, data.as_bytes(), &mut written) {
                    return Ok(PostAction::Continue);
                }
                if let Some(t) = mine.get() {
                    live.borrow_mut().retain(|l| *l != t);
                }
                Ok(PostAction::Remove)
            })
            .map_err(|e| io::Error::other(e.error))?;
        token.set(Some(t));
        self.live.borrow_mut().push(t);
        Ok(())
    }

    /// Transfers not yet finished.
    pub fn pending(&self) -> usize {
        self.live.borrow().len()
    }
}

impl<D> Drop for Transfers<D> {
    fn drop(&mut self) {
        for t in self.live.take() {
            self.handle.remove(t);
        }
    }
}

/// The MIME type `QMimeData::text()` reads from a selection offer (QWaylandMimeData):
/// `text/plain;charset=utf-8`, else `text/plain`.
pub fn text_mime(types: &[String]) -> Option<&'static str> {
    ["text/plain;charset=utf-8", "text/plain"]
        .into_iter()
        .find(|m| types.iter().any(|t| t == m))
}

/// The paste waiting for its selection owner, read from the loop as
/// `gdk_clipboard_read_text_async` does: the pipe of a `receive` request is made non-blocking
/// and read whenever it is readable, until the owner closes its end, with no deadline. At most
/// one read is pending: starting another or `cancel` (the selection or the editor changed)
/// drops it and closes its pipe, so it delivers nothing.
pub struct PasteRead<D: 'static, R: Copy + 'static> {
    handle: LoopHandle<'static, D>,
    live: Rc<Cell<Option<(RegistrationToken, R)>>>,
}

impl<D, R: Copy> PasteRead<D, R> {
    pub fn new(handle: LoopHandle<'static, D>) -> Self {
        Self {
            handle,
            live: Rc::default(),
        }
    }

    /// Reads `pipe` for `req` from the loop; once the owner closed it, `done` gets everything it
    /// wrote (or the read error).
    pub fn start(
        &self,
        req: R,
        pipe: OwnedFd,
        done: impl FnOnce(&mut D, R, io::Result<Vec<u8>>) + 'static,
    ) -> io::Result<()> {
        self.cancel();
        set_nonblocking(&pipe)?;
        let live = self.live.clone();
        let mut got = Vec::new();
        let mut done = Some(done);
        let source = Generic::new(File::from(pipe), Interest::READ, Mode::Level);
        let t = self
            .handle
            .insert_source(source, move |_, pipe, d| {
                let Some(end) = read_some(pipe, &mut got) else {
                    return Ok(PostAction::Continue);
                };
                live.set(None);
                if let Some(f) = done.take() {
                    f(d, req, end.map(|()| std::mem::take(&mut got)));
                }
                Ok(PostAction::Remove)
            })
            .map_err(|e| io::Error::other(e.error))?;
        self.live.set(Some((t, req)));
        Ok(())
    }

    /// The paste still waiting for its data.
    pub fn pending(&self) -> Option<R> {
        self.live.get().map(|(_, r)| r)
    }

    /// Tells whether a read is pending, for code that has no way to this reader.
    pub fn busy(&self) -> impl Fn() -> bool + 'static {
        let live = self.live.clone();
        move || live.get().is_some()
    }

    /// Drops the pending read: its pipe is closed and it delivers nothing.
    pub fn cancel(&self) {
        if let Some((t, _)) = self.live.take() {
            self.handle.remove(t);
        }
    }
}

impl<D, R: Copy> Drop for PasteRead<D, R> {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Reads what the pipe holds; `Some` once the read is over (EOF or an error).
fn read_some(mut pipe: &File, got: &mut Vec<u8>) -> Option<io::Result<()>> {
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => return Some(Ok(())),
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return None,
            Err(e) => return Some(Err(e)),
        }
    }
}

/// Writes until the pipe is full; true once the transfer is over (all written or the reader
/// gone).
fn write_some(mut pipe: &File, data: &[u8], written: &mut usize) -> bool {
    while *written < data.len() {
        match pipe.write(&data[*written..]) {
            Ok(0) => return true,
            Ok(n) => *written += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return false,
            Err(_) => return true,
        }
    }
    true
}

fn set_nonblocking(fd: &OwnedFd) -> io::Result<()> {
    let fd = fd.as_raw_fd();
    // SAFETY: F_GETFL on a descriptor we own.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: F_SETFL on a descriptor we own, adding O_NONBLOCK to its flags.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use calloop::EventLoop;
    use std::io::Read;
    use std::os::fd::AsFd;
    use std::time::Duration;

    /// Larger than any pipe buffer (64 KiB by default).
    fn text() -> Arc<str> {
        (0..1 << 20)
            .map(|i| char::from(b'a' + (i % 26) as u8))
            .collect::<String>()
            .into()
    }

    /// Whether every write end of the pipe `reader` reads from is closed.
    fn hung_up(reader: &impl AsFd) -> bool {
        let mut p = libc::pollfd {
            fd: reader.as_fd().as_raw_fd(),
            events: 0,
            revents: 0,
        };
        // SAFETY: one valid pollfd, zero timeout.
        assert!(unsafe { libc::poll(&mut p, 1, 0) } >= 0);
        p.revents & libc::POLLHUP != 0
    }

    /// Blocks until every write end of the pipe `reader` reads from is closed. A process another
    /// test thread forks meanwhile holds a copy of every fd until it execs or exits (close-on-exec
    /// acts only at exec), so the hangup is an event to wait for, not a state to sample.
    fn wait_hung_up(reader: &impl AsFd) {
        let mut p = libc::pollfd {
            fd: reader.as_fd().as_raw_fd(),
            events: 0,
            revents: 0,
        };
        loop {
            // SAFETY: one valid pollfd, no timeout.
            if unsafe { libc::poll(&mut p, 1, -1) } < 0 {
                let e = io::Error::last_os_error();
                assert_eq!(e.kind(), io::ErrorKind::Interrupted, "poll: {e}");
                continue;
            }
            assert_ne!(p.revents & libc::POLLHUP, 0, "revents {:#x}", p.revents);
            return;
        }
    }

    /// Dispatches until every transfer is over, each turn sleeping until a pipe's event.
    fn finish(ev: &mut EventLoop<'static, ()>, sends: &Transfers<()>) {
        while sends.pending() > 0 {
            ev.dispatch(None, &mut ()).unwrap();
        }
    }

    fn spin(ev: &mut EventLoop<'static, ()>) {
        for _ in 0..8 {
            ev.dispatch(Some(Duration::ZERO), &mut ()).unwrap();
        }
    }

    /// A reader that holds its end open but never reads blocks nothing (no thread, not the
    /// loop), and cancelling the source closes its pipe.
    #[test]
    fn stalled_reader_blocks_nothing_and_cancel_closes_its_pipe() {
        let mut ev = EventLoop::<()>::try_new().unwrap();
        let (reader, writer) = io::pipe().unwrap();
        let sends = Transfers::new(ev.handle());
        sends.send(text(), writer.into()).unwrap();
        spin(&mut ev);
        assert_eq!(sends.pending(), 1);
        assert!(!hung_up(&reader));
        drop(sends);
        wait_hung_up(&reader);
    }

    /// A reader that reads gets the whole text, then EOF.
    #[test]
    fn reading_reader_gets_the_full_text() {
        let mut ev = EventLoop::<()>::try_new().unwrap();
        let (mut reader, writer) = io::pipe().unwrap();
        let sends = Transfers::new(ev.handle());
        let data = text();
        sends.send(data.clone(), writer.into()).unwrap();
        let got = std::thread::spawn(move || {
            let mut s = String::new();
            reader.read_to_string(&mut s).unwrap();
            s
        });
        finish(&mut ev, &sends);
        assert_eq!(got.join().unwrap(), *data);
    }

    /// A reader that closes its end mid-transfer ends it (EPIPE).
    #[test]
    fn closed_reader_ends_the_transfer() {
        let mut ev = EventLoop::<()>::try_new().unwrap();
        let (reader, writer) = io::pipe().unwrap();
        let sends = Transfers::new(ev.handle());
        sends.send(text(), writer.into()).unwrap();
        spin(&mut ev);
        assert_eq!(sends.pending(), 1);
        drop(reader);
        finish(&mut ev, &sends);
    }

    /// QMimeData::text prefers the UTF-8 type and takes plain text/plain otherwise.
    #[test]
    fn text_mime_follows_qmimedata_text() {
        let t = |v: &[&str]| text_mime(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            t(&["text/plain", "text/plain;charset=utf-8"]),
            Some("text/plain;charset=utf-8")
        );
        assert_eq!(t(&["UTF8_STRING", "text/plain"]), Some("text/plain"));
        assert_eq!(t(&["image/png", "UTF8_STRING"]), None);
    }
}
