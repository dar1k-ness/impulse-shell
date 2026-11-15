//! A `wl_keyboard.enter` that reaches a shell while it is busy, impulse-shell against Quickshell.
//!
//! Keys in one batch with the enter of a session lock surface (what was typed while the shell
//! read nothing the compositor sent): impulse-shell does with them what Quickshell does.
//! The batch is made without a clock: `proxy::Hold` stands between the shell and sway, keeps
//! back everything sway sends from the surface's `wl_keyboard.enter` on, and passes it to the
//! shell in one write once the last of the keys is in it.

use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

/// A session lock whose surface has one field with the focus; it logs its active focus and its
/// text.
const SHELL: &str = r#"import QtQuick
import Quickshell
import Quickshell.Wayland
ShellRoot {
    WlSessionLock {
        locked: true
        surface: Component {
            WlSessionLockSurface {
                color: "black"
                TextInput {
                    anchors.fill: parent
                    focus: true
                    onActiveFocusChanged: console.log("TXT af=" + activeFocus)
                    onTextChanged: console.log("TXT text=" + text)
                    Component.onCompleted: console.log("TXT ready")
                }
            }
        }
    }
}
"#;

/// evdev codes of the keys typed into the batch, and what they type.
const KEYS: [u32; 8] = [30, 31, 32, 33, 34, 35, 36, 37];
const TYPED: &str = "asdfghjk";
/// The key pressed once the surface is active: it ends the scenario.
const KEY_Z: u32 = 44;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Default)]
struct Globals {
    seat: Option<wl_seat::WlSeat>,
    vkbd: Option<ZwpVirtualKeyboardManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
    fn event(
        g: &mut Self,
        reg: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = ev
        {
            match interface.as_str() {
                "wl_seat" if g.seat.is_none() => g.seat = Some(reg.bind(name, 1, qh, ())),
                "zwp_virtual_keyboard_manager_v1" => g.vkbd = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

delegate_noop!(Globals: ignore wl_seat::WlSeat);
delegate_noop!(Globals: ZwpVirtualKeyboardManagerV1);
delegate_noop!(Globals: ZwpVirtualKeyboardV1);

/// The keyboard of sway's seat (the headless seat has none of its own).
struct Keyboard {
    queue: EventQueue<Globals>,
    globals: Globals,
    kbd: ZwpVirtualKeyboardV1,
    _keymap: std::fs::File,
    time: u32,
}

impl Keyboard {
    fn connect(sway: &testenv::Sway) -> Option<Self> {
        let conn =
            Connection::from_socket(UnixStream::connect(sway.display()).expect("sway's socket"))
                .expect("a Wayland connection to sway");
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut globals = Globals::default();
        queue.roundtrip(&mut globals).unwrap();
        let (Some(seat), Some(vkbd)) = (globals.seat.clone(), globals.vkbd.clone()) else {
            testenv::unavailable("compositor lacks zwp_virtual_keyboard_v1");
            return None;
        };
        let kbd = vkbd.create_virtual_keyboard(&seat, &qh, ());
        let ctx = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
        let km = xkbcommon::xkb::Keymap::new_from_names(
            &ctx,
            "",
            "",
            "us",
            "",
            None,
            xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .expect("xkb compiles the us keymap");
        let text = km.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
        let mut keymap = tempfile::tempfile().unwrap();
        keymap.write_all(text.as_bytes()).unwrap();
        keymap.write_all(&[0]).unwrap();
        kbd.keymap(1, keymap.as_fd(), text.len() as u32 + 1);
        let mut k = Keyboard {
            queue,
            globals,
            kbd,
            _keymap: keymap,
            time: 0,
        };
        k.sent();
        Some(k)
    }

    /// Sway has handled everything sent so far, and has queued the events that follow from it
    /// for its clients.
    fn sent(&mut self) {
        self.queue.roundtrip(&mut self.globals).unwrap();
        self.time += 50;
    }

    /// Presses and releases the key `code`.
    fn key(&mut self, code: u32) {
        for state in [1, 0] {
            self.kbd.key(self.time, code, state);
            self.sent();
        }
    }
}

/// What a shell's QML logged after `TXT `, in order.
fn texts(log: &str) -> Vec<&str> {
    log.lines()
        .filter_map(|l| l.split_once("TXT ").map(|(_, t)| t.trim_end()))
        // Quickshell colours its lines.
        .map(|t| t.split('\u{1b}').next().unwrap())
        .collect()
}

/// A shell that reaches the test's sway through `display`, and its output as it
/// arrives.
struct Client {
    name: &'static str,
    _child: Kill,
    log: Arc<Log>,
}

impl Client {
    fn start(
        sway: &testenv::Sway,
        display: &Path,
        name: &'static str,
        dir: &Path,
        qml: &str,
    ) -> Self {
        std::fs::write(dir.join("shell.qml"), qml).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        // libwayland writes its log to stderr in pieces: line-buffered, a line of it stays whole.
        let mut cmd = Command::new("stdbuf");
        cmd.arg("-eL");
        match name {
            "qs" => cmd.arg("qs"),
            _ => cmd.arg(env!("CARGO_BIN_EXE_impulse-shell")),
        };
        sway.env(&mut cmd)
            .arg("-p")
            .arg(dir)
            .envs(testenv::home_vars(&dir.join("home")))
            .env("WAYLAND_DISPLAY", display)
            .env("RI_INTERP", "1")
            .env("QT_QPA_PLATFORM", "wayland")
            .env("WAYLAND_DEBUG", "1")
            .stdin(Stdio::null())
            .stdout(w.try_clone().unwrap())
            .stderr(w);
        let child = Kill(cmd.spawn().unwrap());
        drop(cmd);
        Client {
            name,
            _child: child,
            log: Log::capture(r),
        }
    }

    /// Blocks until the QML logged a line for which `f` holds.
    #[track_caller]
    fn logged(&self, what: &str, f: impl Fn(&str) -> bool) {
        if let Err(log) = self.log.wait(|l| texts(l).iter().any(|t| f(t))) {
            panic!("{} exited before {what}:\n{log}", self.name);
        }
    }
}

/// Runs the scenario on shell `name` and returns the text of the field at its end, without the
/// last key's letter. The shell locks the session; sway gives the mapped lock surface the
/// keyboard, and from that `enter` on the shell gets nothing. The eight keys are typed; the
/// shell then gets the `enter` and the keys in one write. Once the field has active focus the
/// key Z is pressed, and its letter in the field ends the scenario.
fn typed(name: &'static str) -> Option<String> {
    if !testenv::require_all(&["sway", "qs", "stdbuf"]) {
        return None;
    }
    let sway = testenv::Sway::start();
    let mut kbd = Keyboard::connect(&sway)?;
    let hold = proxy::Hold::start(sway.display().to_path_buf(), 2 * KEYS.len());
    let dir = tempfile::tempdir().unwrap();
    let shell = Client::start(&sway, &hold.path, name, dir.path(), SHELL);
    hold.wait_enter();
    for code in KEYS {
        kbd.key(code);
    }
    hold.wait_passed();
    shell.logged("the field to have active focus", |t| t == "af=true");
    kbd.key(KEY_Z);
    shell.logged("the key pressed in the active surface", |t| {
        t.starts_with("text=") && t.ends_with('z')
    });

    // The batch was what the comment says: the shell read the keys after the enter and before
    // its field had active focus.
    let log = shell.log.text();
    let lines: Vec<&str> = log.lines().collect();
    let at = |parts: &[&str]| {
        lines
            .iter()
            .position(|l| parts.iter().all(|p| l.contains(p)))
            .unwrap_or_else(|| panic!("{name}: no line with {parts:?}:\n{log}"))
    };
    let enter = at(&["wl_keyboard", ".enter("]);
    let last = at(&[
        "wl_keyboard",
        ".key(",
        &format!(", {}, 0)", KEYS[KEYS.len() - 1]),
    ]);
    let active = at(&["TXT af=true"]);
    assert!(
        enter < last && last < active,
        "{name}: enter at line {enter}, the last key at {last}, active focus at {active}:\n{log}"
    );
    let text = texts(&log)
        .iter()
        .rev()
        .find_map(|t| t.strip_prefix("text="))
        .unwrap()
        .to_owned();
    Some(text.strip_suffix('z').unwrap().to_owned())
}

/// Neither shell's field gets a key of the batch. Qt Quick gives a key to the active window
/// only, and a surface is active once the compositor answered the `wl_display.sync` its client
/// sent on the enter (`key_before_activation_equiv.rs` has the same for a layer surface).
#[test]
fn keys_in_one_batch_with_enter_reach_no_field_of_a_lock_surface_as_in_quickshell() {
    let (Some(ours), Some(qs)) = (typed("impulse-shell"), typed("qs")) else {
        return;
    };
    assert_eq!(ours, qs);
    assert_eq!(qs, "", "of {TYPED:?}");
}

/// A Wayland proxy between a shell and sway that passes every message and fd on unchanged, but
/// late: from the first `wl_keyboard.enter` on, what sway sends is kept until `keys`
/// `wl_keyboard.key` events are in it, and then written to the shell at once.
mod proxy {
    use std::collections::HashSet;
    use std::os::fd::{AsRawFd, RawFd};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::{Arc, Condvar, Mutex};

    use testenv::wire::{arg, header, recv, send, split};

    const WL_DISPLAY: u32 = 1;
    // wayland.xml opcodes.
    const DISPLAY_GET_REGISTRY: u16 = 1;
    const DISPLAY_DELETE_ID: u16 = 1;
    const REGISTRY_BIND: u16 = 0;
    const SEAT_GET_KEYBOARD: u16 = 1;
    const KEYBOARD_ENTER: u16 = 1;
    const KEYBOARD_KEY: u16 = 3;

    /// The objects of a connection the proxy tells apart.
    #[derive(Default)]
    struct Objects {
        registries: HashSet<u32>,
        seats: HashSet<u32>,
        keyboards: HashSet<u32>,
    }

    #[derive(PartialEq)]
    enum Event {
        Enter,
        Key,
        Other,
    }

    impl Objects {
        /// A message of the shell.
        fn request(&mut self, m: &[u8]) {
            let (obj, op) = header(m);
            if obj == WL_DISPLAY && op == DISPLAY_GET_REGISTRY {
                self.registries.insert(arg(m, 0));
            } else if self.registries.contains(&obj) && op == REGISTRY_BIND && m.len() >= 16 {
                // bind(name, interface string, version, new id)
                let len = arg(m, 1) as usize;
                let id = arg(m, 2 + len.div_ceil(4) + 1);
                if &m[16..16 + len.saturating_sub(1)] == b"wl_seat" {
                    self.seats.insert(id);
                }
            } else if self.seats.contains(&obj) && op == SEAT_GET_KEYBOARD {
                self.keyboards.insert(arg(m, 0));
            }
        }

        /// A message of sway.
        fn event(&mut self, m: &[u8]) -> Event {
            let (obj, op) = header(m);
            if obj == WL_DISPLAY && op == DISPLAY_DELETE_ID {
                // The id may now name another object.
                let id = arg(m, 0);
                self.registries.remove(&id);
                self.seats.remove(&id);
                self.keyboards.remove(&id);
            } else if self.keyboards.contains(&obj) && op == KEYBOARD_ENTER {
                return Event::Enter;
            } else if self.keyboards.contains(&obj) && op == KEYBOARD_KEY {
                return Event::Key;
            }
            Event::Other
        }
    }

    /// Listens on a fresh socket in a new directory and runs `pump` on each client and its
    /// connection to the compositor at `upstream`.
    fn listen(
        upstream: PathBuf,
        pump: impl Fn(UnixStream, UnixStream) + Send + Sync + 'static,
    ) -> (PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wayland-proxy");
        let listener = UnixListener::bind(&path).unwrap();
        let pump = Arc::new(pump);
        std::thread::spawn(move || {
            for client in listener.incoming().flatten() {
                let server = UnixStream::connect(&upstream).unwrap();
                let pump = pump.clone();
                std::thread::spawn(move || pump(client, server));
            }
        });
        (path, dir)
    }

    /// Which of the two has something to read; `None` when neither will again.
    fn readable(cfd: RawFd, sfd: RawFd) -> Option<[bool; 2]> {
        let mut pfd = [cfd, sfd].map(|fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        });
        // SAFETY: two valid pollfds.
        if unsafe { libc::poll(pfd.as_mut_ptr(), 2, -1) } < 0 {
            return None;
        }
        Some(pfd.map(|p| p.revents != 0))
    }

    /// One direction of a connection: what was read and is not a whole message yet, and the
    /// fds that came with it.
    #[derive(Default)]
    struct Stream {
        bytes: Vec<u8>,
        fds: Vec<RawFd>,
    }

    impl Stream {
        /// The whole messages now read from `fd`; `None` when its other end is gone.
        fn read(&mut self, fd: RawFd) -> Option<Vec<Vec<u8>>> {
            let mut buf = vec![0u8; 1 << 16];
            let n = recv(fd, &mut buf, &mut self.fds);
            if n <= 0 {
                return None;
            }
            self.bytes.extend_from_slice(&buf[..n as usize]);
            Some(split(&mut self.bytes))
        }
    }

    #[derive(Clone, Copy, PartialEq, PartialOrd)]
    enum Stage {
        Before,
        Held,
        Passed,
    }

    pub struct Hold {
        pub path: PathBuf,
        stage: Arc<(Mutex<Stage>, Condvar)>,
        _dir: tempfile::TempDir,
    }

    impl Hold {
        pub fn start(upstream: PathBuf, keys: usize) -> Self {
            let stage = Arc::new((Mutex::new(Stage::Before), Condvar::new()));
            let s = stage.clone();
            let (path, dir) = listen(upstream, move |client, server| {
                pump(&s, keys, client, server)
            });
            Hold {
                path,
                stage,
                _dir: dir,
            }
        }

        fn wait(&self, stage: Stage) {
            let (lock, changed) = &*self.stage;
            let _s = changed
                .wait_while(lock.lock().unwrap(), |s| *s < stage)
                .unwrap();
        }

        /// Sway sent the shell a `wl_keyboard.enter`, which is kept back.
        pub fn wait_enter(&self) {
            self.wait(Stage::Held);
        }

        /// The enter and the keys were written to the shell.
        pub fn wait_passed(&self) {
            self.wait(Stage::Passed);
        }
    }

    /// Copies both directions of one connection.
    fn pump(stage: &(Mutex<Stage>, Condvar), keys: usize, client: UnixStream, server: UnixStream) {
        let set = |s: Stage| {
            *stage.0.lock().unwrap() = s;
            stage.1.notify_all();
        };
        let (cfd, sfd) = (client.as_raw_fd(), server.as_raw_fd());
        let (mut up, mut down) = (Stream::default(), Stream::default());
        let mut objects = Objects::default();
        // What is kept back, and the key events in it; `None` before the enter and after the
        // write.
        let mut held: Option<(Vec<u8>, usize)> = None;
        let mut passed = false;
        while let Some([shell, sway]) = readable(cfd, sfd) {
            if shell {
                let Some(messages) = up.read(cfd) else {
                    return;
                };
                let mut out = Vec::new();
                for m in messages {
                    objects.request(&m);
                    out.extend_from_slice(&m);
                }
                send(sfd, &out, &mut up.fds);
            }
            if sway {
                let Some(messages) = down.read(sfd) else {
                    return;
                };
                let mut out = Vec::new();
                for m in messages {
                    let event = objects.event(&m);
                    if held.is_none() && !passed && event == Event::Enter {
                        // What came before the enter goes on as it came.
                        if !out.is_empty() {
                            send(cfd, &out, &mut down.fds);
                            out.clear();
                        }
                        held = Some((Vec::new(), 0));
                        set(Stage::Held);
                    }
                    match &mut held {
                        Some((bytes, n)) => {
                            bytes.extend_from_slice(&m);
                            *n += usize::from(event == Event::Key);
                            if *n == keys {
                                // One write: the shell reads the enter and the keys together.
                                send(cfd, bytes, &mut down.fds);
                                held = None;
                                passed = true;
                                set(Stage::Passed);
                            }
                        }
                        None => out.extend_from_slice(&m),
                    }
                }
                if held.is_none() && !out.is_empty() {
                    send(cfd, &out, &mut down.fds);
                }
            }
        }
    }
}
