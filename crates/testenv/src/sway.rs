//! A headless sway of a test's own. The shared one (WAYLAND_DISPLAY, tools/testenv/entrypoint)
//! serves every test binary at once: its screen, seat and outputs are what the other tests
//! change and capture meanwhile, so a test that reads them runs its own.

use std::io::{BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};

/// A headless sway with one 1920x1080 output (HEADLESS-1) in a runtime directory of its own;
/// killed on drop.
pub struct Sway {
    child: Child,
    /// Kept open: sway's `exec` children write to it.
    _stdout: ChildStdout,
    dir: tempfile::TempDir,
    /// Absolute path of its Wayland socket.
    display: std::path::PathBuf,
}

impl Sway {
    /// Returns once sway runs its config's `exec` commands, which it defers until its backend
    /// started (the output exists) and its socket listens; the one here prints the socket name
    /// sway gives its children as WAYLAND_DISPLAY. Panics with sway's log if it exits first.
    pub fn start() -> Self {
        Self::start_with(None)
    }

    /// [`Sway::start`] that says on this process's stderr what it waits for and copies sway's
    /// log there as it arrives (see [`crate::Log::capture_shown`]): for a test whose hang has
    /// to tell whether sway, its client or the capture stood still.
    pub fn start_shown() -> Self {
        Self::start_with(Some("sway"))
    }

    fn start_with(show: Option<&'static str>) -> Self {
        let dir = tempfile::tempdir().expect("sway runtime dir");
        let conf = dir.path().join("sway.conf");
        std::fs::write(
            &conf,
            "output HEADLESS-1 resolution 1920x1080\nexec echo \"$WAYLAND_DISPLAY\"\n",
        )
        .expect("write sway.conf");
        let (r, w) = std::io::pipe().expect("pipe for sway's log");
        let mut child = Command::new("sway")
            .arg("-c")
            .arg(&conf)
            .env("XDG_RUNTIME_DIR", dir.path())
            .env("WLR_BACKENDS", "headless")
            .env("WLR_HEADLESS_OUTPUTS", "1")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .env("WLR_RENDERER", "pixman")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("WAYLAND_SOCKET")
            .env_remove("SWAYSOCK")
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(w)
            .spawn()
            .expect("spawn sway");
        let log = match show {
            Some(name) => {
                elog::eprintln!("waiting for sway to start");
                crate::Log::capture_shown(name, r)
            }
            None => crate::Log::capture(r),
        };
        let mut stdout = std::io::BufReader::new(child.stdout.take().expect("sway stdout"));
        let mut name = String::new();
        // EOF (an empty line) once sway exits before it ran the `exec`.
        let _ = stdout.read_line(&mut name);
        let name = name.trim();
        if name.is_empty() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("sway exited before it started:\n{}", log.until_closed());
        }
        let display = dir.path().join(name);
        Sway {
            child,
            _stdout: stdout.into_inner(),
            dir,
            display,
        }
    }

    /// Absolute path of its Wayland socket (a WAYLAND_DISPLAY value).
    pub fn display(&self) -> &Path {
        &self.display
    }

    /// Its process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Its XDG_RUNTIME_DIR.
    pub fn runtime_dir(&self) -> &Path {
        self.dir.path()
    }

    /// Its IPC socket (sway-ipc(7), a SWAYSOCK value for swaymsg), which listens once `start`
    /// returned.
    pub fn ipc_socket(&self) -> std::path::PathBuf {
        std::fs::read_dir(self.dir.path())
            .expect("read sway's runtime dir")
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("sway-ipc.") && n.ends_with(".sock"))
            })
            .expect("sway's IPC socket")
    }

    /// Points `cmd` at this sway, and at no other compositor.
    pub fn env<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        cmd.env("WAYLAND_DISPLAY", &self.display)
            .env("XDG_RUNTIME_DIR", self.dir.path())
            .env_remove("WAYLAND_SOCKET")
            .env_remove("SWAYSOCK")
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
    }
}

impl Drop for Sway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A subscription to sway's window events (new, title, close, ...) on its IPC socket
/// (sway-ipc(7)): each event wakes [`SwayWindows::wait_until`].
pub struct SwayWindows {
    stream: UnixStream,
    /// Events received so far, and whether sway closed the socket.
    events: Arc<(Mutex<(u64, bool)>, Condvar)>,
}

const IPC_MAGIC: &[u8; 6] = b"i3-ipc";
const IPC_SUBSCRIBE: u32 = 2;

/// One sway IPC message: its type and payload.
fn read_msg(r: &mut impl Read) -> std::io::Result<(u32, Vec<u8>)> {
    let mut head = [0u8; 14];
    r.read_exact(&mut head)?;
    assert_eq!(&head[..6], IPC_MAGIC, "not a sway IPC message");
    let len = u32::from_ne_bytes(head[6..10].try_into().unwrap());
    let ty = u32::from_ne_bytes(head[10..14].try_into().unwrap());
    let mut body = vec![0; len as usize];
    r.read_exact(&mut body)?;
    Ok((ty, body))
}

impl SwayWindows {
    /// Subscribes on the sway IPC socket `sock` and returns once sway confirmed it.
    pub fn subscribe(sock: &Path) -> Self {
        let mut stream = UnixStream::connect(sock)
            .unwrap_or_else(|e| panic!("connect to sway IPC {}: {e}", sock.display()));
        let payload = br#"["window"]"#;
        let mut msg = IPC_MAGIC.to_vec();
        msg.extend((payload.len() as u32).to_ne_bytes());
        msg.extend(IPC_SUBSCRIBE.to_ne_bytes());
        msg.extend(payload);
        stream.write_all(&msg).expect("send the sway subscribe");
        let (ty, body) = read_msg(&mut stream).expect("read the sway subscribe reply");
        let reply = String::from_utf8_lossy(&body);
        assert!(
            ty == IPC_SUBSCRIBE && reply.contains("true"),
            "sway refused the window subscription: {reply}"
        );
        let events = Arc::new((Mutex::new((0, false)), Condvar::new()));
        let (mut r, e) = (stream.try_clone().unwrap(), events.clone());
        std::thread::spawn(move || {
            loop {
                let gone = read_msg(&mut r).is_err();
                let mut g = e.0.lock().unwrap();
                g.0 += 1;
                g.1 = gone;
                drop(g);
                e.1.notify_all();
                if gone {
                    return;
                }
            }
        });
        SwayWindows { stream, events }
    }

    /// Blocks until `done()` holds, re-checking it after each window event.
    pub fn wait_until(&self, mut done: impl FnMut() -> bool) {
        let (m, grew) = &*self.events;
        loop {
            let seen = m.lock().unwrap().0;
            if done() {
                return;
            }
            let g = grew
                .wait_while(m.lock().unwrap(), |(n, gone)| *n == seen && !*gone)
                .unwrap();
            assert!(!g.1, "sway closed its IPC socket");
        }
    }
}

impl Drop for SwayWindows {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}
