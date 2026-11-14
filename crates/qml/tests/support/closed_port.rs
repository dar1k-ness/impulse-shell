//! A port of 127.0.0.1 where nothing listens, for as long as a test needs it closed.
#![allow(dead_code)]

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

/// A port of 127.0.0.1 where a connection is refused until this is dropped: its socket is
/// bound to the port and never listens, so the system gives the port to no other socket,
/// of this process or another. A port that was only found free and let go can be taken by
/// a server of another test before the test asks it.
pub struct ClosedPort {
    socket: OwnedFd,
    port: u16,
}

impl ClosedPort {
    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://127.0.0.1:<port>`, without a path.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

pub fn closed_port() -> ClosedPort {
    // SAFETY: socket(2) takes only numbers.
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    assert!(fd >= 0, "socket: {}", std::io::Error::last_os_error());
    // SAFETY: `fd` is the new descriptor socket returned; nothing else owns it.
    let socket = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut addr = libc::sockaddr_in {
        sin_family: libc::AF_INET as libc::sa_family_t,
        sin_port: 0,
        sin_addr: libc::in_addr {
            s_addr: u32::from(std::net::Ipv4Addr::LOCALHOST).to_be(),
        },
        sin_zero: [0; 8],
    };
    let mut len = size_of::<libc::sockaddr_in>() as libc::socklen_t;
    // SAFETY: bind(2) reads the `len` bytes of the one address it is given.
    let r = unsafe { libc::bind(socket.as_raw_fd(), (&raw const addr).cast(), len) };
    assert_eq!(r, 0, "bind: {}", std::io::Error::last_os_error());
    // SAFETY: getsockname(2) writes at most `len` bytes to the one address it is given.
    let r = unsafe { libc::getsockname(socket.as_raw_fd(), (&raw mut addr).cast(), &raw mut len) };
    assert_eq!(r, 0, "getsockname: {}", std::io::Error::last_os_error());
    ClosedPort {
        socket,
        port: u16::from_be(addr.sin_port),
    }
}
