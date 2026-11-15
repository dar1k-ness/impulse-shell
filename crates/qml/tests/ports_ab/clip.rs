//! Clipboard history and on-screen keyboard (`ClipboardHistory`, `KeySender`; `Ydotool.qml`).
//! The shell runs `cliphist` (its db under the scene's `XDG_CACHE_HOME`) itself and serves
//! the clipboard and the keys from its own process; what it must give is what bash, `wl-copy`
//! and `ydotool` give when run by hand in the scene.
//!
//! Clipboard: every offered mime type with its bytes, read with `wl-paste` while the shell runs
//! (it serves the selection from a thread of its process, so it ends with the process;
//! `wl-copy` forks a daemon that outlives it). Keys: `ydotool` talks to
//! `ydotoold` over a datagram socket (`$YDOTOOL_SOCKET`) with raw `input_event`s, recorded here
//! (a container has no uinput for a real `ydotoold`); the shell's virtual-keyboard keys are
//! read by a focused Wayland window of this test. Both are reduced to the (evdev keycode,
//! pressed) transitions a keyboard device makes: uinput drops a key-up of a key that is up.

use crate::ab::{self, Scene};
use std::collections::BTreeSet;
use std::process::Command;
use std::sync::{Arc, Mutex};

type Keys = Arc<Mutex<Vec<(u32, bool)>>>;

/// One test at a time owns the clipboard and the keyboard focus.
pub static SEAT: Mutex<()> = Mutex::new(());

pub fn seat() -> std::sync::MutexGuard<'static, ()> {
    SEAT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `test` in a process of its own (`testenv::run_in_own_process`), holding the seat
/// meanwhile; true in that process, where the test's body runs. The key window
/// ([`window::keys`]) is one per process and keeps the keyboard focus it got, so every test
/// that types into it runs in a process of its own (as nextest runs every test): a window of
/// another process would take the focus from it. With `hypr`, `HYPRLAND_INSTANCE_SIGNATURE`
/// names a fresh instance directory under `$XDG_RUNTIME_DIR/hypr` there from the start, whose
/// sockets the test serves in [`hypr_dir`].
fn own_seat(test: &str, hypr: bool) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let _seat = seat();
    if !hypr {
        testenv::run_in_own_process(test, std::iter::empty::<(&str, &str)>());
        return false;
    }
    let dir = std::path::Path::new(&std::env::var("XDG_RUNTIME_DIR").unwrap()).join("hypr");
    std::fs::create_dir_all(&dir).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("ri-vkbd-")
        .tempdir_in(&dir)
        .unwrap();
    let sig = dir.path().file_name().unwrap().to_owned();
    testenv::run_in_own_process(test, [("HYPRLAND_INSTANCE_SIGNATURE", sig)]);
    false
}

/// The Hyprland instance directory [`own_seat`] made for this process.
fn hypr_dir() -> std::path::PathBuf {
    let rt = std::env::var_os("XDG_RUNTIME_DIR").unwrap();
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").unwrap();
    std::path::Path::new(&rt).join("hypr").join(sig)
}

fn wayland() -> bool {
    let on = std::env::var_os("RI_PORTS_WAYLAND").is_some()
        && std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !on {
        testenv::unavailable(
            "the clipboard/key ports need a compositor they may use: run under one with RI_PORTS_WAYLAND=1",
        );
    }
    on
}

/// A focused toplevel that records the keys it gets.
mod window {
    use super::Keys;
    use std::io::Write;
    use std::os::fd::AsFd;
    use wayland_client::protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
    };
    use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, delegate_noop};
    use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
    use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
        zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
        zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
    };
    use xkbcommon::xkb;

    #[derive(Default)]
    struct G {
        compositor: Option<wl_compositor::WlCompositor>,
        shm: Option<wl_shm::WlShm>,
        wm: Option<xdg_wm_base::XdgWmBase>,
        seat: Option<wl_seat::WlSeat>,
        vkm: Option<ZwpVirtualKeyboardManagerV1>,
        surface: Option<wl_surface::WlSurface>,
        buffer: Option<wl_buffer::WlBuffer>,
        focused: bool,
        ctrl: bool,
        keys: Keys,
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for G {
        fn event(
            g: &mut G,
            r: &wl_registry::WlRegistry,
            e: wl_registry::Event,
            _: &(),
            _: &Connection,
            qh: &QueueHandle<G>,
        ) {
            if let wl_registry::Event::Global {
                name,
                interface,
                version,
            } = e
            {
                match interface.as_str() {
                    "wl_compositor" => g.compositor = Some(r.bind(name, version.min(4), qh, ())),
                    "wl_shm" => g.shm = Some(r.bind(name, 1, qh, ())),
                    "xdg_wm_base" => g.wm = Some(r.bind(name, 1, qh, ())),
                    "wl_seat" if g.seat.is_none() => {
                        g.seat = Some(r.bind(name, version.min(5), qh, ()))
                    }
                    "zwp_virtual_keyboard_manager_v1" => g.vkm = Some(r.bind(name, 1, qh, ())),
                    _ => {}
                }
            }
        }
    }

    impl Dispatch<xdg_wm_base::XdgWmBase, ()> for G {
        fn event(
            _: &mut G,
            wm: &xdg_wm_base::XdgWmBase,
            e: xdg_wm_base::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<G>,
        ) {
            if let xdg_wm_base::Event::Ping { serial } = e {
                wm.pong(serial);
            }
        }
    }

    impl Dispatch<xdg_surface::XdgSurface, ()> for G {
        fn event(
            g: &mut G,
            xs: &xdg_surface::XdgSurface,
            e: xdg_surface::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<G>,
        ) {
            if let xdg_surface::Event::Configure { serial } = e {
                xs.ack_configure(serial);
                if let (Some(s), Some(b)) = (&g.surface, &g.buffer) {
                    s.attach(Some(b), 0, 0);
                    s.commit();
                }
            }
        }
    }

    impl Dispatch<wl_keyboard::WlKeyboard, ()> for G {
        fn event(
            g: &mut G,
            _: &wl_keyboard::WlKeyboard,
            e: wl_keyboard::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<G>,
        ) {
            match e {
                wl_keyboard::Event::Enter { .. } => {
                    g.focused = true;
                    FOCUS.lock().unwrap().push("enter");
                }
                wl_keyboard::Event::Leave { .. } => {
                    g.focused = false;
                    FOCUS.lock().unwrap().push("leave");
                }
                wl_keyboard::Event::Key {
                    key, state, time, ..
                } => {
                    let down = matches!(state, WEnum::Value(wl_keyboard::KeyState::Pressed));
                    // A paste target: Ctrl+V reads the clipboard, as a text field does.
                    if key == 29 {
                        g.ctrl = down;
                    } else if key == 47 && down && g.ctrl {
                        std::thread::spawn(|| {
                            std::process::Command::new("wl-paste")
                                .arg("--no-newline")
                                .stdout(std::process::Stdio::null())
                                .status()
                        });
                    }
                    g.keys.lock().unwrap().push((key, down));
                    TIMES.lock().unwrap().push(time);
                }
                wl_keyboard::Event::Modifiers {
                    mods_depressed,
                    mods_locked,
                    group,
                    ..
                } => {
                    GROUPS.lock().unwrap().push(group);
                    MODS.lock().unwrap().push(Mods {
                        depressed: mods_depressed,
                        locked: mods_locked,
                        group,
                    });
                }
                wl_keyboard::Event::Keymap { fd, size, .. } => {
                    use std::os::unix::fs::FileExt;
                    let mut buf = vec![0u8; size as usize];
                    if std::fs::File::from(fd).read_exact_at(&mut buf, 0).is_ok() {
                        let text = String::from_utf8_lossy(&buf);
                        *KEYMAP.lock().unwrap() = Some(text.trim_end_matches('\0').to_owned());
                    }
                }
                _ => {}
            }
            changed();
        }
    }

    /// The answer to the `wl_display.sync` number `n` of `marker`.
    impl Dispatch<wl_callback::WlCallback, u64> for G {
        fn event(
            _: &mut G,
            _: &wl_callback::WlCallback,
            _: wl_callback::Event,
            n: &u64,
            _: &Connection,
            _: &QueueHandle<G>,
        ) {
            *SYNCED.lock().unwrap() = *n;
            changed();
        }
    }

    delegate_noop!(G: ignore wl_compositor::WlCompositor);
    delegate_noop!(G: ignore wl_surface::WlSurface);
    delegate_noop!(G: ignore wl_shm::WlShm);
    delegate_noop!(G: ignore wl_shm_pool::WlShmPool);
    delegate_noop!(G: ignore wl_buffer::WlBuffer);
    delegate_noop!(G: ignore wl_seat::WlSeat);
    delegate_noop!(G: ignore xdg_toplevel::XdgToplevel);
    delegate_noop!(G: ignore ZwpVirtualKeyboardManagerV1);
    delegate_noop!(G: ignore ZwpVirtualKeyboardV1);

    fn memfd(name: &str, data: &[u8], len: u64) -> std::fs::File {
        let fd = rustix::fs::memfd_create(name, rustix::fs::MemfdFlags::CLOEXEC).unwrap();
        let mut f = std::fs::File::from(fd);
        f.write_all(data).unwrap();
        f.set_len(len).unwrap();
        f
    }

    fn run(keys: Keys, ready: std::sync::mpsc::Sender<()>) -> Option<()> {
        let conn = Connection::connect_to_env().ok()?;
        let mut q = conn.new_event_queue();
        let qh = q.handle();
        conn.display().get_registry(&qh, ());
        let mut g = G {
            keys,
            ..G::default()
        };
        q.roundtrip(&mut g).ok()?;
        let (comp, shm, wm, seat, vkm) = (
            g.compositor.clone()?,
            g.shm.clone()?,
            g.wm.clone()?,
            g.seat.clone()?,
            g.vkm.clone()?,
        );
        // The headless seat has no keyboard (no keymap for the port's virtual keyboard to
        // copy) until one exists and types: this one stands for the physical keyboard.
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let km = xkb::Keymap::new_from_names(&ctx, "", "", "us,ru", "", None, 0)?
            .get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        let kmf = memfd("ab-keymap", km.as_bytes(), km.len() as u64 + 1);
        let vk = vkm.create_virtual_keyboard(&seat, &qh, ());
        vk.keymap(1, kmf.as_fd(), km.len() as u32 + 1);
        vk.key(0, 191, 1);
        vk.key(1, 191, 0);
        let (w, h) = (64, 64);
        let pix = memfd("ab-buffer", &[], (w * h * 4) as u64);
        let pool = shm.create_pool(pix.as_fd(), w * h * 4, &qh, ());
        g.buffer = Some(pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Xrgb8888, &qh, ()));
        let surface = comp.create_surface(&qh, ());
        let xs = wm.get_xdg_surface(&surface, &qh, ());
        let top = xs.get_toplevel(&qh, ());
        top.set_title("ri-ab-keys".into());
        surface.commit();
        g.surface = Some(surface);
        seat.get_keyboard(&qh, ());
        while !g.focused {
            q.blocking_dispatch(&mut g).ok()?;
        }
        q.roundtrip(&mut g).ok()?;
        g.keys.lock().unwrap().clear();
        let _ = SEAT_KB.set((conn.clone(), vk, qh.clone()));
        let _ = ready.send(());
        loop {
            q.blocking_dispatch(&mut g).ok()?;
        }
    }

    /// The layout (`wl_keyboard.modifiers` group) of each modifiers event the window got.
    pub static GROUPS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
    /// Each modifiers event the window got.
    pub static MODS: std::sync::Mutex<Vec<Mods>> = std::sync::Mutex::new(Vec::new());

    /// A `wl_keyboard.modifiers` event.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Mods {
        pub depressed: u32,
        pub locked: u32,
        pub group: u32,
    }

    impl Mods {
        /// Whether the xkb modifier `name` (of the window's last keymap) is locked.
        pub fn locks(&self, name: &str) -> bool {
            let text = KEYMAP
                .lock()
                .unwrap()
                .clone()
                .expect("the window got a keymap");
            let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
            let km = xkb::Keymap::new_from_string(&ctx, text, xkb::KEYMAP_FORMAT_TEXT_V1, 0)
                .expect("the window's keymap compiles");
            let i = km.mod_get_index(name);
            i < u32::BITS && self.locked & (1 << i) != 0
        }
    }

    /// The `time` of each key event the window got.
    pub static TIMES: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
    /// The keymap the window got last.
    pub static KEYMAP: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    /// Counts the window's keyboard events; signalled after each one.
    static EVENTS: (std::sync::Mutex<u64>, std::sync::Condvar) =
        (std::sync::Mutex::new(0), std::sync::Condvar::new());

    fn changed() {
        *EVENTS.0.lock().unwrap() += 1;
        EVENTS.1.notify_all();
    }

    /// Blocks until `done` holds, checking it again after each keyboard event of the window.
    pub fn wait_for(mut done: impl FnMut() -> bool) {
        let mut n = EVENTS.0.lock().unwrap();
        while !done() {
            n = EVENTS.1.wait(n).unwrap();
        }
    }

    /// The keysym evdev key `code` gives in the window: its last keymap, on its last layout.
    pub fn sym(code: u32) -> u32 {
        let text = KEYMAP
            .lock()
            .unwrap()
            .clone()
            .expect("the window got a keymap");
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let km = xkb::Keymap::new_from_string(&ctx, text, xkb::KEYMAP_FORMAT_TEXT_V1, 0)
            .expect("the window's keymap compiles");
        let mut st = xkb::State::new(&km);
        let group = GROUPS.lock().unwrap().last().copied().unwrap_or(0);
        st.update_mask(0, 0, 0, 0, 0, group);
        st.key_get_one_sym(xkb::Keycode::new(code + 8)).raw()
    }

    /// Switches the stand-in physical keyboard (`us,ru`) to `group`, until the window is on it.
    pub fn switch_layout(group: u32) {
        let (conn, vk, _) = SEAT_KB.get().expect("the key window was started");
        vk.modifiers(0, 0, 0, group);
        let _ = conn.flush();
        wait_for(|| GROUPS.lock().unwrap().last() == Some(&group));
    }

    /// Gives the stand-in physical keyboard the xkb `layout` keymap (from its next key on),
    /// as a changed `kb_layout` does to the real one.
    pub fn set_keymap(layout: &str) {
        let (conn, vk, _) = SEAT_KB.get().expect("the key window was started");
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let km = xkb::Keymap::new_from_names(&ctx, "", "", layout, "", None, 0)
            .expect("keymap")
            .get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        let kmf = memfd("ab-keymap", km.as_bytes(), km.len() as u64 + 1);
        vk.keymap(1, kmf.as_fd(), km.len() as u32 + 1);
        let _ = conn.flush();
    }

    static SEAT_KB: std::sync::OnceLock<(Connection, ZwpVirtualKeyboardV1, QueueHandle<G>)> =
        std::sync::OnceLock::new();
    /// Each `wl_keyboard.enter` and `leave` the window got, in order.
    static FOCUS: std::sync::Mutex<Vec<&str>> = std::sync::Mutex::new(Vec::new());
    /// The number of the last `wl_display.sync` of `marker` the compositor answered.
    static SYNCED: std::sync::Mutex<u64> = std::sync::Mutex::new(0);

    /// Types the marker key on the stand-in physical keyboard and waits for it in the window.
    /// The key and a `wl_display.sync` go out on the window's connection, in this order: sway
    /// sends a key to the focused window while it handles the request, so the key the window
    /// is to get comes before the answer to the sync. An answer with no key before it means
    /// the key went to another window (one a test outside nextest's `sway` group opened on
    /// the same sway), and no later event would bring it: the test fails there, naming that
    /// window, instead of waiting until nextest stops it.
    fn marker(keys: &Keys) {
        static N: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
        let Some((conn, vk, qh)) = SEAT_KB.get() else {
            return;
        };
        let n = {
            let mut n = N.lock().unwrap();
            *n += 1;
            *n
        };
        vk.key(2, 191, 1);
        vk.key(3, 191, 0);
        conn.display().sync(qh, n);
        let _ = conn.flush();
        let arrived = || keys.lock().unwrap().contains(&(191, false));
        wait_for(|| arrived() || *SYNCED.lock().unwrap() >= n);
        if !arrived() {
            let focus = FOCUS.lock().unwrap();
            let last = &focus[focus.len().saturating_sub(6)..];
            panic!(
                "a key typed on the seat did not reach the test's window: sway answered the \
                 wl_display.sync sent after it and sent no key\n\
                 the window's last wl_keyboard focus events: {last:?} ({} in all)\n\
                 sway's focused window: {}",
                focus.len(),
                focused_window()
            );
        }
    }

    /// The window sway's tree marks focused: its title, app_id, pid, the command line of
    /// that process and the nextest test it belongs to.
    fn focused_window() -> String {
        fn find(node: &serde_json::Value) -> Option<&serde_json::Value> {
            if node["focused"] == true && node["pid"].is_number() {
                return Some(node);
            }
            ["nodes", "floating_nodes"]
                .iter()
                .filter_map(|k| node[*k].as_array())
                .flatten()
                .find_map(find)
        }
        let sock = std::env::var_os("SWAYSOCK")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::fs::read_dir(std::env::var_os("XDG_RUNTIME_DIR")?)
                    .ok()?
                    .flatten()
                    .map(|e| e.path())
                    .find(|p| p.to_string_lossy().contains("sway-ipc."))
            });
        let Some(sock) = sock else {
            return "unknown (no sway IPC socket)".into();
        };
        let out = std::process::Command::new("swaymsg")
            .env("SWAYSOCK", sock)
            .args(["-t", "get_tree", "-r"])
            .output();
        let tree = out
            .ok()
            .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok());
        let Some(tree) = tree else {
            return "unknown (swaymsg -t get_tree failed)".into();
        };
        let Some(w) = find(&tree) else {
            return "none".into();
        };
        let pid = w["pid"].as_u64().unwrap_or(0);
        let read = |f: &str| {
            let bytes = std::fs::read(format!("/proc/{pid}/{f}")).unwrap_or_default();
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let env = read("environ");
        let var = |k: &str| {
            env.split('\0')
                .find_map(|e| e.strip_prefix(k)?.strip_prefix('='))
                .unwrap_or("?")
                .to_owned()
        };
        format!(
            "title {}, app_id {}, pid {pid}, command `{}`, test {} {}",
            w["name"],
            w["app_id"],
            read("cmdline").replace('\0', " ").trim_end(),
            var("NEXTEST_BINARY_ID"),
            var("NEXTEST_TEST_NAME")
        )
    }

    /// Types on the stand-in physical keyboard and forgets the key: the seat has no keymap
    /// for the port's virtual keyboard to copy once the port's last one is gone (sway keeps
    /// the keyboard that typed last), as a real seat does after a key on the real keyboard.
    pub fn touch(keys: &Keys) {
        marker(keys);
        keys.lock().unwrap().clear();
    }

    /// Every key the compositor sent the window before now has arrived: a marker key sent
    /// after them comes after them (taken out again).
    pub fn barrier(keys: &Keys) {
        marker(keys);
        keys.lock().unwrap().retain(|&(c, _)| c != 191);
    }

    /// The keys the window got so far; `None` when it never got the focus.
    pub fn keys() -> Option<Keys> {
        static W: std::sync::OnceLock<Option<Keys>> = std::sync::OnceLock::new();
        W.get_or_init(|| {
            let keys = Keys::default();
            let k = keys.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || run(k, tx));
            rx.recv().ok().map(|_| keys)
        })
        .clone()
    }
}

/// The key transitions in `ev`, from all keys up.
fn transitions(ev: &[(u32, bool)]) -> String {
    let mut down = BTreeSet::new();
    ev.iter()
        .filter(|&&(c, p)| if p { down.insert(c) } else { down.remove(&c) })
        .map(|&(c, p)| format!("{c}{}", if p { '+' } else { '-' }))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What `ydotool` and the port typed, as transitions.
struct Typing {
    /// The test's own sender to the socket, for `barrier`'s marker.
    marker: std::os::unix::net::UnixDatagram,
    path: std::path::PathBuf,
    socket: Keys,
    window: Keys,
}

/// A code no key has: the marker datagram `Typing::barrier` sends.
const MARKER: u16 = u16::MAX;

impl Typing {
    fn start(sc: &mut Scene) -> Typing {
        let path = sc.dir.join("ydotool.sock");
        let s = std::os::unix::net::UnixDatagram::bind(&path).unwrap();
        sc.set("YDOTOOL_SOCKET", &path.display().to_string());
        let socket = Keys::default();
        let log = socket.clone();
        // Read all along: ydotool's sendto blocks once the socket's small datagram queue is full.
        std::thread::spawn(move || {
            let mut b = [0u8; 64];
            while let Ok(n) = s.recv(&mut b) {
                // struct input_event { timeval; u16 type; u16 code; i32 value }
                if n >= 24 && u16::from_ne_bytes([b[16], b[17]]) == 1 {
                    let code = u16::from_ne_bytes([b[18], b[19]]) as u32;
                    let value = i32::from_ne_bytes([b[20], b[21], b[22], b[23]]);
                    log.lock().unwrap().push((code, value != 0));
                    // Under the waiters' lock, so none checks before the push and waits after it.
                    drop(SOCKET_READ.0.lock().unwrap());
                    SOCKET_READ.1.notify_all();
                }
            }
        });
        let marker = std::os::unix::net::UnixDatagram::unbound().unwrap();
        let window = window::keys().expect("the key window never got the keyboard focus");
        Typing {
            marker,
            path,
            socket,
            window,
        }
    }

    /// Every datagram sent to the socket before now has been read: the socket keeps them in
    /// order, so a marker sent after them is read after them (taken out again).
    fn barrier(&self) {
        let mut ev = [0u8; 24];
        ev[16..18].copy_from_slice(&1u16.to_ne_bytes());
        ev[18..20].copy_from_slice(&MARKER.to_ne_bytes());
        self.marker.send_to(&ev, &self.path).unwrap();
        let mark = (MARKER as u32, false);
        let mut g = SOCKET_READ.0.lock().unwrap();
        while !self.socket.lock().unwrap().contains(&mark) {
            g = SOCKET_READ.1.wait(g).unwrap();
        }
        drop(g);
        self.socket.lock().unwrap().retain(|&e| e != mark);
    }

    fn clear(&self) {
        window::touch(&self.window);
        self.barrier();
        self.socket.lock().unwrap().clear();
    }

    /// One side's typing: only one of the two ever gets keys.
    fn typed(&self) -> String {
        window::barrier(&self.window);
        self.barrier();
        let mut ev = self.socket.lock().unwrap().clone();
        ev.extend(self.window.lock().unwrap().iter().copied());
        transitions(&ev)
    }
}

/// Signalled each time the `Typing` thread logged a datagram.
static SOCKET_READ: (Mutex<()>, std::sync::Condvar) = (Mutex::new(()), std::sync::Condvar::new());

fn sh(cmd: &str) -> Vec<u8> {
    Command::new("bash")
        .args(["-c", cmd])
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap()
        .stdout
}

fn digest(b: &[u8]) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    b.hash(&mut h);
    let head: String = String::from_utf8_lossy(&b[..b.len().min(48)]).into();
    format!("{} bytes {:016x} {head:?}", b.len(), h.finish())
}

/// Every type the clipboard offers (sorted: wl-copy and wl-clipboard-rs list them in
/// different orders) with its bytes; `None` when nothing is copied.
fn clipboard() -> Option<String> {
    let o = Command::new("wl-paste")
        .arg("--list-types")
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    if !o.status.success() {
        return None;
    }
    let mut types: Vec<String> = String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    types.sort();
    let mut out = String::new();
    for t in types {
        let b = Command::new("wl-paste")
            .args(["-n", "-t", &t])
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap()
            .stdout;
        out += &format!("{t}: {}\n", digest(&b));
    }
    Some(out)
}

/// Clears the clipboard, blocking until the compositor announces the empty selection: `wl-paste
/// --watch` runs its command on each selection event, the current selection first, with
/// `CLIPBOARD_STATE` naming it (`nil`/`clear`: none).
fn clear_clipboard() {
    use std::io::BufRead;
    let mut watch = Command::new("wl-paste")
        .args([
            "--watch",
            "sh",
            "-c",
            "cat >/dev/null; echo \"$CLIPBOARD_STATE\"",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut states = std::io::BufReader::new(watch.stdout.take().unwrap()).lines();
    let empty = |s: &str| s == "nil" || s == "clear";
    let mut state = states.next().and_then(Result::ok);
    sh("wl-copy --clear; pkill -x wl-copy");
    while !state.as_deref().is_some_and(empty) {
        state = Some(
            states
                .next()
                .and_then(Result::ok)
                .expect("wl-paste --watch ended before the clipboard cleared"),
        );
    }
    let _ = watch.kill();
    let _ = watch.wait();
    assert_eq!(clipboard(), None, "the clipboard did not clear");
}

/// Runs `cmd` with the scene's environment.
fn in_scene(sc: &Scene, cmd: &str) -> Vec<u8> {
    let o = Command::new("bash")
        .args(["-c", cmd])
        .envs(sc.env.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    assert!(o.status.success(), "{cmd}: {o:?}");
    o.stdout
}

/// The scene's cliphist history (newest first, as `cliphist list` prints it): text with
/// quotes, `%`/backslashes (the original feeds the entry to `printf` as a format), newlines,
/// non-ASCII, and two PNGs.
fn history(sc: &Scene) -> Vec<String> {
    let texts = [
        "plain text",
        "it's \"quoted\" $HOME `id` 100% %s \\n back\\slash",
        "line one\nline two\n",
        "Привет, 日本",
    ];
    for t in texts {
        let f = sc.dir.join("entry");
        std::fs::write(&f, t).unwrap();
        in_scene(sc, &format!("cliphist store < '{}'", f.display()));
    }
    in_scene(sc, "magick -size 4x3 xc:red png:- | cliphist store");
    in_scene(sc, "printf 'between images' | cliphist store");
    in_scene(sc, "magick -size 2x5 xc:blue png:- | cliphist store");
    let list = String::from_utf8(in_scene(sc, "cliphist list")).unwrap();
    let e: Vec<String> = list.lines().map(str::to_owned).collect();
    assert_eq!(e.len(), 7, "{list}");
    e
}

const HISTORY: &str = r#"import QtQuick
import Impulse

Item {
    id: root
    property var decoded: []
    Connections {
        target: ClipboardHistory
        function onImageDecoded(path, ok) {
            root.decoded = root.decoded.concat([[path.split("/").pop(), ok]])
        }
    }
    function go(input, i) {
        if (i > 0)
            return 0;
        const c = JSON.parse(input);
        if (c.bin !== undefined) ClipboardHistory.binary = c.bin;
        if (c.press !== undefined) ClipboardHistory.pasteCommand = c.press;
        if (c.copy !== undefined)
            ClipboardHistory.copy(c.copy);
        else if (c.superpaste !== undefined)
            ClipboardHistory.superpaste(c.superpaste);
        else if (c.decode !== undefined)
            ClipboardHistory.decodeImage(c.decode, c.path);
        else if (c.removeImage !== undefined)
            ClipboardHistory.removeImage(c.removeImage);
        return c.wait;
    }
    function state() { return JSON.stringify(root.decoded) }
}
"#;

/// A scene for `ClipboardHistory`: history, key recording, a `pastelog` paste command that
/// notes the clipboard at each paste, and `stash`, a non-cliphist history tool
/// (`stash decode <id>`) over the same db. `wl-copy` and `ydotool` are traced: the shell
/// must run neither.
fn history_scene(tag: &str) -> Option<(Scene, Typing, Vec<String>)> {
    if !wayland()
        || !ab::require(&[
            "cliphist", "wl-copy", "wl-paste", "xdg-mime", "ydotool", "magick",
        ])
    {
        return None;
    }
    let mut sc = Scene::new(tag);
    let typing = Typing::start(&mut sc);
    let entries = history(&sc);
    sc.trace(&["wl-copy", "ydotool"]);
    let log = sc.dir.join("paste.log");
    sc.stub(
        "pastelog",
        &format!(
            "{{ for t in $(wl-paste -l | sort); do printf '%s %s\\n' \"$t\" \"$(wl-paste -n -t \"$t\" | sha256sum | cut -c1-16)\"; done; echo --; }} >> '{}'",
            log.display()
        ),
    );
    sc.stub(
        "stash",
        "[ \"$1\" = decode ] && exec cliphist decode \"$2\"\nexit 2",
    );
    sc.main(HISTORY);
    Some((sc, typing, entries))
}

/// `cliphist decode | wl-copy` of `entry`, as a user would copy it by hand.
fn copy_by_hand(sc: &Scene, entry: &str) {
    let f = sc.dir.join("entry");
    std::fs::write(&f, entry).unwrap();
    in_scene(
        sc,
        // wl-copy's daemon keeps what it inherits open: nothing of ours.
        &format!(
            "cliphist decode < '{}' | wl-copy >/dev/null 2>&1",
            f.display()
        ),
    );
}

/// What one run of the shell did: the clipboard (the last one served while it ran), the
/// `pastelog` notes, the keys typed, and the traced tools it ran.
struct Did {
    clipboard: Option<String>,
    pasted: String,
    keys: String,
    trace: String,
    state: String,
}

fn run(sc: &Scene, typing: &Typing, input: serde_json::Value) -> Did {
    clear_clipboard();
    typing.clear();
    sc.rm("paste.log");
    sc.take_trace();
    let mut last = None;
    let (state, keys) = ab::side(sc, false, &input.to_string(), &mut || {
        if let Some(c) = clipboard() {
            last = Some(c);
        }
        typing.typed()
    });
    Did {
        clipboard: last,
        pasted: sc.read("paste.log"),
        keys,
        trace: sc.take_trace(),
        state,
    }
}

/// `ClipboardHistory.copy(entry)` puts in the clipboard what `cliphist decode | wl-copy`
/// does (every offered type with its bytes), for cliphist and for a stash-like tool, from
/// the shell's own process.
#[test]
fn clipboard_history_copy() {
    if !own_seat(concat!(module_path!(), "::clipboard_history_copy"), false) {
        return;
    }
    let _g = seat();
    let Some((sc, typing, e)) = history_scene("copy") else {
        return;
    };
    let cases = [
        ("cliphist", &e[6]),
        ("cliphist", &e[5]),
        ("cliphist", &e[3]),
        ("cliphist", &e[0]),
        ("stash", &e[4]),
        ("stash", &e[2]),
    ];
    for (bin, entry) in cases {
        clear_clipboard();
        copy_by_hand(&sc, entry);
        let want = clipboard().expect("wl-copy copied nothing");
        let did = run(
            &sc,
            &typing,
            serde_json::json!({ "bin": bin, "copy": entry, "wait": 1500 }),
        );
        assert_eq!(did.clipboard.as_deref(), Some(want.as_str()), "{entry}");
        assert_eq!(did.keys, "", "{entry}");
        assert_eq!(did.trace, "", "{entry}");
        assert!(did.state.contains(r#""errors":[]"#), "{}", did.state);
    }
    assert!(clipboard_has(&sc, &typing, &e[0], "image/png"));
    clear_clipboard();
}

fn clipboard_has(sc: &Scene, typing: &Typing, entry: &str, mime: &str) -> bool {
    let did = run(
        sc,
        typing,
        serde_json::json!({ "copy": entry, "wait": 1500 }),
    );
    did.clipboard.is_some_and(|c| c.contains(mime))
}

/// `ClipboardHistory.superpaste(entries)`: every entry copied and pasted in order, each once
/// the window has read the one before; the paste key is Ctrl+V on the shell's own keyboard,
/// or the user's `pasteCommand`.
#[test]
fn clipboard_history_superpaste() {
    if !own_seat(
        concat!(module_path!(), "::clipboard_history_superpaste"),
        false,
    ) {
        return;
    }
    let _g = seat();
    let Some((sc, typing, e)) = history_scene("superpaste") else {
        return;
    };
    let images: Vec<String> = vec![e[2].clone(), e[0].clone()];
    let lists: [Vec<String>; 4] = [
        vec![e[1].clone(), e[0].clone()],
        e.iter().rev().cloned().collect(),
        vec![],
        images.clone(),
    ];
    for list in &lists {
        // The notes `pastelog` takes when each entry is copied by hand and then pasted.
        clear_clipboard();
        sc.rm("paste.log");
        for entry in list {
            copy_by_hand(&sc, entry);
            in_scene(&sc, "pastelog");
        }
        let want = sc.read("paste.log");
        let did = run(
            &sc,
            &typing,
            serde_json::json!({ "superpaste": list, "press": "pastelog", "wait": 2500 }),
        );
        assert_eq!(did.pasted, want, "{list:?}");
        assert_eq!(did.pasted.matches("--\n").count(), list.len(), "{list:?}");
        assert_eq!(did.keys, "", "{list:?}");
        assert_eq!(did.trace, "", "{list:?}");
    }
    let paste = "29+ 47+ 47- 29-";
    for list in [&lists[0], &images, &lists[1]] {
        let did = run(
            &sc,
            &typing,
            serde_json::json!({ "superpaste": list, "wait": 2500 }),
        );
        assert_eq!(did.keys, vec![paste; list.len()].join(" "), "{list:?}");
        assert_eq!(did.trace, "", "{list:?}");
    }
    clear_clipboard();
}

/// The listing of `dir`: names, kinds and contents.
fn listing(dir: &std::path::Path) -> String {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| {
                    let p = e.path();
                    let n = e.file_name().to_string_lossy().into_owned();
                    if p.is_dir() {
                        format!("{n}/")
                    } else {
                        format!("{n}: {}", digest(&std::fs::read(&p).unwrap_or_default()))
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.join("\n")
}

/// `ClipboardHistory.decodeImage(entry, path)` writes the entry's image to `path` unless the
/// file is there, reports `imageDecoded(path, ok)` and leaves nothing behind when the entry is
/// gone; `removeImage(path)` removes a file and nothing else. No `bash`, no `rm`.
#[test]
fn clipboard_history_images() {
    if !ab::require(&["cliphist", "magick", "bash"]) {
        return;
    }
    let sc = Scene::new("clipimage");
    let e = history(&sc);
    sc.trace(&["bash", "rm"]);
    sc.main(HISTORY);
    let dir = sc.dir.join("decode");
    let d = dir.display().to_string();
    let fresh = || {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("other"), "keep").unwrap();
        sc.take_trace();
    };
    let go = |input: serde_json::Value| {
        let (state, fx) = ab::side(&sc, false, &input.to_string(), &mut || listing(&dir));
        assert_eq!(sc.take_trace(), "", "{input}");
        (state, fx)
    };
    for entry in [&e[0], &e[2]] {
        fresh();
        let (state, fx) =
            go(serde_json::json!({ "decode": entry, "path": format!("{d}/12"), "wait": 1000 }));
        assert!(state.contains(r#"[[\"12\",true]]"#), "{state}");
        assert!(fx.contains("12: ") && fx.contains("PNG"), "{fx}");
        assert_eq!(fx.lines().count(), 2, "{fx}");
    }
    // A file that is there is kept as it is.
    fresh();
    std::fs::write(dir.join("12"), "decoded before").unwrap();
    let (state, fx) =
        go(serde_json::json!({ "decode": e[0], "path": format!("{d}/12"), "wait": 1000 }));
    assert!(state.contains(r#"[[\"12\",true]]"#), "{state}");
    assert!(fx.contains("decoded before"), "{fx}");
    // An entry that is gone: reported, and no empty file left.
    fresh();
    let gone = "999\t[[ binary data 1 B png 1x1 ]]";
    let (state, fx) =
        go(serde_json::json!({ "decode": gone, "path": format!("{d}/12"), "wait": 1000 }));
    assert!(state.contains(r#"[[\"12\",false]]"#), "{state}");
    assert_eq!(fx.lines().count(), 1, "{fx}");
    // A directory that is not there.
    fresh();
    let (state, _) =
        go(serde_json::json!({ "decode": e[0], "path": format!("{d}/no/such/1"), "wait": 1000 }));
    assert!(state.contains(r#"[[\"1\",false]]"#), "{state}");
    // removeImage: a file goes, a directory and other files stay.
    for (make, path, left) in [
        ("file", format!("{d}/12"), 1),
        ("none", format!("{d}/12"), 1),
        ("dir", format!("{d}/12"), 2),
        ("file", format!("{d}/with space"), 1),
        ("file", format!("{d}/../decode/7"), 1),
    ] {
        fresh();
        match make {
            "file" => std::fs::write(&path, "png bytes").unwrap(),
            "dir" => std::fs::create_dir(&path).unwrap(),
            _ => {}
        }
        let (_, fx) = go(serde_json::json!({ "removeImage": path, "wait": 200 }));
        assert_eq!(fx.lines().count(), left, "{make} {path}: {fx}");
    }
}

const OSK: &str = r#"import QtQuick
import ri.services

Item {
    id: root
    property var modes: []
    function go(input, i) {
        const steps = JSON.parse(input);
        if (i >= steps.length)
            return 0;
        const s = steps[i];
        if (s[0] === "press") Ydotool.press(s[1]);
        else if (s[0] === "release") Ydotool.release(s[1]);
        else if (s[0] === "shiftMode") Ydotool.shiftMode = s[1];
        else if (s[0] === "shiftKeys") Ydotool.shiftKeys = s[1];
        else if (s[0] === "releaseShift") Ydotool.releaseShiftKeys();
        else if (s[0] === "releaseAll") Ydotool.releaseAllKeys();
        root.modes = root.modes.concat([Ydotool.shiftMode]);
        return 300;
    }
    function state() { return JSON.stringify({ modes: root.modes }) }
}
"#;

/// The original's `Ydotool.qml` (the file as it is) types through `KeySender`: the keys reach the
/// focused window, shift keys and "release all" lift what is held, `shiftMode` resets, and
/// no `ydotool` runs.
#[test]
fn key_sender_keys() {
    if !wayland() || !own_seat(concat!(module_path!(), "::key_sender_keys"), false) {
        return;
    }
    let _g = seat();
    if !ab::require(&["ydotool"]) {
        return;
    }
    let mut sc = Scene::new("keysender");
    let typing = Typing::start(&mut sc);
    sc.trace(&["ydotool"]);
    sc.write(
        "services/Ydotool.qml",
        &std::fs::read_to_string(ab::ri().join("services/Ydotool.qml"))
            .unwrap()
            .replace("import ri.modules.common\n", ""),
    );
    sc.main(OSK);
    let cases = [
        // On-screen keyboard shift: Shift, a, release shift keys (shift mode back to 0).
        (
            r#"[["shiftMode",1],["press",42],["press",30],["release",30],["releaseShift"]]"#,
            "42+ 30+ 30- 42-",
        ),
        // Shift lock and modifiers, then release everything.
        (
            r#"[["shiftMode",2],["press",54],["press",29],["press",56],["press",100],["press",30],["releaseAll"]]"#,
            "54+ 29+ 56+ 100+ 30+ 29- 30- 54- 56- 100-",
        ),
        // Releasing keys that are up changes nothing.
        (
            r#"[["release",30],["releaseShift"],["releaseAll"],["press",2],["release",2]]"#,
            "2+ 2-",
        ),
        // Both shifts, then a key typed after them; pressing a held key again.
        (
            r#"[["press",54],["press",42],["press",42],["releaseShift"],["press",16],["release",16]]"#,
            "54+ 42+ 42- 54- 16+ 16-",
        ),
        // A user's shift keys; the others stay down until releaseAll.
        (
            r#"[["shiftKeys",[54]],["shiftMode",1],["press",42],["press",54],["releaseShift"],["press",3],["releaseAll"]]"#,
            "42+ 54+ 54- 3+ 3- 42-",
        ),
        // High key codes.
        (
            r#"[["press",248],["press",249],["releaseAll"],["release",249]]"#,
            "248+ 249+ 248- 249-",
        ),
    ];
    for (k, (input, want)) in cases.iter().enumerate() {
        typing.clear();
        sc.take_trace();
        let (state, keys) = ab::side(&sc, false, input, &mut || typing.typed());
        assert_eq!(&keys, want, "{input}");
        assert_eq!(sc.take_trace(), "", "{input}: the shell ran ydotool");
        if k == 0 {
            assert!(state.contains("[1,1,1,1,0]"), "{state}");
        }
    }
}

/// The port's virtual keyboard has its own layout state (the first layout when it is created,
/// whatever `switchxkblayout` moved it to since), and its modifiers events hand that layout to
/// the focused window. A paste on the second layout of the real keyboard leaves the window on
/// the real keyboard's layout, also when Hyprland reports the port's keyboard as main.
#[test]
fn vkbd_keys_keep_the_real_keyboard_layout() {
    use std::io::{Read, Write};
    if !wayland()
        || !own_seat(
            concat!(module_path!(), "::vkbd_keys_keep_the_real_keyboard_layout"),
            true,
        )
    {
        return;
    }
    let _seat = seat();
    let ipc = std::os::unix::net::UnixListener::bind(hypr_dir().join(".socket.sock")).unwrap();
    // Hyprland writes its lock once its sockets listen.
    std::fs::write(hypr_dir().join("hyprland.lock"), "1\nwayland-1\n").unwrap();
    std::thread::spawn(move || {
        for mut s in ipc.incoming().flatten() {
            let mut buf = [0u8; 256];
            let n = s.read(&mut buf).unwrap_or(0);
            let reply = if &buf[..n] == b"j/devices" {
                r#"{"mice":[],"keyboards":[{"name":"hl-virtual-keyboard-rusty-impulse","layout":"us,ru","active_keymap":"English (US)","active_layout_index":0,"main":true},{"name":"usb-keyboard","layout":"us,ru","active_keymap":"Russian","active_layout_index":1,"main":false}]}"#
            } else {
                "unknown request"
            };
            let _ = s.write_all(reply.as_bytes());
        }
    });
    let keys = window::keys().expect("the key window never got the keyboard focus");
    window::touch(&keys);
    window::switch_layout(1);
    window::GROUPS.lock().unwrap().clear();
    // Ctrl+V, as a superpaste presses it.
    assert!(qml::ports::vkbd::send_wait(&[
        (29, true),
        (47, true),
        (47, false),
        (29, false)
    ]));
    window::wait_for(|| keys.lock().unwrap().contains(&(29, false)));
    assert_eq!(transitions(&keys.lock().unwrap()), "29+ 47+ 47- 29-");
    let groups = window::GROUPS.lock().unwrap().clone();
    assert!(!groups.is_empty(), "Ctrl sent no modifiers");
    assert!(
        groups.iter().all(|&g| g == 1),
        "the window left the real keyboard's layout: {groups:?}"
    );
}

/// A Hyprland instance for the port's keyboard: `j/devices` answers what [`FakeHypr::set`]
/// gave it, counting the reads, and the event socket's connections queue for
/// [`FakeHypr::events`], in this process's [`hypr_dir`].
struct FakeHypr {
    devices: Arc<Mutex<String>>,
    reads: Arc<std::sync::atomic::AtomicUsize>,
    events: std::os::unix::net::UnixListener,
}

impl FakeHypr {
    fn start() -> FakeHypr {
        use std::io::{Read, Write};
        let dir = hypr_dir();
        // The event socket listens before `.socket.sock` exists, as Hyprland's does.
        let events = std::os::unix::net::UnixListener::bind(dir.join(".socket2.sock")).unwrap();
        events.set_nonblocking(true).unwrap();
        let ipc = std::os::unix::net::UnixListener::bind(dir.join(".socket.sock")).unwrap();
        // Hyprland writes its lock once its sockets listen.
        std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
        let devices = Arc::new(Mutex::new(String::new()));
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (d, r) = (devices.clone(), reads.clone());
        std::thread::spawn(move || {
            for mut s in ipc.incoming().flatten() {
                let mut buf = [0u8; 256];
                let n = s.read(&mut buf).unwrap_or(0);
                let reply = if &buf[..n] == b"j/devices" {
                    r.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    d.lock().unwrap().clone()
                } else {
                    "unknown request".into()
                };
                let _ = s.write_all(reply.as_bytes());
            }
        });
        FakeHypr {
            devices,
            reads,
            events,
        }
    }

    /// `j/devices` from now on: the port's keyboard, main after it typed, and the real one.
    fn set(&self, layout: &str, index: u32, active: &str) {
        self.set_locks(layout, index, active, false, false);
    }

    /// [`FakeHypr::set`], with the real keyboard's CapsLock and NumLock as given.
    fn set_locks(&self, layout: &str, index: u32, active: &str, caps: bool, num: bool) {
        *self.devices.lock().unwrap() = format!(
            r#"{{"mice":[],"keyboards":[{{"name":"hl-virtual-keyboard-rusty-impulse","rules":"","model":"","layout":"","variant":"","options":"","active_layout_index":0,"active_keymap":"English (US)","capsLock":false,"numLock":false,"main":true}},{{"name":"usb-keyboard","rules":"","model":"","layout":"{layout}","variant":"","options":"","active_layout_index":{index},"active_keymap":"{active}","capsLock":{caps},"numLock":{num},"main":false}}]}}"#
        );
    }

    fn reads(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The event socket connection made so far, if any (a connect completes into the backlog,
    /// so one made by a call that returned is there).
    fn events(&self) -> Option<std::os::unix::net::UnixStream> {
        self.events.accept().ok().map(|(s, _)| s)
    }
}

/// KEY_V on the Russian layout.
const CYRILLIC_EM: u32 = 0x6cd;

/// A new keymap of the real keyboard (a changed `kb_layout`, a config reload) reaches the
/// port's keyboard: it types with the keymap of the keyboard whose layout index it takes, not
/// with the seat's keymap from when it connected.
#[test]
fn vkbd_types_with_the_real_keyboards_current_keymap() {
    use std::io::Write;
    if !wayland()
        || !own_seat(
            concat!(
                module_path!(),
                "::vkbd_types_with_the_real_keyboards_current_keymap"
            ),
            true,
        )
    {
        return;
    }
    let _seat = seat();
    let h = FakeHypr::start();
    h.set("us", 0, "English (US)");
    let keys = window::keys().expect("the key window never got the keyboard focus");
    window::set_keymap("us");
    window::touch(&keys);
    assert!(qml::ports::vkbd::send_wait(&[(30, true), (30, false)]));
    window::wait_for(|| keys.lock().unwrap().contains(&(30, false)));
    // The user adds a layout and switches the real keyboard to it.
    window::set_keymap("us,ru");
    window::touch(&keys);
    window::switch_layout(1);
    h.set("us,ru", 1, "Russian");
    if let Some(mut ev) = h.events() {
        ev.write_all(b"activelayout>>usb-keyboard,Russian\n")
            .unwrap();
    }
    keys.lock().unwrap().clear();
    assert!(qml::ports::vkbd::send_wait(&[
        (29, true),
        (47, true),
        (47, false),
        (29, false)
    ]));
    window::wait_for(|| keys.lock().unwrap().contains(&(29, false)));
    assert_eq!(transitions(&keys.lock().unwrap()), "29+ 47+ 47- 29-");
    assert_eq!(
        window::sym(47),
        CYRILLIC_EM,
        "the port typed V with a stale keymap"
    );
}

/// The port's keyboard reads `j/devices` once and again only after Hyprland's event socket
/// reports a layout change, not for every batch of keys (nor for the lock state of keys that
/// send no modifiers event).
#[test]
fn vkbd_reads_devices_again_only_after_a_layout_event() {
    use std::io::Write;
    if !wayland()
        || !own_seat(
            concat!(
                module_path!(),
                "::vkbd_reads_devices_again_only_after_a_layout_event"
            ),
            true,
        )
    {
        return;
    }
    let _seat = seat();
    let h = FakeHypr::start();
    h.set("us,ru", 0, "English (US)");
    let keys = window::keys().expect("the key window never got the keyboard focus");
    window::touch(&keys);
    for _ in 0..3 {
        assert!(qml::ports::vkbd::send_wait(&[(30, true), (30, false)]));
    }
    assert_eq!(h.reads(), 1, "j/devices was read for every batch");
    let mut ev = h
        .events()
        .expect("the keyboard follows Hyprland's event socket");
    h.set("us,ru", 1, "Russian");
    ev.write_all(b"activelayout>>usb-keyboard,Russian\n")
        .unwrap();
    // `send_wait` waited for the port's connection only. The window has the three batches
    // (with the first one's keymap and modifiers, still on layout 0) once it has the third
    // release; the modifiers events after it are those of the paste.
    let released = |k: &[(u32, bool)]| k.iter().filter(|&&k| k == (30, false)).count();
    window::wait_for(|| released(&keys.lock().unwrap()) == 3);
    keys.lock().unwrap().clear();
    window::GROUPS.lock().unwrap().clear();
    assert!(qml::ports::vkbd::send_wait(&[
        (29, true),
        (47, true),
        (47, false),
        (29, false)
    ]));
    window::wait_for(|| keys.lock().unwrap().contains(&(29, false)));
    assert_eq!(h.reads(), 2);
    let groups = window::GROUPS.lock().unwrap().clone();
    assert!(
        !groups.is_empty() && groups.iter().all(|&g| g == 1),
        "the keys missed the layout change: {groups:?}"
    );
    assert_eq!(window::sym(47), CYRILLIC_EM);
}

/// Sends Ctrl+V (as a superpaste presses it) and returns the modifiers events
/// the window got for it.
fn paste(keys: &Keys) -> Vec<window::Mods> {
    keys.lock().unwrap().clear();
    window::MODS.lock().unwrap().clear();
    assert!(qml::ports::vkbd::send_wait(&[
        (29, true),
        (47, true),
        (47, false),
        (29, false)
    ]));
    window::wait_for(|| {
        keys.lock().unwrap().contains(&(29, false))
            && window::MODS
                .lock()
                .unwrap()
                .last()
                .is_some_and(|m| m.depressed == 0)
    });
    assert_eq!(transitions(&keys.lock().unwrap()), "29+ 47+ 47- 29-");
    window::MODS.lock().unwrap().clone()
}

/// The port's keyboard's modifiers events hand the window its locked modifiers too: a paste
/// leaves the window with the real keyboard's CapsLock and NumLock (`capsLock`, `numLock` of
/// `j/devices`, also when changed without an event), and a NumLock of the port's own stays.
#[test]
fn vkbd_keys_keep_the_real_keyboards_lock_state() {
    use xkbcommon::xkb;
    if !wayland()
        || !own_seat(
            concat!(
                module_path!(),
                "::vkbd_keys_keep_the_real_keyboards_lock_state"
            ),
            true,
        )
    {
        return;
    }
    let _seat = seat();
    let h = FakeHypr::start();
    h.set_locks("us,ru", 0, "English (US)", true, true);
    let keys = window::keys().expect("the key window never got the keyboard focus");
    window::touch(&keys);
    let locks = |mods: &[window::Mods]| -> Vec<(bool, bool)> {
        mods.iter()
            .map(|m| (m.locks(xkb::MOD_NAME_CAPS), m.locks(xkb::MOD_NAME_NUM)))
            .collect()
    };
    let mods = paste(&keys);
    assert!(
        !mods.is_empty() && locks(&mods).iter().all(|&l| l == (true, true)),
        "the paste turned the real keyboard's locks off: {mods:?}"
    );
    // NumLock goes off on the real keyboard; Hyprland reports no event for it.
    h.set_locks("us,ru", 0, "English (US)", true, false);
    let mods = paste(&keys);
    assert!(
        locks(&mods).iter().all(|&l| l == (true, false)),
        "the paste missed the real keyboard's NumLock change: {mods:?}"
    );
    // The on-screen keyboard's own NumLock key.
    keys.lock().unwrap().clear();
    window::MODS.lock().unwrap().clear();
    assert!(qml::ports::vkbd::send_wait(&[(69, true), (69, false)]));
    window::wait_for(|| {
        keys.lock().unwrap().contains(&(69, false)) && window::MODS.lock().unwrap().len() >= 2
    });
    let mods = paste(&keys);
    assert!(
        locks(&mods).iter().all(|&l| l == (true, true)),
        "the paste undid the port's own NumLock: {mods:?}"
    );
}

/// The real keyboard switched alone while the port sends no keys (`switchxkblayout current
/// next`, an xkb `grp:` option: Hyprland keeps a layout per keyboard and leaves the port's on
/// the first): the port's keyboard goes to the real one's layout at Hyprland's event, not with
/// its next keys, so a later switch of all keyboards moves both from the same layout.
#[test]
fn vkbd_follows_the_real_keyboard_switched_alone() {
    use std::io::Write;
    if !wayland()
        || !own_seat(
            concat!(
                module_path!(),
                "::vkbd_follows_the_real_keyboard_switched_alone"
            ),
            true,
        )
    {
        return;
    }
    let _seat = seat();
    let h = FakeHypr::start();
    h.set("us,ru", 0, "English (US)");
    let keys = window::keys().expect("the key window never got the keyboard focus");
    window::touch(&keys);
    assert!(qml::ports::vkbd::send_wait(&[(30, true), (30, false)]));
    window::wait_for(|| keys.lock().unwrap().contains(&(30, false)));
    let mut ev = h
        .events()
        .expect("the keyboard follows Hyprland's event socket");
    keys.lock().unwrap().clear();
    window::GROUPS.lock().unwrap().clear();
    // `j/devices` now has the real keyboard on the second layout and the port's on the first.
    h.set("us,ru", 1, "Russian");
    ev.write_all(b"activelayout>>usb-keyboard,Russian\n")
        .unwrap();
    // Nothing but the port's keyboard sends this window a layout from here on.
    window::wait_for(|| window::GROUPS.lock().unwrap().last() == Some(&1));
    assert_eq!(h.reads(), 2, "one read of j/devices for the event");
    assert!(keys.lock().unwrap().is_empty(), "following sends no keys");
}

/// Releasing the keys the port's keyboard holds (the on-screen keyboard closing with Shift
/// held) goes out on the real keyboard's current layout: the released Shift's modifiers event
/// does not move the window back to the layout of the last batch.
#[test]
fn vkbd_release_all_keeps_the_real_keyboard_layout() {
    use std::io::Write;
    if !wayland()
        || !own_seat(
            concat!(
                module_path!(),
                "::vkbd_release_all_keeps_the_real_keyboard_layout"
            ),
            true,
        )
    {
        return;
    }
    let _seat = seat();
    let h = FakeHypr::start();
    h.set("us,ru", 1, "Russian");
    let keys = window::keys().expect("the key window never got the keyboard focus");
    window::touch(&keys);
    window::switch_layout(1);
    assert!(qml::ports::vkbd::send_wait(&[(42, true)]));
    window::wait_for(|| keys.lock().unwrap().contains(&(42, true)));
    let mut ev = h
        .events()
        .expect("the keyboard follows Hyprland's event socket");
    // The user switches the real keyboard back to the first layout.
    window::switch_layout(0);
    h.set("us,ru", 0, "English (US)");
    ev.write_all(b"activelayout>>usb-keyboard,English (US)\n")
        .unwrap();
    keys.lock().unwrap().clear();
    window::MODS.lock().unwrap().clear();
    qml::ports::vkbd::release_all();
    window::wait_for(|| {
        keys.lock().unwrap().contains(&(42, false))
            && window::MODS
                .lock()
                .unwrap()
                .last()
                .is_some_and(|m| m.depressed == 0)
    });
    assert_eq!(*keys.lock().unwrap(), [(42, false)]);
    let mods = window::MODS.lock().unwrap().clone();
    assert!(
        mods.iter().all(|m| m.group == 0),
        "releasing Shift moved the window off the real keyboard's layout: {mods:?}"
    );
}

/// One call's keys go out as one batch with strictly increasing `time`s: no pause between
/// them, and no two events of the batch claim the same millisecond.
#[test]
fn vkbd_batch_times_strictly_increase() {
    if !wayland()
        || !own_seat(
            concat!(module_path!(), "::vkbd_batch_times_strictly_increase"),
            false,
        )
    {
        return;
    }
    let _seat = seat();
    let keys = window::keys().expect("the key window never got the keyboard focus");
    window::touch(&keys);
    window::TIMES.lock().unwrap().clear();
    let batch: Vec<(u32, bool)> = (0..32).flat_map(|_| [(30, true), (30, false)]).collect();
    assert!(qml::ports::vkbd::send_wait(&batch));
    window::wait_for(|| window::TIMES.lock().unwrap().len() >= batch.len());
    let t = window::TIMES.lock().unwrap().clone();
    assert!(t.windows(2).all(|w| w[1] > w[0]), "times {t:?}");
}
