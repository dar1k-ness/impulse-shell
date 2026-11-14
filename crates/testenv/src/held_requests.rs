//! A Wayland proxy that can hold a client's requests back, for the order a test cannot get by
//! timing: the compositor does something (removes an output) before it reads what the client
//! had already sent. It can also hold the compositor's events back from some of a client's
//! connections: one connection of the client hears of something before another does.

use std::net::Shutdown;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Default)]
struct Gate {
    holding: bool,
    /// The events of the connections that asked for the first global and not for the second
    /// are held.
    holding_events: Option<(&'static str, &'static str)>,
    /// Each connection's client and compositor ends.
    conns: Vec<(UnixStream, UnixStream)>,
}

#[derive(Default)]
struct Shared {
    gate: Mutex<Gate>,
    opened: Condvar,
    /// Every byte the compositor has sent, all connections in the order read.
    events: Mutex<Vec<u8>>,
}

/// A socket (`path`, a WAYLAND_DISPLAY value) whose clients talk to the compositor at
/// `upstream`: every byte and fd passes unchanged, the compositor's events at once, the
/// clients' requests unless they are held.
pub struct HeldRequests {
    path: PathBuf,
    shared: Arc<Shared>,
    _dir: tempfile::TempDir,
}

impl HeldRequests {
    /// The compositor of this process's WAYLAND_DISPLAY behind a fresh socket.
    pub fn start() -> Self {
        let d = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").expect("WAYLAND_DISPLAY"));
        let upstream = if d.is_absolute() {
            d
        } else {
            Path::new(&std::env::var_os("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR")).join(d)
        };
        Self::start_at(upstream)
    }

    /// The compositor listening at the socket `upstream` behind a fresh socket.
    pub fn start_at(upstream: PathBuf) -> Self {
        let dir = tempfile::tempdir().expect("proxy dir");
        let path = dir.path().join("wayland-proxy");
        let listener = UnixListener::bind(&path).expect("bind the proxy socket");
        let shared = Arc::new(Shared::default());
        let sh = shared.clone();
        std::thread::spawn(move || {
            for client in listener.incoming().flatten() {
                let server = UnixStream::connect(&upstream).expect("connect to the compositor");
                let ends = (client.try_clone().unwrap(), server.try_clone().unwrap());
                sh.gate.lock().unwrap().conns.push(ends);
                let (c, s) = (client.try_clone().unwrap(), server.try_clone().unwrap());
                let asked = Arc::new(Asked::default());
                let (sh2, asked2) = (sh.clone(), asked.clone());
                std::thread::spawn(move || events(&sh2, &asked2, &s, &c));
                let sh = sh.clone();
                std::thread::spawn(move || requests(&sh, &asked, &client, &server));
            }
        });
        HeldRequests {
            path,
            shared,
            _dir: dir,
        }
    }

    /// The socket clients connect to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Requests the compositor has not been handed yet stay here from now on.
    pub fn hold(&self) {
        self.shared.gate.lock().unwrap().holding = true;
    }

    /// The compositor's events stay here from now on for every connection that has asked for
    /// a global whose interface name contains `interface` and for none whose name contains
    /// `without` (both of [`WATCHED`]): the client's other connections go on hearing the
    /// compositor.
    pub fn hold_events_of(&self, interface: &'static str, without: &'static str) {
        for i in [interface, without] {
            assert!(WATCHED.contains(&i), "{i} is not watched");
        }
        self.shared.gate.lock().unwrap().holding_events = Some((interface, without));
    }

    /// Hands the held events on, in order, and stops holding events.
    pub fn release_events(&self) {
        self.shared.gate.lock().unwrap().holding_events = None;
        self.shared.opened.notify_all();
    }

    /// Ends every connection at both ends, as a compositor that dies does: what is held is
    /// never handed over.
    pub fn close(&self) {
        let g = self.shared.gate.lock().unwrap();
        for (client, server) in &g.conns {
            let _ = client.shutdown(Shutdown::Both);
            let _ = server.shutdown(Shutdown::Both);
        }
    }

    /// Hands the compositor every request the clients have written so far, in order, and
    /// stops holding.
    pub fn release(&self) {
        let mut g = self.shared.gate.lock().unwrap();
        g.holding = false;
        for (client, server) in &g.conns {
            while forward(client, server, libc::MSG_DONTWAIT, |_| {}) > 0 {}
        }
        self.shared.opened.notify_all();
    }

    /// Hands the compositor every request the clients have written so far, in order, and
    /// goes on holding: what a client writes in answer to the compositor's answer stays here.
    pub fn pass(&self) {
        let g = self.shared.gate.lock().unwrap();
        assert!(g.holding, "pass() without hold()");
        for (client, server) in &g.conns {
            while forward(client, server, libc::MSG_DONTWAIT, |_| {}) > 0 {}
        }
    }

    /// What the compositor has sent so far, as text: a client that has seen an event (or the
    /// end of its stream) finds here everything sent to it before that.
    pub fn events(&self) -> String {
        String::from_utf8_lossy(&self.shared.events.lock().unwrap()).into_owned()
    }
}

/// The interface names [`HeldRequests::hold_events_of`] can tell connections by.
pub const WATCHED: [&str; 2] = ["data_control_manager_v1", "wl_compositor"];

/// Which of [`WATCHED`] a connection's requests have named (`wl_registry.bind` carries the
/// interface name as text).
#[derive(Default)]
struct Asked([AtomicBool; WATCHED.len()]);

impl Asked {
    /// Looks at the requests passed on last. A name split between two reads is not found:
    /// a client binds its globals in its first few hundred bytes.
    fn look(&self, bytes: &[u8]) {
        for (name, asked) in WATCHED.iter().zip(&self.0) {
            if bytes.windows(name.len()).any(|w| w == name.as_bytes()) {
                asked.store(true, Ordering::Release);
            }
        }
    }

    fn asked(&self, interface: &str) -> bool {
        WATCHED
            .iter()
            .zip(&self.0)
            .any(|(name, asked)| *name == interface && asked.load(Ordering::Acquire))
    }

    fn held(&self, gate: &Gate) -> bool {
        gate.holding_events
            .is_some_and(|(with, without)| self.asked(with) && !self.asked(without))
    }
}

/// The compositor's events, as they come unless this connection's are held, each added to
/// `shared.events` as it is passed on.
fn events(shared: &Shared, asked: &Asked, server: &UnixStream, client: &UnixStream) {
    loop {
        let mut pfd = libc::pollfd {
            fd: server.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        if unsafe { libc::poll(&mut pfd, 1, -1) } < 0 {
            break;
        }
        let g = shared.gate.lock().unwrap();
        let _open = shared.opened.wait_while(g, |g| asked.held(g)).unwrap();
        let record = |b: &[u8]| shared.events.lock().unwrap().extend_from_slice(b);
        if forward(server, client, 0, record) <= 0 {
            break;
        }
    }
    let _ = client.shutdown(Shutdown::Both);
}

/// One connection's requests: each batch is passed on under the gate's lock, so `hold` and
/// `release` never run in the middle of one.
fn requests(shared: &Shared, asked: &Asked, client: &UnixStream, server: &UnixStream) {
    loop {
        let mut pfd = libc::pollfd {
            fd: client.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        if unsafe { libc::poll(&mut pfd, 1, -1) } < 0 {
            break;
        }
        let g = shared.gate.lock().unwrap();
        let _open = shared.opened.wait_while(g, |g| g.holding).unwrap();
        // Nothing left to read (EAGAIN) when `release` passed it on already.
        if forward(client, server, libc::MSG_DONTWAIT, |b| asked.look(b)) == 0 {
            break;
        }
    }
    let _ = server.shutdown(Shutdown::Both);
}

/// Passes on what one read of `from` returns, fds included, after showing the bytes to
/// `look`. The bytes read: 0 at the end of the stream or on an error, -1 when a non-blocking
/// read has nothing.
fn forward(
    from: &UnixStream,
    to: &UnixStream,
    flags: libc::c_int,
    look: impl FnOnce(&[u8]),
) -> isize {
    let mut buf = [0u8; 1 << 16];
    let mut fds = Vec::new();
    let n = recv(from.as_raw_fd(), &mut buf, &mut fds, flags);
    if n > 0 {
        look(&buf[..n as usize]);
        send(to.as_raw_fd(), &buf[..n as usize], &mut fds);
        return n;
    }
    let again = n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock;
    if again { -1 } else { 0 }
}

pub(crate) fn recv(fd: RawFd, buf: &mut [u8], fds: &mut Vec<RawFd>, flags: libc::c_int) -> isize {
    let mut cmsg = [0u64; 32];
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    // SAFETY: msghdr is plain data; every pointer in it outlives the call.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&cmsg) as _;
    // SAFETY: as above; the kernel fills at most the lengths given.
    let n = unsafe { libc::recvmsg(fd, &mut msg, libc::MSG_CMSG_CLOEXEC | flags) };
    if n > 0 {
        // SAFETY: walking the control messages the kernel just wrote into `cmsg`.
        let mut c = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        while !c.is_null() {
            // SAFETY: a non-null header CMSG_FIRSTHDR/CMSG_NXTHDR returned lies within `cmsg`.
            let h = unsafe { &*c };
            if h.cmsg_level == libc::SOL_SOCKET && h.cmsg_type == libc::SCM_RIGHTS {
                // SAFETY: CMSG_LEN only computes a length.
                let len = h.cmsg_len as usize - unsafe { libc::CMSG_LEN(0) } as usize;
                // SAFETY: `c` is a valid header; its `len` data bytes follow it in `cmsg`.
                let p = unsafe { libc::CMSG_DATA(c) }.cast::<RawFd>();
                for i in 0..len / 4 {
                    // SAFETY: fd `i` of the `len` bytes at `p`, read unaligned.
                    fds.push(unsafe { p.wrapping_add(i).read_unaligned() });
                }
            }
            // SAFETY: `c` is a header of `msg`'s control buffer.
            c = unsafe { libc::CMSG_NXTHDR(&msg, c) };
        }
    }
    n
}

/// Sends `bytes` with `fds` (passed on and closed here) attached to the first chunk.
pub(crate) fn send(fd: RawFd, mut bytes: &[u8], fds: &mut Vec<RawFd>) {
    while !bytes.is_empty() {
        let mut cmsg = [0u64; 32];
        let mut iov = libc::iovec {
            iov_base: bytes.as_ptr() as *mut _,
            iov_len: bytes.len(),
        };
        // SAFETY: as in `recv`.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        if !fds.is_empty() {
            let len = std::mem::size_of_val(fds.as_slice()) as u32;
            msg.msg_control = cmsg.as_mut_ptr().cast();
            // SAFETY: CMSG_SPACE of at most 28 fds fits `cmsg`.
            msg.msg_controllen = unsafe { libc::CMSG_SPACE(len) } as _;
            // SAFETY: `msg_controllen` is CMSG_SPACE(len) of `cmsg`, room for one header.
            let c = unsafe { libc::CMSG_FIRSTHDR(&msg) };
            // SAFETY: CMSG_FIRSTHDR of that buffer is non-null and lies within `cmsg`.
            let h = unsafe { &mut *c };
            h.cmsg_level = libc::SOL_SOCKET;
            h.cmsg_type = libc::SCM_RIGHTS;
            // SAFETY: CMSG_LEN only computes a length.
            h.cmsg_len = unsafe { libc::CMSG_LEN(len) } as _;
            // SAFETY: `c` is a valid header; its data follows it within `cmsg`.
            let data = unsafe { libc::CMSG_DATA(c) }.cast::<RawFd>();
            // SAFETY: the `len` bytes of fds fit the data CMSG_SPACE(len) reserved, which is
            // aligned for them (`cmsg` is u64s, headers and data are CMSG_ALIGNed).
            unsafe { std::ptr::copy_nonoverlapping(fds.as_ptr(), data, fds.len()) };
        }
        // SAFETY: as in `recv`.
        let n = unsafe { libc::sendmsg(fd, &msg, libc::MSG_NOSIGNAL) };
        for f in fds.drain(..) {
            // SAFETY: our received copy of an fd, passed on or not.
            unsafe { libc::close(f) };
        }
        if n < 0 {
            return;
        }
        bytes = &bytes[n as usize..];
    }
}
