//! The Wayland wire format, for a test's proxy between a client and its compositor: whole
//! messages and the fds that travel with them.

use std::os::fd::RawFd;

/// The object id and the opcode of the message `m`.
pub fn header(m: &[u8]) -> (u32, u16) {
    let w = |i: usize| u32::from_ne_bytes(m[i..i + 4].try_into().unwrap());
    (w(0), (w(4) & 0xffff) as u16)
}

/// Word `i` of the arguments of `m`.
pub fn arg(m: &[u8], i: usize) -> u32 {
    u32::from_ne_bytes(m[8 + 4 * i..12 + 4 * i].try_into().unwrap())
}

/// Whole messages at the front of `buf`.
pub fn split(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while buf.len() >= 8 {
        let size = (u32::from_ne_bytes(buf[4..8].try_into().unwrap()) >> 16) as usize;
        if size < 8 || buf.len() < size {
            break;
        }
        out.push(buf.drain(..size).collect());
    }
    out
}

/// Reads what `fd` has into `buf` and the fds that came with it onto `fds`; the count of
/// bytes, 0 at the end of the stream, negative on an error.
pub fn recv(fd: RawFd, buf: &mut [u8], fds: &mut Vec<RawFd>) -> isize {
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
    let n = unsafe { libc::recvmsg(fd, &mut msg, libc::MSG_CMSG_CLOEXEC) };
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
pub fn send(fd: RawFd, mut bytes: &[u8], fds: &mut Vec<RawFd>) {
    while !bytes.is_empty() || !fds.is_empty() {
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
        if n < 0 {
            return;
        }
        for f in fds.drain(..) {
            // SAFETY: our received copy of an fd now passed on.
            unsafe { libc::close(f) };
        }
        bytes = &bytes[n as usize..];
    }
}
