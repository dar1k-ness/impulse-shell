//! The cursor on a compositor without wp_cursor_shape_v1: rusty-impulse sets it the way every Wayland
//! client did before that protocol, `wl_pointer.set_cursor` with a surface holding the image of
//! the XCURSOR theme. Sway has the protocol, so rusty-impulse talks to it through `proxy::Hide`, which
//! leaves the `wp_cursor_shape_manager_v1` global out of the registry.

use std::process::{Child, Command, Stdio};

use wayland_client::protocol::{wl_output, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

/// Two halves of a bar with different cursors.
const SHELL: &str = r#"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 400
        implicitHeight: 100
        MouseArea {
            width: 200; height: 100
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onContainsMouseChanged: if (containsMouse) console.log("TXT hand")
        }
        MouseArea {
            x: 200; width: 200; height: 100
            hoverEnabled: true
            cursorShape: Qt.IBeamCursor
            onContainsMouseChanged: if (containsMouse) console.log("TXT ibeam")
        }
    }
}
"#;

#[derive(Default)]
struct G {
    seat: Option<wl_seat::WlSeat>,
    vptr: Option<ZwlrVirtualPointerManagerV1>,
    output: Option<wl_output::WlOutput>,
    mode: (u32, u32),
}

impl Dispatch<wl_registry::WlRegistry, ()> for G {
    fn event(
        g: &mut Self,
        reg: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = ev
        {
            match interface.as_str() {
                "wl_seat" if g.seat.is_none() => g.seat = Some(reg.bind(name, 1, qh, ())),
                "zwlr_virtual_pointer_manager_v1" => {
                    g.vptr = Some(reg.bind(name, version.min(2), qh, ()))
                }
                "wl_output" if g.output.is_none() => g.output = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for G {
    fn event(
        g: &mut Self,
        _: &wl_output::WlOutput,
        ev: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode {
            flags,
            width,
            height,
            ..
        } = ev
            && flags
                .into_result()
                .is_ok_and(|f| f.contains(wl_output::Mode::Current))
        {
            g.mode = (width as u32, height as u32);
        }
    }
}

delegate_noop!(G: ignore wl_seat::WlSeat);
delegate_noop!(G: ZwlrVirtualPointerManagerV1);
delegate_noop!(G: ZwlrVirtualPointerV1);

struct Shell(Child);

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The shell's log up to its `n`th `TXT` line, once it printed that many; panics if it exits
/// first. `WAYLAND_DEBUG` logs a request when it is made, so the requests of the turns before
/// that line are all above it.
fn until_text(log: &testenv::Log, n: usize, what: &str) -> String {
    let texts = |s: &str| s.matches("TXT ").count();
    if let Err(s) = log.wait(|s| texts(s) >= n) {
        panic!("the shell exited before {what}:\n{s}");
    }
    let s = log.text().replace('@', "#");
    let end = s.match_indices("TXT ").nth(n - 1).unwrap().0;
    s[..end].to_string()
}

/// `(surface, hotspot)` of every `wl_pointer.set_cursor` request in `log`.
fn set_cursors(log: &str) -> Vec<(&str, &str)> {
    log.lines()
        .filter(|l| l.contains("-> wl_pointer#"))
        .filter_map(|l| l.split_once(".set_cursor(")?.1.strip_suffix(')'))
        .filter_map(|a| a.split_once(", ")?.1.split_once(", "))
        .collect()
}

/// The buffers attached to `surface`, in order.
fn attached<'a>(log: &'a str, surface: &str) -> Vec<&'a str> {
    let req = format!("-> {surface}.attach(");
    log.lines()
        .filter_map(|l| l.split_once(&req)?.1.split(',').next())
        .collect()
}

/// Before the fix rusty-impulse set a cursor only through wp_cursor_shape_v1, so without the protocol
/// it sent nothing after `wl_pointer.enter` and the compositor kept whatever image the last
/// client left.
#[test]
fn cursor_is_set_from_the_theme_without_the_shape_protocol() {
    let Ok(conn) = Connection::connect_to_env() else {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    };
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut g = G::default();
    q.roundtrip(&mut g).unwrap();
    q.roundtrip(&mut g).unwrap();
    let (Some(seat), Some(vptr)) = (g.seat.clone(), g.vptr.clone()) else {
        testenv::unavailable("compositor lacks zwlr_virtual_pointer_v1");
        return;
    };
    let ptr = match &g.output {
        Some(o) if vptr.version() >= 2 => {
            vptr.create_virtual_pointer_with_output(Some(&seat), Some(o), &qh, ())
        }
        _ => vptr.create_virtual_pointer(Some(&seat), &qh, ()),
    };
    let (w, h) = g.mode;
    assert!(w >= 400 && h >= 300, "first output mode {:?}", g.mode);
    let mut time = 0;
    let mut move_to = |x: u32, y: u32| {
        ptr.motion_absolute(time, x, y, w, h);
        ptr.frame();
        time += 1;
        q.roundtrip(&mut g).unwrap();
    };
    // The pointer starts below the bar and enters it once the bar is on the output.
    move_to(50, h - 50);

    let px = proxy::Hide::start(proxy::upstream(), "wp_cursor_shape_manager_v1");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
    let xdg = dir.path().join("xdg");
    let (r, out) = std::io::pipe().unwrap();
    let sh = Shell(
        Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("WAYLAND_DISPLAY", &px.path)
            .env_remove("XCURSOR_THEME")
            .env_remove("XCURSOR_SIZE")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdout(Stdio::null())
            .stderr(out)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let log = testenv::Log::capture(r);
    if let Err(s) = log.wait(|s| s.contains(".enter(wl_output")) {
        panic!("the shell exited before its bar was on the output:\n{s}");
    }
    move_to(50, 50);
    until_text(&log, 1, "the pointer entered the hand half");
    move_to(250, 50);
    // The turn that printed the first line set the hand; it is over once the second is printed.
    let s = until_text(&log, 2, "the pointer entered the I-beam half");
    assert!(
        !s.contains("wp_cursor_shape_manager_v1"),
        "the proxy did not hide the cursor-shape global"
    );
    let hand = set_cursors(&s);
    assert_eq!(
        hand.len(),
        1,
        "wl_pointer.set_cursor requests for the hand: {hand:?}"
    );
    let surface = hand[0].0;
    assert!(
        surface.starts_with("wl_surface#"),
        "the cursor was set without a surface: {hand:?}"
    );
    move_to(50, 50);
    let s = until_text(&log, 3, "the pointer entered the hand half again");
    let both = set_cursors(&s);
    assert_eq!(
        both.len(),
        2,
        "wl_pointer.set_cursor requests for the hand and the I-beam: {both:?}"
    );
    assert_eq!(both[1].0, surface, "the cursor surface changed");
    let images = attached(&s, surface);
    assert!(
        images.len() == 2 && images[0] != images[1] && images[0].starts_with("wl_buffer#"),
        "the hand and the I-beam are not two images of the theme: {images:?}"
    );
    drop(sh);
}

/// A Wayland proxy between rusty-impulse and sway that forwards every message and fd unchanged, except
/// the `wl_registry.global` events of one interface, which it drops.
mod proxy {
    use std::collections::HashSet;
    use std::os::fd::{AsRawFd, RawFd};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};

    const WL_DISPLAY: u32 = 1;

    pub struct Hide {
        pub path: PathBuf,
        _dir: tempfile::TempDir,
    }

    impl Hide {
        /// Listens on a fresh socket and connects each client to the compositor at `upstream`,
        /// which then has no global of `interface`.
        pub fn start(upstream: PathBuf, interface: &'static str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("wayland-proxy");
            let listener = UnixListener::bind(&path).unwrap();
            std::thread::spawn(move || {
                for client in listener.incoming().flatten() {
                    let server = UnixStream::connect(&upstream).unwrap();
                    std::thread::spawn(move || pump(interface, client, server));
                }
            });
            Hide { path, _dir: dir }
        }
    }

    fn header(m: &[u8]) -> (u32, u16) {
        let w = |i: usize| u32::from_ne_bytes(m[i..i + 4].try_into().unwrap());
        (w(0), (w(4) & 0xffff) as u16)
    }

    fn arg(m: &[u8], i: usize) -> u32 {
        u32::from_ne_bytes(m[8 + 4 * i..12 + 4 * i].try_into().unwrap())
    }

    /// Whole messages at the front of `buf`.
    fn split(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
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

    fn recv(fd: RawFd, buf: &mut [u8], fds: &mut Vec<RawFd>) -> isize {
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
    fn send(fd: RawFd, mut bytes: &[u8], fds: &mut Vec<RawFd>) {
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

    /// Copies both directions of one connection, without the globals of `hidden`.
    fn pump(hidden: &str, client: UnixStream, server: UnixStream) {
        let (cfd, sfd) = (client.as_raw_fd(), server.as_raw_fd());
        let (mut up, mut down) = (Vec::new(), Vec::new());
        let (mut up_fds, mut down_fds) = (Vec::new(), Vec::new());
        let mut buf = vec![0u8; 1 << 16];
        let mut registries = HashSet::new();
        loop {
            let mut pfd = [cfd, sfd].map(|fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
            // SAFETY: two valid pollfds.
            if unsafe { libc::poll(pfd.as_mut_ptr(), 2, -1) } < 0 {
                return;
            }
            if pfd[0].revents != 0 {
                let n = recv(cfd, &mut buf, &mut up_fds);
                if n <= 0 {
                    return;
                }
                up.extend_from_slice(&buf[..n as usize]);
                let mut out = Vec::new();
                for m in split(&mut up) {
                    let (obj, op) = header(&m);
                    if obj == WL_DISPLAY && op == 1 {
                        registries.insert(arg(&m, 0));
                    }
                    out.extend_from_slice(&m);
                }
                send(sfd, &out, &mut up_fds);
            }
            if pfd[1].revents != 0 {
                let n = recv(sfd, &mut buf, &mut down_fds);
                if n <= 0 {
                    return;
                }
                down.extend_from_slice(&buf[..n as usize]);
                let mut out = Vec::new();
                for m in split(&mut down) {
                    let (obj, op) = header(&m);
                    if obj == WL_DISPLAY && op == 1 {
                        // delete_id: the id may now name another object.
                        registries.remove(&arg(&m, 0));
                    }
                    // global(name, interface string, version)
                    if registries.contains(&obj) && op == 0 && m.len() >= 16 {
                        let len = arg(&m, 1) as usize;
                        if m.get(16..16 + len.saturating_sub(1)) == Some(hidden.as_bytes()) {
                            continue;
                        }
                    }
                    out.extend_from_slice(&m);
                }
                send(cfd, &out, &mut down_fds);
            }
        }
    }

    /// The compositor's socket, from WAYLAND_DISPLAY.
    pub fn upstream() -> PathBuf {
        let d = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").unwrap());
        if d.is_absolute() {
            return d;
        }
        Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join(d)
    }
}
