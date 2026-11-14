//! The process's clipboard selection source (`QClipboard::setMimeData`), on the Wayland
//! connection and event queue of its host: the shell's own, so a source's `cancelled` and the
//! `wl_data_device.selection` that replaced it are handled in the order the compositor sent
//! them, as in Qt (QWaylandDataDevice owns both). A process without a host (tests, tools)
//! gets one that is nothing else: a thread with a connection of its own ([`Alone`]). The
//! selection is set through ext-data-control, else the deprecated wlr-data-control (as the
//! watcher in [`super::wl`]): the core protocol sets one only with the serial of an input
//! event. Each `send` is answered through a non-blocking pipe, at once as far as the pipe
//! takes it and from the host's loop when it can take more ([`Host::watch`]); a source goes
//! with its transfers once the compositor cancels it (another selection replaced it). A
//! reader that stops reading holds a pipe, not a thread and not the loop.
//!
//! [`wait`] follows the current source and the reads of it, for superpaste to paste the next
//! entry only after the target has read the previous one.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, mpsc};
use wayland_client::protocol::{wl_callback, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as ext_device, ext_data_control_manager_v1 as ext_manager,
    ext_data_control_offer_v1 as ext_offer, ext_data_control_source_v1 as ext_source,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1 as dc_device, zwlr_data_control_manager_v1 as dc_manager,
    zwlr_data_control_offer_v1 as dc_offer, zwlr_data_control_source_v1 as dc_source,
};

use crate::svc::Stop;
/// (MIME type, bytes) offered by one source.
pub type Offers = Vec<(String, Arc<[u8]>)>;

/// The text names `wl-copy` adds to a text type (wl-clipboard's `util/string.c`).
const TEXT_MIMES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "STRING",
    "UTF8_STRING",
    "TEXT",
];

/// wl-clipboard's `mime_type_is_text`.
fn is_text(m: &str) -> bool {
    matches!(m, "TEXT" | "STRING" | "UTF8_STRING")
        || m.starts_with("text/")
        || m.contains("json")
        || ["script", "xml", "yaml", "csv", "ini"]
            .iter()
            .any(|s| m.ends_with(s))
}

/// What `wl-copy [--type mime]` offers for `data`: the given or detected type and, for text,
/// the other text names with the same bytes.
pub fn offers(data: Vec<u8>, mime: Option<&str>) -> Offers {
    let base = mime.map_or_else(|| tree_magic_mini::from_u8(&data).to_owned(), str::to_owned);
    let data: Arc<[u8]> = data.into();
    let mut v = vec![(base.clone(), data.clone())];
    if is_text(&base) {
        v.extend(
            TEXT_MIMES
                .iter()
                .filter(|m| **m != base)
                .map(|m| ((*m).to_owned(), data.clone())),
        );
    }
    v
}

/// What `QClipboard::setText` offers on Wayland.
pub fn text_offers(text: String) -> Offers {
    let data: Arc<[u8]> = text.into_bytes().into();
    ["text/plain", "text/plain;charset=utf-8"]
        .iter()
        .map(|m| ((*m).to_owned(), data.clone()))
        .collect()
}

/// The current source of this process and the reads of it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Served {
    /// The source holding the selection (ids count from 1), 0 when none of ours does.
    pub source: u64,
    /// `send` requests it got.
    pub sends: u64,
    /// Those answered in full or closed by the reader.
    pub answered: u64,
    /// Pastes this process served from the source's data itself ([`took`]): they ask for no
    /// `send`.
    pub taken: u64,
    /// The host connection the sources are on (they count from 1), 0 without one.
    host: u64,
    /// The last [`sync`] the compositor answered on it.
    synced: u64,
}

impl Served {
    fn serve(&mut self, source: u64) {
        (self.source, self.sends, self.answered, self.taken) = (source, 0, 0, 0);
    }
}

static STATE: Mutex<Served> = Mutex::new(Served {
    source: 0,
    sends: 0,
    answered: 0,
    taken: 0,
    host: 0,
    synced: 0,
});
static CHANGED: Condvar = Condvar::new();

fn state() -> MutexGuard<'static, Served> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn update(f: impl FnOnce(&mut Served)) {
    f(&mut state());
    CHANGED.notify_all();
}

/// Makes [`wait`]ers look again: something their condition reads besides [`Served`] changed
/// (a stop, the keyboard focus). Taking the lock orders it after their last check.
pub fn notify() {
    drop(state());
    CHANGED.notify_all();
}

/// Blocks until `done` holds, checked again on every change of [`Served`] and every
/// [`notify`]; returns the state it held in.
pub fn wait(mut done: impl FnMut(&Served) -> bool) -> Served {
    let mut g = state();
    while !done(&g) {
        g = CHANGED.wait(g).unwrap_or_else(|e| e.into_inner());
    }
    *g
}

/// The offers of the last [`set`] and its number, while that source holds the selection as
/// far as the host's connection has been told: from the call until the source's `cancelled`
/// is handled (QWaylandDataDevice::selectionSource, reset by its `cancelled`).
static OWN: Mutex<(u64, Option<Offers>)> = Mutex::new((0, None));

fn own() -> MutexGuard<'static, (u64, Option<Offers>)> {
    OWN.lock().unwrap_or_else(|e| e.into_inner())
}

/// The source of [`set`] number `set` is not the selection (any more).
fn disown(set: u64) {
    let mut o = own();
    if o.0 == set {
        o.1 = None;
    }
}

/// Whether the selection is this process's own source ([`own_text`] is `Some`).
pub fn owns() -> bool {
    own().1.is_some()
}

/// `QClipboard::text()` while this process owns the clipboard: QWaylandClipboard::mimeData
/// returns the data of its own selection source, set by setMimeData and kept until the
/// compositor cancels it, without asking the compositor. The text is the one QMimeData::text
/// reads (`text/plain;charset=utf-8`, else `text/plain`, else ""). `None` once the source's
/// `cancelled` has been handled: on the host's queue that is before the
/// `wl_data_device.selection` of whatever replaced it (wlroots' wlr_seat_set_selection and
/// Hyprland's CSeatManager::setCurrentSelection cancel the old source first).
pub fn own_text() -> Option<String> {
    let o = own();
    let offers = o.1.as_ref()?;
    let data = ["text/plain;charset=utf-8", "text/plain"]
        .iter()
        .find_map(|m| offers.iter().find(|(t, _)| t == m))
        .map_or(&[][..], |(_, d)| &d[..]);
    Some(String::from_utf8_lossy(data).into_owned())
}

/// A paste of this process went in with [`own_text`]: the read of the current source that a
/// paste in another client shows as a `send`.
pub fn took() {
    update(|st| {
        if st.source != 0 {
            st.taken += 1;
        }
    });
}

fn set_nonblocking(fd: &OwnedFd) -> io::Result<()> {
    let raw = fd.as_raw_fd();
    // SAFETY: fcntl on a descriptor this function borrows alive.
    let fl = unsafe { libc::fcntl(raw, libc::F_GETFL) };
    // SAFETY: as above.
    if fl < 0 || unsafe { libc::fcntl(raw, libc::F_SETFL, fl | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// One read of a source in flight.
struct Transfer {
    /// What the host watches it by ([`Host::watch`]).
    id: u64,
    pipe: File,
    data: Arc<[u8]>,
    written: usize,
}

impl Transfer {
    /// Writes what the pipe takes now; true once it is done (all sent, or the reader left).
    fn write_some(&mut self) -> bool {
        while self.written < self.data.len() {
            match self.pipe.write(&self.data[self.written..]) {
                Ok(0) => return true,
                Ok(n) => self.written += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return false,
                Err(_) => return true,
            }
        }
        true
    }
}

pub use dc_device::ZwlrDataControlDeviceV1 as WlrDevice;
pub use dc_manager::ZwlrDataControlManagerV1 as WlrManager;
pub use dc_offer::ZwlrDataControlOfferV1 as WlrOffer;
pub use dc_source::ZwlrDataControlSourceV1 as WlrSource;
pub use ext_device::ExtDataControlDeviceV1 as ExtDevice;
pub use ext_manager::ExtDataControlManagerV1 as ExtManager;
pub use ext_offer::ExtDataControlOfferV1 as ExtOffer;
pub use ext_source::ExtDataControlSourceV1 as ExtSource;
pub use wl_callback::WlCallback as SyncCallback;

enum Manager {
    Ext(ExtManager),
    Wlr(WlrManager),
}

enum Device {
    Ext(ExtDevice),
    Wlr(WlrDevice),
}

#[derive(PartialEq)]
enum SourceProxy {
    Ext(ExtSource),
    Wlr(WlrSource),
}

impl Device {
    /// Both protocols reject a source of the other one, so the pair always matches.
    fn set_selection(&self, source: &SourceProxy) {
        match (self, source) {
            (Device::Ext(d), SourceProxy::Ext(s)) => d.set_selection(Some(s)),
            (Device::Wlr(d), SourceProxy::Wlr(s)) => d.set_selection(Some(s)),
            _ => unreachable!("a source of the device's own protocol"),
        }
    }

    fn destroy(&self) {
        match self {
            Device::Ext(d) => d.destroy(),
            Device::Wlr(d) => d.destroy(),
        }
    }
}

impl SourceProxy {
    fn offer(&self, mime: String) {
        match self {
            SourceProxy::Ext(s) => s.offer(mime),
            SourceProxy::Wlr(s) => s.offer(mime),
        }
    }

    fn destroy(&self) {
        match self {
            SourceProxy::Ext(s) => s.destroy(),
            SourceProxy::Wlr(s) => s.destroy(),
        }
    }
}

/// The user data of the host's data-control objects.
pub struct Tag;

/// The user data of a [`sync`]'s `wl_callback`: the host connection and the request's number.
pub struct SyncNo {
    host: u64,
    no: u64,
}

/// What the events of the source's objects are dispatched to: the state of the event queue
/// they were created on ([`host`]). The events themselves are handled by [`Clip`]
/// ([`crate::delegate_clipboard_source`]).
pub trait Host:
    'static
    + Dispatch<ExtManager, Tag>
    + Dispatch<ExtDevice, Tag>
    + Dispatch<ExtOffer, Tag>
    + Dispatch<ExtSource, Tag>
    + Dispatch<WlrManager, Tag>
    + Dispatch<WlrDevice, Tag>
    + Dispatch<WlrOffer, Tag>
    + Dispatch<WlrSource, Tag>
    + Dispatch<SyncCallback, SyncNo>
{
    /// Read `read`'s pipe is full: call [`writable`]`(read)` whenever `pipe` (a duplicate of
    /// it, non-blocking) can be written, until it returns true or [`Host::unwatch`]; then
    /// close `pipe`.
    fn watch(&mut self, read: u64, pipe: OwnedFd);
    /// Read `read` is over (its source was cancelled): close its pipe.
    fn unwatch(&mut self, read: u64);
}

/// Requests that create an object on the host's event queue.
trait Queue: Send {
    fn source(&self, m: &Manager) -> SourceProxy;
    fn device(&self, m: &Manager, seat: &wl_seat::WlSeat) -> Device;
    fn sync(&self, conn: &Connection, n: SyncNo);
}

impl<D: Host> Queue for QueueHandle<D> {
    fn source(&self, m: &Manager) -> SourceProxy {
        match m {
            Manager::Ext(m) => SourceProxy::Ext(m.create_data_source(self, Tag)),
            Manager::Wlr(m) => SourceProxy::Wlr(m.create_data_source(self, Tag)),
        }
    }

    fn device(&self, m: &Manager, seat: &wl_seat::WlSeat) -> Device {
        match m {
            Manager::Ext(m) => Device::Ext(m.get_data_device(seat, self, Tag)),
            Manager::Wlr(m) => Device::Wlr(m.get_data_device(seat, self, Tag)),
        }
    }

    fn sync(&self, conn: &Connection, n: SyncNo) {
        conn.display().sync(self, n);
    }
}

struct Source {
    id: u64,
    /// The number of the [`set`] it came from.
    set: u64,
    proxy: SourceProxy,
    offers: Offers,
    reads: Vec<Transfer>,
}

/// The source side of one host connection.
struct Src {
    /// Its number in [`Served`].
    host: u64,
    conn: Connection,
    queue: Box<dyn Queue>,
    /// Wakes the loop of [`Alone`] for a flush that could not finish.
    wake: Option<File>,
    mgr: Manager,
    dev: Option<Device>,
    sources: Vec<Source>,
    /// The source made last, 0 when none was.
    last: u64,
    syncs: u64,
    reads: u64,
}

impl Src {
    fn flush(&self) {
        if let Err(wayland_client::backend::WaylandError::Io(e)) = self.conn.flush()
            && e.kind() == io::ErrorKind::WouldBlock
            && let Some(w) = &self.wake
        {
            // A full pipe already has a wake-up pending.
            let _ = (&*w).write(&[1]);
        }
    }

    /// Every source is gone (the device is, or the connection): the reads to stop watching.
    fn drop_sources(&mut self) -> Vec<u64> {
        let mut reads = vec![];
        for s in self.sources.drain(..) {
            disown(s.set);
            s.proxy.destroy();
            reads.extend(s.reads.iter().map(|t| t.id));
        }
        update(|st| st.serve(0));
        reads
    }
}

static SRC: Mutex<Option<Src>> = Mutex::new(None);
/// A host was given ([`host`]): the process never starts [`Alone`].
static HOSTED: AtomicBool = AtomicBool::new(false);
static HOSTS: AtomicU64 = AtomicU64::new(0);
static SOURCES: AtomicU64 = AtomicU64::new(0);

/// The source side, on [`Alone`] (started here) in a process that gave no host.
fn src() -> MutexGuard<'static, Option<Src>> {
    let mut g = SRC.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() && !HOSTED.load(Ordering::Acquire) {
        *g = Alone::start();
    }
    g
}

fn attach<D: Host>(
    conn: &Connection,
    qh: &QueueHandle<D>,
    registry: &wl_registry::WlRegistry,
    ext: Option<u32>,
    wlr: Option<u32>,
    wake: Option<File>,
) -> Option<Src> {
    // wl-clipboard-rs's order: ext-data-control, else the deprecated wlr-data-control.
    let mgr = match (ext, wlr) {
        (Some(name), _) => Manager::Ext(registry.bind(name, 1, qh, Tag)),
        (None, Some(name)) => Manager::Wlr(registry.bind(name, 1, qh, Tag)),
        (None, None) => return None,
    };
    let host = HOSTS.fetch_add(1, Ordering::Relaxed) + 1;
    update(|st| {
        *st = Served {
            host,
            ..Served::default()
        }
    });
    Some(Src {
        host,
        conn: conn.clone(),
        queue: Box::new(qh.clone()),
        wake,
        mgr,
        dev: None,
        sources: vec![],
        last: 0,
        syncs: 0,
        reads: 0,
    })
}

/// Puts the process's selection source on `conn`, its objects on the event queue of `qh`:
/// `ext` and `wlr` are the names of the `ext_data_control_manager_v1` and
/// `zwlr_data_control_manager_v1` globals of `registry`. Called once, before anything is
/// copied; the seat comes with [`seat`]. False when the compositor has neither global:
/// nothing is offered then, and nothing is owned.
pub fn host<D: Host>(
    conn: &Connection,
    qh: &QueueHandle<D>,
    registry: &wl_registry::WlRegistry,
    ext: Option<u32>,
    wlr: Option<u32>,
) -> bool {
    HOSTED.store(true, Ordering::Release);
    let s = attach(conn, qh, registry, ext, wlr, None);
    let ok = s.is_some();
    *SRC.lock().unwrap_or_else(|e| e.into_inner()) = s;
    ok
}

/// The seat whose selection is set: the first one given.
pub fn seat(seat: &wl_seat::WlSeat) {
    let mut g = SRC.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = g.as_mut()
        && s.dev.is_none()
    {
        s.dev = Some(s.queue.device(&s.mgr, seat));
    }
}

/// Sets the clipboard selection to `offers` after every earlier set, without waiting for the
/// compositor (`QClipboard::setMimeData`: the last call wins). From any thread: the requests
/// go out on the host's connection in the order of the calls.
pub fn set(offers: Offers) {
    let mut g = src();
    // Without a device nothing is offered, and nothing is owned.
    let Some(s) = g.as_mut() else { return };
    let Some(dev) = &s.dev else { return };
    let set = {
        let mut o = own();
        o.0 += 1;
        o.1 = Some(offers.clone());
        o.0
    };
    let id = SOURCES.fetch_add(1, Ordering::Relaxed) + 1;
    let proxy = s.queue.source(&s.mgr);
    for (m, _) in &offers {
        proxy.offer(m.clone());
    }
    dev.set_selection(&proxy);
    s.last = id;
    s.sources.push(Source {
        id,
        set,
        proxy,
        offers,
        reads: vec![],
    });
    update(|st| st.serve(id));
    s.flush();
}

/// Waits until the compositor has handled every earlier [`set`]: the source set last, `None`
/// when none could be set (no compositor, no data-control) or `stop` was set first. The
/// answer is an event of the host's queue, so the wait ends with `stop` and never holds a
/// thread the host's loop is waiting for.
pub fn sync(stop: &Stop) -> Option<u64> {
    let (host, no, last) = {
        let mut g = src();
        let s = g.as_mut()?;
        s.syncs += 1;
        let n = SyncNo {
            host: s.host,
            no: s.syncs,
        };
        s.queue.sync(&s.conn, n);
        s.flush();
        (s.host, s.syncs, s.last)
    };
    let _wake = stop.on_stop(notify);
    let stopped = || stop.load(Ordering::Acquire);
    let st = wait(|st| stopped() || st.host != host || st.synced >= no);
    (st.host == host && st.synced >= no && last != 0).then_some(last)
}

/// [`set`], then [`sync`].
pub fn set_wait(offers: Offers, stop: &Stop) -> Result<u64, String> {
    set(offers);
    sync(stop).ok_or_else(|| "cannot set the clipboard selection".to_owned())
}

/// Writes what read `read`'s pipe takes now; true once the read is over (answered in full,
/// the reader left, or its source is gone). For the host's loop ([`Host::watch`]).
pub fn writable(read: u64) -> bool {
    let mut g = SRC.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = g.as_mut() else { return true };
    for src in &mut s.sources {
        let Some(i) = src.reads.iter().position(|t| t.id == read) else {
            continue;
        };
        if !src.reads[i].write_some() {
            return false;
        }
        src.reads.remove(i);
        let id = src.id;
        update(|st| {
            if st.source == id {
                st.answered += 1;
            }
        });
        return true;
    }
    true
}

/// Handles the events of the source's objects for a [`Host`].
pub struct Clip;

/// Dispatches the events of the clipboard source's objects on `$host`'s queue to [`Clip`].
#[macro_export]
macro_rules! delegate_clipboard_source {
    ($host:ty) => {
        $crate::delegate_clipboard_source!(@each $host:
            ExtManager, ExtDevice, ExtOffer, ExtSource, WlrManager, WlrDevice, WlrOffer, WlrSource);
        ::wayland_client::delegate_dispatch!($host:
            [$crate::ports::clipsrc::SyncCallback: $crate::ports::clipsrc::SyncNo]
            => $crate::ports::clipsrc::Clip);
    };
    (@each $host:ty: $($i:ident),*) => {
        $(::wayland_client::delegate_dispatch!($host:
            [$crate::ports::clipsrc::$i: $crate::ports::clipsrc::Tag]
            => $crate::ports::clipsrc::Clip);)*
    };
}
pub use delegate_clipboard_source;

/// The handlers of one data-control protocol (both have the same events).
macro_rules! data_control_dispatch {
    ($manager:ident :: $Manager:ident, $device:ident :: $Device:ident, $offer:ident :: $Offer:ident, $source:ident :: $Source:ident, $wrap:path) => {
        impl<D: Host> Dispatch<$device::$Device, Tag, D> for Clip {
            fn event(
                host: &mut D,
                _: &$device::$Device,
                e: $device::Event,
                _: &Tag,
                _: &Connection,
                _: &QueueHandle<D>,
            ) {
                match e {
                    // Reads are the data device's and the watcher's: offers are not kept.
                    $device::Event::Selection { id: Some(o) }
                    | $device::Event::PrimarySelection { id: Some(o) } => o.destroy(),
                    // The seat is gone, and its selection with it.
                    $device::Event::Finished => {
                        let reads = {
                            let mut g = SRC.lock().unwrap_or_else(|e| e.into_inner());
                            let Some(s) = g.as_mut() else { return };
                            if let Some(d) = s.dev.take() {
                                d.destroy();
                            }
                            s.drop_sources()
                        };
                        for r in reads {
                            host.unwatch(r);
                        }
                    }
                    _ => {}
                }
            }

            wayland_client::event_created_child!(D, $device::$Device, [
                $device::EVT_DATA_OFFER_OPCODE => ($offer::$Offer, Tag),
            ]);
        }

        impl<D: Host> Dispatch<$source::$Source, Tag, D> for Clip {
            fn event(
                host: &mut D,
                p: &$source::$Source,
                e: $source::Event,
                _: &Tag,
                _: &Connection,
                _: &QueueHandle<D>,
            ) {
                let mut g = SRC.lock().unwrap_or_else(|e| e.into_inner());
                let Some(s) = g.as_mut() else { return };
                let Some(i) = s.sources.iter().position(|x| x.proxy == $wrap(p.clone())) else {
                    return;
                };
                match e {
                    $source::Event::Send { mime_type, fd } => {
                        let src = &mut s.sources[i];
                        let Some((_, data)) = src.offers.iter().find(|(m, _)| *m == mime_type) else {
                            return;
                        };
                        if set_nonblocking(&fd).is_err() {
                            return;
                        }
                        s.reads += 1;
                        let mut t = Transfer {
                            id: s.reads,
                            pipe: File::from(fd),
                            data: data.clone(),
                            written: 0,
                        };
                        let id = src.id;
                        // What the pipe takes goes out now; the host's loop writes the rest
                        // as the reader reads.
                        let watch = match t.write_some() {
                            true => None,
                            false => t.pipe.try_clone().ok().map(|f| (t.id, OwnedFd::from(f))),
                        };
                        if watch.is_some() {
                            src.reads.push(t);
                        }
                        update(|st| {
                            if st.source == id {
                                st.sends += 1;
                                st.answered += watch.is_none() as u64;
                            }
                        });
                        drop(g);
                        if let Some((read, pipe)) = watch {
                            host.watch(read, pipe);
                        }
                    }
                    $source::Event::Cancelled => {
                        let src = s.sources.remove(i);
                        disown(src.set);
                        src.proxy.destroy();
                        update(|st| {
                            if st.source == src.id {
                                st.serve(0);
                            }
                        });
                        drop(g);
                        for t in &src.reads {
                            host.unwatch(t.id);
                        }
                    }
                    _ => {}
                }
            }
        }

        impl<D: Host> Dispatch<$manager::$Manager, Tag, D> for Clip {
            fn event(
                _: &mut D,
                _: &$manager::$Manager,
                _: $manager::Event,
                _: &Tag,
                _: &Connection,
                _: &QueueHandle<D>,
            ) {
            }
        }

        impl<D: Host> Dispatch<$offer::$Offer, Tag, D> for Clip {
            fn event(
                _: &mut D,
                _: &$offer::$Offer,
                _: $offer::Event,
                _: &Tag,
                _: &Connection,
                _: &QueueHandle<D>,
            ) {
            }
        }
    };
}

data_control_dispatch!(
    ext_manager::ExtDataControlManagerV1,
    ext_device::ExtDataControlDeviceV1,
    ext_offer::ExtDataControlOfferV1,
    ext_source::ExtDataControlSourceV1,
    SourceProxy::Ext
);
data_control_dispatch!(
    dc_manager::ZwlrDataControlManagerV1,
    dc_device::ZwlrDataControlDeviceV1,
    dc_offer::ZwlrDataControlOfferV1,
    dc_source::ZwlrDataControlSourceV1,
    SourceProxy::Wlr
);

impl<D: Host> Dispatch<wl_callback::WlCallback, SyncNo, D> for Clip {
    fn event(
        _: &mut D,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        n: &SyncNo,
        _: &Connection,
        _: &QueueHandle<D>,
    ) {
        update(|st| {
            if st.host == n.host {
                st.synced = st.synced.max(n.no);
            }
        });
    }
}

/// The host of a process that gave none: a thread with a Wayland connection of its own that
/// does nothing but dispatch the source's events and write its pipes.
#[derive(Default)]
struct Alone {
    seat: Option<wl_seat::WlSeat>,
    ext: Option<u32>,
    wlr: Option<u32>,
    watched: Vec<(u64, OwnedFd)>,
}

impl Host for Alone {
    fn watch(&mut self, read: u64, pipe: OwnedFd) {
        self.watched.push((read, pipe));
    }

    fn unwatch(&mut self, read: u64) {
        self.watched.retain(|(r, _)| *r != read);
    }
}

fn pipe(flags: i32) -> io::Result<(OwnedFd, OwnedFd)> {
    use std::os::fd::FromRawFd;
    let mut fds = [0; 2];
    // SAFETY: `fds` is a live array of the two ints pipe2(2) fills.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), flags) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pipe2 succeeded, so this is a new descriptor owned by nothing else.
    let r = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    // SAFETY: as `r`.
    let w = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    Ok((r, w))
}

type AloneLoop = (Connection, wayland_client::EventQueue<Alone>, Alone, u64);

impl Alone {
    /// The thread, then the connection it serves. `None` (logged) while the system refuses
    /// the thread or its pipe (the limit on threads or descriptors), and without a
    /// compositor with data-control and a seat: the next use tries again.
    fn start() -> Option<Src> {
        let thread = || -> io::Result<(File, mpsc::Sender<AloneLoop>)> {
            let (r, w) = pipe(libc::O_CLOEXEC | libc::O_NONBLOCK)?;
            let (tx, rx) = mpsc::channel::<AloneLoop>();
            std::thread::Builder::new()
                .name("clipboard-src".into())
                .spawn(move || {
                    // Nothing to serve when the connection could not be made.
                    if let Ok((conn, q, alone, host)) = rx.recv() {
                        alone.run(&conn, q, File::from(r));
                        lost(host);
                    }
                })?;
            Ok((File::from(w), tx))
        };
        let (wake, tx) = match thread() {
            Ok(t) => t,
            Err(e) => {
                elog::eprintln!("clipboard: cannot start the clipboard thread: {e}");
                return None;
            }
        };
        let conn = Connection::connect_to_env().ok()?;
        let mut q = conn.new_event_queue();
        let qh = q.handle();
        let reg = conn.display().get_registry(&qh, ());
        let mut alone = Alone::default();
        q.roundtrip(&mut alone).ok()?;
        let seat = alone.seat.clone()?;
        let mut s = attach(&conn, &qh, &reg, alone.ext, alone.wlr, Some(wake))?;
        s.dev = Some(s.queue.device(&s.mgr, &seat));
        tx.send((conn, q, alone, s.host)).ok()?;
        Some(s)
    }

    fn run(mut self, conn: &Connection, mut q: wayland_client::EventQueue<Alone>, mut wake: File) {
        use std::io::Read;
        use wayland_client::backend::WaylandError;
        loop {
            if q.dispatch_pending(&mut self).is_err() {
                return;
            }
            let out_pending = match conn.flush() {
                Ok(()) => false,
                Err(WaylandError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => true,
                Err(_) => return,
            };
            let Some(guard) = q.prepare_read() else {
                continue;
            };
            let mut fds = vec![
                libc::pollfd {
                    fd: wake.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: guard.connection_fd().as_raw_fd(),
                    events: libc::POLLIN | if out_pending { libc::POLLOUT } else { 0 },
                    revents: 0,
                },
            ];
            fds.extend(self.watched.iter().map(|(_, p)| libc::pollfd {
                fd: p.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            }));
            // SAFETY: `fds` is a live array of `fds.len()` pollfds for the duration of the call.
            if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) } < 0 {
                continue;
            }
            if fds[1].revents != 0 {
                if let Err(WaylandError::Io(e)) = guard.read()
                    && e.kind() != io::ErrorKind::WouldBlock
                {
                    return;
                }
            } else {
                drop(guard);
            }
            let mut k = 2;
            self.watched.retain(|(read, _)| {
                let ready = fds[k].revents != 0;
                k += 1;
                !(ready && writable(*read))
            });
            if fds[0].revents != 0 {
                let mut buf = [0u8; 64];
                while matches!(wake.read(&mut buf), Ok(n) if n > 0) {}
            }
        }
    }
}

/// Connection `host` broke: its sources went with it.
fn lost(host: u64) {
    let mut g = SRC.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_some_and(|s| s.host == host) {
        for s in g.take().iter().flat_map(|s| &s.sources) {
            disown(s.set);
        }
        update(|st| *st = Served::default());
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for Alone {
    fn event(
        s: &mut Alone,
        r: &wl_registry::WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Alone>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = e
        {
            match interface.as_str() {
                "wl_seat" if s.seat.is_none() => {
                    s.seat = Some(r.bind(name, version.min(2), qh, ()))
                }
                "ext_data_control_manager_v1" => s.ext = Some(name),
                "zwlr_data_control_manager_v1" => s.wlr = Some(name),
                _ => {}
            }
        }
    }
}

delegate_noop!(Alone: ignore wl_seat::WlSeat);
delegate_clipboard_source!(Alone);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_types_follow_wl_copy() {
        let names = |o: Offers| o.into_iter().map(|(m, _)| m).collect::<Vec<_>>();
        assert_eq!(
            names(offers(b"hi".to_vec(), Some("text/plain"))),
            [
                "text/plain",
                "text/plain;charset=utf-8",
                "STRING",
                "UTF8_STRING",
                "TEXT"
            ]
        );
        assert_eq!(names(offers(vec![1, 2], Some("image/png"))), ["image/png"]);
        assert_eq!(
            names(text_offers("x".into())),
            ["text/plain", "text/plain;charset=utf-8"]
        );
        assert!(is_text("application/json") && !is_text("application/octet-stream"));
    }

    /// The system refuses the clipboard thread (the limit on threads is reached at the first
    /// copy): setting the selection reports that it could not be set, instead of panicking,
    /// and the refusal is in the log. In a process of its own: the thread is started once per
    /// process, and the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_clipboard_thread_reports_the_selection_as_not_set() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_clipboard_thread_reports_the_selection_as_not_set"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        crate::testenv::refuse_new_threads();
        let (set, log) =
            crate::testenv::stderr_with(|| set_wait(text_offers("x".into()), &Stop::default()));
        assert_eq!(set, Err("cannot set the clipboard selection".to_owned()));
        assert!(
            log.contains("clipboard: cannot start the clipboard thread:"),
            "stderr: {log:?}"
        );
    }
}
