//! The `KeySender` singleton (`import Impulse`; on-screen keyboard, paste key) through
//! `zwp_virtual_keyboard_v1` from this process instead of a `ydotool` process per key (and
//! the `ydotoold` uinput daemon). Keycodes are evdev codes as with ydotool; the keyboard
//! uses the real keyboard's keymap, so keys mean what they mean on the physical keyboard.
//!
//! One thread owns the connection; key events queue to it in call order. Each call's keys go
//! out as one batch, with strictly increasing `time`s, and the batch ends with a
//! `wl_display.sync`: the socket keeps them in order, so no pause between keys is needed (the
//! delay `ydotool key -d` takes is for the kernel's uinput batching).
//!
//! The keyboard has its own keymap and xkb state, so its layout (group) is its own too: its
//! `modifiers` events carry that group, and the compositor forwards them to the focused client
//! (`wl_keyboard.modifiers`), so a paste's Ctrl would put the client on another layout than
//! the real keyboard; Hyprland sends nothing when the real keyboard types again. Each batch
//! therefore starts on the keymap and layout of the real keyboard Hyprland reports: both come
//! from the same `j/devices` entry (its xkb rules, model, layout, variant and options, compiled
//! as Hyprland compiles them, and its `active_layout_index`), checked against the layout name
//! Hyprland reports for it (`active_keymap`). A keyboard whose keymap Hyprland did not compile
//! from those names (`kb_file`) uses the seat's keymap when that one names the same layout.
//! The seat's `wl_keyboard` stays bound, so its keymap is the current one whenever it changes
//! (wl_keyboard.keymap may come at any time). `j/devices` is read again only when Hyprland's
//! event socket reported a layout or configuration change (`activelayout>>`,
//! `configreloaded>>`) or (re)connected, not for every key; without an event socket it is read
//! for every batch. A thread of its own reads the event socket as Hyprland writes it, also
//! while no keys are sent: Hyprland queues the events of a client that does not read them and
//! drops the client once that queue overflows.
//!
//! The `modifiers` events carry the keyboard's locked modifiers too, and Hyprland hands the
//! focused window those of the keyboard that sent them, so CapsLock and NumLock would go off in
//! the window with each paste. The lock state is the real keyboard's (`capsLock`, `numLock`:
//! `XKB_MOD_NAME_CAPS`, `XKB_MOD_NAME_NUM` locked in its state) whenever it changed since this
//! keyboard last saw it, else this keyboard's own (a NumLock key of its own stays). No event
//! reports a lock change, so `j/devices` is read again for the lock state, but only when a
//! `modifiers` event goes out: a new keymap or layout, or a key that changes the modifiers.

use crate::eval::{Interp, R};
use crate::value::Value;
use std::collections::BTreeSet;
use std::io::Read;
use std::net::Shutdown;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use wayland_client::protocol::{wl_keyboard, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use xkbcommon::xkb;

enum Msg {
    /// (evdev keycode, pressed) events, sent as one batch; `done` gets whether they were sent.
    Keys(Vec<(u32, bool)>, Option<Sender<bool>>),
    /// Release every key this keyboard holds down.
    ReleaseAll,
}

/// The channel to the thread that owns the connection, started on first use.
struct Port {
    tx: Mutex<Option<Sender<Msg>>>,
    connect: fn() -> Option<Kb>,
    layout: Layout,
}

/// The real keyboard's keymap names, layout and locks, from the state kept between batches;
/// with `true`, its lock state as it is now (read again unless read for this batch already).
type Layout = fn(&mut Hyprland, bool) -> Option<RealKb>;

static PORT: Port = Port::new(Kb::connect, Hyprland::real_keyboard);

impl Port {
    const fn new(connect: fn() -> Option<Kb>, layout: Layout) -> Port {
        Port {
            tx: Mutex::new(None),
            connect,
            layout,
        }
    }

    fn post(&self, m: Msg) {
        // A panic elsewhere while holding `tx` must not take the main thread down on the next
        // OSK key.
        let mut g = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        let m = match g.as_ref() {
            Some(tx) => match tx.send(m) {
                Ok(()) => return,
                Err(e) => e.0,
            },
            None => m,
        };
        // First use, or the worker died: start a fresh one.
        let (tx, rx) = channel();
        let (connect, layout) = (self.connect, self.layout);
        if let Err(e) = std::thread::Builder::new()
            .name("ri-port-vkbd".into())
            .spawn(move || worker(rx, connect, layout))
        {
            // Refused by the system (`EAGAIN`): these keys are not sent. `tx` has no receiver
            // then, so the next call's send fails and comes here to start the thread again.
            elog::eprintln!("KeySender: cannot start the virtual keyboard thread: {e}");
        }
        let _ = tx.send(m);
        *g = Some(tx);
    }

    fn send_wait(&self, keys: &[(u32, bool)]) -> bool {
        let (tx, rx) = channel();
        self.post(Msg::Keys(keys.to_vec(), Some(tx)));
        rx.recv().unwrap_or(false)
    }
}

/// Queues key events.
pub fn send(keys: &[(u32, bool)]) {
    PORT.post(Msg::Keys(keys.to_vec(), None));
}

/// Sends key events and waits until the compositor has them (`ydotool key` returning).
pub fn send_wait(keys: &[(u32, bool)]) -> bool {
    PORT.send_wait(keys)
}

pub fn release_all() {
    PORT.post(Msg::ReleaseAll);
}

/// Blocks until every key event queued so far has reached the compositor (or failed to);
/// starts no keyboard when none was used. For a host that is about to exit, and tests.
pub fn drain() {
    let (tx, rx) = channel();
    let queued = PORT
        .tx
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|t| t.send(Msg::Keys(vec![], Some(tx))).is_ok());
    if queued {
        let _ = rx.recv();
    }
}

#[derive(Default)]
struct St {
    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mgr: Option<ZwpVirtualKeyboardManagerV1>,
    /// The seat's current keymap (wl_keyboard.keymap), without the terminating NUL.
    keymap: Option<String>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for St {
    fn event(
        s: &mut St,
        r: &wl_registry::WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<St>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = e
        {
            match interface.as_str() {
                "wl_seat" if s.seat.is_none() => {
                    s.seat = Some(r.bind(name, version.min(7), qh, ()))
                }
                "zwp_virtual_keyboard_manager_v1" => s.mgr = Some(r.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for St {
    fn event(
        s: &mut St,
        _: &wl_keyboard::WlKeyboard,
        e: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<St>,
    ) {
        if let wl_keyboard::Event::Keymap { format, fd, size } = e {
            if format != WEnum::Value(wl_keyboard::KeymapFormat::XkbV1) {
                s.keymap = None;
                return;
            }
            let mut buf = vec![0u8; size as usize];
            let file = std::fs::File::from(fd);
            use std::os::unix::fs::FileExt;
            if file.read_exact_at(&mut buf, 0).is_ok() {
                let text = String::from_utf8_lossy(&buf);
                s.keymap = Some(text.trim_end_matches('\0').to_owned());
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for St {
    fn event(
        s: &mut St,
        seat: &wl_seat::WlSeat,
        e: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<St>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(c),
        } = e
        {
            // get_keyboard on a seat without a keyboard is a protocol error that kills the
            // connection; a keyboard plugged in later gets bound then.
            let has = c.contains(wl_seat::Capability::Keyboard);
            match (has, s.keyboard.take()) {
                (true, None) => s.keyboard = Some(seat.get_keyboard(qh, ())),
                (true, kb) => s.keyboard = kb,
                (false, Some(kb)) => {
                    if kb.version() >= 3 {
                        kb.release();
                    }
                    s.keymap = None;
                }
                (false, None) => {}
            }
        }
    }
}

delegate_noop!(St: ignore ZwpVirtualKeyboardManagerV1);
delegate_noop!(St: ignore ZwpVirtualKeyboardV1);

/// xkbcommon's default keymap (`XKB_DEFAULT_*`, else the library's build defaults).
fn default_keymap(ctx: &xkb::Context) -> Option<xkb::Keymap> {
    xkb::Keymap::new_from_names(ctx, "", "", "", "", None, xkb::KEYMAP_COMPILE_NO_FLAGS)
}

fn parse_keymap(ctx: &xkb::Context, text: &str) -> Option<xkb::Keymap> {
    xkb::Keymap::new_from_string(
        ctx,
        text.to_owned(),
        xkb::KEYMAP_FORMAT_TEXT_V1,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
}

/// A real keyboard as Hyprland's `j/devices` reports it.
#[derive(Clone, Debug, Default, PartialEq)]
struct RealKb {
    /// The xkb rules, model, layout, variant and options Hyprland compiled its keymap from.
    names: [String; 5],
    /// `active_layout_index`.
    index: u32,
    /// `active_keymap`: the name of layout `index` in that keymap, when reported.
    active: Option<String>,
    /// `capsLock` and `numLock`, when reported.
    locks: Option<Locks>,
}

/// Which of CapsLock and NumLock are on: `XKB_MOD_NAME_CAPS` and `XKB_MOD_NAME_NUM` locked,
/// as Hyprland reports them for a keyboard and sets them (`numlock_by_default`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Locks {
    caps: bool,
    num: bool,
}

impl Locks {
    fn of(x: &xkb::State) -> Locks {
        Locks {
            caps: x.mod_name_is_active(xkb::MOD_NAME_CAPS, xkb::STATE_MODS_LOCKED),
            num: x.mod_name_is_active(xkb::MOD_NAME_NUM, xkb::STATE_MODS_LOCKED),
        }
    }

    /// The locks that differ between `self` and `o` (as a key toggles them).
    fn xor(self, o: Locks) -> Locks {
        Locks {
            caps: self.caps != o.caps,
            num: self.num != o.num,
        }
    }

    /// `locked` (a mask of `km`'s modifiers) with exactly these locks on.
    fn mask(self, km: &xkb::Keymap, mut locked: u32) -> u32 {
        for (name, on) in [
            (xkb::MOD_NAME_CAPS, self.caps),
            (xkb::MOD_NAME_NUM, self.num),
        ] {
            let i = km.mod_get_index(name);
            if i < u32::BITS {
                if on {
                    locked |= 1 << i;
                } else {
                    locked &= !(1 << i);
                }
            }
        }
        locked
    }
}

impl RealKb {
    /// The keymap Hyprland compiles from the keyboard's names (`IKeyboard::updateXKBTranslationState`).
    fn compile(&self, ctx: &xkb::Context) -> Option<xkb::Keymap> {
        let [r, m, l, v, o] = &self.names;
        xkb::Keymap::new_from_names(
            ctx,
            r,
            m,
            l,
            v,
            Some(o.clone()),
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
    }

    /// Whether `km` is this keyboard's keymap as far as Hyprland tells: it has layout `index`,
    /// under the name Hyprland reports for it.
    fn matches(&self, km: &xkb::Keymap) -> bool {
        self.index < km.num_layouts()
            && self
                .active
                .as_deref()
                .is_none_or(|a| km.layout_get_name(self.index) == a)
    }
}

/// What the keys need from Hyprland, kept between batches.
#[derive(Default)]
struct Hyprland {
    /// The real keyboard the layout was last taken from.
    last: Option<String>,
    /// `j/devices` as last read; good until the event socket reports a change, except for the
    /// lock state.
    devices: Option<String>,
    /// `devices` was read for the batch being sent.
    current: bool,
    events: Option<Subscription>,
}

/// An event after which a keyboard's keymap or layout may differ: a layout switch (also sent
/// when a keyboard is configured, `CInputManager::applyConfigToKeyboard`) or a config reload.
fn keyboard_event(line: &[u8]) -> bool {
    line.starts_with(b"activelayout>>") || line.starts_with(b"configreloaded>>")
}

/// Hyprland's event socket (non-blocking), read by the `ri-vkbd-events` thread whenever it
/// is readable, and by each batch for what arrived up to then.
struct Events {
    sock: UnixStream,
    st: Mutex<EventsSt>,
}

#[derive(Default)]
struct EventsSt {
    /// The start of a line not complete yet.
    partial: Vec<u8>,
    /// A keyboard event arrived since the last batch took this.
    changed: bool,
    /// Hyprland closed the socket, or reading it failed.
    closed: bool,
}

impl Events {
    /// Reads everything that already arrived.
    fn drain(&self) -> MutexGuard<'_, EventsSt> {
        let mut st = self.st.lock().unwrap_or_else(|e| e.into_inner());
        let mut chunk = [0u8; 4096];
        while !st.closed {
            match (&self.sock).read(&mut chunk) {
                Ok(0) => st.closed = true,
                Ok(n) => {
                    st.partial.extend_from_slice(&chunk[..n]);
                    while let Some(i) = st.partial.iter().position(|&b| b == b'\n') {
                        st.changed |= keyboard_event(&st.partial[..i]);
                        st.partial.drain(..=i);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => st.closed = true,
            }
        }
        st
    }

    /// The reading thread: waits for the socket to be readable and drains it, until it closes.
    fn follow(&self) {
        loop {
            let mut p = libc::pollfd {
                fd: self.sock.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: `p` is one writable pollfd over `self.sock`, open while `self` lives.
            let n = unsafe { libc::poll(&mut p, 1, -1) };
            if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                self.st.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
                return;
            }
            if self.drain().closed {
                return;
            }
        }
    }
}

/// The batches' handle on the event socket. Dropping it shuts the socket down, which ends the
/// reading thread (it sees end of file) and the subscription with it.
struct Subscription(Arc<Events>);

impl Subscription {
    fn connect(dir: &Path) -> Option<Subscription> {
        let sock = UnixStream::connect(dir.join(".socket2.sock")).ok()?;
        sock.set_nonblocking(true).ok()?;
        let ev = Arc::new(Events {
            sock,
            st: Mutex::default(),
        });
        let reader = ev.clone();
        // Without the thread nobody would read between batches: no subscription then, and
        // the socket is closed with `ev`. The next batch connects again.
        std::thread::Builder::new()
            .name("ri-vkbd-events".into())
            .spawn(move || reader.follow())
            .inspect_err(|e| {
                elog::eprintln!("KeySender: cannot start the ri-vkbd-events thread: {e}");
            })
            .ok()?;
        Some(Subscription(ev))
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.0.sock.shutdown(Shutdown::Both);
    }
}

impl Hyprland {
    /// Whether `j/devices` may have changed since it was read: the event socket reported a
    /// keyboard event, or it is not connected (a broken one is connected again first, so the
    /// reply read next is followed by its events).
    fn keyboards_changed(&mut self, dir: &Path) -> bool {
        if let Some(ev) = &self.events {
            let mut st = ev.0.drain();
            if !st.closed {
                return std::mem::take(&mut st.changed);
            }
            drop(st);
            self.events = None;
        }
        self.events = Subscription::connect(dir);
        true
    }

    /// The real keyboard the user types on, from `j/devices`: at the start of a batch read again
    /// only after a change; with `locks`, read again for its current lock state unless it was
    /// read for this batch already.
    fn real_keyboard(&mut self, locks: bool) -> Option<RealKb> {
        let dir = crate::hypr::socket_dir()?;
        if !locks {
            self.current = false;
        } else if !self.current {
            self.devices = None;
        }
        if self.keyboards_changed(&dir) || self.devices.is_none() {
            self.devices = crate::hypr::request(&dir, "j/devices");
            self.current = true;
        }
        let kb = real_keyboard(self.devices.as_deref()?, &mut self.last);
        if kb.is_none() {
            // An error reply is not kept.
            self.devices = None;
        }
        kb
    }
}

/// The main keyboard when it is a real one; while a virtual keyboard (this one, after it
/// typed) is main, the real keyboard `last` named, else the first.
fn real_keyboard(devices: &str, last: &mut Option<String>) -> Option<RealKb> {
    let devices: serde_json::Value = serde_json::from_str(devices).ok()?;
    let real: Vec<&serde_json::Value> = devices
        .get("keyboards")?
        .as_array()?
        .iter()
        .filter(|k| {
            k["name"]
                .as_str()
                .is_some_and(|n| !crate::hypr::is_virtual_keyboard(n))
        })
        .collect();
    let kb = real
        .iter()
        .find(|k| k["main"] == true)
        .or_else(|| {
            real.iter()
                .find(|k| last.is_some() && k["name"].as_str() == last.as_deref())
        })
        .or_else(|| real.first())?;
    *last = kb["name"].as_str().map(str::to_owned);
    let s = |k: &str| kb[k].as_str().unwrap_or_default().to_owned();
    Some(RealKb {
        names: [
            s("rules"),
            s("model"),
            s("layout"),
            s("variant"),
            s("options"),
        ],
        index: u32::try_from(kb["active_layout_index"].as_u64()?).ok()?,
        active: kb["active_keymap"].as_str().map(str::to_owned),
        locks: match (kb["capsLock"].as_bool(), kb["numLock"].as_bool()) {
            (Some(caps), Some(num)) => Some(Locks { caps, num }),
            _ => None,
        },
    })
}

struct Kb {
    conn: Connection,
    q: EventQueue<St>,
    st: St,
    vk: ZwpVirtualKeyboardV1,
    ctx: xkb::Context,
    /// The keymap uploaded to `vk` (as xkbcommon prints it) and the state that tracks it.
    keymap: Option<(String, xkb::State)>,
    down: BTreeSet<u32>,
    t0: Instant,
    /// `time` of the last key event.
    last_time: Option<u32>,
    /// The real keyboard's locks as this keyboard last took them.
    real_locks: Option<Locks>,
}

impl Kb {
    fn connect() -> Option<Kb> {
        let conn = Connection::connect_to_env().ok()?;
        let mut q = conn.new_event_queue();
        let qh = q.handle();
        conn.display().get_registry(&qh, ());
        let mut st = St::default();
        q.roundtrip(&mut st).ok()?;
        let (seat, mgr) = (st.seat.clone()?, st.mgr.clone()?);
        // wl_seat.capabilities (binding the keyboard), then its keymap.
        q.roundtrip(&mut st).ok()?;
        q.roundtrip(&mut st).ok()?;
        let vk = mgr.create_virtual_keyboard(&seat, &qh, ());
        Some(Kb {
            conn,
            q,
            st,
            vk,
            ctx: xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
            keymap: None,
            down: BTreeSet::new(),
            t0: Instant::now(),
            last_time: None,
            real_locks: None,
        })
    }

    /// The keymap for the next keys and the layout to put them on: the real keyboard's
    /// compiled from its names, else the seat's when it names the same layout; without one
    /// that matches, the seat's keymap (or the one in use, or xkbcommon's default) as it is.
    fn choose(&self, real: Option<&RealKb>) -> Option<(xkb::Keymap, Option<u32>)> {
        let seat = || {
            self.st
                .keymap
                .as_deref()
                .and_then(|t| parse_keymap(&self.ctx, t))
        };
        if let Some(r) = real {
            if let Some(km) = r.compile(&self.ctx).filter(|km| r.matches(km)) {
                return Some((km, Some(r.index)));
            }
            if let Some(km) = seat().filter(|km| r.matches(km)) {
                return Some((km, Some(r.index)));
            }
        }
        let km = seat()
            .or_else(|| self.keymap.as_ref().map(|(_, s)| s.get_keymap()))
            .or_else(|| default_keymap(&self.ctx))?;
        Some((km, real.map(|r| r.index)))
    }

    /// Uploads `km` unless it is the keymap in use; the keys held down stay down and the locks
    /// stay on in the new state. `Some(uploaded)`, `None` when it could not be uploaded.
    fn use_keymap(&mut self, km: &xkb::Keymap) -> Option<bool> {
        let text = km.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        if self.keymap.as_ref().is_some_and(|(t, _)| *t == text) {
            return Some(false);
        }
        let fd =
            rustix::fs::memfd_create("ri-vkbd-keymap", rustix::fs::MemfdFlags::CLOEXEC).ok()?;
        let file = std::fs::File::from(fd);
        use std::io::Write;
        // wl_keyboard.keymap sizes include the terminating NUL.
        (&file).write_all(text.as_bytes()).ok()?;
        (&file).write_all(&[0]).ok()?;
        self.vk.keymap(
            wl_keyboard::KeymapFormat::XkbV1 as u32,
            file.as_fd(),
            text.len() as u32 + 1,
        );
        let mut state = xkb::State::new(km);
        for &c in &self.down {
            state.update_key(xkb::Keycode::new(c + 8), xkb::KeyDirection::Down);
        }
        if let Some((_, old)) = &self.keymap {
            let locked = Locks::of(old).mask(km, state.serialize_mods(xkb::STATE_MODS_LOCKED));
            state.update_mask(
                state.serialize_mods(xkb::STATE_MODS_DEPRESSED),
                state.serialize_mods(xkb::STATE_MODS_LATCHED),
                locked,
                0,
                0,
                0,
            );
        }
        self.keymap = Some((text, state));
        Some(true)
    }

    /// Takes the seat's keymap events so far, then puts the keyboard on the keymap and layout
    /// of `real` for the next keys, telling the compositor when either changed.
    fn prepare(&mut self, real: Option<RealKb>, locks: &mut dyn FnMut() -> Option<Locks>) -> bool {
        if self.q.roundtrip(&mut self.st).is_err() {
            return false;
        }
        let Some((km, group)) = self.choose(real.as_ref()) else {
            return false;
        };
        let Some(uploaded) = self.use_keymap(&km) else {
            return false;
        };
        let on = |g| {
            self.keymap
                .as_ref()
                .is_some_and(|(_, x)| x.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE) == g)
        };
        if let Some(g) = group.filter(|&g| uploaded || !on(g)) {
            self.set_layout(g, locks);
        }
        true
    }

    /// Sends one batch of keys: on the real keyboard's keymap and layout, with its current lock
    /// state taken (from `layout`) only for a `modifiers` event.
    fn batch(&mut self, keys: &[(u32, bool)], hypr: &mut Hyprland, layout: Layout) -> bool {
        if !keys.is_empty() {
            let real = layout(hypr, false);
            let mut locks = || layout(hypr, true).and_then(|r| r.locks);
            if !self.prepare(real, &mut locks) {
                return false;
            }
            for &(c, p) in keys {
                self.key(c, p, &mut locks);
            }
        }
        self.sync()
    }

    /// Takes the real keyboard's `locks` when they changed since this keyboard last took them,
    /// with the locks this keyboard's own key just `toggled` on top.
    fn follow_locks(&mut self, locks: Option<Locks>, toggled: Locks) {
        let Some((_, x)) = &mut self.keymap else {
            return;
        };
        let Some(real) = locks.filter(|&l| self.real_locks != Some(l)) else {
            return;
        };
        self.real_locks = Some(real);
        let locked = real
            .xor(toggled)
            .mask(&x.get_keymap(), x.serialize_mods(xkb::STATE_MODS_LOCKED));
        x.update_mask(
            x.serialize_mods(xkb::STATE_MODS_DEPRESSED),
            x.serialize_mods(xkb::STATE_MODS_LATCHED),
            locked,
            x.serialize_layout(xkb::STATE_LAYOUT_DEPRESSED),
            x.serialize_layout(xkb::STATE_LAYOUT_LATCHED),
            x.serialize_layout(xkb::STATE_LAYOUT_LOCKED),
        );
    }

    /// Tells the compositor this keyboard's modifiers and layout.
    fn send_modifiers(&self) {
        if let Some((_, x)) = &self.keymap {
            self.vk.modifiers(
                x.serialize_mods(xkb::STATE_MODS_DEPRESSED),
                x.serialize_mods(xkb::STATE_MODS_LATCHED),
                x.serialize_mods(xkb::STATE_MODS_LOCKED),
                x.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE),
            );
        }
    }

    /// A `time` later than the previous key's, from this keyboard's clock.
    fn time(&mut self) -> u32 {
        let now = self.t0.elapsed().as_millis() as u32;
        let t = match self.last_time {
            Some(l) if (now.wrapping_sub(l) as i32) <= 0 => l.wrapping_add(1),
            _ => now,
        };
        self.last_time = Some(t);
        t
    }

    fn key(&mut self, code: u32, pressed: bool, locks: &mut dyn FnMut() -> Option<Locks>) {
        // A key already up/down is not sent again (the kernel drops such uinput events).
        if pressed == self.down.contains(&code) {
            return;
        }
        if pressed {
            self.down.insert(code);
        } else {
            self.down.remove(&code);
        }
        let t = self.time();
        self.vk.key(t, code, pressed as u32);
        let Some((_, x)) = &mut self.keymap else {
            return;
        };
        let dir = if pressed {
            xkb::KeyDirection::Down
        } else {
            xkb::KeyDirection::Up
        };
        let before = Locks::of(x);
        if x.update_key(xkb::Keycode::new(code + 8), dir) != 0 {
            let toggled = before.xor(Locks::of(x));
            self.follow_locks(locks(), toggled);
            self.send_modifiers();
        }
    }

    /// Puts this keyboard on layout `group` (held modifiers kept, the real keyboard's locks
    /// taken) and tells the compositor.
    fn set_layout(&mut self, group: u32, locks: &mut dyn FnMut() -> Option<Locks>) {
        self.follow_locks(locks(), Locks::default());
        let Some((_, x)) = &mut self.keymap else {
            return;
        };
        x.update_mask(
            x.serialize_mods(xkb::STATE_MODS_DEPRESSED),
            x.serialize_mods(xkb::STATE_MODS_LATCHED),
            x.serialize_mods(xkb::STATE_MODS_LOCKED),
            0,
            0,
            group,
        );
        self.send_modifiers();
    }

    fn sync(&mut self) -> bool {
        self.conn.flush().is_ok() && self.q.roundtrip(&mut self.st).is_ok()
    }
}

fn worker(rx: Receiver<Msg>, connect: fn() -> Option<Kb>, layout: Layout) {
    let mut kb: Option<Kb> = None;
    let mut hypr = Hyprland::default();
    for m in rx {
        if kb.is_none() {
            kb = connect();
        }
        let ok = match (&mut kb, m) {
            (Some(k), Msg::Keys(keys, done)) => {
                let ok = k.batch(&keys, &mut hypr, layout);
                if let Some(d) = done {
                    let _ = d.send(ok);
                }
                ok
            }
            (Some(k), Msg::ReleaseAll) => {
                // On the real keyboard's current layout, as any other keys: a released Shift's
                // `modifiers` event must not move the window back to the layout of the last batch.
                let up: Vec<(u32, bool)> = k.down.iter().map(|&c| (c, false)).collect();
                k.batch(&up, &mut hypr, layout)
            }
            (None, Msg::Keys(_, Some(d))) => {
                let _ = d.send(false);
                true
            }
            (None, _) => true,
        };
        if !ok {
            kb = None;
        }
    }
}

fn keycode(v: Option<&Value>) -> Option<u32> {
    let n = v?.to_number();
    (n.is_finite() && n >= 0.0).then_some(n as u32)
}

/// `KeySender` (qmltypes/Impulse/key-sender.qmltypes).
pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "KeySender",
    methods: &[
        ("press", press),
        ("release", release),
        ("releaseAll", release_all_keys),
    ],
    init: None,
};

/// [`crate::impulse::hooked`] for `method` of the singleton: under a spawn hook no keyboard
/// is created and no key reaches the compositor.
fn hooked(method: &str, key: Option<u32>) -> bool {
    let key = key.map(|c| c.to_string());
    let args: Vec<&str> = key.iter().map(String::as_str).collect();
    crate::impulse::hooked("KeySender", method, &args)
}

/// `press(keycode)`: the key (a Linux evdev code) goes down.
fn press(_: &mut Interp, _: &Value, args: &[Value]) -> R<Value> {
    if let Some(c) = keycode(args.first())
        && !hooked("press", Some(c))
    {
        send(&[(c, true)]);
    }
    Ok(Value::Undefined)
}

/// `release(keycode)`: the key goes up.
fn release(_: &mut Interp, _: &Value, args: &[Value]) -> R<Value> {
    if let Some(c) = keycode(args.first())
        && !hooked("release", Some(c))
    {
        send(&[(c, false)]);
    }
    Ok(Value::Undefined)
}

/// `releaseAll()`: every key this keyboard holds down goes up.
fn release_all_keys(_: &mut Interp, _: &Value, _: &[Value]) -> R<Value> {
    if !hooked("releaseAll", None) {
        release_all();
    }
    Ok(Value::Undefined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keymap_compiles() {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let km = default_keymap(&ctx).expect("xkbcommon default keymap");
        assert!(km.num_layouts() >= 1);
        let text = km.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        let parsed = parse_keymap(&ctx, &text).expect("keymap parses back");
        // What `use_keymap` compares: printing is stable across a round trip.
        assert_eq!(parsed.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1), text);
    }

    #[test]
    fn layout_is_the_real_keyboards() {
        let devices = |main_real: bool| {
            format!(
                r#"{{"mice":[],"keyboards":[
                {{"name":"mic-buttons","main":false,"layout":"us","active_layout_index":0}},
                {{"name":"usb-keyboard","main":{main_real},"rules":"","model":"pc105","layout":"us,ru","variant":"","options":"grp:alt_shift_toggle","active_layout_index":1,"active_keymap":"Russian","capsLock":false,"numLock":true}},
                {{"name":"hl-virtual-keyboard-rusty-impulse","main":{},"active_layout_index":0}}]}}"#,
                !main_real
            )
        };
        let usb = RealKb {
            names: [
                "".into(),
                "pc105".into(),
                "us,ru".into(),
                "".into(),
                "grp:alt_shift_toggle".into(),
            ],
            index: 1,
            active: Some("Russian".into()),
            locks: Some(Locks {
                caps: false,
                num: true,
            }),
        };
        let mut last = None;
        assert_eq!(real_keyboard(&devices(true), &mut last), Some(usb.clone()));
        assert_eq!(last.as_deref(), Some("usb-keyboard"));
        // This keyboard typed last, so Hyprland reports it as main: the real one stays.
        assert_eq!(real_keyboard(&devices(false), &mut last), Some(usb));
        let mut none = None;
        assert_eq!(
            real_keyboard(&devices(false), &mut none).map(|k| k.index),
            Some(0)
        );
        assert_eq!(
            real_keyboard(
                r#"{"keyboards":[{"name":"hl-virtual-keyboard","main":true,"active_layout_index":1}]}"#,
                &mut None
            ),
            None
        );
        assert_eq!(real_keyboard("unknown request", &mut None), None);
    }

    /// The keymap and the layout index come from the same keyboard: compiled from its names,
    /// the keymap has the layout Hyprland reports at that index; one that lacks it (a stale or
    /// another keyboard's keymap) is not taken.
    #[test]
    fn real_keyboard_keymap_pairs_with_its_layout_index() {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let kb = |layout: &str, index, active: &str| RealKb {
            names: [
                String::new(),
                String::new(),
                layout.into(),
                String::new(),
                String::new(),
            ],
            index,
            active: Some(active.into()),
            locks: None,
        };
        let usru = kb("us,ru", 1, "Russian");
        let km = usru.compile(&ctx).unwrap();
        assert!(usru.matches(&km));
        let mut st = xkb::State::new(&km);
        st.update_mask(0, 0, 0, 0, 0, usru.index);
        // KEY_V in the Russian layout.
        assert_eq!(
            st.key_get_one_sym(xkb::Keycode::new(47 + 8)),
            xkb::Keysym::new(0x6cd)
        );
        let us = kb("us", 0, "English (US)").compile(&ctx).unwrap();
        assert!(!usru.matches(&us));
        // An index whose layout is another than the one Hyprland names.
        assert!(!kb("us,ru", 0, "Russian").matches(&km));
        assert!(!kb("us,ru", 2, "Russian").matches(&km));
        assert!(
            RealKb {
                active: None,
                ..usru.clone()
            }
            .matches(&km)
        );
    }

    /// Lock state goes in and out by the modifier names Hyprland uses (`Lock`, `Mod2`), whatever
    /// else is locked kept.
    #[test]
    fn locks_are_hyprlands_lock_and_numlock_modifiers() {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let km = xkb::Keymap::new_from_names(&ctx, "", "pc105", "us,ru", "", None, 0).unwrap();
        let (caps, num) = (
            1 << km.mod_get_index(xkb::MOD_NAME_CAPS),
            1 << km.mod_get_index(xkb::MOD_NAME_NUM),
        );
        let both = Locks {
            caps: true,
            num: true,
        };
        assert_eq!(both.mask(&km, 0), caps | num);
        let shift = 1 << km.mod_get_index(xkb::MOD_NAME_SHIFT);
        assert_eq!(Locks::default().mask(&km, caps | num | shift), shift);
        let mut st = xkb::State::new(&km);
        assert_eq!(Locks::of(&st), Locks::default());
        // KEY_NUMLOCK locks NumLock, as Hyprland's `numLock` reads it.
        st.update_key(xkb::Keycode::new(69 + 8), xkb::KeyDirection::Down);
        st.update_key(xkb::Keycode::new(69 + 8), xkb::KeyDirection::Up);
        assert_eq!(
            Locks::of(&st),
            Locks {
                caps: false,
                num: true
            }
        );
        assert_eq!(st.serialize_mods(xkb::STATE_MODS_LOCKED) & num, num);
        assert_eq!(
            both.xor(Locks::of(&st)),
            Locks {
                caps: true,
                num: false
            }
        );
    }

    /// `j/devices` is read again only after the event socket reported a keyboard change, or
    /// when there is no event socket; a closed one is connected again.
    #[test]
    fn devices_are_read_again_only_after_a_keyboard_event() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let mut h = Hyprland::default();
        // No event socket: every batch reads.
        assert!(h.keyboards_changed(dir.path()));
        assert!(h.keyboards_changed(dir.path()));
        let s2 = std::os::unix::net::UnixListener::bind(dir.path().join(".socket2.sock")).unwrap();
        assert!(h.keyboards_changed(dir.path()), "connecting reads once");
        let (mut ev, _) = s2.accept().unwrap();
        assert!(!h.keyboards_changed(dir.path()));
        ev.write_all(b"workspace>>2\nactivewindow>>kitty,~\n")
            .unwrap();
        assert!(!h.keyboards_changed(dir.path()));
        // A line split across reads counts once it is complete.
        ev.write_all(b"activelay").unwrap();
        assert!(!h.keyboards_changed(dir.path()));
        ev.write_all(b"out>>usb-keyboard,Russian\n").unwrap();
        assert!(h.keyboards_changed(dir.path()));
        assert!(!h.keyboards_changed(dir.path()));
        ev.write_all(b"configreloaded>>\n").unwrap();
        assert!(h.keyboards_changed(dir.path()));
        // Hyprland dropped the client: connect again, and read again.
        drop(ev);
        assert!(h.keyboards_changed(dir.path()));
        let (_ev, _) = s2.accept().unwrap();
        assert!(!h.keyboards_changed(dir.path()));
    }

    /// Hyprland drops a client whose events queue up unread, so the event socket is read while
    /// no keys are sent too, without a reconnect; it closes with the keyboard.
    #[test]
    fn event_socket_is_read_between_batches() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let s2 = std::os::unix::net::UnixListener::bind(dir.path().join(".socket2.sock")).unwrap();
        let mut h = Hyprland::default();
        assert!(h.keyboards_changed(dir.path()));
        let (mut ev, _) = s2.accept().unwrap();
        // Far more than the socket buffers hold: the write completes only once the events are
        // read, and no batch reads them meanwhile.
        ev.write_all(&b"activewindow>>kitty,~\n".repeat(1 << 19))
            .unwrap();
        ev.write_all(b"activelayout>>usb-keyboard,Russian\n")
            .unwrap();
        assert!(h.keyboards_changed(dir.path()));
        assert!(!h.keyboards_changed(dir.path()));
        s2.set_nonblocking(true).unwrap();
        assert_eq!(
            s2.accept().map(|_| ()).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "the same subscription throughout"
        );
        drop(h);
        assert_eq!(ev.read(&mut [0u8; 64]).unwrap(), 0, "unsubscribed");
    }

    #[test]
    fn poisoned_sender_lock_does_not_panic_the_caller() {
        // Its own port with no compositor, whatever display the test environment has.
        let port = Port::new(|| None, |_, _| None);
        std::thread::scope(|s| {
            let _ = s
                .spawn(|| {
                    let _g = port.tx.lock().unwrap_or_else(|e| e.into_inner());
                    panic!("poison the sender lock");
                })
                .join();
        });
        assert!(port.tx.is_poisoned());
        let sent = std::panic::catch_unwind(|| port.send_wait(&[(30, true), (30, false)]));
        // The worker answers false (nothing to send to) instead of the caller panicking.
        assert!(!sent.unwrap());
        // The worker keeps serving the poisoned lock's channel.
        assert!(!port.send_wait(&[]));
    }

    /// The system refuses the keyboard's thread (the limit on threads is reached): those keys
    /// are not sent, and the next call starts the thread again instead of writing to a channel
    /// nobody reads until the shell restarts.
    #[test]
    fn a_refused_thread_is_started_again_by_the_next_keys() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CONNECTS: AtomicUsize = AtomicUsize::new(0);
        // Its own port with no compositor; `connect` runs on the worker only.
        let port = Port::new(
            || {
                CONNECTS.fetch_add(1, Ordering::SeqCst);
                None
            },
            |_, _| None,
        );
        // On a thread of its own: the refusal holds for the thread that asks for it and the
        // ones it starts, and this one must be able to start the worker afterwards.
        std::thread::scope(|s| {
            s.spawn(|| {
                crate::testenv::refuse_new_threads();
                assert!(!port.send_wait(&[(30, true), (30, false)]));
            });
        });
        assert_eq!(CONNECTS.load(Ordering::SeqCst), 0, "no worker ran");
        // Answered by a worker (it found no compositor), which `connect` ran on.
        assert!(!port.send_wait(&[(30, true), (30, false)]));
        assert_eq!(CONNECTS.load(Ordering::SeqCst), 1);
    }

    /// The system refuses the thread that reads Hyprland's event socket: no subscription, the
    /// refusal is in the log, and the connection is closed (Hyprland reads its end), not left
    /// open with nobody reading it. In a process of its own: the test reads what that process
    /// wrote to its stderr.
    #[test]
    fn a_refused_events_thread_is_logged_and_closes_the_socket() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_events_thread_is_logged_and_closes_the_socket"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let s2 = std::os::unix::net::UnixListener::bind(dir.path().join(".socket2.sock")).unwrap();
        let mut h = Hyprland::default();
        crate::testenv::refuse_new_threads();
        let (changed, text) = crate::testenv::stderr_with(|| h.keyboards_changed(dir.path()));
        assert!(changed);
        assert!(h.events.is_none());
        assert!(
            text.contains("KeySender: cannot start the ri-vkbd-events thread:"),
            "stderr: {text:?}"
        );
        let (mut ev, _) = s2.accept().unwrap();
        assert_eq!(ev.read(&mut [0u8; 64]).unwrap(), 0, "closed");
    }

    /// The refused thread is reported in the log. In a process of its own:
    /// the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_thread_is_logged() {
        const TEST: &str = concat!(module_path!(), "::a_refused_thread_is_logged");
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let port = Port::new(|| None, |_, _| None);
        crate::testenv::refuse_new_threads();
        let (sent, text) =
            crate::testenv::stderr_with(|| port.send_wait(&[(30, true), (30, false)]));
        assert!(!sent);
        assert!(
            text.contains("KeySender: cannot start the virtual keyboard thread:"),
            "stderr: {text:?}"
        );
    }
}
