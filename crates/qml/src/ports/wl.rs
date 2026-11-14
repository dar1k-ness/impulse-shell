//! Wayland client side of the ports: output capture through `zwlr_screencopy_manager_v1`
//! (what `grim` does) on a short-lived connection on the job's thread, the clipboard text
//! watcher on a thread of its own; clipboard writes go to [`super::clipsrc`].

use image::RgbaImage;
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as ext_device, ext_data_control_manager_v1 as ext_manager,
    ext_data_control_offer_v1 as ext_offer,
};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1 as dc_device, zwlr_data_control_manager_v1 as dc_manager,
    zwlr_data_control_offer_v1 as dc_offer,
};
use wayland_protocols_wlr::output_power_management::v1::client::{
    zwlr_output_power_manager_v1, zwlr_output_power_v1,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

/// `wl-copy` (optionally `--type mime`): `data` becomes the clipboard selection, served by
/// [`super::clipsrc`]; returns once the compositor has it, with the source's id, or once
/// `stop` is set.
pub fn copy(data: Vec<u8>, mime: Option<&str>, stop: &crate::svc::Stop) -> Result<u64, String> {
    super::clipsrc::set_wait(super::clipsrc::offers(data, mime), stop)
}

/// Follows the clipboard selection through data-control on its own thread and calls `f` with
/// its text on every change (`QClipboard::text()`: "" when nothing offers text).
pub fn watch_text(f: impl Fn(String) + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || {
            if let Ok(conn) = Connection::connect_to_env() {
                watch(&conn, &f);
            }
        });
    if let Err(e) = spawned {
        // Refused by the system (`EAGAIN`): the selection is not followed.
        elog::eprintln!("clipboard: cannot start the thread that follows the selection: {e}");
    }
}

/// A selection offer of either data-control protocol.
#[derive(Clone, PartialEq)]
enum Offer {
    Ext(ext_offer::ExtDataControlOfferV1),
    Wlr(dc_offer::ZwlrDataControlOfferV1),
}

impl Offer {
    fn receive(&self, mime: String, fd: std::os::fd::BorrowedFd<'_>) {
        match self {
            Offer::Ext(o) => o.receive(mime, fd),
            Offer::Wlr(o) => o.receive(mime, fd),
        }
    }

    fn destroy(&self) {
        match self {
            Offer::Ext(o) => o.destroy(),
            Offer::Wlr(o) => o.destroy(),
        }
    }
}

#[derive(Default)]
struct Sel {
    seat: Option<wl_seat::WlSeat>,
    /// wl_registry name and version of each data-control manager global.
    ext: Option<(u32, u32)>,
    wlr: Option<(u32, u32)>,
    /// Offers introduced by `data_offer`, with their MIME types, until a selection takes them.
    offers: Vec<(Offer, Vec<String>)>,
    /// The latest selection the watch loop has not taken yet.
    sel: Option<Option<(Offer, Vec<String>)>>,
    finished: bool,
}

impl Sel {
    /// `selection`: the previous selection offer is no longer valid and "the client must destroy
    /// the previous selection offer, if any, upon receiving this event" (data-control XML), also
    /// when the loop has not got to it yet.
    fn selection(&mut self, id: Option<Offer>) {
        if let Some(Some((old, _))) = self.sel.take() {
            old.destroy();
        }
        let id = id.map(|o| {
            let mimes = match self.offers.iter().position(|(x, _)| *x == o) {
                Some(i) => self.offers.swap_remove(i).1,
                None => vec![],
            };
            (o, mimes)
        });
        self.sel = Some(id);
    }

    /// Primary selection offers are not watched: destroyed as they come.
    fn primary_selection(&mut self, o: Offer) {
        self.offers.retain(|(x, _)| *x != o);
        o.destroy();
    }

    fn offer(&mut self, o: &Offer, mime: String) {
        if let Some((_, m)) = self.offers.iter_mut().find(|(x, _)| x == o) {
            m.push(mime);
        }
    }
}

enum Device {
    Ext(ext_device::ExtDataControlDeviceV1),
    Wlr(dc_device::ZwlrDataControlDeviceV1),
}

/// The selection's text being read from its owner through a pipe, until EOF.
struct Transfer {
    offer: Offer,
    pipe: std::fs::File,
    out: Vec<u8>,
}

/// The selection's `text/plain` flavour requested through a non-blocking pipe; `None` when it
/// offers no text.
fn start_transfer(offer: Offer, mimes: &[String]) -> Option<Transfer> {
    use rustix::fs::{OFlags, fcntl_setfl};
    use rustix::pipe::{PipeFlags, pipe_with};
    use std::os::fd::AsFd;
    let m = ["text/plain;charset=utf-8", "text/plain"]
        .into_iter()
        .find(|t| mimes.iter().any(|m| m == t))?;
    let (r, w) = pipe_with(PipeFlags::CLOEXEC).ok()?;
    // Only our read end: the write end is the owner's, which may write to it blocking.
    fcntl_setfl(&r, OFlags::NONBLOCK).ok()?;
    offer.receive(m.to_owned(), w.as_fd());
    Some(Transfer {
        offer,
        pipe: r.into(),
        out: Vec::new(),
    })
}

/// The watch loop: one poll(2) over the Wayland connection and the transfer in flight, so
/// selection events are handled while an owner is still writing (a newer selection abandons
/// the transfer) and a transfer ends only at EOF, as `gdk_clipboard_read_text_async` does.
fn watch(conn: &Connection, f: &dyn Fn(String)) -> Option<()> {
    use rustix::event::{PollFd, PollFlags, poll};
    use std::io::{ErrorKind, Read};
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    let reg = conn.display().get_registry(&qh, ());
    let mut s = Sel::default();
    q.roundtrip(&mut s).ok()?;
    let seat = s.seat.clone()?;
    // wl-clipboard-rs's order: ext-data-control, else the deprecated wlr-data-control.
    let dev = match (s.ext, s.wlr) {
        (Some((name, _)), _) => Device::Ext(
            reg.bind::<ext_manager::ExtDataControlManagerV1, _, _>(name, 1, &qh, ())
                .get_data_device(&seat, &qh, ()),
        ),
        (None, Some((name, version))) => Device::Wlr(
            reg.bind::<dc_manager::ZwlrDataControlManagerV1, _, _>(name, version.min(2), &qh, ())
                .get_data_device(&seat, &qh, ()),
        ),
        (None, None) => return None,
    };
    let mut transfer: Option<Transfer> = None;
    while !s.finished {
        q.dispatch_pending(&mut s).ok()?;
        if let Some(sel) = s.sel.take() {
            if let Some(t) = transfer.take() {
                t.offer.destroy();
            }
            match sel {
                Some((o, mimes)) => {
                    transfer = start_transfer(o.clone(), &mimes);
                    if transfer.is_none() {
                        o.destroy();
                        f(String::new());
                    }
                }
                None => f(String::new()),
            }
            continue;
        }
        let out_pending = match conn.flush() {
            Ok(()) => false,
            Err(wayland_client::backend::WaylandError::Io(e))
                if e.kind() == ErrorKind::WouldBlock =>
            {
                true
            }
            Err(_) => return None,
        };
        let Some(guard) = q.prepare_read() else {
            continue;
        };
        let mut wl_events = PollFlags::IN;
        if out_pending {
            wl_events |= PollFlags::OUT;
        }
        let (wl_ready, pipe_ready) = {
            let mut fds = vec![PollFd::from_borrowed_fd(guard.connection_fd(), wl_events)];
            if let Some(t) = &transfer {
                fds.push(PollFd::new(&t.pipe, PollFlags::IN));
            }
            match poll(&mut fds, None) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(_) => return None,
            }
            (
                fds[0]
                    .revents()
                    .intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP),
                fds.get(1).is_some_and(|p| !p.revents().is_empty()),
            )
        };
        if wl_ready {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(e))
                    if e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => return None,
            }
        } else {
            drop(guard);
        }
        if pipe_ready && let Some(t) = &mut transfer {
            let mut buf = [0u8; 65536];
            match t.pipe.read(&mut buf) {
                Ok(n) if n > 0 => t.out.extend_from_slice(&buf[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                // EOF: the owner wrote everything (or failed and closed it).
                _ => {
                    let t = transfer.take()?;
                    t.offer.destroy();
                    f(String::from_utf8_lossy(&t.out).into_owned());
                }
            }
        }
    }
    if let Some(t) = transfer {
        t.offer.destroy();
    }
    match dev {
        Device::Ext(d) => d.destroy(),
        Device::Wlr(d) => d.destroy(),
    }
    let _ = conn.flush();
    Some(())
}

impl Dispatch<wl_registry::WlRegistry, ()> for Sel {
    fn event(
        s: &mut Sel,
        r: &wl_registry::WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Sel>,
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
                "ext_data_control_manager_v1" => s.ext = Some((name, version)),
                "zwlr_data_control_manager_v1" => s.wlr = Some((name, version)),
                _ => {}
            }
        }
    }
}

/// The device and offer handlers of one data-control protocol (both have the same events).
macro_rules! data_control_dispatch {
    ($device:ident :: $Device:ident, $offer:ident :: $Offer:ident, $wrap:path) => {
        impl Dispatch<$device::$Device, ()> for Sel {
            fn event(
                s: &mut Sel,
                _: &$device::$Device,
                e: $device::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Sel>,
            ) {
                match e {
                    $device::Event::DataOffer { id } => s.offers.push(($wrap(id), vec![])),
                    $device::Event::Selection { id } => s.selection(id.map($wrap)),
                    $device::Event::PrimarySelection { id: Some(o) } => {
                        s.primary_selection($wrap(o))
                    }
                    $device::Event::Finished => s.finished = true,
                    _ => {}
                }
            }

            wayland_client::event_created_child!(Sel, $device::$Device, [
                $device::EVT_DATA_OFFER_OPCODE => ($offer::$Offer, ()),
            ]);
        }

        impl Dispatch<$offer::$Offer, ()> for Sel {
            fn event(
                s: &mut Sel,
                o: &$offer::$Offer,
                e: $offer::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Sel>,
            ) {
                if let $offer::Event::Offer { mime_type } = e {
                    s.offer(&$wrap(o.clone()), mime_type);
                }
            }
        }
    };
}

data_control_dispatch!(
    ext_device::ExtDataControlDeviceV1,
    ext_offer::ExtDataControlOfferV1,
    Offer::Ext
);
data_control_dispatch!(
    dc_device::ZwlrDataControlDeviceV1,
    dc_offer::ZwlrDataControlOfferV1,
    Offer::Wlr
);

delegate_noop!(Sel: ignore wl_seat::WlSeat);
delegate_noop!(Sel: ignore ext_manager::ExtDataControlManagerV1);
delegate_noop!(Sel: ignore dc_manager::ZwlrDataControlManagerV1);

#[derive(Default)]
struct Output {
    out: Option<wl_output::WlOutput>,
    /// wl_registry name of the wl_output global.
    global: u32,
    /// The global was removed: the output is gone.
    gone: bool,
    /// zwlr_output_power_v1 mode is off (DPMS): the compositor presents no frames, so a copy
    /// would wait until it is switched back on.
    off: bool,
    name: String,
    /// wl_output.scale: whole, a fractional scale rounded up (Hyprland and sway send 2 for 1.25).
    scale: i32,
    /// Size of the current wl_output.mode, in pixels.
    mode: (i32, i32),
    transform: Option<wl_output::Transform>,
    /// Logical position and size (xdg-output).
    logical: (i32, i32, i32, i32),
}

impl Output {
    /// Pixels per logical unit, as grim guesses it (`xdg_output_handle_done`): the mode's width,
    /// turned by the transform, over the logical width. This is the fractional scale a frame of
    /// the output has, and the one QML multiplies a region by (`HyprlandMonitor.scale`).
    /// wl_output.scale only when a size is unknown.
    fn pixel_scale(&self) -> f64 {
        use wl_output::Transform as T;
        let turned = matches!(
            self.transform,
            Some(T::_90 | T::_270 | T::Flipped90 | T::Flipped270)
        );
        let w = if turned { self.mode.1 } else { self.mode.0 };
        if w > 0 && self.logical.2 > 0 {
            w as f64 / self.logical.2 as f64
        } else {
            self.scale.max(1) as f64
        }
    }
}

#[derive(Default)]
struct Frame {
    shm: Option<(wl_shm::Format, u32, u32, u32)>,
    buffer_done: bool,
    y_invert: bool,
    ready: bool,
    failed: bool,
}

#[derive(Default)]
struct St {
    shm: Option<wl_shm::WlShm>,
    sc: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    xdg: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1>,
    power: Option<zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1>,
    outputs: Vec<Output>,
    frame: Frame,
    /// The compositor answered the wl_display.sync of [`roundtrip`].
    synced: bool,
}

impl Dispatch<wl_callback::WlCallback, ()> for St {
    fn event(
        s: &mut St,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<St>,
    ) {
        s.synced = true;
    }
}

/// The error of a capture its job's stop ended ([`capture_or_stop`]).
pub const STOPPED: &str = "stopped";

/// The error of a capture the compositor did not answer in time ([`capture_within`]).
pub const TIMED_OUT: &str = "no answer from the compositor";

/// What ends a capture's wait for the compositor besides its answer. A compositor that took
/// the request and never answers it would otherwise hold the waiting thread for good.
#[derive(Clone, Copy, Default)]
struct Wait<'a> {
    /// An eventfd the job's stop writes: the wait ends with [`STOPPED`].
    halt: Option<&'a std::os::fd::OwnedFd>,
    /// The wait ends with [`TIMED_OUT`] then. No event tells that a compositor will not
    /// answer, so only a deadline can end the wait of a caller nothing stops.
    deadline: Option<std::time::Instant>,
}

/// Waits for events of the compositor and dispatches them, as `EventQueue::blocking_dispatch`,
/// until `wait` ends it.
fn dispatch(
    conn: &Connection,
    q: &mut wayland_client::EventQueue<St>,
    st: &mut St,
    wait: Wait,
) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    use wayland_client::backend::WaylandError;
    if wait.halt.is_none() && wait.deadline.is_none() {
        return q.blocking_dispatch(st).map(drop).map_err(|e| e.to_string());
    }
    loop {
        if q.dispatch_pending(st).map_err(|e| e.to_string())? > 0 {
            return Ok(());
        }
        match conn.flush() {
            Ok(()) => {}
            Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
        let Some(guard) = q.prepare_read() else {
            continue;
        };
        let ms = match wait.deadline {
            None => -1,
            Some(d) => {
                let left = d.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return Err(TIMED_OUT.into());
                }
                i32::try_from(left.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX)
            }
        };
        let fd = |fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let mut fds = [
            fd(guard.connection_fd().as_raw_fd()),
            // A negative descriptor is ignored by poll(2).
            fd(wait.halt.map_or(-1, |h| h.as_raw_fd())),
        ];
        // SAFETY: `fds` is two writable pollfds whose descriptors stay open for the call.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, ms) };
        if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if n > 0 && fds[1].revents != 0 {
            return Err(STOPPED.into());
        }
        if n <= 0 || fds[0].revents == 0 {
            // Interrupted or timed out: the deadline is checked again above.
            continue;
        }
        match guard.read() {
            Ok(_) => {}
            Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// `EventQueue::roundtrip`: every request sent so far is answered when this returns. The wait
/// is [`dispatch`]'s, so `wait` ends it.
fn roundtrip(
    conn: &Connection,
    q: &mut wayland_client::EventQueue<St>,
    st: &mut St,
    wait: Wait,
) -> Result<(), String> {
    st.synced = false;
    conn.display().sync(&q.handle(), ());
    while !st.synced {
        dispatch(conn, q, st, wait)?;
    }
    Ok(())
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
        if let wl_registry::Event::GlobalRemove { name } = e {
            for o in s.outputs.iter_mut().filter(|o| o.global == name) {
                o.gone = true;
            }
            return;
        }
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = e
        {
            match interface.as_str() {
                "wl_shm" => s.shm = Some(r.bind(name, 1, qh, ())),
                "zwlr_screencopy_manager_v1" => s.sc = Some(r.bind(name, version.min(3), qh, ())),
                "zxdg_output_manager_v1" => s.xdg = Some(r.bind(name, version.min(3), qh, ())),
                "zwlr_output_power_manager_v1" => s.power = Some(r.bind(name, 1, qh, ())),
                "wl_output" => {
                    let i = s.outputs.len();
                    s.outputs.push(Output {
                        out: Some(r.bind(name, version.min(4), qh, i)),
                        global: name,
                        scale: 1,
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<zwlr_output_power_v1::ZwlrOutputPowerV1, usize> for St {
    fn event(
        s: &mut St,
        _: &zwlr_output_power_v1::ZwlrOutputPowerV1,
        e: zwlr_output_power_v1::Event,
        &i: &usize,
        _: &Connection,
        _: &QueueHandle<St>,
    ) {
        // `failed` (no power control for this output) leaves the state unknown: copy as usual.
        if let zwlr_output_power_v1::Event::Mode { mode } = e {
            s.outputs[i].off = mode == WEnum::Value(zwlr_output_power_v1::Mode::Off);
        }
    }
}

impl Dispatch<wl_output::WlOutput, usize> for St {
    fn event(
        s: &mut St,
        _: &wl_output::WlOutput,
        e: wl_output::Event,
        &i: &usize,
        _: &Connection,
        _: &QueueHandle<St>,
    ) {
        let o = &mut s.outputs[i];
        match e {
            wl_output::Event::Name { name } => o.name = name,
            wl_output::Event::Scale { factor } => o.scale = factor,
            wl_output::Event::Mode {
                flags: WEnum::Value(f),
                width,
                height,
                ..
            } if f.contains(wl_output::Mode::Current) => o.mode = (width, height),
            wl_output::Event::Geometry {
                transform: WEnum::Value(t),
                ..
            } => o.transform = Some(t),
            _ => {}
        }
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, usize> for St {
    fn event(
        s: &mut St,
        _: &zxdg_output_v1::ZxdgOutputV1,
        e: zxdg_output_v1::Event,
        &i: &usize,
        _: &Connection,
        _: &QueueHandle<St>,
    ) {
        let o = &mut s.outputs[i];
        match e {
            zxdg_output_v1::Event::LogicalPosition { x, y } => (o.logical.0, o.logical.1) = (x, y),
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                (o.logical.2, o.logical.3) = (width, height)
            }
            zxdg_output_v1::Event::Name { name } if o.name.is_empty() => o.name = name,
            _ => {}
        }
    }
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for St {
    fn event(
        s: &mut St,
        _: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        e: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<St>,
    ) {
        use zwlr_screencopy_frame_v1::Event;
        let f = &mut s.frame;
        match e {
            Event::Buffer {
                format: WEnum::Value(format),
                width,
                height,
                stride,
            } if f.shm.is_none() => f.shm = Some((format, width, height, stride)),
            Event::BufferDone => f.buffer_done = true,
            Event::Flags {
                flags: WEnum::Value(fl),
            } => f.y_invert = fl.contains(zwlr_screencopy_frame_v1::Flags::YInvert),
            Event::Ready { .. } => f.ready = true,
            Event::Failed => f.failed = true,
            _ => {}
        }
    }
}

delegate_noop!(St: ignore wl_shm::WlShm);
delegate_noop!(St: ignore wl_shm_pool::WlShmPool);
delegate_noop!(St: ignore wl_buffer::WlBuffer);
delegate_noop!(St: ignore zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1);
delegate_noop!(St: ignore zxdg_output_manager_v1::ZxdgOutputManagerV1);
delegate_noop!(St: ignore zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1);

/// Bytes of a `w`x`h` buffer with `stride`-byte rows; `None` when it can't hold 4-byte
/// pixels or doesn't fit the address space / a `wl_shm` pool (`i32`).
fn shm_size(w: u32, h: u32, stride: u32) -> Option<usize> {
    if (stride as u64) < w as u64 * 4 {
        return None;
    }
    let size = (stride as usize).checked_mul(h as usize)?;
    i32::try_from(size).ok()?;
    Some(size)
}

/// Pixels of a captured shm buffer as RGBA.
fn to_rgba(
    fmt: wl_shm::Format,
    w: u32,
    h: u32,
    stride: u32,
    data: &[u8],
    y_invert: bool,
) -> Result<RgbaImage, String> {
    use wl_shm::Format as F;
    let bad = || {
        format!(
            "bad screencopy buffer {w}x{h} stride {stride} ({} bytes)",
            data.len()
        )
    };
    match shm_size(w, h, stride) {
        Some(n) if n <= data.len() => {}
        _ => return Err(bad()),
    }
    let (w, h, stride) = (w as usize, h as usize, stride as usize);
    let mut img = RgbaImage::new(w as u32, h as u32);
    for y in 0..h {
        let sy = if y_invert { h - 1 - y } else { y };
        let row = &data[sy * stride..sy * stride + w * 4];
        for x in 0..w {
            let p = &row[x * 4..x * 4 + 4];
            let v = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            let px = match fmt {
                F::Xrgb8888 => [p[2], p[1], p[0], 255],
                F::Argb8888 => [p[2], p[1], p[0], p[3]],
                F::Xbgr8888 => [p[0], p[1], p[2], 255],
                F::Abgr8888 => [p[0], p[1], p[2], p[3]],
                F::Xrgb2101010 | F::Argb2101010 => {
                    let c = |s: u32| ((v >> s) & 0x3ff) as f32 * 255.0 / 1023.0;
                    let a = if fmt == F::Argb2101010 {
                        ((v >> 30) * 85) as u8
                    } else {
                        255
                    };
                    [
                        c(20).round() as u8,
                        c(10).round() as u8,
                        c(0).round() as u8,
                        a,
                    ]
                }
                F::Xbgr2101010 | F::Abgr2101010 => {
                    let c = |s: u32| ((v >> s) & 0x3ff) as f32 * 255.0 / 1023.0;
                    let a = if fmt == F::Abgr2101010 {
                        ((v >> 30) * 85) as u8
                    } else {
                        255
                    };
                    [
                        c(0).round() as u8,
                        c(10).round() as u8,
                        c(20).round() as u8,
                        a,
                    ]
                }
                _ => return Err(format!("unsupported buffer format {fmt:?}")),
            };
            img.put_pixel(x as u32, y as u32, image::Rgba(px));
        }
    }
    Ok(img)
}

/// The buffer turned to the output's logical orientation (as grim does).
fn untransform(img: RgbaImage, t: Option<wl_output::Transform>) -> RgbaImage {
    use image::imageops::{flip_horizontal, rotate90, rotate180, rotate270};
    use wl_output::Transform as T;
    match t {
        Some(T::_90) => rotate270(&img),
        Some(T::_180) => rotate180(&img),
        Some(T::_270) => rotate90(&img),
        Some(T::Flipped) => flip_horizontal(&img),
        Some(T::Flipped90) => rotate270(&flip_horizontal(&img)),
        Some(T::Flipped180) => rotate180(&flip_horizontal(&img)),
        Some(T::Flipped270) => rotate90(&flip_horizontal(&img)),
        _ => img,
    }
}

/// Why a copy of output `i` can no longer complete: the compositor failed it, the output went
/// away (a removed output's frame may get no further event), or it was switched off (no frame
/// is presented until it is back on).
fn copy_ended(st: &St, i: usize) -> Result<(), String> {
    let o = &st.outputs[i];
    if st.frame.failed {
        Err("failed to copy output".into())
    } else if o.gone {
        Err(format!("output '{}' removed", o.name))
    } else if o.off {
        Err(format!("output '{}' is off", o.name))
    } else {
        Ok(())
    }
}

fn capture_one(
    conn: &Connection,
    q: &mut wayland_client::EventQueue<St>,
    st: &mut St,
    i: usize,
    cursor: bool,
    wait: Wait,
) -> Result<RgbaImage, String> {
    let qh = q.handle();
    let sc = st
        .sc
        .clone()
        .ok_or("compositor doesn't support wlr-screencopy-unstable-v1")?;
    let shm = st.shm.clone().ok_or("compositor doesn't support wl_shm")?;
    let out = st.outputs[i].out.clone().ok_or("no output")?;
    st.frame = Frame::default();
    let frame = sc.capture_output(i32::from(cursor), &out, &qh, ());
    loop {
        copy_ended(st, i)?;
        if st.frame.shm.is_some() && (st.frame.buffer_done || frame.version() < 3) {
            break;
        }
        dispatch(conn, q, st, wait)?;
    }
    let (fmt, w, h, stride) = st.frame.shm.ok_or("no shm buffer")?;
    let size = shm_size(w, h, stride)
        .ok_or_else(|| format!("bad screencopy buffer {w}x{h} stride {stride}"))?;
    let fd = rustix::fs::memfd_create("ri-screencopy", rustix::fs::MemfdFlags::CLOEXEC)
        .map_err(|e| e.to_string())?;
    rustix::fs::ftruncate(&fd, size as u64).map_err(|e| e.to_string())?;
    let file = std::fs::File::from(fd);
    use std::os::fd::AsFd;
    let pool = shm.create_pool(file.as_fd(), size as i32, &qh, ());
    let buf = pool.create_buffer(0, w as i32, h as i32, stride as i32, fmt, &qh, ());
    frame.copy(&buf);
    while !st.frame.ready {
        copy_ended(st, i)?;
        dispatch(conn, q, st, wait)?;
    }
    let mut data = vec![0u8; size];
    use std::os::unix::fs::FileExt;
    file.read_exact_at(&mut data, 0)
        .map_err(|e| e.to_string())?;
    frame.destroy();
    buf.destroy();
    pool.destroy();
    let _ = conn.flush();
    let img = to_rgba(fmt, w, h, stride, &data, st.frame.y_invert)?;
    Ok(untransform(img, st.outputs[i].transform))
}

/// `grim [-o output] -`: one output, or all outputs composed in the layout at the highest
/// output scale. The scale is the fractional one ([`Output::pixel_scale`]): an output at 1.25
/// comes out at its mode's size, not blown up to wl_output.scale 2.
#[cfg(test)]
pub fn capture(output: Option<&str>) -> Result<RgbaImage, String> {
    capture_on(
        &Connection::connect_to_env().map_err(|e| e.to_string())?,
        output,
        false,
        Wait::default(),
    )
}

/// `grim [-c] [-o output] -` (the cursor on the image when `cursor`) for a caller nothing
/// stops: a compositor that has not answered `limit` after the call fails the capture with
/// [`TIMED_OUT`].
pub fn capture_within(
    output: Option<&str>,
    cursor: bool,
    limit: std::time::Duration,
) -> Result<RgbaImage, String> {
    let deadline = std::time::Instant::now().checked_add(limit);
    capture_on(
        &Connection::connect_to_env().map_err(|e| e.to_string())?,
        output,
        cursor,
        Wait {
            halt: None,
            deadline,
        },
    )
}

/// [`capture`] for a job: `stop` (`Process.running = false`, the Process or the World
/// destroyed) ends the wait for the compositor, and the capture fails with [`STOPPED`].
pub fn capture_or_stop(output: Option<&str>, stop: &crate::svc::Stop) -> Result<RgbaImage, String> {
    let halt = std::sync::Arc::new(crate::sysbus::eventfd().ok_or("eventfd: no descriptor")?);
    let wake = halt.clone();
    let _on_stop = stop.on_stop(move || crate::sysbus::signal(&wake));
    capture_on(
        &Connection::connect_to_env().map_err(|e| e.to_string())?,
        output,
        false,
        Wait {
            halt: Some(&halt),
            deadline: None,
        },
    )
}

fn capture_on(
    conn: &Connection,
    output: Option<&str>,
    cursor: bool,
    wait: Wait,
) -> Result<RgbaImage, String> {
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut st = St::default();
    roundtrip(conn, &mut q, &mut st, wait)?;
    for (i, o) in st.outputs.iter().enumerate() {
        if let Some(out) = &o.out {
            if let Some(x) = &st.xdg {
                x.get_xdg_output(out, &qh, i);
            }
            if let Some(p) = &st.power {
                p.get_output_power(out, &qh, i);
            }
        }
    }
    roundtrip(conn, &mut q, &mut st, wait)?;
    let picked: Vec<usize> = match output {
        Some(n) => {
            let i = st
                .outputs
                .iter()
                .position(|o| o.name == n)
                .ok_or(format!("unknown output '{n}'"))?;
            vec![i]
        }
        // An output that is off has nothing on screen to copy (and would never deliver it).
        None => (0..st.outputs.len())
            .filter(|&i| !st.outputs[i].off)
            .collect(),
    };
    if picked.is_empty() {
        return Err("no outputs".into());
    }
    // grim: the image is the logical layout at the greatest scale of its outputs, at least 1.
    // One output at its own scale is its frame as copied, pixel for pixel.
    let scale = picked
        .iter()
        .map(|&i| st.outputs[i].pixel_scale())
        .fold(1.0, f64::max);
    let px = |logical: i32| (logical as f64 * scale).round() as i32;
    let mut shots = vec![];
    for &i in &picked {
        shots.push((i, capture_one(conn, &mut q, &mut st, i, cursor, wait)?));
    }
    if shots.len() == 1 {
        let (i, img) = shots.pop().unwrap();
        let (lw, lh) = (st.outputs[i].logical.2, st.outputs[i].logical.3);
        // The logical size is whole, so the scale guessed from it can be a pixel off the frame.
        let near = |a: i32, b: u32| (a - b as i32).abs() <= 1;
        if lw <= 0 || (near(px(lw), img.width()) && near(px(lh), img.height())) {
            return Ok(img);
        }
        shots.push((i, img));
    }
    let x0 = picked
        .iter()
        .map(|&i| st.outputs[i].logical.0)
        .min()
        .unwrap_or(0);
    let y0 = picked
        .iter()
        .map(|&i| st.outputs[i].logical.1)
        .min()
        .unwrap_or(0);
    let x1 = picked
        .iter()
        .map(|&i| st.outputs[i].logical.0 + st.outputs[i].logical.2)
        .max()
        .unwrap_or(0);
    let y1 = picked
        .iter()
        .map(|&i| st.outputs[i].logical.1 + st.outputs[i].logical.3)
        .max()
        .unwrap_or(0);
    let mut canvas = RgbaImage::new(px(x1 - x0).max(0) as u32, px(y1 - y0).max(0) as u32);
    for (i, img) in shots {
        let o = &st.outputs[i];
        let (w, h) = (px(o.logical.2).max(0) as u32, px(o.logical.3).max(0) as u32);
        let img = if (w, h) == img.dimensions() {
            img
        } else {
            image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
        };
        image::imageops::replace(
            &mut canvas,
            &img,
            px(o.logical.0 - x0) as i64,
            px(o.logical.1 - y0) as i64,
        );
    }
    Ok(canvas)
}

/// PNG bytes of an image (what grim/magick write for these files).
pub fn png(img: &RgbaImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

#[cfg(test)]
mod tests {
    use super::{shm_size, to_rgba};
    use wayland_client::protocol::wl_shm::Format;

    /// The system refuses the thread that follows the selection (the limit on threads is
    /// reached): `watch_text` returns and the refusal is in the log. In a process of its own:
    /// the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_selection_thread_is_logged() {
        const TEST: &str = concat!(module_path!(), "::a_refused_selection_thread_is_logged");
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        crate::testenv::refuse_new_threads();
        let ((), text) = crate::testenv::stderr_with(|| super::watch_text(|_| {}));
        assert!(
            text.contains("clipboard: cannot start the thread that follows the selection:"),
            "stderr: {text:?}"
        );
    }

    #[test]
    fn bad_screencopy_buffers_are_errors_not_panics() {
        // stride shorter than a row of 4-byte pixels
        assert!(to_rgba(Format::Xrgb8888, 4, 2, 8, &[0; 64], false).is_err());
        assert!(to_rgba(Format::Xrgb8888, 4, 2, 8, &[0; 64], true).is_err());
        // data shorter than stride * h
        assert!(to_rgba(Format::Argb8888, 4, 2, 16, &[0; 20], false).is_err());
        // stride * h overflows u32
        assert!(to_rgba(Format::Xrgb8888, 1 << 16, 1 << 16, 1 << 18, &[0; 64], false).is_err());
        assert_eq!(shm_size(1 << 16, 1 << 16, 1 << 18), None);
        assert_eq!(shm_size(4, 2, 8), None);
        assert_eq!(shm_size(4, 2, 16), Some(32));
        assert!(to_rgba(Format::C8, 1, 1, 4, &[0; 4], false).is_err());
    }

    #[test]
    fn screencopy_rows_honour_stride_and_invert() {
        // 2x2 Xrgb8888 with 4 padding bytes per row
        let data = [
            1, 2, 3, 0, 4, 5, 6, 0, 9, 9, 9, 9, //
            7, 8, 9, 0, 10, 11, 12, 0, 9, 9, 9, 9,
        ];
        let img = to_rgba(Format::Xrgb8888, 2, 2, 12, &data, false).unwrap();
        assert_eq!(img.get_pixel(1, 0).0, [6, 5, 4, 255]);
        assert_eq!(img.get_pixel(0, 1).0, [9, 8, 7, 255]);
        let img = to_rgba(Format::Xrgb8888, 2, 2, 12, &data, true).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [9, 8, 7, 255]);
        assert_eq!(img.get_pixel(1, 1).0, [6, 5, 4, 255]);
    }
}

/// The clipboard watcher against a fake data-control compositor on a socket pair: every step
/// waits for the watcher's next request or reported text, never for time.
#[cfg(test)]
mod watch_tests {
    use std::collections::VecDeque;
    use std::fs::File;
    use std::io::Write;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, mpsc};
    use wayland_protocols::ext::data_control::v1::server::{
        ext_data_control_device_v1 as ext_device, ext_data_control_manager_v1 as ext_manager,
        ext_data_control_offer_v1 as ext_offer,
    };
    use wayland_protocols_wlr::data_control::v1::server::{
        zwlr_data_control_device_v1 as wlr_device, zwlr_data_control_manager_v1 as wlr_manager,
        zwlr_data_control_offer_v1 as wlr_offer,
    };
    use wayland_server::backend::{ClientData, ObjectId};
    use wayland_server::protocol::wl_seat;
    use wayland_server::{
        Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, New, Resource,
    };

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Proto {
        Ext,
        Wlr,
    }

    #[derive(Debug)]
    enum Req {
        Device(Proto),
        Receive {
            offer: ObjectId,
            mime: String,
            fd: OwnedFd,
        },
        Destroy(ObjectId),
    }

    enum Dev {
        Ext(ext_device::ExtDataControlDeviceV1),
        Wlr(wlr_device::ZwlrDataControlDeviceV1),
    }

    enum Off {
        Ext(ext_offer::ExtDataControlOfferV1),
        Wlr(wlr_offer::ZwlrDataControlOfferV1),
    }

    impl Off {
        fn id(&self) -> ObjectId {
            match self {
                Off::Ext(o) => o.id(),
                Off::Wlr(o) => o.id(),
            }
        }
    }

    #[derive(Default)]
    struct Comp {
        dev: Option<Dev>,
        log: VecDeque<Req>,
    }

    struct NoData;
    impl ClientData for NoData {}

    impl GlobalDispatch<wl_seat::WlSeat, ()> for Comp {
        fn bind(
            _: &mut Comp,
            _: &DisplayHandle,
            _: &Client,
            r: New<wl_seat::WlSeat>,
            _: &(),
            d: &mut DataInit<'_, Comp>,
        ) {
            d.init(r, ());
        }
    }

    impl Dispatch<wl_seat::WlSeat, ()> for Comp {
        fn request(
            _: &mut Comp,
            _: &Client,
            _: &wl_seat::WlSeat,
            _: wl_seat::Request,
            _: &(),
            _: &DisplayHandle,
            _: &mut DataInit<'_, Comp>,
        ) {
        }
    }

    macro_rules! fake_data_control {
        ($proto:expr, $manager:ident :: $Manager:ident, $device:ident :: $Device:ident,
         $offer:ident :: $Offer:ident, $wrap:path) => {
            impl GlobalDispatch<$manager::$Manager, ()> for Comp {
                fn bind(
                    _: &mut Comp,
                    _: &DisplayHandle,
                    _: &Client,
                    r: New<$manager::$Manager>,
                    _: &(),
                    d: &mut DataInit<'_, Comp>,
                ) {
                    d.init(r, ());
                }
            }

            impl Dispatch<$manager::$Manager, ()> for Comp {
                fn request(
                    s: &mut Comp,
                    _: &Client,
                    _: &$manager::$Manager,
                    rq: $manager::Request,
                    _: &(),
                    _: &DisplayHandle,
                    d: &mut DataInit<'_, Comp>,
                ) {
                    if let $manager::Request::GetDataDevice { id, .. } = rq {
                        s.dev = Some($wrap(d.init(id, ())));
                        s.log.push_back(Req::Device($proto));
                    }
                }
            }

            impl Dispatch<$device::$Device, ()> for Comp {
                fn request(
                    _: &mut Comp,
                    _: &Client,
                    _: &$device::$Device,
                    _: $device::Request,
                    _: &(),
                    _: &DisplayHandle,
                    _: &mut DataInit<'_, Comp>,
                ) {
                }
            }

            impl Dispatch<$offer::$Offer, ()> for Comp {
                fn request(
                    s: &mut Comp,
                    _: &Client,
                    o: &$offer::$Offer,
                    rq: $offer::Request,
                    _: &(),
                    _: &DisplayHandle,
                    _: &mut DataInit<'_, Comp>,
                ) {
                    match rq {
                        $offer::Request::Receive { mime_type, fd } => {
                            s.log.push_back(Req::Receive {
                                offer: o.id(),
                                mime: mime_type,
                                fd,
                            })
                        }
                        $offer::Request::Destroy => s.log.push_back(Req::Destroy(o.id())),
                        _ => {}
                    }
                }
            }
        };
    }

    fake_data_control!(
        Proto::Ext,
        ext_manager::ExtDataControlManagerV1,
        ext_device::ExtDataControlDeviceV1,
        ext_offer::ExtDataControlOfferV1,
        Dev::Ext
    );
    fake_data_control!(
        Proto::Wlr,
        wlr_manager::ZwlrDataControlManagerV1,
        wlr_device::ZwlrDataControlDeviceV1,
        wlr_offer::ZwlrDataControlOfferV1,
        Dev::Wlr
    );

    /// The fake compositor and the watcher thread connected to it.
    struct Fake {
        display: Display<Comp>,
        st: Comp,
        client: Client,
        /// Our end of the client's socket, shut down to disconnect it.
        socket: UnixStream,
        texts: mpsc::Receiver<String>,
        watcher: Option<std::thread::JoinHandle<()>>,
    }

    impl Fake {
        /// Advertises a seat and the managers of `protos` (in this order) and starts the watcher.
        fn start(protos: &[Proto]) -> Fake {
            let display = Display::<Comp>::new().unwrap();
            let dh = display.handle();
            dh.create_global::<Comp, wl_seat::WlSeat, ()>(1, ());
            for p in protos {
                match p {
                    Proto::Wlr => {
                        dh.create_global::<Comp, wlr_manager::ZwlrDataControlManagerV1, ()>(2, ())
                    }
                    Proto::Ext => {
                        dh.create_global::<Comp, ext_manager::ExtDataControlManagerV1, ()>(1, ())
                    }
                };
            }
            let (ours, theirs) = UnixStream::pair().unwrap();
            let socket = ours.try_clone().unwrap();
            let client = display
                .handle()
                .insert_client(ours, Arc::new(NoData))
                .unwrap();
            let conn = wayland_client::Connection::from_socket(theirs).unwrap();
            let (tx, texts) = mpsc::channel();
            let watcher = std::thread::spawn(move || {
                super::watch(&conn, &move |t| {
                    let _ = tx.send(t);
                });
            });
            Fake {
                display,
                st: Comp::default(),
                client,
                socket,
                texts,
                watcher: Some(watcher),
            }
        }

        /// The watcher's next request, dispatching clients until it arrives.
        fn next(&mut self) -> Req {
            use rustix::event::{PollFd, PollFlags, poll};
            loop {
                if let Some(r) = self.st.log.pop_front() {
                    return r;
                }
                self.display.flush_clients().unwrap();
                {
                    let fd = self.display.backend().poll_fd();
                    poll(&mut [PollFd::from_borrowed_fd(fd, PollFlags::IN)], None).unwrap();
                }
                self.display.dispatch_clients(&mut self.st).unwrap();
            }
        }

        fn text(&mut self) -> String {
            self.texts.recv().expect("the watcher stopped")
        }

        /// `data_offer` with its `offer` events.
        fn offer(&mut self, mimes: &[&str]) -> Off {
            let dh = self.display.handle();
            let o = match self.st.dev.as_ref().expect("no data device") {
                Dev::Ext(d) => {
                    let o = self
                        .client
                        .create_resource::<ext_offer::ExtDataControlOfferV1, (), Comp>(
                            &dh,
                            d.version(),
                            (),
                        )
                        .unwrap();
                    d.data_offer(&o);
                    Off::Ext(o)
                }
                Dev::Wlr(d) => {
                    let o = self
                        .client
                        .create_resource::<wlr_offer::ZwlrDataControlOfferV1, (), Comp>(
                            &dh,
                            d.version(),
                            (),
                        )
                        .unwrap();
                    d.data_offer(&o);
                    Off::Wlr(o)
                }
            };
            for m in mimes {
                match &o {
                    Off::Ext(o) => o.offer((*m).to_owned()),
                    Off::Wlr(o) => o.offer((*m).to_owned()),
                }
            }
            o
        }

        fn select(&mut self, o: Option<&Off>) {
            match (self.st.dev.as_ref().unwrap(), o) {
                (Dev::Ext(d), Some(Off::Ext(o))) => d.selection(Some(o)),
                (Dev::Ext(d), None) => d.selection(None),
                (Dev::Wlr(d), Some(Off::Wlr(o))) => d.selection(Some(o)),
                (Dev::Wlr(d), None) => d.selection(None),
                _ => unreachable!("offer of the other protocol"),
            }
        }

        fn select_primary(&mut self, o: &Off) {
            match (self.st.dev.as_ref().unwrap(), o) {
                (Dev::Ext(d), Off::Ext(o)) => d.primary_selection(Some(o)),
                (Dev::Wlr(d), Off::Wlr(o)) => d.primary_selection(Some(o)),
                _ => unreachable!("offer of the other protocol"),
            }
        }

        fn flush(&mut self) {
            self.display.flush_clients().unwrap();
        }

        /// Waits for the `receive` of `o` and returns its MIME type and the pipe to write.
        fn receive(&mut self, o: &Off) -> (String, File) {
            match self.next() {
                Req::Receive { offer, mime, fd } if offer == o.id() => (mime, File::from(fd)),
                r => panic!("expected receive of {:?}, got {r:?}", o.id()),
            }
        }

        fn destroyed(&mut self, o: &Off) {
            match self.next() {
                Req::Destroy(id) if id == o.id() => {}
                r => panic!("expected destroy of {:?}, got {r:?}", o.id()),
            }
        }

        /// The watcher bound `p`'s data device; the first selection (none) reads as "".
        fn connected(&mut self, p: Proto) {
            match self.next() {
                Req::Device(got) => assert_eq!(got, p, "data-control protocol the watcher bound"),
                r => panic!("expected get_data_device, got {r:?}"),
            }
            self.select(None);
            self.flush();
            assert_eq!(self.text(), "");
        }
    }

    impl Drop for Fake {
        /// Disconnecting the client ends the watcher.
        fn drop(&mut self) {
            let _ = self.socket.shutdown(std::net::Shutdown::Both);
            if let Some(w) = self.watcher.take() {
                w.join().unwrap();
            }
        }
    }

    /// Each protocol as the watcher meets it: ext when both are advertised, else wlr.
    const SETUPS: [(&[Proto], Proto); 2] = [
        (&[Proto::Wlr, Proto::Ext], Proto::Ext),
        (&[Proto::Wlr], Proto::Wlr),
    ];

    #[test]
    fn ext_data_control_is_preferred_and_wlr_is_the_fallback() {
        for (protos, used) in SETUPS {
            let mut c = Fake::start(protos);
            c.connected(used);
            let o = c.offer(&["text/plain"]);
            c.select(Some(&o));
            c.flush();
            let (mime, mut w) = c.receive(&o);
            assert_eq!(mime, "text/plain");
            w.write_all("über".as_bytes()).unwrap();
            drop(w);
            c.destroyed(&o);
            assert_eq!(c.text(), "über");
        }
    }

    #[test]
    fn a_selection_superseded_in_the_same_dispatch_is_destroyed() {
        for (protos, used) in SETUPS {
            let mut c = Fake::start(protos);
            c.connected(used);
            let a = c.offer(&["text/plain"]);
            c.select(Some(&a));
            let b = c.offer(&["text/plain", "text/plain;charset=utf-8"]);
            c.select(Some(&b));
            c.flush();
            c.destroyed(&a);
            let (mime, mut w) = c.receive(&b);
            assert_eq!(mime, "text/plain;charset=utf-8");
            w.write_all(b"second").unwrap();
            drop(w);
            c.destroyed(&b);
            assert_eq!(c.text(), "second");
            let none = c.offer(&["image/png"]);
            c.select(Some(&none));
            c.select(None);
            c.flush();
            c.destroyed(&none);
            assert_eq!(c.text(), "");
        }
    }

    #[test]
    fn selection_text_is_read_to_eof_while_events_go_on() {
        for (protos, used) in SETUPS {
            let mut c = Fake::start(protos);
            c.connected(used);
            let b = c.offer(&["text/plain"]);
            c.select(Some(&b));
            c.flush();
            let (_, mut w) = c.receive(&b);
            w.write_all(b"part one, ").unwrap();
            // The owner stalls mid-transfer: the watcher still handles events (the primary
            // offer it destroys) and does not report the text read so far.
            let p = c.offer(&["text/plain"]);
            c.select_primary(&p);
            c.flush();
            c.destroyed(&p);
            w.write_all(b"part two").unwrap();
            drop(w);
            c.destroyed(&b);
            assert_eq!(c.text(), "part one, part two");

            // A newer selection abandons the stalled transfer: only the new text is reported.
            let stale = c.offer(&["text/plain"]);
            c.select(Some(&stale));
            c.flush();
            let (_, mut ws) = c.receive(&stale);
            ws.write_all(b"stale").unwrap();
            let fresh = c.offer(&["text/plain"]);
            c.select(Some(&fresh));
            c.flush();
            c.destroyed(&stale);
            let (_, mut wf) = c.receive(&fresh);
            wf.write_all(b"fresh").unwrap();
            drop(wf);
            c.destroyed(&fresh);
            assert_eq!(c.text(), "fresh");
            drop(ws);
        }
    }
}
