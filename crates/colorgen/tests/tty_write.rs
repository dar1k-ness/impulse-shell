//! `Live::write_tty` on a real terminal (a pty this test holds the other end of): the colours
//! arrive whole when the terminal takes no more output for a while (`EAGAIN`), and an output
//! that never takes any does not hold the caller.
use colorgen::live::{Live, write_whole};
use colorgen::sys::Sys;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::time::Duration;

/// A terminal: `master` is the emulator's end, `slave` an open fd of the terminal at `path`.
/// Raw (what is written comes out as it is), and neither end blocks.
struct Pty {
    master: OwnedFd,
    slave: OwnedFd,
    path: PathBuf,
}

fn pty() -> Pty {
    use std::os::unix::fs::OpenOptionsExt;
    // SAFETY: posix_openpt(3) returns a new fd or -1.
    let m = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK) };
    assert!(m >= 0, "{}", std::io::Error::last_os_error());
    // SAFETY: `m` is a freshly opened fd owned by nobody else.
    let master = unsafe { OwnedFd::from_raw_fd(m) };
    let mut name = [0u8; 64];
    // SAFETY: grantpt(3) on the pty master just opened.
    let granted = unsafe { libc::grantpt(m) };
    // SAFETY: unlockpt(3) on the same.
    let unlocked = unsafe { libc::unlockpt(m) };
    // SAFETY: ptsname_r(3) on the same; the name goes into `name`, of the length given.
    let named = unsafe { libc::ptsname_r(m, name.as_mut_ptr().cast(), name.len()) };
    assert_eq!(
        (granted, unlocked, named),
        (0, 0, 0),
        "{}",
        std::io::Error::last_os_error()
    );
    let end = name.iter().position(|&b| b == 0).unwrap();
    let path = PathBuf::from(std::str::from_utf8(&name[..end]).unwrap());
    let slave: OwnedFd = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(&path)
        .unwrap()
        .into();
    // SAFETY: termios is plain integers and arrays of them: all zeroes is a value of it.
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: tcgetattr(3) on the terminal's fd fills the termios this function owns.
    let got = unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut t) };
    assert_eq!(got, 0, "{}", std::io::Error::last_os_error());
    // SAFETY: cfmakeraw(3) changes only that termios.
    unsafe { libc::cfmakeraw(&mut t) };
    // SAFETY: tcsetattr(3) on the terminal's fd reads that termios.
    let set = unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &t) };
    assert_eq!(set, 0, "{}", std::io::Error::last_os_error());
    Pty {
        master,
        slave,
        path,
    }
}

impl Pty {
    /// Writes to the terminal until it takes no more: how many bytes it took.
    fn fill(&self) -> usize {
        let mut n = 0;
        while let Some(k) = write_now(&self.slave, &[b'.'; 4096]) {
            n += k;
        }
        n
    }

    /// What the emulator's end has to read right now.
    fn drain(&self, into: &mut Vec<u8>) {
        let mut buf = [0u8; 65536];
        loop {
            // SAFETY: read(2) into `buf`, of the length given, from an fd this test owns.
            let k =
                unsafe { libc::read(self.master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if k <= 0 {
                let e = std::io::Error::last_os_error();
                assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock, "{e}");
                return;
            }
            into.extend_from_slice(&buf[..k as usize]);
        }
    }
}

/// One write(2) that does not block: the bytes taken, `None` when it would block.
fn write_now(fd: &OwnedFd, data: &[u8]) -> Option<usize> {
    // SAFETY: write(2) of `data`, of the length given, to an fd this test owns.
    let k = unsafe { libc::write(fd.as_raw_fd(), data.as_ptr().cast(), data.len()) };
    if k < 0 {
        let e = std::io::Error::last_os_error();
        assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock, "{e}");
        return None;
    }
    Some(k as usize)
}

#[test]
fn colours_arrive_whole_at_a_terminal_that_was_full() {
    let p = pty();
    // Nobody has read the terminal's output: it is full, the next write gets `EAGAIN`.
    let filler = p.fill();
    assert!(filler > 0);
    // Several times what the terminal holds: it fills up again and again while this is written.
    let data: Vec<u8> = (0..8 * filler).map(|i| b'a' + (i % 23) as u8).collect();

    let (done, writing) = std::io::pipe().unwrap();
    let (path, colours) = (p.path.clone(), data.clone());
    let writer = std::thread::spawn(move || {
        Live.write_tty(&path, &colours);
        drop(writing);
    });
    // The emulator reads on until the writer has returned.
    let mut got = Vec::new();
    loop {
        let mut polls = [p.master.as_fd(), done.as_fd()].map(|fd| libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        });
        // SAFETY: `polls` is a valid array of two pollfds on fds that outlive the call.
        let n = unsafe { libc::poll(polls.as_mut_ptr(), 2, -1) };
        assert!(n > 0, "{}", std::io::Error::last_os_error());
        p.drain(&mut got);
        if polls[1].revents != 0 {
            break;
        }
    }
    writer.join().unwrap();
    p.drain(&mut got);
    assert_eq!(
        got.len() - filler,
        data.len(),
        "the terminal got a part of the colours"
    );
    assert!(got[filler..] == data[..], "the colours changed on the way");
}

/// A terminal whose output is stopped, or that nobody reads, takes nothing however long the
/// writer waits. A full pipe stands for it: unlike a pty, it has no part that empties by itself.
#[test]
fn an_output_that_takes_nothing_does_not_hold_the_writer() {
    let (mut r, w) = std::io::pipe().unwrap();
    let w = OwnedFd::from(w);
    // SAFETY: fcntl(2) sets a flag of an fd this test owns.
    let set = unsafe { libc::fcntl(w.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
    assert_eq!(set, 0, "{}", std::io::Error::last_os_error());
    let mut filler = 0;
    // A pipe with no room for a large write may still have room for a small one.
    for chunk in [&[b'.'; 4096][..], &[b'.'; 1]] {
        while let Some(k) = write_now(&w, chunk) {
            filler += k;
        }
    }
    assert!(!write_whole(
        w.as_fd(),
        b"\x1b]4;0;#000000\x1b\\",
        Duration::from_millis(1)
    ));
    drop(w);
    let mut got = Vec::new();
    std::io::Read::read_to_end(&mut r, &mut got).unwrap();
    assert_eq!(got.len(), filler, "wrote a part of the colours");
}
