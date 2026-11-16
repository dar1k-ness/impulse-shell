//! impulse-shell: native Wayland front-end running the original QML through the Rust runtime.

// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use qml::value::Value;
use shell::sched::{Deadlines, Due};
use shell::{
    input::{Axis, AxisCompression, MotionCompression, Pointer},
    keys::{KbdWin, kbd_target},
    layer::{Exclusion, Geometry, KeyboardInteractivity, Layer, LayerShell, LayerSurface, Panel},
    scene,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region, SurfaceData},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::EventLoop,
        protocols::xdg::shell::client::xdg_positioner::{
            Anchor as PAnchor, ConstraintAdjustment, Gravity,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{
            AxisScroll, CursorIcon, PointerEvent, PointerEventKind, PointerHandler,
            PointerThemeError, ThemeSpec, ThemedPointer,
        },
        touch::TouchHandler,
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgPositioner, XdgShell, XdgSurface,
            popup::{Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    globals::registry_queue_init,
    protocol::{
        wl_callback, wl_display, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm,
        wl_subcompositor, wl_subsurface, wl_surface, wl_touch,
    },
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::ZwlrLayerShellV1,
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};
use wayland_protocols_wlr::output_management::v1::client::{
    zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
    zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
    zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
};
use wayland_protocols_wlr::output_power_management::v1::client::{
    zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1,
    zwlr_output_power_v1::{self, ZwlrOutputPowerV1},
};

use wayland_protocols::wp::alpha_modifier::v1::client::{
    wp_alpha_modifier_surface_v1::WpAlphaModifierSurfaceV1, wp_alpha_modifier_v1::WpAlphaModifierV1,
};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::presentation_time::client::{
    wp_presentation::WpPresentation, wp_presentation_feedback,
    wp_presentation_feedback::WpPresentationFeedback,
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use wayland_protocols_hyprland::focus_grab::v1::client::{
    hyprland_focus_grab_manager_v1::HyprlandFocusGrabManagerV1,
    hyprland_focus_grab_v1::{self, HyprlandFocusGrabV1},
};
use wayland_protocols_hyprland::global_shortcuts::v1::client::{
    hyprland_global_shortcut_v1::{self, HyprlandGlobalShortcutV1},
    hyprland_global_shortcuts_manager_v1::HyprlandGlobalShortcutsManagerV1,
};
use wayland_protocols_hyprland::toplevel_export::v1::client::{
    hyprland_toplevel_export_frame_v1::{self, HyprlandToplevelExportFrameV1},
    hyprland_toplevel_export_manager_v1::HyprlandToplevelExportManagerV1,
};

use smithay_client_toolkit::data_device_manager::{
    DataDeviceManagerState, WritePipe,
    data_device::{DataDevice, DataDeviceHandler},
    data_offer::{DataOfferHandler, DragOffer},
};
use smithay_client_toolkit::primary_selection::{
    PrimarySelectionManagerState,
    device::{PrimarySelectionDevice, PrimarySelectionDeviceHandler},
    selection::{PrimarySelectionSource, PrimarySelectionSourceHandler},
};
use smithay_client_toolkit::reexports::client::protocol::{
    wl_data_device::WlDataDevice, wl_data_device_manager::DndAction,
};
use smithay_client_toolkit::reexports::protocols::wp::idle_inhibit::zv1::client::{
    zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1, zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
};
use smithay_client_toolkit::reexports::protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1::{self, ZwpLinuxBufferParamsV1},
    zwp_linux_dmabuf_feedback_v1::{self, ZwpLinuxDmabufFeedbackV1},
    zwp_linux_dmabuf_v1::{self, ZwpLinuxDmabufV1},
};
use smithay_client_toolkit::reexports::protocols::wp::primary_selection::zv1::client::{
    zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1,
    zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1,
};
use smithay_client_toolkit::session_lock::{
    SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
    SessionLockSurfaceConfigure,
};
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    zwp_text_input_v3::{self, ZwpTextInputV3},
};

/// A `WlSessionLock` holding (or asking for) the session lock, with its per-screen surfaces.
struct Session {
    item: usize,
    lock: SessionLock,
    /// The compositor confirmed the lock: surfaces are shown.
    locked: bool,
    /// The compositor has read the `lock` request ([`LockSync`]) and did not refuse it: only
    /// then are lock surfaces asked for.
    answered: bool,
    /// The `surface` Component, instantiated once per output.
    tmpl: usize,
    surfaces: Vec<(usize, wl_output::WlOutput)>,
}

enum Surf {
    /// Quickshell `WlSessionLockSurface` on ext_session_lock_v1.
    Lock(SessionLockSurface),
    /// Quickshell `PopupWindow`: xdg_popup parented to the surface of its `anchor.window` (a
    /// panel, a toplevel or another popup); `key` is its geometry.
    Popup { popup: Popup, key: PopupKey },
    /// A `PanelWindow` (the bar, sidebars, a `StyledPopup` LazyLoader...).
    /// `on` is the wl_output the layer surface was created for (None: compositor's choice).
    Panel {
        layer: LayerSurface,
        key: PanelKey,
        on: Option<wl_output::WlOutput>,
    },
    /// A top-level `ApplicationWindow`/`Window` (`impulse-shell -p app.qml`): an xdg_toplevel.
    Toplevel(Window),
}

#[derive(Clone, PartialEq)]
struct PanelKey {
    namespace: String,
    /// `implicitWidth`/`implicitHeight`: the size until the compositor configures one.
    size: (u32, u32),
    geom: Geometry,
    layer: u32,
    masked: bool,
    /// `screen.name` of the PanelWindow (None: compositor's choice).
    output: Option<String>,
}

impl Surf {
    fn wl_surface(&self) -> &wl_surface::WlSurface {
        match self {
            Surf::Popup { popup, .. } => popup.wl_surface(),
            Surf::Panel { layer, .. } => layer.wl_surface(),
            Surf::Lock(l) => l.wl_surface(),
            Surf::Toplevel(t) => t.wl_surface(),
        }
    }

    fn layer(&self) -> Option<&LayerSurface> {
        match self {
            Surf::Panel { layer: l, .. } => Some(l),
            Surf::Popup { .. } | Surf::Lock(_) | Surf::Toplevel(_) => None,
        }
    }
}

#[derive(Clone, PartialEq)]
struct PopupKey {
    parent: usize,
    size: (i32, i32),
    rect: (i32, i32, i32, i32),
    edges: u32,
    gravity: u32,
    adjust: u32,
}

type SyncKey = (u64, usize, usize, Option<bool>, u64);

/// A replaced panel reserving space, kept mapped until `succ`, the panel taking its place,
/// commits its first frame ([`App::release_retiring`]).
struct Retiring {
    win: Win,
    /// Item of the successor's window; None until one is paired ([`pair_successors`]).
    succ: Option<usize>,
}

/// The windows laid out while hidden (`App::prelayout_hidden`).
const PRELAID_WINDOWS: [&str; 5] = [
    "PanelWindow",
    "PopupWindow",
    "FloatingWindow",
    "ApplicationWindow",
    "Window",
];

fn is_panel(w: &qml::runtime::World, i: usize) -> bool {
    w.items[i].types.iter().any(|t| t == "PanelWindow")
}

struct Win {
    /// Running crossfades' start frames over the window, the oldest on top; dropped before
    /// `gl` and `surf`.
    veils: Vec<Veil>,
    /// Veils of ended crossfades, unmapped and kept for the next ones: a theme switch draws
    /// its start frame into a surface with buffers allocated already. Also dropped before
    /// `gl` and `surf`.
    spare: Vec<Veil>,
    item: usize,
    /// Dropped before `surf`: the EGL surface must go before its wl_surface.
    gl: Option<shell::gpu::Surface>,
    /// Also dropped before `surf`.
    scale: WinScale,
    surf: Surf,
    size: (u32, u32),
    configured: bool,
    dirty: bool,
    /// The last frame showed a self-animating item: keep drawing every frame.
    anim: bool,
    /// A frame callback is outstanding: draw again only once the compositor has shown the last
    /// frame.
    pending: bool,
    /// When that callback was requested.
    asked: Option<std::time::Instant>,
    /// The last frame was committed without a frame callback (nothing on it animates and it is
    /// not one of a run, see `in_frame_run`): the next draw waits one refresh interval from
    /// it, so redraws stay at the display's rate.
    shown: Option<std::time::Instant>,
    /// When the last frame was committed, with a frame callback or without one.
    committed: Option<std::time::Instant>,
    /// World layout revision and size of the last layout: unchanged means geometry is current.
    laid: (u64, (u32, u32)),
    /// Consecutive frames whose full layout ended unsettled.
    unsettled: u8,
    /// Input region last sent for a masked window.
    region: Option<Vec<[i32; 4]>>,
    /// The last GPU frame, reshown with new screencopy textures when nothing else changed.
    last: Option<shell::gpu::Frame>,
    /// `last` had a texture swapped since it was shown.
    swapped: bool,
    /// The CPU fallback's last committed pixels: the next buffer damages only what differs.
    shm_prev: Option<qsg::raster::Canvas>,
    /// None until the first buffer commit, then whether the compositor has mapped the surface.
    mapped: Option<bool>,
}

/// Crossfades of one window running at once on their own veils; the next restarts in-window.
const MAX_VEILS: usize = 3;

/// wl_subcompositor and wp_alpha_modifier_v1: a crossfade shows its start frame on a
/// subsurface that the compositor fades out (`Veil`).
struct VeilMgr {
    sub: wl_subcompositor::WlSubcompositor,
    alpha: WpAlphaModifierV1,
}

impl VeilMgr {
    /// An empty, unmapped veil for `parent`: a synchronized subsurface at 0,0 above it with no
    /// input region.
    fn veil(
        &self,
        compositor: &CompositorState,
        qh: &QueueHandle<App>,
        parent: &wl_surface::WlSurface,
    ) -> Veil {
        let surf = compositor.wl_compositor().create_surface(qh, ());
        let sub = self.sub.get_subsurface(&surf, parent, qh, ());
        sub.set_position(0, 0);
        sub.set_sync();
        if let Ok(r) = Region::new(compositor) {
            surf.set_input_region(Some(r.wl_region()));
        }
        let alpha = self.alpha.get_surface(&surf, qh, ());
        Veil {
            gl: None,
            alpha,
            viewport: None,
            sub,
            surf,
            mapped: false,
            start: None,
            elapsed: Duration::ZERO,
            dur: Duration::ZERO,
        }
    }
}

/// A crossfade (`Crossfade.request`) through the compositor: the window's previous frame drawn once
/// on a subsurface over it, faded out with wp_alpha_modifier_v1 on the animation clock while
/// the window keeps drawing only what changes. `Gpu::begin_fade`, which blends inside the
/// window's buffer and so redraws the whole window every frame, is the fallback.
struct Veil {
    /// Dropped before the subsurface's wl_surface.
    gl: Option<shell::gpu::Surface>,
    alpha: WpAlphaModifierSurfaceV1,
    viewport: Option<WpViewport>,
    sub: wl_subsurface::WlSubsurface,
    surf: wl_surface::WlSurface,
    /// A buffer is attached: committing it with its window shows the veil.
    mapped: bool,
    start: Option<Instant>,
    elapsed: Duration,
    dur: Duration,
}

impl Veil {
    /// Starts a `dur` crossfade with its window's buffer scale and viewport destination
    /// (`WinScale::buf`/`dest`); its window's commits set its opacity (`advance`).
    fn arm(
        &mut self,
        vp: Option<&WpViewporter>,
        qh: &QueueHandle<App>,
        (buf, dest): (i32, Option<(u32, u32)>),
        dur: Duration,
    ) {
        self.surf.set_buffer_scale(buf);
        match (dest, self.viewport.as_ref()) {
            (Some((w, h)), Some(v)) => v.set_destination(w as i32, h as i32),
            (Some((w, h)), None) => {
                if let Some(vp) = vp {
                    let v = vp.get_viewport(&self.surf, qh, ());
                    v.set_destination(w as i32, h as i32);
                    self.viewport = Some(v);
                }
            }
            // wp_viewport.set_destination(-1, -1) unsets the destination size.
            (None, Some(v)) => v.set_destination(-1, -1),
            (None, None) => {}
        }
        self.start = None;
        self.elapsed = Duration::ZERO;
        self.dur = dur;
    }

    /// Unmaps the veil with its window's next commit (wl_surface.attach of a null buffer;
    /// a synchronized subsurface's state applies with its parent's). Its surfaces stay for
    /// the next crossfade.
    fn hide(&mut self) {
        if std::mem::take(&mut self.mapped) {
            self.surf.attach(None, 0, 0);
            self.surf.commit();
        }
    }

    /// The veil's wp_alpha_modifier multiplier at animation time `now`; the fade starts with
    /// the first frame that shows it.
    fn advance(&mut self, now: Instant) -> u32 {
        let start = *self.start.get_or_insert(now);
        self.elapsed = self.elapsed.max(now.saturating_duration_since(start));
        let t = self.elapsed.as_secs_f32() / self.dur.as_secs_f32().max(0.001);
        (f64::from(shell::gpu::fade_opacity(t)) * f64::from(u32::MAX)).round() as u32
    }

    fn done(&self) -> bool {
        self.elapsed >= self.dur
    }
}

impl Drop for Veil {
    fn drop(&mut self) {
        drop(self.gl.take());
        self.alpha.destroy();
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
        self.sub.destroy();
        self.surf.destroy();
    }
}

/// A window's HiDPI state (QWaylandWindow scale + QWaylandFractionalScale + QWaylandViewport).
struct WinScale {
    /// wp_fractional_scale_v1 and wp_viewport of the surface.
    obj: Option<(WpFractionalScaleV1, WpViewport)>,
    /// wp_fractional_scale_v1.preferred_scale (120ths), 0 until sent.
    frac: u32,
    /// Integer scale (wl_surface.preferred_buffer_scale, else the entered outputs').
    int: i32,
    /// devicePixelRatio published to the window item.
    dpr: f64,
    /// wl_surface.set_buffer_scale and wp_viewport.set_destination last requested.
    buf: i32,
    dest: Option<(u32, u32)>,
    /// The factor of the window's screen (`QT_SCALE_FACTOR`, `QT_SCREEN_SCALE_FACTORS`):
    /// the surface is this many times the size QML sees.
    factor: f64,
    /// The surface size the compositor configured and the window size it made.
    surf: Option<((u32, u32), (u32, u32))>,
    /// The output a toplevel entered last (Qt moves the window to that screen).
    on: Option<String>,
}

impl Default for WinScale {
    fn default() -> Self {
        Self {
            obj: None,
            frac: 0,
            int: 1,
            dpr: 1.0,
            buf: 1,
            dest: None,
            factor: 1.0,
            surf: None,
            on: None,
        }
    }
}

/// `v` QML pixels in surface pixels on a screen of factor `f` (QHighDpi::toNativePixels).
fn to_surface(v: i32, f: f64) -> i32 {
    if f == 1.0 {
        v
    } else {
        (v as f64 * f).round() as i32
    }
}

impl WinScale {
    /// The scale of the window's output: the compositor's fractional one, else `wl_output`'s.
    fn output(&self) -> f64 {
        if self.obj.is_some() && self.frac > 0 {
            self.frac as f64 / 120.0
        } else {
            self.int.max(1) as f64
        }
    }
}

impl Win {
    /// The device pixels of the window's buffer: its surface's at the output's scale, as Qt
    /// sizes it (QWaylandWindow). With a screen's factor that is not always the window's size
    /// times its scale: a surface 1556 wide at a factor of 2.5 is a window 622 wide.
    fn device_size(&self) -> (usize, usize) {
        let (w, h) = self.surface_size();
        shell::gpu::device_size(w as usize, h as usize, self.scale.output())
    }

    /// The surface's size in the compositor's space: the configured one while the window
    /// still has the size that gave, else the window's times its screen's factor.
    fn surface_size(&self) -> (u32, u32) {
        let f = self.scale.factor;
        match self.scale.surf {
            Some((s, l)) if l == self.size => s,
            _ => (
                to_surface(self.size.0 as i32, f) as u32,
                to_surface(self.size.1 as i32, f) as u32,
            ),
        }
    }

    /// The compositor configured a surface of `s`: the window is that over its screen's
    /// factor (QHighDpi::fromNativePixels).
    fn set_surface_size(&mut self, s: (u32, u32)) {
        let f = self.scale.factor;
        let px = |v: u32| match v {
            0 => 0,
            v if f == 1.0 => v,
            v => ((v as f64 / f).round() as u32).max(1),
        };
        self.size = (px(s.0), px(s.1));
        self.scale.surf = Some((s, self.size));
    }
}

impl Drop for WinScale {
    fn drop(&mut self) {
        if let Some((f, v)) = self.obj.take() {
            f.destroy();
            v.destroy();
        }
    }
}

/// The vsync interval (ms) QSGAnimationDriver takes from the primary screen's refresh rate.
fn primary_vsync(outputs: &OutputState) -> Option<f64> {
    let o = outputs.outputs().next()?;
    let hz = outputs
        .info(&o)?
        .modes
        .iter()
        .find(|m| m.current)?
        .refresh_rate;
    // QSGDefaultAnimationDriver's float m_vsync from QScreen::refreshRate (mHz / 1000.f).
    (hz >= 1000).then(|| f64::from(1000.0f32 / (hz as f32 / 1000.0)))
}

/// The next animation step waits for a frame callback when every window showing a running
/// animation (`(pending, animating)` per window) still awaits its callback: the first to come
/// back is the step. A window that could draw the step, or an animation shown in no window
/// (`loose`: a script step, a non-visual target), keeps the clock on vsync. A hidden animating
/// window thus stops the clock only while nothing else animates. `unseen`: the running
/// animations include endless ones that show nothing (see `animating_windows`), which wait
/// like a hidden window.
fn awaits_frame(wins: impl IntoIterator<Item = (bool, bool)>, loose: bool, unseen: bool) -> bool {
    let mut any = unseen;
    for (pending, animating) in wins {
        if animating && !pending {
            return false;
        }
        any |= animating;
    }
    any && !loose
}

/// What of the shell nobody can see ([`App::sync_unseen`]).
#[derive(Clone, Copy, Default, PartialEq)]
struct Sight {
    /// The session is locked: only the lock surfaces are shown.
    locked: bool,
    /// No output is on.
    dark: bool,
}

type AnimKey = (u64, Vec<usize>);

/// Collects the items `job` animates; `false` if it runs a script step, whose effects may land
/// anywhere.
fn job_targets(job: &qml::runtime::Job, out: &mut Vec<usize>) -> bool {
    use qml::runtime::JobKind;
    match &job.kind {
        JobKind::Prop { targets, .. }
        | JobKind::Smooth { targets, .. }
        | JobKind::Spring { targets, .. }
        | JobKind::Action { targets, .. } => {
            out.extend(targets.iter().map(|t| t.0));
            true
        }
        JobKind::Script => false,
        JobKind::Pause => true,
        JobKind::Anchor { targets, .. } => {
            out.extend(targets.iter().copied());
            true
        }
        JobKind::Seq(jobs) | JobKind::Par(jobs) => {
            let mut shown = true;
            for j in jobs {
                shown &= job_targets(j, out);
            }
            shown
        }
    }
}

/// Qt's animation clock (QSGDefaultAnimationDriver): while animations run, each frame moves it
/// one vsync on whatever the wall time did, so slow frames slow animations down, until a run of
/// lagging frames switches it to wall time (and ten good frames back). Idle, it follows the wall,
/// and with Timers left it gets back what the driver's time was behind by (`read_wall`).
struct AnimClock {
    t: f64,
    wall: f64,
    vsync: f64,
    /// `QSG_FIXED_ANIMATION_STEP`: the step each frame moves animations by, whatever the wall
    /// time did ([`fixed_animation_step`]).
    fixed: Option<f64>,
    running: bool,
    timer_mode: bool,
    lag: f64,
    bad: u32,
    good: u32,
    /// The latest time handed out (and the wall time then): where animations registered
    /// while the driver was stopped start, as QAnimationDriver::start() resets its timer.
    given: (f64, f64),
    /// A window got its frame callback while animating: the next pass is a rendered frame.
    frame_due: bool,
    /// Where the running driver's time started from zero.
    origin: f64,
    /// The running driver's m_time: float vsyncs summed from zero, so `t - origin` can't
    /// round across a whole ms.
    m: f64,
    /// This pass answers a frame callback: its draws add no extra driver step.
    answered: bool,
    /// On wall time: how much of it since `wall` `catch_up` gave animations already.
    caught: f64,
    /// Animations an event started stand at their start until a frame showed them there: Qt
    /// starts its driver in the handler and steps it after the frame that answers the event.
    unshown: bool,
    /// The running driver took the extra step that follows its first frame sent outside a
    /// frame callback (`App::draw_frame`). Later such frames, each after steps that sent
    /// nothing and went by the vsync timer, add none: the driver would run a step ahead of
    /// its time for each.
    led: bool,
    /// The driver's last step came with a frame callback (or began the run), not by the
    /// vsync timer.
    on_frame: bool,
    /// The running driver has taken its first step.
    stepped: bool,
    /// Exactly one window is exposed (`App::one_window`). Only then does
    /// QSGThreadedRenderLoop step the driver as that window syncs, first right after the
    /// update the window gets as the driver starts (startOrStopAnimationTimer,
    /// polishAndSync). With none or several a timer of one vsync steps it, first one
    /// interval after its start.
    one_window: bool,
    /// The end of a Timer's countdown started the driver before the Timer's handler ran
    /// (`World::timer_started_driver`), and the handler started the animations registered
    /// now. Qt draws a frame for the driver it started and steps it once before those
    /// animations are in its list: their first step comes with that frame's callback.
    late: bool,
    /// The wall time when the running driver's time started from zero.
    origin_wall: f64,
    /// The driver stopped with its time this far behind the wall time that went by since it
    /// started (ahead: below zero), at wall time `stopped`: QUnifiedTimer::localRestart stops
    /// the driver without taking its time over, and elapsed() is the wall clock again. The
    /// clock gets it at the next QUnifiedTimer::updateAnimationTimers (`read_wall`, `catch_up`,
    /// the first step of a driver started again).
    owed: f64,
    stopped: f64,
    /// The vsync timer stepping animations no drawn frame paces: its armed deadline
    /// ([`shell::sched::frame_deadline`]).
    timer: Option<f64>,
    /// The wall time the last turn looked at the clock (`advance`).
    seen: f64,
    /// The turn of the last step sent frames, and with what in flight: `Some(true)` a frame
    /// callback was outstanding when the step began, `Some(false)` the step was a frame
    /// callback's and none was, `None` no frame is sent (steps by the vsync timer).
    flying: Option<bool>,
    /// `frame_due` was set by this window's frame callback, not by a frame that leads a step.
    called: Option<usize>,
    /// Steps due at once with no frame callback to wait for. QSGThreadedRenderLoop is two
    /// frames ahead of the screen: one frame is with the compositor, the next waits in the
    /// render thread's swapBuffers for that one's callback (QWaylandGLContext::swapBuffers,
    /// `waitForFrameSync`), and the main thread has taken the step after it. A turn that
    /// outlasted two frames leaves it nothing in flight, and it takes two steps at once to be
    /// two ahead again (QWaylandWindow::requestUpdate finds no callback outstanding). A
    /// callback still on its way when the turn began is the first of the two; the answer to
    /// a frame sent after the turn is a step of its own.
    ahead: u8,
}

impl AnimClock {
    fn new() -> Self {
        Self {
            t: 0.0,
            wall: 0.0,
            vsync: f64::from(1000.0f32 / 60.0),
            fixed: fixed_animation_step(),
            running: false,
            timer_mode: false,
            lag: 0.0,
            bad: 0,
            good: 0,
            given: (0.0, 0.0),
            frame_due: false,
            origin: 0.0,
            m: 0.0,
            unshown: false,
            answered: false,
            caught: 0.0,
            led: false,
            on_frame: true,
            stepped: false,
            one_window: true,
            late: false,
            origin_wall: 0.0,
            owed: 0.0,
            stopped: 0.0,
            timer: None,
            seen: 0.0,
            flying: None,
            called: None,
            ahead: 0,
        }
    }

    /// Window `k`'s frame callback came while animations run: a step is due. A second callback
    /// of the window before that step is taken answers the frame drawn as the first one came
    /// (a compositor that had nothing to show for a while answers at once) and is a step of
    /// its own: Qt's render thread sends that frame only after the main thread has begun the
    /// first one's step, and each callback delivers an update request. So is a callback that
    /// came before the extra step after a frame that leads one (`App::draw_frame`) was taken:
    /// Qt's main thread took that step as the frame was drawn, before its callback.
    fn callback(&mut self, k: usize) {
        if self.frame_due && self.called.is_none_or(|c| c == k) {
            self.ahead = self.ahead.saturating_add(1);
        }
        self.frame_due = true;
        self.called.get_or_insert(k);
        self.answered = true;
    }

    /// The loop's next animation frame after a turn at `now` (see
    /// [`shell::sched::frame_deadline`]); arms the vsync timer when it is the one to wait for.
    ///
    /// A tick that came due while the turn ran, after the turn looked at the clock (`seen`),
    /// stays armed and is due at once: the next turn is its step, as a QTimer that timed out
    /// while its thread was busy fires once the event loop is back. Turns that follow each
    /// other without a pause (posted events that post themselves again) would else move
    /// every tick on by an interval and never step.
    fn arm(
        &mut self,
        now: f64,
        animating: bool,
        awaiting: bool,
        give_up: Option<f64>,
        registered: bool,
    ) -> Option<(f64, Due)> {
        // QUnifiedTimer::startAnimationDriver, QSGThreadedRenderLoop::animationStarted: the
        // one window exposed gets an update at once, and the driver takes its first step
        // right after that sync. Qt's frame shows nothing new and is not sent here: the step
        // is due now.
        if self.first(registered) && !awaiting {
            return Some((now, Due::Frame));
        }
        if !(self.running && animating) {
            self.ahead = 0;
        } else if now - self.seen >= 2.0 * self.vsync
            && let Some(flying) = self.flying
        {
            self.ahead = if flying { 1 } else { 2 };
        }
        if let Some(at) = self.timer.filter(|&at| at > self.seen && at <= now)
            && animating
            && !awaiting
        {
            return Some((at, Due::Frame));
        }
        let frame = shell::sched::frame_deadline(
            now,
            animating,
            awaiting,
            give_up,
            self.timer,
            self.running.then_some(self.wall),
            self.vsync,
        );
        // The end of a wait for a frame callback is no tick of the vsync timer.
        self.timer = frame
            .filter(|&(_, why)| why == Due::Frame && !awaiting)
            .map(|(at, _)| at);
        frame
    }

    fn now(&self, wall: f64) -> f64 {
        if !self.running {
            self.t + (wall - self.wall)
        } else if self.timer_mode {
            // On wall time QAnimationDriver::advanceAnimation reads the time when a frame
            // steps the driver: an event between two frames finds animations where the
            // last one left them.
            self.t
        } else {
            // QSGDefaultAnimationDriver::elapsed(): whole ms of its double m_time.
            self.origin + self.m.trunc()
        }
    }

    /// The time animations started by an event handled now begin at.
    fn stamp(&mut self, wall: f64) -> f64 {
        let t = self.now(wall);
        self.given = (t, wall);
        t
    }

    /// Animations were started, now: QUnifiedTimer::maybeUpdateAnimationsToCurrentTime. More
    /// than 50 ms after the last step of a driver on wall time, where its elapsed() goes on
    /// between frames, the time the running ones step to before the new ones join them. The
    /// driver's own timer of late and timely frames is not read. A driver that stopped behind
    /// the wall clock counts as having stepped that much longer ago.
    fn catch_up(&mut self, wall: f64) -> Option<f64> {
        if !self.running && self.owed != 0.0 {
            if wall - self.stopped + self.owed <= 50.0 {
                return None;
            }
            return Some(self.take_owed(wall));
        }
        let since = wall - self.wall - self.caught;
        if !self.running || !self.timer_mode || since <= 50.0 {
            return None;
        }
        self.caught += since;
        self.m += since;
        self.t = self.origin + self.m.trunc();
        self.given = (self.t, wall);
        Some(self.t)
    }

    /// The pause timer QUnifiedTimer::localRestart sets as it stops the driver with only Timers
    /// left: it fires when the Timer due first (`due`, on this clock) is due on the time the
    /// driver stopped at, and its updateAnimationTimers reads the wall clock: every Timer gets
    /// the time the driver was behind by, or waits for the time it was ahead by. With no Timer
    /// left QUnifiedTimer::stopTimer forgets its time: nothing is there to get it.
    fn read_wall(&mut self, wall: f64, due: f64) {
        if self.running || self.owed == 0.0 {
            return;
        }
        if due == f64::INFINITY {
            self.owed = 0.0;
        } else if self.now(wall) >= due {
            self.take_owed(wall);
        }
    }

    fn take_owed(&mut self, wall: f64) -> f64 {
        self.t = self.now(wall) + std::mem::take(&mut self.owed);
        self.wall = wall;
        self.given = (self.t, wall);
        self.t
    }

    /// A window's frame was `sent` to the compositor, or changed no pixel and was not.
    /// `leads`: drawing it set `frame_due` for the extra driver step that follows the first
    /// frame drawn outside a frame callback. An unsent frame gets no callback to wait for, so
    /// it takes that step back, else the steps would follow each other at once.
    fn sent(&mut self, sent: bool, leads: bool) {
        if leads {
            self.led = sent;
            self.frame_due &= sent;
        }
    }

    /// A wakeup now would be a new animation frame for `advance`.
    fn step_due(&self, wall: f64, awaiting: bool) -> bool {
        !self.running
            || self.frame_due
            || self.ahead > 0
            || (!awaiting && (self.first(true) || self.tick(wall)))
    }

    /// Animations are `registered` and their driver has its first step to take at once: one
    /// window is exposed (`one_window`), and there is no frame that shows them at their start
    /// to wait for.
    fn first(&self, registered: bool) -> bool {
        let driven = registered && !qml::debug::on(qml::debug::Switch::WALL_ANIM);
        let at_once = self.one_window && !self.late;
        driven && at_once && (!self.running || !(self.stepped || self.unshown))
    }

    /// The vsync timer is due. Each of its ticks is a step, as each event of
    /// QSGThreadedRenderLoop's animation timer is a QSGDefaultAnimationDriver::advance(): the
    /// timer keeps its grid, so the tick after one handled late comes a part of an interval
    /// after it. Not armed (a wait for a frame callback just ended): most of an interval
    /// after the last step.
    fn tick(&self, wall: f64) -> bool {
        self.timer
            .map_or(wall - self.wall >= 0.75 * self.vsync, |at| wall >= at)
    }

    /// The first animation registered, at time `since`: QSGDefaultAnimationDriver::start()
    /// zeroes the clock. No step yet.
    fn begin(&mut self, wall: f64, since: f64) -> f64 {
        self.running = true;
        self.origin = since.min(self.given.0).max(self.t - self.vsync);
        self.origin_wall = self.wall + (self.origin - self.t);
        // QUnifiedTimer::startAnimationDriver: the driver starts at elapsed(), the wall
        // clock. What a driver that stopped left behind comes with its first step.
        self.origin += std::mem::take(&mut self.owed);
        self.m = 0.0;
        self.t = self.origin;
        self.wall = wall;
        self.unshown = true;
        self.led = false;
        self.on_frame = true;
        self.stepped = false;
        self.late = false;
        self.stamp(wall)
    }

    /// A window committed a frame.
    fn shown(&mut self) {
        self.unshown = false;
    }

    /// A frame; `running`: animations are registered, the earliest at time `since`. Qt advances
    /// once per rendered frame: while a drawn frame awaits its callback (`awaiting`), only the
    /// callback (`frame_due`) is a new frame, however soon it comes.
    fn advance(&mut self, wall: f64, running: bool, since: f64, awaiting: bool) -> f64 {
        let frame_due = std::mem::take(&mut self.frame_due);
        let called = self.called.take().is_some();
        self.seen = wall;
        if !running || qml::debug::on(qml::debug::Switch::WALL_ANIM) {
            if self.running && self.fixed.is_none() {
                // The driver stops at its last step: the wall clock goes on from there.
                self.t = self.now(wall);
                self.owed = (self.wall - self.origin_wall) - (self.t - self.origin);
                self.stopped = self.wall;
                self.running = false;
            }
            self.t = self.now(wall);
            self.wall = wall;
            self.running = false;
            self.ahead = 0;
            self.flying = None;
            self.late = false;
        } else {
            let delta = wall - self.wall;
            let first = !awaiting && self.first(true);
            if !self.running && self.late && !frame_due && (awaiting || !self.tick(wall)) {
                // The frame sent for the driver the Timer's end started is with the
                // compositor; with no window to send one, the vsync timer's tick is to come.
            } else if !self.running {
                // Its first frame is one vsync on from its start.
                let late = self.late;
                self.begin(wall, since);
                self.unshown = false;
                self.m = self.fixed.unwrap_or(self.vsync);
                self.t = self.origin + self.m;
                self.stepped = true;
                // With one window the frame that shows this step is followed by the extra
                // step of `App::draw_frame`. Not when the step came with the frame callback
                // of the frame a Timer's end sent, nor when a timer steps the driver.
                self.led = late || !self.one_window;
            } else if self.unshown {
                // The frame that shows the start is still to be drawn.
            } else if frame_due || self.ahead > 0 || (!awaiting && (first || self.tick(wall))) {
                self.stepped = true;
                // (Else not a new frame: an IPC or service wakeup between two.)
                // A step such a wakeup took before the vsync timer leaves the next one a whole
                // interval after where this one was due: wakeups without end (posted events
                // that post themselves again) step once per vsync, not once per 0.75 of it.
                // A step owed after a turn of two frames or more; a frame callback that came
                // is taken first.
                let ahead = !frame_due && self.ahead > 0;
                self.ahead -= u8::from(ahead);
                let frame_due = frame_due || ahead;
                self.flying = (awaiting || called || ahead).then_some(awaiting);
                // A tick missed while a turn ran and stepped at once is a step of now.
                let early = (0.75 * self.vsync..self.vsync).contains(&delta);
                self.wall = if !frame_due && !self.timer_mode && early {
                    self.wall + self.vsync
                } else {
                    wall
                };
                // A frame callback after a step by the vsync timer comes less than an
                // interval after it (the timer is not in step with the output): the driver
                // moves by the time that passed. A whole interval for each would end an
                // animation whose frames are now sent, now not, before its time.
                let step = if frame_due && !self.on_frame {
                    delta.clamp(0.0, self.vsync)
                } else {
                    self.vsync
                };
                self.on_frame = frame_due || first;
                // A step by the timer is no first frame to follow with an extra step.
                self.led |= !frame_due;
                if frame_due {
                    // The timer's next tick is an interval after this step, not on its grid.
                    self.timer = None;
                }
                if let Some(step) = self.fixed {
                    self.m += step;
                    self.t = self.origin + self.m;
                } else if !self.timer_mode {
                    self.m += step;
                    self.t = self.origin + self.m;
                    if delta > 1.25 * self.vsync {
                        self.lag += delta / self.vsync;
                        self.bad += 1;
                        if self.lag > 10.0 && self.bad > 2 {
                            // From here elapsed() is the whole ms of m_time plus what the
                            // wall clock moved.
                            self.timer_mode = true;
                            self.m = self.m.trunc();
                            self.t = self.origin + self.m;
                        }
                    } else {
                        self.lag = 0.0;
                        self.bad = 0;
                    }
                } else {
                    // QSGDefaultAnimationDriver::elapsed() is whole ms on wall time too: of
                    // steps that follow each other at once only one that crosses a ms moves
                    // anything (QUnifiedTimer::updateAnimationTimers, `delta > 0`).
                    self.m += delta - self.caught;
                    self.t = self.origin + self.m.trunc();
                    if delta < 1.25 * self.vsync {
                        self.good += 1;
                    } else {
                        self.good = 0;
                    }
                    if self.good > 10 {
                        self.timer_mode = false;
                        self.bad = 0;
                        self.lag = 0.0;
                    }
                }
                self.caught = 0.0;
            }
        }
        let t = self.now(wall);
        self.given = (t, self.wall);
        t
    }
}

/// qsg_useConsistentTiming: with `QSG_FIXED_ANIMATION_STEP` set and not `no`, QUnifiedTimer's
/// consistent timing moves animations a fixed `DEFAULT_TIMER_INTERVAL` (16 ms) per frame and
/// never follows the wall time, so an animation takes the same frames on any machine.
fn fixed_animation_step() -> Option<f64> {
    std::env::var_os("QSG_FIXED_ANIMATION_STEP")
        .filter(|v| !v.is_empty() && v != "no")
        .map(|_| 16.0)
}

/// `App::animating_windows`: per window, whether a running animation draws in it; whether one
/// is loose (paced by no window); whether one shows nothing.
type AnimAttr = (Vec<bool>, bool, bool);

/// A copied dmabuf frame: item, toplevel, size, y-invert, the view's size.
type DmaReady = (usize, u32, (usize, usize), bool, (usize, usize));

/// How long before the callback an editor's drag is to have its motion handled, in ms, over
/// what handling one takes: the callback comes when the compositor sends it, not on the dot.
const MOTION_AHEAD_MARGIN: f64 = 1.0;

struct App {
    /// `QT_SCALE_FACTOR` and `QT_SCREEN_SCALE_FACTORS`, read after the config's pragmas.
    factors: shell::hidpi::Factors,
    /// Qt's animation driver time, which animations, Behaviors and Timers run on.
    clock: AnimClock,
    /// Initial screens published and startup windows made (hotplug handling may run).
    started: bool,
    registry: RegistryState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    gpu: Option<shell::gpu::Gpu>,
    rt: qml::runtime::Runtime,
    fonts: std::rc::Rc<std::cell::RefCell<scene::Fonts>>,
    wins: Vec<Win>,
    start: Instant,
    /// Hidden windows already laid out ahead of their first show, and the windows shown.
    prelaid: std::collections::HashSet<usize>,
    /// The number of objects made and whether a window had its size from the compositor when
    /// the last look found no hidden window to lay out: nothing to look for until one changes.
    prelaid_at: Option<(usize, bool)>,
    /// The last turn's deadlines found the loop idle with one of them still to lay out.
    prelayout_idle: bool,
    /// Window roots fully laid out at a size, with no layout-affecting write inside them since.
    clean: std::collections::HashMap<usize, (u32, u32)>,
    /// Local resizes inside a hidden prelaid window, by that window: re-examined when it draws.
    parked: rustc_hash::FxHashMap<usize, (Vec<usize>, rustc_hash::FxHashSet<usize>)>,
    /// Hidden windows whose layout went stale: size and when it first did; laid out again
    /// while the shown windows are left alone, so showing them needs no full pass.
    stale_hidden: std::collections::HashMap<usize, ((u32, u32), f64)>,
    /// Replaced panels that reserve space, kept mapped until their successor maps
    /// ([`App::release_retiring`]).
    retiring: Vec<Retiring>,
    /// `PopupWindow`s whose `anchor.window` cannot parent a popup, said once each.
    popup_refused: std::collections::HashSet<usize>,
    /// Work waiting for its window to leave the screen ([`App::sync_off_screen`]).
    off_screen: Vec<OffScreen>,
    /// Work whose window's surfaces are gone, waiting for the compositor to show the screen
    /// without it ([`App::show_without`]).
    unshown: Vec<Unshown>,
    unshown_ids: u64,
    /// Work whose layer Hyprland is animating away.
    closing: Vec<Closing>,
    hypr_close: shell::hyprclose::Watch,
    /// `ShellControl.ipcCall` calls the compositor has caught up with ([`App::order_self_ipc`]):
    /// the loop's turn runs their handlers.
    self_ipc: Vec<(String, String)>,
    /// Window roots put in `clean`; dead ones are dropped when the world purges its dead.
    roots: std::collections::HashSet<usize>,
    frames: u64,
    seat: SeatState,
    pointer: Option<ThemedPointer>,
    /// The surface wl_pointer.enter last entered and no leave followed: pointer focus is a
    /// surface, while `ptr` tracks the window item, which a replacement surface shares.
    ptr_focus: Option<wl_surface::WlSurface>,
    ptr: Pointer,
    axis_merge: AxisCompression,
    /// Which motions wait, and the one that does: (window item, position, time).
    motion_merge: MotionCompression<(usize, (f64, f64), u32)>,
    /// The time of the last frame callback (`wl_callback.done`).
    frame_time: Option<u32>,
    /// A motion reached the scene and no frame has answered it yet.
    motion_unanswered: bool,
    /// A motion reached the scene since the last refresh began (`frame_time` changed).
    motion_this_frame: bool,
    /// When the refresh began: its first frame callback.
    frame_at: Instant,
    /// The editor's drag has had the next refresh's motion already, ahead of its callback
    /// (`motion_ahead`).
    motion_early: bool,
    /// The frame callback being answered found its motion handled ahead of it.
    frame_has_motion: bool,
    /// How long the editor's drag takes to handle a motion, in ms: the last few, averaged.
    motion_cost: f64,
    /// What handling the motion not answered yet has taken so far, in ms.
    motion_took: f64,
    /// Pointer frames read from the socket and not handled yet, each with when it was read.
    ptr_frames: std::collections::VecDeque<(Instant, Vec<PointerEvent>)>,
    /// `animating_windows` as of the `AnimKey` it was computed for.
    anim_attr: std::cell::RefCell<Option<(AnimKey, AnimAttr)>>,
    wheel_deltas: shell::input::WheelDeltas,
    /// wl_touch device and the window of the touch point driving the pointer.
    touch: Option<wl_touch::WlTouch>,
    touch_win: Option<usize>,
    last_rev: u64,
    /// The binding errors there were when `log_errors` last looked.
    seen_bind: std::collections::HashSet<String>,
    /// Each output's info (Debug form) as last published to `Rustshell.screens`.
    output_infos: std::collections::HashMap<wayland_client::backend::ObjectId, OutputSnapshot>,
    /// `sync_key()` at the last sync passes.
    sync_key: SyncKey,
    /// Output hotplug/updates: `sync_panels` maps waiting panels onto the outputs it finds.
    outputs_gen: u64,
    /// The monitors' EDID identities (wlr-output-management), which pick each output's layout.
    head_mgr: Option<ZwlrOutputManagerV1>,
    heads: shell::output_heads::Heads<wayland_client::backend::ObjectId>,
    /// ColorQuantizer source+size last computed per item.
    quant: Quantizer,
    /// Qt's QWaylandPrimarySelectionDeviceManagerV1: Hyprland drops every client's primary
    /// selection while the pointer is over a client without a primary selection device.
    primary: Option<PrimarySelectionManagerState>,
    primary_dev: Option<PrimarySelectionDevice>,
    /// Our primary selection source, its text and the reads of it in flight
    /// (QWaylandPrimarySelectionSourceV1).
    primary_src: Option<(
        PrimarySelectionSource,
        std::sync::Arc<str>,
        shell::selection::Transfers<App>,
    )>,
    /// QWaylandDataDeviceManager and the seat's QWaylandDataDevice: the clipboard a paste reads.
    data_mgr: Option<DataDeviceManagerState>,
    data_dev: Option<DataDevice>,
    /// The paste reading its selection owner's pipe.
    paste: shell::selection::PasteRead<App, shell::keys::Paste>,
    /// The `paste()` of a script reading its selection owner's pipe; the calls after it wait
    /// in `World::text_pastes`.
    script_paste: shell::selection::PasteRead<App, shell::keys::Paste>,
    /// QWaylandDataDevice::selectionOffer's text, for a script's `paste()` to insert inside
    /// the call: read from the owner as soon as `wl_data_device.selection` announces the offer
    /// (`read_offer`). `None` until the owner has written it, and without keyboard focus,
    /// when the compositor sends no offer.
    offer_text: std::rc::Rc<std::cell::RefCell<Option<String>>>,
    offer_read: shell::selection::PasteRead<App, ()>,
    /// The reads of the process's clipboard source whose pipes are full, written on as their
    /// readers read (`qml::ports::clipsrc::Host`).
    clip_writes: std::collections::HashMap<u64, calloop::RegistrationToken>,
    loop_handle: calloop::LoopHandle<'static, App>,
    /// The Qt::CursorShape last set for the pointer's current enter (-1: none yet).
    cur_shape: i32,
    /// The cursor theme lacks an image, said once.
    cursor_missing: bool,
    compositor: CompositorState,
    xdg: XdgShell,
    layer_shell: LayerShell,
    qh: QueueHandle<App>,
    conn: Connection,
    display: wl_display::WlDisplay,
    grab_mgr: Option<HyprlandFocusGrabManagerV1>,
    idle_mgr: Option<ZwpIdleInhibitManagerV1>,
    /// wp_fractional_scale_manager_v1 and wp_viewporter (both or neither are used).
    frac_mgr: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
    /// wp_presentation, for the `presented win{k}` lines under ri::present.
    presentation: Option<WpPresentation>,
    /// zwlr_output_power_manager_v1: whether an output shows frames at all.
    output_power: Option<ZwlrOutputPowerManagerV1>,
    /// The zwlr_output_power_v1 of each output and whether it says the output is on (true
    /// until its first `mode`): what [`App::sync_unseen`] and the [`Probe`]s read. The one
    /// the shell asks for: wlroots answers `failed` to a second for the same output.
    output_powers:
        std::collections::HashMap<wayland_client::backend::ObjectId, (ZwlrOutputPowerV1, bool)>,
    /// What [`App::sync_unseen`] last found.
    sight: Sight,
    /// None without wl_subcompositor or wp_alpha_modifier_v1: crossfades blend on the GPU.
    veil_mgr: Option<VeilMgr>,
    /// zwp_text_input_v3 (QWaylandTextInputv3) for fcitx5/IBus.
    ti_mgr: Option<ZwpTextInputManagerV3>,
    ti: Option<ZwpTextInputV3>,
    /// Surface the text input entered, whether it is enabled, the state last committed (and the
    /// revision/editor it was computed for) and the events waiting for `done`.
    ti_surface: Option<wl_surface::WlSurface>,
    ti_enabled: bool,
    ti_sent: Option<shell::keys::ImeState>,
    ti_key: Option<(u64, Option<usize>)>,
    ti_pending: TextInputPending,
    /// zwp_text_input_v3 state must be reset (`enable` again) for `Qt.inputMethod.reset()`.
    ti_reset: bool,
    /// What `Qt.inputMethod`'s geometry was last computed for (revision, editor).
    im_key: Option<(u64, Option<usize>)>,
    /// Live zwp_idle_inhibitor_v1 per enabled `IdleInhibitor` item, with its surface.
    inhibitors: std::collections::HashMap<usize, (ZwpIdleInhibitorV1, wl_surface::WlSurface)>,
    /// After a reload, the previous image's Wayland socket and when this image began to
    /// draw (set at the first [`App::release_predecessor`], so loading does not count): its
    /// windows stay on the screen until this image has drawn its own.
    predecessor: Option<(std::os::fd::OwnedFd, Option<std::time::Instant>)>,
    /// The panels this image starts with, made while the previous image's windows are on the
    /// screen, each with the wp_alpha_modifier that keeps it unseen: they draw and load under
    /// the old windows' places and are shown together when those leave
    /// ([`App::release_predecessor`]), so the screen changes once. Empty without
    /// wp_alpha_modifier_v1: they show as they draw.
    cloaks: Vec<(wl_surface::WlSurface, WpAlphaModifierSurfaceV1)>,
    /// The namespaces of the panels this image had at its first frames: what the next
    /// image of a reload starts with too (`shell::reload::leaves_before_exec`).
    start_panels: Option<std::collections::HashSet<String>>,
    shortcut_mgr: Option<HyprlandGlobalShortcutsManagerV1>,
    shortcuts_rev: Option<u64>,
    /// The live GlobalShortcut items and their latest property change at the last sync.
    shortcuts_seen: (Vec<usize>, u64),
    /// Quickshell `ScreencopyView` over hyprland_toplevel_export_v1, per view item.
    export_mgr: Option<HyprlandToplevelExportManagerV1>,
    /// Screencopy into GPU buffers (like Qt): the frame never touches the CPU.
    dmabuf: Option<ZwpLinuxDmabufV1>,
    /// linux-dmabuf v4+ default feedback: device and format modifiers for capture buffers.
    dma_feedback: Option<ZwpLinuxDmabufFeedbackV1>,
    dma_params: shell::dmabuf::Feedback,
    dma_params_next: shell::dmabuf::FeedbackBuilder,
    /// Bumped by new dmabuf parameters: slots made before are remade at their next frame.
    dma_epoch: u64,
    gbm: Option<std::rc::Rc<shell::gbm::Gbm>>,
    dma_off: bool,
    dma_ok: bool,
    /// Per toplevel: its dmabuf frame target, kept for reopening like `cap_cache`.
    dma_keep: rustc_hash::FxHashMap<u32, DmaSlot>,
    /// Replaced slots whose texture is still shown by some frame.
    dma_grave: Vec<DmaSlot>,
    /// dmabuf frames copied since the last `apply_screencopies`: item, toplevel, size, y-invert,
    /// the view's size.
    dma_ready: Vec<DmaReady>,
    /// Ready frames whose GPU compares were submitted (`Gpu::compare_submit`), in queue order:
    /// read back once the GPU is done with them, at a later pass.
    dma_wait: Vec<DmaReady>,
    /// A helper thread pinged the loop since the last service poll.
    pinged: bool,
    /// `unlock_and_destroy` requests the compositor has not answered a wl_display.sync for.
    unlocks_in_flight: u32,
    /// Screencopy conversion thread, started with the first captured frame. It reads the
    /// buffers of `caps` and `cap_grave` in place: `App::drop` ends it before they are freed.
    sc: Option<ScWorker>,
    /// Head of the deadline queue built at the end of the last turn (loop ms), and the earliest
    /// deadline that needs the services polled (Timers, owed polls). A wakeup by Wayland traffic
    /// alone (e.g. screencopy frames) before them runs no full turn.
    due_at: f64,
    poll_due: f64,
    /// Revision a follow-up poll was last scheduled for (once per revision).
    owed_rev: u64,
    /// The last full turn left animations, frame-driven items or `Qt.callLater` work running.
    animating: bool,
    /// World revision at the last service poll: JS writes (e.g. `Process.running`) poll now.
    poll_rev: u64,
    /// Number of items at the last [`App::panels_made`].
    panels_made: usize,
    /// Captures dropped while the worker still reads their buffer.
    cap_grave: Vec<Capture>,
    frames_unanswered: Unanswered<HyprlandToplevelExportFrameV1>,
    caps: rustc_hash::FxHashMap<usize, Capture>,
    /// `ScreencopyView`s of a screen and their one-shot capture.
    screen_caps: rustc_hash::FxHashMap<usize, ScreenCap>,
    /// `paintCursor` of each of them when its capture started.
    screen_cap_cursor: rustc_hash::FxHashMap<usize, bool>,
    /// Last frame per toplevel handle (downscaled once its view is gone), shown by a new view
    /// of the same window until its own capture arrives: overview reopens with previews.
    cap_cache: rustc_hash::FxHashMap<u32, (u64, std::rc::Rc<qsg::raster::Canvas>)>,
    cap_seq: u64,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    kbd: shell::keys::Keyboard,
    /// Focused editor and its caret blink phase as last painted.
    caret: Option<(usize, bool)>,
    /// Keyboard interactivity last set per layer surface (by wl_surface protocol id).
    kb_modes: std::collections::HashMap<u32, u32>,
    /// The seat keyboard's xkb keymap, state and compose state (the only copy).
    xkb: shell::keys::Xkb,
    /// Client-side key repeat (the compositor only sends repeat_info) and the serial of the
    /// press it repeats.
    key_repeat: shell::keys::KeyRepeat,
    /// Keysym each held key was delivered with on press (by raw code): its release reports the
    /// same key even when Ctrl, and so the shortcut keysym, changed in between.
    held_syms: std::collections::HashMap<u32, u32>,
    key_serial: u32,
    /// One hyprland_global_shortcut_v1 per (appid, name); every live `GlobalShortcut` item with
    /// that key receives its events (the compositor rejects duplicate registrations).
    shortcuts: std::collections::HashMap<(String, String), (HyprlandGlobalShortcutV1, Vec<usize>)>,
    /// Live hyprland_focus_grab_v1 per `HyprlandFocusGrab` item, with the surfaces it holds.
    grabs: std::collections::HashMap<usize, (HyprlandFocusGrabV1, Vec<wl_surface::WlSurface>)>,
    /// Grabs the compositor cleared (click outside), handled before the pointer frames read
    /// with them.
    grab_cleared: Vec<usize>,
    /// A cleared grab was handled since the last sync pass.
    grab_flushed: bool,
    /// Toplevels of deleted windows, kept until this turn's focus grabs are committed.
    late_close: Vec<usize>,
    lock_state: SessionLockState,
    session: Option<Session>,
    /// The lock that failed or was refused: not retried until its `locked` turns false.
    lock_denied: Option<usize>,
    /// Counts `lock` requests: the [`LockSync`] of a lock that is gone names an older one.
    lock_serial: u64,
    /// wl_keyboard focus as Qt's window activation sees it, keyed by the surface wl_keyboard
    /// names; its active surface holds the keyboard (the delivery target may differ, see
    /// `retarget_kbd`).
    kbd_act: shell::keys::Activation<wl_surface::WlSurface>,
}

impl App {
    /// Everything the sync passes (`sync_popups` .. `sync_screencopy`) read besides their own
    /// state: world writes (`revision`; layout and native writes included), the surfaces and
    /// which of them are configured, the session lock, and the outputs.
    fn sync_key(&self) -> SyncKey {
        (
            self.rt.revision(),
            self.wins.len(),
            self.wins.iter().filter(|w| w.configured).count(),
            self.session.as_ref().map(|s| s.locked),
            self.outputs_gen,
        )
    }

    /// Repaint only the windows that contain an item whose visible state changed.
    fn mark_dirty(&mut self) {
        let reaped = self.rt.reap();
        let freed = self.rt.world.borrow_mut().items.take_freed();
        if !freed.is_empty() {
            shell::scene::forget_chunks(&self.rt.world.borrow(), &freed);
        }
        if !reaped.is_empty() {
            shell::scene::forget_items(&reaped);
            shell::scene::forget_ratios(&self.rt.world.borrow(), &reaped);
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.forget_items(&reaped);
            }
            // Destroyed instances dropped references old JS objects held.
            qml::eval::request_sweep();
        }
        // Table purges and malloc_trim block: never while something animates.
        let animating = self.wins.iter().any(|w| w.anim);
        // Freed arenas after large teardowns or heavy JS allocation; throttled.
        thread_local!(static GC: std::cell::Cell<(usize, isize, Option<Instant>)> = const { std::cell::Cell::new((0, 0, None)) });
        GC.with(|g| {
            let (n, base, last) = g.get();
            let n = n + reaped.len();
            let objs = qml::value::LIVE[1].load(std::sync::atomic::Ordering::Relaxed);
            if shell::sched::purge_due(animating, n, objs - base, last.map(|l| l.elapsed())) {
                let t = Instant::now();
                {
                    let mut w = self.rt.world.borrow_mut();
                    w.purge_dead();
                    self.roots.retain(|r| !w.dead.contains(r));
                    self.clean.retain(|r, _| !w.dead.contains(r));
                }
                qml::eval::request_sweep();
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.trim(60);
                }
                if qml::log_on!("ri::wstat") {
                    let m = shell::heap::stats();
                    let mb = |f: fn(&shell::heap::Stats) -> usize| m.as_ref().map_or(0, f) >> 20;
                    tracing::debug!(
                        target: "ri::wstat",
                        "GC live={objs} in {:?} heap: allocated={}M active={}M resident={}M retained={}M",
                        t.elapsed(),
                        mb(|m| m.allocated),
                        mb(|m| m.active),
                        mb(|m| m.resident),
                        mb(|m| m.retained)
                    );
                    #[cfg(feature = "interp")]
                    let (ln, lb) = qml::ir::lowered_stats();
                    #[cfg(not(feature = "interp"))]
                    let (ln, lb) = (0, 0);
                    tracing::debug!(
                        target: "ri::wstat",
                        "CSTAT {} {} gpu={:?} lowered={ln}/{lb}B re={}",
                        self.fonts.borrow().stats(),
                        shell::scene::cache_stats(),
                        self.gpu.as_ref().map(|g| g.stats()),
                        qml::builtins::re_cache_len()
                    );
                }
                let objs = qml::value::LIVE[1].load(std::sync::atomic::Ordering::Relaxed);
                g.set((0, objs, Some(Instant::now())));
            } else {
                g.set((n, base, last));
            }
        });
        // JS reference cycles (Qt collects them with its GC): one bounded increment per turn.
        let t = qml::log_on!("ri::wstat").then(Instant::now);
        let gc = qml::eval::collect_step(qml::eval::GC_SLICE);
        if let Some(t) = t
            && (gc.freed > 0 || gc.swept)
        {
            tracing::debug!(
                target: "ri::wstat",
                "GC step freed={} examined={} swept={} in {:?}",
                gc.freed,
                gc.examined,
                gc.swept,
                t.elapsed()
            );
        }
        let restructured = std::mem::take(&mut self.rt.world.borrow_mut().restructured);
        if !restructured.is_empty() {
            shell::scene::invalidate_parents(&self.rt.world.borrow(), &restructured);
        }
        let moved = std::mem::take(&mut self.rt.world.borrow_mut().moved);
        if !moved.is_empty() {
            shell::scene::invalidate_moved(&self.rt.world.borrow(), &moved);
        }
        let touched = std::mem::take(&mut self.rt.world.borrow_mut().touched);
        let repainted = std::mem::take(&mut self.rt.world.borrow_mut().repainted);
        if touched.is_empty() && moved.is_empty() {
            return;
        }
        if !touched.is_empty() {
            shell::scene::invalidate_layers(&self.rt.world.borrow(), &touched);
        }
        let mut only = shell::scene::only_repainted(&touched, &moved, &repainted);
        let rest = touched.into_iter().filter(|t| !only.contains(t));
        self.dirty_by(rest.chain(moved).collect());
        // One the window's last frame draws is swapped there for its new picture, and the
        // window is not painted again; the others change it like the rest.
        only.retain(|&c| !self.swap_repainted(c));
        self.dirty_by(only);
    }

    /// The window of each of `changed` that can alter its pixels (`shown_changes`) is drawn
    /// again.
    fn dirty_by(&mut self, changed: Vec<usize>) {
        if changed.is_empty() {
            return;
        }
        let roots: std::collections::HashMap<usize, usize> = self
            .wins
            .iter()
            .enumerate()
            .map(|(k, w)| (w.item, k))
            .collect();
        let shown = self.shown_changes(changed);
        let w = self.rt.world.borrow();
        let mut seen = std::collections::HashSet::new();
        for t in shown {
            let mut cur = Some(t);
            while let Some(i) = cur {
                if !seen.insert(i) {
                    break;
                }
                if let Some(&k) = roots.get(&i) {
                    if !self.wins[k].dirty && qml::log_on!("ri::dirty") {
                        let ns = w.items[i]
                            .props
                            .get("WlrLayershell.namespace")
                            .map(|p| p.value.to_js_string());
                        tracing::debug!(
                            target: "ri::dirty",
                            "dirty win{k} {ns:?} by {t} {:?} parent {:?}",
                            w.items[t].types.first(),
                            w.items[t].parent.map(|p| w.items[p].types.first().cloned())
                        );
                    }
                    self.wins[k].dirty = true;
                    break;
                }
                cur = w.items.get(i).and_then(|it| it.parent);
            }
        }
    }

    /// Canvas `c` painted something new: whether its window, drawn and with nothing else to
    /// draw, shows it by its last frame with the new picture in place of the old one
    /// (`scene::swap_repainted`), as a window does a new frame of a `ScreencopyView`.
    fn swap_repainted(&mut self, c: usize) -> bool {
        let mut w = self.rt.world.borrow_mut();
        let mut cur = Some(c);
        let k = loop {
            let Some(i) = cur else { return false };
            if let Some(k) = self.wins.iter().position(|win| win.item == i) {
                break k;
            }
            cur = w.items.get(i).and_then(|it| it.parent);
        };
        let win = &mut self.wins[k];
        if win.dirty || win.anim || !win.veils.is_empty() || win.gl.is_none() {
            return false;
        }
        let Some(fr) = win.last.as_mut() else {
            return false;
        };
        if !shell::scene::swap_repainted(&mut w, c, fr) {
            return false;
        }
        win.swapped = true;
        true
    }

    /// Changes that can alter a window's pixels. Qt's `QQuickItemPrivate::dirty` schedules a
    /// frame only for an item that is effectively visible, a texture provider or an effect's
    /// source itself: a change under a hidden ancestor shows with the window's next frame.
    /// A paint-only change under a hidden ancestor that no effect samples (the bar's hidden
    /// tray overflow arrow rotating on `bar.bottom`) and any change inside a hidden sampled
    /// subtree below its root (the bar's occupied-workspace pills inside the hidden mask
    /// layer) schedule none. Other geometry writes under hidden ancestors still draw. A
    /// delegate its view culls counts as hidden: Qt does schedule a frame for a change inside
    /// one, and that frame draws the same pixels (a press ripple fading in a search row
    /// scrolled out of the list).
    fn shown_changes(&mut self, changed: Vec<usize>) -> Vec<usize> {
        let mut w = self.rt.world.borrow_mut();
        let mut sampled = None;
        let mut hidden: Vec<(usize, usize)> = Vec::new();
        let mut shown = Vec::with_capacity(changed.len());
        for t in changed {
            if let Some(a) = shell::scene::hidden_ancestor(&mut w, t) {
                hidden.push((t, a));
            } else {
                shown.push(t);
            }
        }
        for (t, a) in hidden {
            let s = sampled.get_or_insert_with(|| shell::scene::sampled_items(&mut w));
            if s.contains(&t) || shell::scene::texture_provider(&mut w, t) {
                shown.push(t);
                continue;
            }
            let mut cur = w.items.get(t).and_then(|it| it.parent);
            let mut in_sampled = false;
            while let Some(i) = cur {
                if s.contains(&i) {
                    in_sampled = true;
                    break;
                }
                if i == a {
                    break;
                }
                cur = w.items.get(i).and_then(|it| it.parent);
            }
            if !in_sampled && w.lrev_items.contains(&t) {
                shown.push(t);
            }
        }
        shown
    }

    /// Drop pending repaints inside window `root` (about to be painted); keep other windows'.
    fn forget_touched(&mut self, root: usize) {
        let mut w = self.rt.world.borrow_mut();
        let touched = std::mem::take(&mut w.touched);
        let mut mine = Vec::new();
        let keep: Vec<usize> = touched
            .into_iter()
            .filter(|&t| {
                let mut cur = Some(t);
                while let Some(i) = cur {
                    if i == root {
                        mine.push(t);
                        return false;
                    }
                    cur = w.items.get(i).and_then(|it| it.parent);
                }
                true
            })
            .collect();
        w.touched = keep;
        w.repainted.retain(|c| !mine.contains(c));
        // Layout writes made during this draw: recorded subtree draws they move go stale.
        shell::scene::invalidate_layers(&w, &mine);
        let in_root = |w: &qml::runtime::World, t: usize| {
            let mut cur = Some(t);
            while let Some(i) = cur {
                if i == root {
                    return true;
                }
                cur = w.items.get(i).and_then(|it| it.parent);
            }
            false
        };
        let moved = std::mem::take(&mut w.moved);
        let (mine, keep): (Vec<usize>, Vec<usize>) =
            moved.into_iter().partition(|&t| in_root(&w, t));
        w.moved = keep;
        shell::scene::invalidate_moved(&w, &mine);
    }

    /// Takes the queued resizes of window `root`; resizes inside other windows stay queued for
    /// their own draw. Items in no window (unmapped popups, destroyed items) aren't drawn, and a
    /// window created for them later lays out from scratch.
    /// Map the items that bumped `layout_rev` to the clean windows enclosing them (except
    /// `own`, whose layout pass made them) and drop those from `clean`.
    fn drain_lrev(&mut self, own: Option<usize>) {
        let mut wd = self.rt.world.borrow_mut();
        let items = std::mem::take(&mut wd.lrev_items);
        if self.clean.is_empty() {
            return;
        }
        for i in items {
            let (mut r, mut top) = (Some(i), i);
            let hit = loop {
                let Some(x) = r else { break None };
                if self.clean.contains_key(&x) {
                    break Some(x);
                }
                top = x;
                r = wd.items.get(x).and_then(|it| it.parent);
            };
            match hit {
                Some(x) if Some(x) != own => {
                    if let Some(sz) = self.clean.remove(&x)
                        && !self.wins.iter().any(|w| w.item == x)
                    {
                        if qml::log_on!("ri::stats") {
                            let p = wd.items[i].parent.unwrap_or(0);
                            tracing::debug!(
                                target: "ri::stats",
                                "stale hidden {x} by lrev {i}:{:?}<{:?}",
                                wd.items[i].types.first(),
                                wd.items[p].types.first()
                            );
                        }
                        let t = self.start.elapsed().as_secs_f64() * 1000.0;
                        self.stale_hidden.entry(x).or_insert((sz, t)).0 = sz;
                    }
                }
                Some(_) => {}
                // Inside a known window that is not clean anyway: nothing to drop.
                None if self.roots.contains(&top) => {}
                // Not under any window root seen so far (detached, reparented): be safe.
                None => {
                    if qml::log_on!("ri::stats") {
                        tracing::debug!(
                            target: "ri::stats",
                            "lrev unrooted {i}:{:?} top {top}",
                            wd.items[i].types.first()
                        );
                    }
                    // An item outside every window is laid out nowhere: it reaches a window
                    // through a reparent (its own lrev) or through bindings writing into one.
                    if !qml::debug::on(qml::debug::Switch::UNROOTED_DROP) {
                        continue;
                    }
                    // Hidden windows dropped here are laid out again in idle, as above.
                    let t = self.start.elapsed().as_secs_f64() * 1000.0;
                    for (x, sz) in std::mem::take(&mut self.clean) {
                        if !self.wins.iter().any(|w| w.item == x) {
                            self.stale_hidden.entry(x).or_insert((sz, t)).0 = sz;
                        }
                    }
                }
            }
            if self.clean.is_empty() {
                break;
            }
        }
    }

    fn take_resized(&mut self, root: usize) -> Vec<usize> {
        let mut wd = self.rt.world.borrow_mut();
        let mut all = std::mem::take(&mut wd.resized);
        if !self.parked.is_empty() {
            self.parked.retain(|h, _| !wd.dead.contains(h));
            if let Some((p, _)) = self.parked.remove(&root) {
                all.splice(0..0, p);
            }
        }
        let (mut mine, mut other) = (Vec::new(), Vec::new());
        let wins: rustc_hash::FxHashSet<usize> = self.wins.iter().map(|w| w.item).collect();
        for i in all {
            if wd.dead.contains(&i) {
                continue;
            }
            // The nearest enclosing window (windows can sit inside Scopes/Variants/other windows).
            let (mut r, mut hidden) = (Some(i), None);
            while let Some(x) = r
                && x != root
                && !wins.contains(&x)
            {
                if hidden.is_none() && self.clean.contains_key(&x) {
                    hidden = Some(x);
                }
                r = wd.items.get(x).and_then(|it| it.parent);
            }
            match r {
                Some(x) if x == root => mine.push(i),
                Some(_) => other.push(i),
                // Inside a hidden prelaid window: settled locally when it is shown.
                // A non-local one needs a full pass at show anyway.
                None => {
                    if let Some(h) = hidden {
                        if scene::is_local_resize(&mut wd, i) {
                            let (v, set) = self.parked.entry(h).or_default();
                            if set.insert(i) {
                                v.push(i);
                            }
                        } else if i == h {
                            // The hidden window's own size: showing it at another size than
                            // `clean` records lays it out then anyway.
                        } else if let Some(sz) = self.clean.remove(&h) {
                            if qml::log_on!("ri::stats") {
                                let p = wd.items[i].parent.unwrap_or(0);
                                tracing::debug!(
                                    target: "ri::stats",
                                    "stale hidden {h} by resize {i}:{:?}<{:?}",
                                    wd.items[i].types.first(),
                                    wd.items[p].types.first()
                                );
                            }
                            let t = self.start.elapsed().as_secs_f64() * 1000.0;
                            self.stale_hidden.entry(h).or_insert((sz, t)).0 = sz;
                        }
                    }
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        other.retain(|i| seen.insert(*i));
        wd.resized = other;
        mine
    }

    /// Quickshell `WlSessionLock`: lock when `locked` turns true, one `surface` instance per
    /// screen, shown on ext_session_lock_v1 surfaces once the compositor confirms; unlock only
    /// when the holder sets `locked` to false and no other `WlSessionLock` wants the lock.
    /// A holder destroyed while locked (a `PanelLoader` swapping families) leaves the session
    /// locked, as in Quickshell; the next `WlSessionLock` with `locked: true` takes the held
    /// lock over with its own surfaces, so the desktop is never shown in between.
    /// Quickshell keeps the lock object across its reload; ours re-executes the shell, which
    /// would close the connection that holds the lock, so it waits ([`App::lock_holds_reload`]).
    fn sync_session_lock(&mut self) {
        let (held, target) = self.lock_wanted();
        match (&self.session, held, target) {
            (None, _, Some(item)) => self.start_lock(item),
            (Some(_), Some((_, true)), _) => {}
            (Some(_), Some((_, false)), Some(item)) => self.adopt_lock(item),
            (Some(_), Some((true, false)), None) => self.end_lock(true),
            (Some(s), Some((false, false)), None) if !s.surfaces.is_empty() => {
                self.drop_lock_surfaces();
            }
            _ => {}
        }
    }

    /// Of the confirmed lock: whether its holder is alive, and whether it still wants the lock.
    /// And the `WlSessionLock` that wants the lock now.
    fn lock_wanted(&mut self) -> (Option<(bool, bool)>, Option<usize>) {
        let (target, held) = {
            let mut w = self.rt.world.borrow_mut();
            let target = w
                .of_type("WlSessionLock")
                .into_iter()
                .find(|&i| !w.dead.contains(&i) && w.read(i, "locked").is_some_and(|v| v.truthy()));
            let held = self.session.as_ref().filter(|s| s.locked).map(|s| {
                let alive = !w.dead.contains(&s.item);
                (
                    alive,
                    alive && w.read(s.item, "locked").is_some_and(|v| v.truthy()),
                )
            });
            (target, held)
        };
        if target.is_none() {
            self.lock_denied = None;
        }
        (held, target.filter(|&t| self.lock_denied != Some(t)))
    }

    /// The shell is ending. `locked = false` followed by `Qt.quit()` in one handler
    /// (quickshell-examples' lockscreen) unlocks in Quickshell, whose setter sends the request;
    /// ours is sent by [`App::sync_session_lock`] in the loop turn the exit does not get to.
    fn unlock_before_exit(&mut self) {
        if let (Some((true, false)), None) = self.lock_wanted() {
            self.end_lock(true);
        }
    }

    /// The `surface` Component of a `WlSessionLock`.
    fn lock_template(&self, item: usize) -> Option<usize> {
        let mut w = self.rt.world.borrow_mut();
        match w.read(item, "surface") {
            Some(Value::Item(c)) => match w.read(c, "\x01template") {
                Some(Value::Num(t)) => Some(t as usize),
                _ => None,
            },
            _ => None,
        }
    }

    fn start_lock(&mut self, item: usize) {
        let Some(tmpl) = self.lock_template(item) else {
            elog::eprintln!("WlSessionLock.surface is null. Aborting lock.");
            self.lock_denied = Some(item);
            return;
        };
        let lock = match self.lock_state.lock(&self.qh) {
            Ok(l) => l,
            Err(_) => {
                elog::eprintln!(
                    "Cannot start session lock: The current compositor does not support the \
                     ext-session-lock-v1 protocol."
                );
                self.lock_denied = Some(item);
                return;
            }
        };
        self.session = Some(Session {
            item,
            lock,
            locked: false,
            answered: false,
            tmpl,
            surfaces: Vec::new(),
        });
        self.cancel_off_screen("the session is locked");
        // A compositor refuses a lock (`finished`) while handling the request, so its refusal
        // comes before the answer to this sync. The lock surfaces wait for that answer: a
        // refused lock then has none to destroy (docs/quickshell-differences.md).
        self.lock_serial += 1;
        self.display.sync(&self.qh, LockSync(self.lock_serial));
    }

    /// The compositor read the `lock` request numbered `serial` and did not refuse it: every
    /// output gets its lock surface (Hyprland sends `locked` once each has one to show).
    fn lock_answered(&mut self, serial: u64) {
        match self.session.as_mut() {
            Some(s) if serial == self.lock_serial && !s.answered => s.answered = true,
            _ => return,
        }
        let outs: Vec<wl_output::WlOutput> = self.outputs.outputs().collect();
        for out in outs {
            self.lock_add_output(&out);
        }
    }

    /// The lock surfaces of a session lock the compositor has not confirmed draw before every
    /// other window: until each of them has committed its first frame, no other window draws,
    /// whatever asks it to ([`App::holds_for_lock_surfaces`]).
    /// They are asked for at the answer to the `lock` request ([`LockSync`]), and for an output
    /// connected later when it appears, and draw once configured; a frame of another window
    /// drawn before that (a blurred wallpaper takes long without a GPU) puts the lock screen
    /// that much later. The wait is not for `locked`: a compositor may send it late or never,
    /// and shows the other windows until then. A first frame is followed by an event
    /// ([`MapSync`]) that wakes the loop, so the windows left changed draw then.
    ///
    /// The other windows wait for the lock screen's keyboard as well
    /// ([`App::lock_keyboard_on_its_way`]), whether `locked` has come or not.
    fn lock_surfaces_draw_first(&self) -> bool {
        self.session.as_ref().is_some_and(|s| {
            !s.locked
                && (!s.answered
                    || self
                        .wins
                        .iter()
                        .any(|w| matches!(w.surf, Surf::Lock(_)) && w.mapped.is_none()))
        }) || self.lock_keyboard_on_its_way()
            || self.lock_check_on_its_way().is_some()
    }

    /// A password check just started and its helper has not asked for the password yet (see
    /// [`qml::Runtime::pam_helper_due`]): the frames of the windows that are not lock surfaces
    /// wait for the turn that answers it, at most this long. A frame drawn in between (a
    /// blurred wallpaper takes 0.8 s without a GPU) starts the check that much later.
    fn lock_check_on_its_way(&self) -> Option<Duration> {
        self.rt.pam_helper_due()
    }

    /// A lock surface is about to take the keyboard: the compositor gives it the keyboard
    /// focus when its first frame maps it (`wl_keyboard.enter` comes before the answer to
    /// [`MapSync`]), and the surface gets keys from the answer to the sync sent at that enter
    /// on ([`KbdSync`]); a key handled before that answer reaches no item, as in Qt. The loop
    /// reads no event while it draws, so a frame of another window drawn in between (a blurred
    /// wallpaper took 1.1 to 1.7 s without a GPU) keeps the lock screen without the keyboard
    /// that long: a password typed meanwhile is lost, and its Return, handled after the
    /// answer, sends an empty one. Both answers are two round trips; a frame drawn after them
    /// only delays the keys.
    fn lock_keyboard_on_its_way(&self) -> bool {
        let activating = self.kbd_act.activating();
        self.wins.iter().any(|w| {
            matches!(w.surf, Surf::Lock(_))
                && (w.mapped == Some(false) || activating == Some(w.surf.wl_surface()))
        })
    }

    /// Whether window `k` may not draw yet because the lock surfaces draw first. `draw_frame`
    /// asks for every path to a frame: a frame callback or a configure that comes in one batch
    /// with the answer to the `lock` request is handled before that answer, when no lock
    /// surface is asked for yet, and would put the window's frame ahead of all of them.
    fn holds_for_lock_surfaces(&self, k: usize) -> bool {
        !matches!(self.wins[k].surf, Surf::Lock(_)) && self.lock_surfaces_draw_first()
    }

    /// `item` takes over the held session lock: the previous holder's surfaces go, `item`'s
    /// `surface` is shown on every output in the same ext_session_lock_v1.
    fn adopt_lock(&mut self, item: usize) {
        let tmpl = self.lock_template(item);
        let old = self.drop_lock_surfaces();
        {
            let mut w = self.rt.world.borrow_mut();
            if let Some(old) = old.filter(|o| !w.dead.contains(o)) {
                w.write(old, "secure", Value::Bool(false));
            }
        }
        let Some(tmpl) = tmpl else {
            elog::eprintln!("WlSessionLock.surface is null. Aborting lock.");
            self.lock_denied = Some(item);
            return;
        };
        let Some(s) = self.session.as_mut() else {
            return;
        };
        s.item = item;
        s.tmpl = tmpl;
        self.rt
            .world
            .borrow_mut()
            .write(item, "secure", Value::Bool(true));
        let outs: Vec<wl_output::WlOutput> = self.outputs.outputs().collect();
        for out in outs {
            self.lock_add_output(&out);
        }
    }

    /// Drops the held lock's surfaces (the session stays locked); returns the holder.
    fn drop_lock_surfaces(&mut self) -> Option<usize> {
        let s = self.session.as_mut()?;
        let items: Vec<usize> = s.surfaces.drain(..).map(|x| x.0).collect();
        let holder = s.item;
        self.wins.retain(|w| !items.contains(&w.item));
        self.leave_gone_focus();
        let mut w = self.rt.world.borrow_mut();
        for i in items {
            if !w.dead.contains(&i) {
                w.destroy(i);
            }
        }
        Some(holder)
    }

    /// The `Rustshell.screens` entry for `out`, matched by output name.
    fn screen_of(&self, out: &wl_output::WlOutput) -> Option<Value> {
        let name = self.outputs.info(out)?.name?;
        let mut w = self.rt.world.borrow_mut();
        let qs = w.singletons.get("Rustshell").copied()?;
        let screens = match w.read(qs, "screens") {
            Some(Value::Obj(o)) => o.borrow().array.clone().unwrap_or_default(),
            _ => return None,
        };
        screens.into_iter().find(|sc| match sc {
            Value::Item(i) => matches!(w.read(*i, "name"), Some(Value::Str(n)) if *n == *name),
            _ => false,
        })
    }

    /// Quickshell `WlSessionLock`: every screen gets its own `surface`, also one connected
    /// while the session is locked (ext-session-lock-v1 asks for a surface on new outputs;
    /// without one the new monitor shows nothing or the compositor's fallback).
    fn lock_add_output(&mut self, out: &wl_output::WlOutput) {
        let Some(s) = &self.session else {
            return;
        };
        if !s.answered
            || s.surfaces.iter().any(|(_, o)| o == out)
            || self.rt.world.borrow().dead.contains(&s.item)
        {
            return;
        }
        let (item, tmpl) = (s.item, s.tmpl);
        let surf = match self.rt.instantiate(tmpl, Some(item), &Value::Undefined) {
            Ok(i) => i,
            Err(e) => {
                elog::eprintln!("WlSessionLock.surface: {e}");
                return;
            }
        };
        if let Some(sc) = self.screen_of(out) {
            self.rt.world.borrow_mut().write(surf, "screen", sc);
        }
        self.rt.settle();
        let surface = self.compositor.create_surface(&self.qh);
        let Some(s) = &mut self.session else {
            return;
        };
        let ls = s.lock.create_lock_surface(surface, out, &self.qh);
        s.surfaces.push((surf, out.clone()));
        self.wins.push(Win {
            gl: None,
            item: surf,
            surf: Surf::Lock(ls),
            size: (0, 0),
            configured: false,
            dirty: true,
            anim: false,
            pending: false,
            shown: None,
            committed: None,
            asked: None,
            laid: (u64::MAX, (0, 0)),
            unsettled: 0,
            region: None,
            last: None,
            shm_prev: None,
            veils: Vec::new(),
            spare: Vec::new(),
            swapped: false,
            mapped: None,
            scale: WinScale::default(),
        });
        self.ensure_scaler(self.wins.len() - 1);
    }

    /// The lock surface of a disconnected output goes with it.
    fn lock_remove_output(&mut self, out: &wl_output::WlOutput) {
        let Some(s) = &mut self.session else {
            return;
        };
        let gone: Vec<usize> = s
            .surfaces
            .iter()
            .filter(|(_, o)| o == out)
            .map(|x| x.0)
            .collect();
        s.surfaces.retain(|(_, o)| o != out);
        self.wins.retain(|w| !gone.contains(&w.item));
        self.leave_gone_focus();
        let mut w = self.rt.world.borrow_mut();
        for i in gone {
            if !w.dead.contains(&i) {
                w.destroy(i);
            }
        }
    }

    /// A reload now would leave the session locked with nothing to unlock it: the exec closes
    /// the Wayland connection, and a compositor keeps a session locked whose lock client is
    /// gone (ext-session-lock-v1: "the compositor must not unlock the session in response"),
    /// while the new image starts unlocked. So it is while a lock is asked for or held, and
    /// after `unlock_and_destroy` until the compositor has read it (the protocol's
    /// wl_display.sync before a client that unlocked goes away).
    fn lock_holds_reload(&self) -> bool {
        self.session.is_some() || self.unlocks_in_flight > 0
    }

    fn end_lock(&mut self, unlock: bool) {
        let Some(s) = self.session.take() else {
            return;
        };
        if unlock {
            s.lock.unlock();
            self.unlocks_in_flight += 1;
            self.display.sync(&self.qh, UnlockSync { held: s.locked });
        } else if s.locked {
            lock_marker(false);
        }
        let items: Vec<usize> = s.surfaces.iter().map(|x| x.0).collect();
        self.wins.retain(|w| !items.contains(&w.item));
        self.leave_gone_focus();
        let mut w = self.rt.world.borrow_mut();
        for i in items {
            if !w.dead.contains(&i) {
                w.destroy(i);
            }
        }
        w.write(s.item, "secure", Value::Bool(false));
        drop(w);
        self.sync_unseen();
    }

    /// Notes what of this shell can be seen: the compositor confirmed the session lock, which
    /// shows the lock surfaces only, or no output is on (an output counts as on until its
    /// zwlr_output_power_v1 says `off`; without the protocol every output does). The
    /// visualizer captures nothing in either state, and the FrameAnimations of the windows
    /// out of sight rest (`animating_windows`).
    fn sync_unseen(&mut self) {
        let locked = self.session.as_ref().is_some_and(|s| s.locked);
        let on = |o: wl_output::WlOutput| self.output_powers.get(&o.id()).is_none_or(|p| p.1);
        let sight = Sight {
            locked,
            dark: !self.outputs.outputs().any(on),
        };
        if sight != self.sight {
            self.sight = sight;
            // The clock is armed again for what came back into sight.
            self.pinged = true;
        }
        qml::cava::set_unseen(&mut self.rt.world.borrow_mut(), locked || sight.dark);
    }

    /// Nobody can see window `k`, and the compositor said so: no output is on, or the session
    /// is locked and `k` is no lock surface.
    fn out_of_sight(&self, k: usize) -> bool {
        self.sight.dark || (self.sight.locked && !matches!(self.wins[k].surf, Surf::Lock(_)))
    }

    /// Qt creates a window's wp_fractional_scale_v1 and wp_viewport with its surface.
    /// The factor of the screen `name` (None: the first, where Qt puts a window that
    /// names none); the screens are the outputs in the order they came.
    fn screen_factor(&self, name: Option<&str>) -> f64 {
        if self.factors.is_identity() {
            return 1.0;
        }
        let names: Vec<String> = (self.outputs.outputs())
            .filter_map(|o| self.outputs.info(&o)?.name)
            .collect();
        let index = match name {
            Some(n) => names.iter().position(|x| x == n),
            None => (!names.is_empty()).then_some(0),
        };
        match index {
            Some(i) => self.factors.screen(i, &names[i]),
            None => self.factors.screen(usize::MAX, name.unwrap_or_default()),
        }
    }

    /// The factor of window `k`'s screen: a panel's and a lock surface's output, a popup's
    /// parent's, the output a toplevel is on.
    fn win_factor(&self, k: usize) -> f64 {
        let w = &self.wins[k];
        let name = |o: &wl_output::WlOutput| self.outputs.info(o).and_then(|i| i.name);
        match &w.surf {
            Surf::Panel { key, on, .. } => {
                let on = on.as_ref().and_then(name).or_else(|| key.output.clone());
                self.screen_factor(on.as_deref())
            }
            Surf::Lock(_) => {
                let out = self.session.as_ref().and_then(|s| {
                    let (_, o) = s.surfaces.iter().find(|(i, _)| *i == w.item)?;
                    name(o)
                });
                self.screen_factor(out.as_deref())
            }
            Surf::Popup { key, .. } => (self.wins.iter())
                .find(|p| p.item == key.parent)
                .map_or(1.0, |p| p.scale.factor),
            Surf::Toplevel(_) => self.screen_factor(w.scale.on.as_deref()),
        }
    }

    fn ensure_scaler(&mut self, k: usize) {
        self.wins[k].scale.factor = self.win_factor(k);
        let (Some(fm), Some(vp)) = (&self.frac_mgr, &self.viewporter) else {
            return;
        };
        let w = &mut self.wins[k];
        if w.scale.obj.is_none() {
            let s = w.surf.wl_surface();
            w.scale.obj = Some((
                fm.get_fractional_scale(s, &self.qh, s.clone()),
                vp.get_viewport(s, &self.qh, ()),
            ));
        }
    }

    /// Device pixels per logical pixel window `k` renders at (QWindow::devicePixelRatio): the
    /// compositor's preferred fractional scale, shown through a viewport, or else its integer
    /// buffer scale; times the factor of the window's screen.
    fn win_scale(&self, k: usize) -> f64 {
        let s = &self.wins[k].scale;
        s.output() * s.factor
    }

    /// Surface state mapping the next buffer, drawn at `dpr`, onto the logical size: buffer
    /// scale 1 plus a viewport destination for a fractional scale, else an integer buffer scale.
    /// Double-buffered: applies with the buffer the frame commits.
    fn apply_scale(&mut self, k: usize, dpr: f64) {
        let w = &mut self.wins[k];
        let size = w.surface_size();
        // A screen's factor makes the buffer that many times the surface as well.
        let frac = w.scale.obj.is_some() && (w.scale.frac > 0 || w.scale.factor != 1.0);
        let (buf, dest) = if frac {
            (1, Some(size))
        } else {
            (((dpr / w.scale.factor).round() as i32).max(1), None)
        };
        let s = w.surf.wl_surface();
        if buf != w.scale.buf {
            s.set_buffer_scale(buf);
            w.scale.buf = buf;
        }
        if dest != w.scale.dest {
            if let Some((_, vp)) = &w.scale.obj {
                match dest {
                    Some((dw, dh)) => vp.set_destination(dw as i32, dh as i32),
                    None => vp.set_destination(-1, -1),
                }
            }
            w.scale.dest = dest;
        }
    }

    /// Quickshell makes a panel's QQuickWindow once the panel's component is complete, before
    /// the `Qt.callLater` calls of that turn: from then an Image of the panel asks its icons at
    /// the window's ratio, which is the wl_output.scale of the panel's screen (the first
    /// output when it names none) until the compositor gives the window a scale.
    fn panels_made(&mut self) {
        let n = self.rt.world.borrow().items.len();
        if std::mem::replace(&mut self.panels_made, n) == n {
            return;
        }
        for (item, key) in self.panel_keys() {
            let out = match &key.output {
                None => self.outputs.outputs().next(),
                Some(n) => (self.outputs.outputs())
                    .find(|o| self.outputs.info(o).and_then(|i| i.name).as_ref() == Some(n)),
            };
            if let Some(info) = out.and_then(|o| self.outputs.info(&o)) {
                scene::window_made(&mut self.rt.world.borrow_mut(), item, info.scale_factor);
            }
        }
    }

    /// The compositor gave window `k` a scale, new or not.
    fn scale_reported(&mut self, k: usize) {
        let (item, dpr) = (self.wins[k].item, self.win_scale(k));
        scene::window_scale(&mut self.rt.world.borrow_mut(), item, dpr);
        self.scale_changed(k);
    }

    /// The compositor changed window `k`'s scale: redraw at the new density and update
    /// `QsWindow.devicePixelRatio` (devicePixelRatioChanged).
    fn scale_changed(&mut self, k: usize) {
        let dpr = self.win_scale(k);
        let w = &mut self.wins[k];
        if w.scale.dpr == dpr {
            return;
        }
        if qml::log_on!("ri::frames") {
            tracing::debug!(target: "ri::frames", "win{k} scale {} -> {dpr}", w.scale.dpr);
        }
        w.scale.dpr = dpr;
        w.dirty = true;
        w.last = None;
        w.swapped = false;
        let item = w.item;
        self.rt.set_device_pixel_ratio(item, dpr);
    }

    /// Loop ms of `t`, an instant the loop recorded.
    fn loop_ms(&self, t: Instant) -> f64 {
        t.saturating_duration_since(self.start).as_secs_f64() * 1000.0
    }

    /// Every deadline the loop has pending after a turn at `now` (`busy`: the turn changed
    /// something, so idle-only work waits).
    fn deadlines(&mut self, now: f64, busy: bool) -> Deadlines {
        let mut q = Deadlines::default();
        let t = self.clock.now(now);
        let timers = self.rt.next_deadline(t);
        // Writes after this turn's service poll (timer and animation handlers, layout) reach
        // the services that mirror them (FileView paths, PowerProfiles.profile) at a follow-up
        // poll; an animating loop polls at its next frame anyway.
        let rev = self.rt.revision();
        if rev != self.poll_rev && rev != self.owed_rev && !self.animating {
            self.owed_rev = rev;
            q.push(now, Due::Poll);
        }
        // Pinged during this turn after its poll (a frame drawn that held output waited for).
        if self.pinged {
            q.push(now, Due::Poll);
        }
        let awaiting = self.awaiting_frame();
        let give_up = self.callback_timeout();
        self.clock.one_window = self.one_window();
        let frame = self.clock.arm(
            now,
            self.animating || self.ptr.swipe_animating(),
            awaiting,
            give_up,
            animations_running(self),
        );
        if let Some((at, why)) = frame {
            q.push(at, why);
        }
        // A Timer runs on the animation clock, which while the driver runs stands still
        // between two steps (on the wall clock too): one due later than the clock shows
        // cannot fire before the next step, however soon it is due.
        let stepping = self.clock.running;
        q.push(
            match frame {
                Some((at, Due::Frame)) if stepping && timers > 0.0 => (now + timers).max(at),
                _ => now + timers,
            },
            Due::Timers,
        );
        // A changed window whose last frame went without a callback draws a refresh interval
        // after that frame.
        if let Some(t) = self
            .wins
            .iter()
            .filter(|w| w.dirty || w.swapped)
            .filter_map(|w| w.shown)
            .map(|t| self.loop_ms(t) + self.clock.vsync)
            .filter(|&t| t > now)
            .reduce(f64::min)
        {
            q.push(t, Due::Throttle);
        }
        if let Some(d) = self.rt.reap_in_ms() {
            q.push(now + d, Due::Reap);
        }
        // Frames held for a password check's helper draw when it is no longer waited for.
        if let Some(d) = self.lock_check_on_its_way()
            && self.wins.iter().any(|w| w.dirty || w.swapped)
        {
            q.push(now + d.as_secs_f64() * 1000.0 + 1.0, Due::Poll);
        }
        if let Some(d) = self.closing.iter().filter_map(|c| c.close.due).min() {
            q.push(self.loop_ms(d), Due::OffScreen);
        }
        // Layers nothing draws any more (a closed panel's) are freed by the clock, in the first
        // turn after their minute: an idle shell draws too few frames for the frame count to
        // free them. No deadline is set for it, so it wakes nothing.
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.trim_idle();
        }
        // An animating loop runs an increment every frame.
        if qml::eval::sweep_running() && !self.animating {
            q.push(now, Due::Gc);
        }
        let hover = self.ptr.hover_due(Instant::now());
        if let Some(h) = [self.ptr.hold_due(&self.rt), hover]
            .into_iter()
            .flatten()
            .reduce(f64::min)
        {
            q.push(now + h, Due::Pointer);
        }
        if self.caret.is_some()
            && let Some(toggle) = shell::scene::caret_phase(&self.rt.world.borrow()).1
        {
            q.push(now + toggle, Due::Caret);
        }
        if let Some(due) = self.key_repeat.due() {
            q.push(due, Due::KeyRepeat);
        }
        q.push(self.screencopy_due(), Due::Screencopy);
        if let Some(t) = self.stale_hidden.values().map(|&(_, t)| t).reduce(f64::min) {
            q.push(shell::sched::stale_relayout_at(t, now, busy), Due::Layout);
        }
        if ((self.clock.frame_due || self.clock.ahead > 0) && self.clock.running)
            || !self.ptr_frames.is_empty()
            || (self.motion_merge.waits() && !self.drag_awaits_frame())
            || self.motion_unanswered
            // Service calls queued by this turn run on the next turn.
            || !self.rt.world.borrow().effects.is_empty()
        {
            q.push(now, Due::Now);
        }
        // A drag's motion held for a frame callback that the compositor withholds is delivered
        // once the window counts as not shown (`unexposed`).
        if self.motion_merge.waits() && self.drag_awaits_frame() {
            q.push(give_up.unwrap_or(now), Due::Pointer);
            // Or ahead of that callback, in time to be drawn with it (`motion_ahead_at`).
            if !self.motion_early {
                q.push(self.motion_ahead_at().max(now), Due::Pointer);
            }
        }
        // Idle (nothing due now, every window's first frame shown): one hidden window is laid
        // out ahead of its first show per idle turn, as QQmlIncubationController::incubateFor
        // spends the event loop's idle time.
        self.prelayout_idle = !busy && q.head() > now && self.prelayout_pending();
        if self.prelayout_idle {
            q.push(now, Due::Layout);
        }
        q
    }

    /// Hidden windows wait for their prelayout, and every window has its size from the
    /// compositor and has committed its first frame.
    fn prelayout_pending(&self) -> bool {
        self.prelaid_at != Some(self.prelayout_state())
            && self.wins.iter().any(|w| w.size.0 > 0)
            && self.first_frames_committed()
    }

    /// Takes the previous image's windows off the screen once every window of this image
    /// has its first frame and its pictures are there (or [`shell::reload::PREDECESSOR_STAYS`]
    /// has passed), and reserves this image's exclusive zones in their place. The first frame
    /// of the wallpaper's window is empty: its Image loads on a worker and then fades in, and
    /// until it has the old wallpaper shows through it.
    fn release_predecessor(&mut self) {
        let drawn = self.started
            && self.first_frames_committed()
            && !self.rt.world.borrow().pictures_arriving();
        let Some((_, since)) = &mut self.predecessor else {
            return;
        };
        let since = since.get_or_insert_with(std::time::Instant::now);
        if !shell::reload::predecessor_leaves(drawn, since.elapsed()) {
            return;
        }
        tracing::debug!(
            target: "ri::frames",
            "predecessor leaves after {}ms, drawn {drawn}",
            since.elapsed().as_millis()
        );
        let old = self.predecessor.take();
        take_predecessors_place(&mut self.cloaks, &self.wins);
        // Written right before the old connection closes, with no wait for an answer: the
        // compositor reads both in one turn. Between two turns it would draw a frame with
        // both bars reserving space, this image's bar under the old one.
        let _ = self.conn.flush();
        drop(old);
    }

    /// Remembers the panels of this image's first frames, once.
    fn note_start_panels(&mut self) {
        if self.start_panels.is_some() || !self.started || !self.first_frames_committed() {
            return;
        }
        let names = (self.wins.iter())
            .filter_map(|w| match &w.surf {
                Surf::Panel { key, .. } => Some(key.namespace.clone()),
                _ => None,
            })
            .collect();
        self.start_panels = Some(names);
    }

    /// Every window has its size from the compositor and has committed its first frame.
    fn first_frames_committed(&self) -> bool {
        self.wins.iter().all(|w| w.configured && w.mapped.is_some())
    }

    /// What a look for hidden windows to lay out depends on: the objects made so far (a
    /// window among them is new) and whether a shown window tells the size of the screen,
    /// which a panel stretched between two edges is laid out at.
    fn prelayout_state(&self) -> (usize, bool) {
        let screen = self.wins.iter().any(|w| w.configured && w.size.0 > 0);
        (self.rt.world.borrow().items.len(), screen)
    }

    /// Now, while a toplevel capture wants its next frame (`Capture::wants_frame`): requests
    /// follow events (a drawn frame, a source change), never a timer.
    fn screencopy_due(&self) -> f64 {
        if self.export_mgr.is_some() && self.caps.values().any(Capture::wants_frame) {
            0.0
        } else {
            f64::INFINITY
        }
    }

    /// Window `k` presented a frame: the `ScreencopyView`s inside it showed their installed
    /// frames, which lets live ones request the next (QQuickItem::updatePaintNode).
    fn note_drawn(&mut self, k: usize) {
        let win = &self.wins[k];
        if win.dirty || win.swapped {
            return;
        }
        let root = win.item;
        let w = self.rt.world.borrow();
        for (&i, cap) in self.caps.iter_mut() {
            if cap.painted || cap.landing || cap.frame.is_some() || w.dead.contains(&i) {
                continue;
            }
            let mut cur = Some(i);
            while let Some(x) = cur {
                if x == root {
                    cap.painted = true;
                    break;
                }
                cur = w.items.get(x).and_then(|it| it.parent);
            }
        }
    }

    /// Ends `i`'s capture after a failed frame, as Quickshell drops a stopped
    /// ScreencopyContext: no content until the view's source changes.
    fn stop_capture(&mut self, i: usize) {
        if let Some(cap) = self.caps.get_mut(&i) {
            cap.stopped = true;
        }
        if qml::log_on!("ri::stats") {
            tracing::debug!(target: "ri::stats", "screencopy stopped {i}");
        }
        scene::set_screencopy(i, None);
        let mut w = self.rt.world.borrow_mut();
        if !w.dead.contains(&i) {
            w.write(i, "hasContent", Value::Bool(false));
            w.touched.push(i);
        }
    }

    /// Window `k`'s last frame waits for its frame callback. A compositor withholds the
    /// callback while it does not show the surface (DPMS off, under a fullscreen window) and
    /// sends it when it shows it again; the window then draws its current state.
    fn awaiting_callback(&self, k: usize) -> bool {
        self.wins[k].pending
    }

    /// Window `k`'s frame callback has been outstanding for `FRAME_CALLBACK_TIMEOUT_MS`: the
    /// compositor does not show the window (QWaylandWindow then reports it as not exposed).
    /// It still draws only once the callback comes, but its animations no longer wait for
    /// it: those that end step on the vsync timer, and endless ones rest like a hidden
    /// window's (`animating_windows`).
    fn unexposed(&self, k: usize) -> bool {
        let w = &self.wins[k];
        w.pending
            && w.asked.is_some_and(|t| {
                t.elapsed().as_secs_f64() * 1000.0 >= shell::sched::FRAME_CALLBACK_TIMEOUT_MS
            })
    }

    /// When the first window still waiting for its frame callback stops being waited for
    /// (`unexposed`), in loop milliseconds.
    fn callback_timeout(&self) -> Option<f64> {
        (0..self.wins.len())
            .filter(|&k| self.wins[k].pending && !self.unexposed(k))
            .filter_map(|k| self.wins[k].asked)
            .map(|t| self.loop_ms(t) + shell::sched::FRAME_CALLBACK_TIMEOUT_MS)
            .reduce(f64::min)
    }

    /// Exactly one window is exposed (`AnimClock::one_window`): it has committed a buffer
    /// and the compositor answers its frames (`unexposed`).
    fn one_window(&self) -> bool {
        let exposed = |&k: &usize| self.wins[k].mapped.is_some() && !self.unexposed(k);
        (0..self.wins.len()).filter(exposed).count() == 1
    }

    fn can_draw(&self, k: usize) -> bool {
        !self.awaiting_callback(k)
            && self.wins[k]
                .shown
                .is_none_or(|t| t.elapsed().as_secs_f64() * 1000.0 >= self.clock.vsync)
    }

    fn awaiting_frame(&self) -> bool {
        self.with_anim_attr(|animating, loose, unseen| {
            awaits_frame(
                (self.wins.iter().enumerate().zip(animating)).map(|((k, w), &a)| {
                    let exposed = !self.unexposed(k);
                    (w.pending && exposed, a || (w.anim && exposed))
                }),
                loose,
                unseen,
            )
        })
    }

    /// Every running animation shows nothing (see `animating_windows`).
    fn only_unseen(&self) -> bool {
        self.with_anim_attr(|animating, loose, unseen| {
            unseen && !loose && !animating.contains(&true)
        })
    }

    /// `animating_windows`, recomputed only when the `AnimKey` it was computed for changed.
    fn with_anim_attr<R>(&self, f: impl FnOnce(&[bool], bool, bool) -> R) -> R {
        let key = self.anim_key();
        let mut attr = self.anim_attr.borrow_mut();
        if attr.as_ref().is_none_or(|(k, _)| *k != key) {
            *attr = Some((key, self.animating_windows()));
        }
        let (_, (animating, loose, unseen)) = attr.as_ref().unwrap();
        f(animating, *loose, *unseen)
    }

    /// What `animating_windows` reads: the running animations, flicks and swipes, the item tree
    /// and the window roots. Steps of the same animations reuse the attribution.
    fn anim_key(&self) -> AnimKey {
        let w = self.rt.world.borrow();
        let mut ids: Vec<usize> = w.runs.iter().map(|r| r.anim).collect();
        ids.push(usize::MAX);
        ids.extend(
            (w.flicks.iter().chain(&w.flicks_x))
                .filter(|(_, f)| f.active)
                .map(|(&i, _)| i),
        );
        ids.push(usize::MAX);
        ids.extend(w.swipes.iter().filter(|(_, s)| !s.settled).map(|(&i, _)| i));
        ids.extend(
            w.view_hl
                .iter()
                .filter(|(_, h)| !h.settled)
                .map(|(&i, _)| i),
        );
        ids.extend(w.view_tracks_moving());
        ids.push(usize::MAX);
        ids.extend(w.frame_anims_ticking().map(|(f, _)| f));
        ids.push(usize::MAX);
        ids.extend(self.wins.iter().map(|win| win.item));
        ids.push(usize::MAX);
        ids.extend((0..self.wins.len()).filter(|&k| self.unexposed(k)));
        ids.push(usize::MAX);
        ids.extend((0..self.wins.len()).filter(|&k| self.out_of_sight(k)));
        ids.push(usize::from(self.sight.dark));
        (w.tree_gen, ids)
    }

    /// Which windows show a running animation, flick or swipe (the nearest window root above its
    /// target), whether one is shown in none, and whether one shows nothing at all: an endless
    /// animation (`loops: Animation.Infinite`) that animates no item, or only items of hidden
    /// windows and of windows the compositor does not show (`unexposed`), never finishes and
    /// changes no pixel, so it takes no steps until its window is shown again (it then resumes
    /// on the animation clock). So does a FrameAnimation of a window out of sight
    /// (`out_of_sight`), until the unlock or the first output that is on.
    fn animating_windows(&self) -> AnimAttr {
        let w = self.rt.world.borrow();
        let roots: rustc_hash::FxHashMap<usize, usize> = (self.wins.iter().enumerate())
            .map(|(k, win)| (win.item, k))
            .collect();
        let mut targets = Vec::new();
        let mut loose = false;
        let mut unseen = false;
        let in_hidden_window = |t: usize| {
            let mut c = Some(t);
            while let Some(i) = c {
                if let Some(&k) = roots.get(&i) {
                    return self.unexposed(k);
                }
                c = w.items.get(i).and_then(|it| it.parent);
            }
            qml::runtime::window_of(&w, t).is_some()
        };
        for r in &w.runs {
            let n = targets.len();
            let scripted = !job_targets(&r.job, &mut targets);
            if !scripted && r.loops < 0 && targets[n..].iter().all(|&t| in_hidden_window(t)) {
                targets.truncate(n);
                unseen = true;
                continue;
            }
            loose |= scripted || targets.len() == n;
        }
        targets.extend(
            (w.flicks.iter().chain(&w.flicks_x))
                .filter(|(_, f)| f.active)
                .map(|(&i, _)| i),
        );
        targets.extend(w.swipes.iter().filter(|(_, s)| !s.settled).map(|(&i, _)| i));
        targets.extend(
            w.view_hl
                .iter()
                .filter(|(_, h)| !h.settled)
                .map(|(&i, _)| i),
        );
        targets.extend(w.view_tracks_moving());
        // A FrameAnimation steps with the frames of the window it is declared in. One of a
        // window out of sight, or any while no output is on, is endless and shows nothing.
        for (f, _) in w.frame_anims_ticking() {
            let mut c = Some(f);
            let win = loop {
                let Some(i) = c else { break None };
                if let Some(&k) = roots.get(&i) {
                    break Some(k);
                }
                c = w.items.get(i).and_then(|it| it.parent);
            };
            if win.map_or(self.sight.dark, |k| self.out_of_sight(k)) {
                unseen = true;
            } else {
                targets.push(f);
            }
        }
        let mut animating = vec![false; self.wins.len()];
        for t in targets {
            let mut c = Some(t);
            while let Some(i) = c {
                if let Some(&k) = roots.get(&i) {
                    animating[k] = true;
                    break;
                }
                c = w.items.get(i).and_then(|it| it.parent);
            }
            loose |= c.is_none();
        }
        (animating, loose, unseen)
    }

    /// A frame drawn now is followed by another at the next vsync (running animations or a
    /// swipe), so it requests a wl_surface.frame callback to pace that one. A static frame
    /// requests none: nothing waits for it, and its callback would only wake the loop.
    fn frame_driven(&self) -> bool {
        animations_running(self) || self.ptr.swipe_animating()
    }

    /// Window `k` drew its last frame a moment ago: the one drawn now is one of a run (a drag,
    /// a resize, a pointer moving over things that answer it), so it requests a frame callback
    /// too and the run goes at the display's rate. Without one the next frame waits a refresh
    /// interval from the end of this one's drawing, and every frame of the run comes later by
    /// the time it took to draw.
    fn in_frame_run(&self, k: usize) -> bool {
        shell::sched::in_frame_run(
            self.wins[k]
                .committed
                .map(|t| t.elapsed().as_secs_f64() * 1000.0),
        )
    }

    /// Shows the window's last frame again with the screencopy textures swapped into it.
    fn swap_draw(&mut self, k: usize) {
        self.swap_frame(k);
        self.note_drawn(k);
    }

    /// A `render` of window `k` showed nothing because an EGL call failed (the renderer logged
    /// it): the window draws again, on a new surface, or the whole shell on a new renderer
    /// when the context is lost.
    fn gpu_failed(&mut self, k: usize, failure: shell::gpu::Failure) {
        match failure {
            shell::gpu::Failure::Surface => {
                let win = &mut self.wins[k];
                for mut v in win.veils.drain(..) {
                    v.hide();
                    win.spare.push(v);
                }
                for v in &mut win.spare {
                    v.gl = None;
                }
                win.gl = None;
                win.dirty = true;
            }
            shell::gpu::Failure::Context => self.gpu_lost(),
        }
    }

    /// The GPU context is lost for good (a driver reset, a suspend that dropped video memory).
    /// Everything made on it is released, as QSGGuiThreadRenderLoop::handleDeviceLoss releases
    /// the scene graph and its QRhi, and every window is drawn again: with a new context, or
    /// on the CPU when there is none or the lost one never showed a frame (a new one would
    /// be lost the same way). The log says which.
    fn gpu_lost(&mut self) {
        let Some(old) = self.gpu.take() else {
            return;
        };
        // The EGL surfaces and the frames drawn on them go before their context.
        let retiring = self.retiring.iter_mut().map(|r| &mut r.win);
        for w in self.wins.iter_mut().chain(retiring) {
            w.veils.clear();
            w.spare.clear();
            w.gl = None;
            w.last = None;
            w.dirty = true;
        }
        // Screencopy frames were textures of the lost context: a copy in flight is dropped,
        // and every capture starts over with its first frame.
        let items: Vec<usize> = self.caps.keys().copied().collect();
        for i in items {
            let Some(cap) = self.caps.get_mut(&i) else {
                continue;
            };
            if let Some(f) = cap.frame.take()
                && let Some(f) = self.frames_unanswered.retire(f, cap.seen)
            {
                f.destroy();
            }
            let fresh = cap.restarted();
            let old = std::mem::replace(cap, fresh);
            if old.busy() {
                self.cap_grave.push(old);
            }
            if scene::screencopy(i).is_some_and(|t| t.px.is_empty()) {
                scene::set_screencopy(i, None);
                let mut w = self.rt.world.borrow_mut();
                if !w.dead.contains(&i) {
                    w.write(i, "hasContent", Value::Bool(false));
                    w.touched.push(i);
                }
            }
        }
        self.dma_ready.clear();
        self.dma_wait.clear();
        // Their textures went with the context and their EGLImages go with the display, which
        // the last `Gpu` on it terminates.
        let slots = self.dma_keep.drain().map(|e| e.1);
        for s in slots.chain(self.dma_grave.drain(..)) {
            s.buf.destroy();
        }
        self.cap_cache.retain(|_, e| !e.1.px.is_empty());
        let shown = old.shown();
        old.retire();
        scene::forget_textures();
        if shown && let Some(backend) = self.display.backend().upgrade() {
            // SAFETY: the pointer is the live `wl_display*` of the connection `run` made
            // `self.display` on, which `run` drops after `self`: it outlives the new `Gpu`.
            self.gpu = unsafe { shell::gpu::Gpu::new(backend.display_ptr() as *mut _) };
        }
        match &self.gpu {
            Some(g) => elog::eprintln!(
                "renderer: GPU (GLES2, {}), a new context after the lost one",
                g.renderer
            ),
            None => {
                elog::eprintln!(
                    "renderer: CPU raster (wl_shm) from now on, the GPU context is lost"
                )
            }
        }
    }

    fn swap_frame(&mut self, k: usize) {
        let t0 = std::time::Instant::now();
        let wls = self.wins[k].surf.wl_surface().clone();
        let callback = self.frame_driven() || self.wins[k].anim || self.in_frame_run(k);
        let win = &mut self.wins[k];
        let (Some(gpu), Some(gs), Some(fr)) =
            (self.gpu.as_mut(), win.gl.as_mut(), win.last.as_mut())
        else {
            win.dirty = true;
            return;
        };
        let qh = &self.qh;
        let pres = self.presentation.as_ref();
        gpu.clock = Some(anim_instant(self.start, &self.rt));
        gpu.render(gs, fr, || {
            if callback {
                wls.frame(qh, FrameCallbackData(wls.clone()));
            }
            log_presented(pres, &wls, qh);
        });
        if let Some(failure) = gpu.take_failure() {
            // Nothing was committed: no frame callback is coming for this frame.
            self.gpu_failed(k, failure);
            return;
        }
        mark_shown(win, callback);
        self.clock.shown();
        win.swapped = false;
        self.frames += 1;
        committed(&self.display, qh, win);
        if qml::log_on!("ri::stats") {
            let d = gpu.last_damage;
            tracing::debug!(
                target: "ri::stats",
                "swapdraw win{k} {}us damage {}x{}",
                t0.elapsed().as_micros(),
                d[2] - d[0],
                d[3] - d[1]
            );
        }
    }

    fn draw(&mut self, k: usize) {
        self.ptr.frame(self.wins[k].item, Instant::now());
        self.draw_frame(k);
        self.note_drawn(k);
    }

    fn draw_frame(&mut self, k: usize) {
        let (w, h) = self.wins[k].size;
        if !self.wins[k].configured || w == 0 || h == 0 {
            return;
        }
        // Every path (configure, frame callback, input, tick) waits: a layer's first frame is
        // drawn right from its configure, and a changed window's next one from its frame
        // callback.
        if self.holds_for_screen_cap(self.wins[k].item) || self.holds_for_lock_surfaces(k) {
            self.wins[k].dirty = true;
            return;
        }
        let retire = self.first_zone_frame(k);
        // QSGThreadedRenderLoop advances animations right after each sync, and a window with no
        // frame callback outstanding gets its next update at once: a frame drawn outside a frame
        // callback (the first after idle) is followed by another driver step before the next.
        // A frame that turns out to change no pixel takes that step back (`leads`).
        let leads = animations_running(self)
            && !self.clock.answered
            && !self.clock.frame_due
            && !self.clock.led
            && !self.clock.late;
        if leads {
            self.clock.frame_due = true;
        }
        let item = self.wins[k].item;
        // updatePolish: views apply the model changes held since the last frame.
        if self.rt.held_windows().contains(&item) {
            self.rt.polish(&[item]);
            self.mark_dirty();
        }
        if qml::log_on!("ri::frames") {
            tracing::debug!(
                target: "ri::frames",
                "draw win{k} {w}x{h} t={:.0}",
                self.start.elapsed().as_secs_f64() * 1000.0
            );
        }
        let t0 = std::time::Instant::now();
        let mut resized = self.take_resized(item);
        let stats = qml::log_on!("ri::stats");
        let mut changed: Vec<usize> = Vec::new();
        if stats {
            scene::take_layout_work();
            changed.extend(&resized);
        }
        self.drain_lrev(None);
        let rev = self.rt.world.borrow().layout_rev;
        // Laid out at this size with nothing inside changed since (layout_rev is global).
        let kept = self.wins[k].laid.0 != rev && self.clean.get(&item) == Some(&(w, h));
        if kept {
            self.wins[k].laid = (rev, (w, h));
        }
        let laid = self.wins[k].laid == (rev, (w, h));
        if laid {
            // The root taking the surface size it was already laid out at changes nothing.
            let wd = self.rt.world.borrow();
            let at = |n: &str| wd.items[item].props.get(n).map(|p| p.value.to_number());
            if at("width") == Some(w as f64) && at("height") == Some(h as f64) {
                resized.retain(|&i| i != item);
            }
        }
        let local = laid && {
            let mut wd = self.rt.world.borrow_mut();
            !qml::debug::on(qml::debug::Switch::NOLOCAL)
                && resized.iter().all(|&i| scene::is_local_resize(&mut wd, i))
        };
        let mut full = false;
        if !local && !resized.is_empty() {
            full = true;
        }
        if qml::log_on!("ri::stats") && !resized.is_empty() {
            let mut wd = self.rt.world.borrow_mut();
            let ids: Vec<usize> = resized
                .iter()
                .copied()
                .filter(|&i| !scene::is_local_resize(&mut wd, i))
                .take(4)
                .collect();
            let bad: Vec<String> = ids
                .iter()
                .map(|&i| {
                    let p = wd.items[i].parent.unwrap_or(0);
                    format!(
                        "{i}:{:?}<{p}:{:?}",
                        wd.items[i].types.first(),
                        wd.items[p].types.first()
                    )
                })
                .collect();
            let first: Vec<String> = resized
                .iter()
                .take(3)
                .map(|&i| format!("{i}:{:?}", wd.items[i].types.first()))
                .collect();
            tracing::debug!(
                target: "ri::stats",
                "resized {} local {local} nonlocal {bad:?} first {first:?}",
                resized.len()
            );
        }
        let lay_full = full || self.wins[k].laid != (self.rt.world.borrow().layout_rev, (w, h));
        if lay_full && qml::log_on!("ri::stats") {
            tracing::debug!(
                target: "ri::stats",
                "full layout win{k} item {item}: full {full} kept {kept} laid {:?} rev {rev} clean {:?} stale {:?}",
                self.wins[k].laid,
                self.clean.get(&item),
                self.stale_hidden.get(&item)
            );
        }
        let lay_check = qml::debug::on(qml::debug::Switch::LAYCHECK)
            && !lay_full
            && (kept || !resized.is_empty());
        if lay_full {
            scene::layout_window(
                &mut self.rt,
                &mut self.fonts.borrow_mut(),
                item,
                [w as f64, h as f64],
            );
            self.wins[k].laid = (self.rt.world.borrow().layout_rev, (w, h));
            self.drain_lrev(Some(item));
            self.clean.insert(item, (w, h));
            self.roots.insert(item);
            // A binding loop through implicit sizes would re-lay the window every frame: after
            // a few unsettled frames the last placement is final until something else changes.
            if scene::take_unsettled() {
                self.wins[k].unsettled = self.wins[k].unsettled.saturating_add(1);
                if self.wins[k].unsettled >= 3 {
                    self.rt.settle();
                    if self.wins[k].unsettled == 3 {
                        tracing::warn!(
                            target: "ri::layout",
                            "window {item}: layout did not settle in 3 frames (a binding loop through implicit sizes), keeping the last placement; still resizing {:?}",
                            scene::unsettled_items(&self.rt.world.borrow())
                        );
                    }
                }
            } else {
                self.wins[k].unsettled = 0;
            }
            // The full pass settles until quiet: resizes it made itself are already placed.
            self.take_resized(item);
        } else if !resized.is_empty() {
            let mut rs = resized;
            // Bindings fed by a local pass (a parent sized from a text's implicitWidth) settle
            // within the same frame, like the full pass does.
            for pass in 0..4 {
                let mut wd = self.rt.world.borrow_mut();
                let rev = wd.layout_rev;
                // In the full pass's order: measure, let the bindings on the measured implicit
                // sizes answer, place.
                scene::measure_targets(&mut wd, &mut self.fonts.borrow_mut(), &rs);
                // The local pass is this frame's layout; its own writes need no full pass.
                wd.layout_rev = rev;
                drop(wd);
                let t = std::time::Instant::now();
                self.rt.settle();
                if qml::log_on!("ri::stats") && t.elapsed().as_micros() > 300 {
                    tracing::debug!(target: "ri::stats", "  local settle {}us", t.elapsed().as_micros());
                }
                let answered = self.take_resized(item);
                if stats {
                    changed.extend(&answered);
                }
                let mut wd = self.rt.world.borrow_mut();
                let answered_rev = wd.layout_rev;
                let mut local = answered.is_empty()
                    || answered_rev == rev
                        && answered.iter().all(|&i| scene::is_local_resize(&mut wd, i));
                if local {
                    rs.extend(answered);
                    scene::relayout_targets(&mut wd, &mut self.fonts.borrow_mut(), &rs, pass);
                    wd.layout_rev = answered_rev;
                    drop(wd);
                    let t = std::time::Instant::now();
                    self.rt.settle();
                    if qml::log_on!("ri::stats") && t.elapsed().as_micros() > 300 {
                        tracing::debug!(target: "ri::stats", "  local settle {}us", t.elapsed().as_micros());
                    }
                    rs = self.take_resized(item);
                    if stats {
                        changed.extend(&rs);
                    }
                    wd = self.rt.world.borrow_mut();
                    if rs.is_empty() {
                        break;
                    }
                    local = wd.layout_rev == rev
                        && rs.iter().all(|&i| scene::is_local_resize(&mut wd, i));
                }
                if !local {
                    drop(wd);
                    scene::layout_window(
                        &mut self.rt,
                        &mut self.fonts.borrow_mut(),
                        item,
                        [w as f64, h as f64],
                    );
                    self.wins[k].laid = (self.rt.world.borrow().layout_rev, (w, h));
                    self.drain_lrev(Some(item));
                    self.clean.insert(item, (w, h));
                    self.roots.insert(item);
                    break;
                }
            }
        }
        if stats {
            changed.sort_unstable();
            changed.dedup();
            let (measured, placed) = scene::take_layout_work();
            if measured + placed > 0 {
                let moving = self.rt.world.borrow().view_moving.len();
                tracing::debug!(
                    target: "ri::stats",
                    "layout work win{k}: changed {} measured {measured} placed {placed} full {lay_full} moving {moving}",
                    changed.len()
                );
            }
        }
        // Writes the layout passes above made inside this window leave it clean.
        self.drain_lrev(Some(item));
        // updatePolish: new canvases paint after the layouts placed them.
        if self.rt.canvases_available() {
            self.rt.settle();
            self.drain_lrev(Some(item));
        }
        {
            let mut wd = self.rt.world.borrow_mut();
            wd.frame_seq += 1;
            let seq = wd.frame_seq;
            wd.win_frames.insert(item, seq);
            if wd.frame_wanted.remove(&item) | wd.frame_wanted.remove(&usize::MAX) {
                self.pinged = true;
            }
        }
        if lay_check {
            // Debug: the local pass must leave the same geometry a full pass computes.
            let before = scene::geom_snapshot(&mut self.rt.world.borrow_mut(), item);
            scene::set_memo_off(true);
            scene::layout_window(
                &mut self.rt,
                &mut self.fonts.borrow_mut(),
                item,
                [w as f64, h as f64],
            );
            scene::set_memo_off(false);
            self.wins[k].laid = (self.rt.world.borrow().layout_rev, (w, h));
            self.drain_lrev(Some(item));
            self.clean.insert(item, (w, h));
            self.roots.insert(item);
            let mut wd = self.rt.world.borrow_mut();
            let after = scene::geom_snapshot(&mut wd, item);
            let bad: Vec<String> = after
                .iter()
                // The root's size belongs to the surface.
                .filter(|(i, g)| {
                    **i != item
                        && before.get(i).is_none_or(|b| {
                            b.iter().zip(g.iter()).any(|(x, y)| (x - y).abs() > 0.01)
                        })
                })
                .take(6)
                .map(|(&i, g)| {
                    format!(
                        "{i}:{:?} {:?}->{g:?}",
                        wd.items[i].types.first(),
                        before.get(&i)
                    )
                })
                .collect();
            if !bad.is_empty() {
                tracing::warn!(target: "ri::check", "laycheck win{k} {bad:?}");
            }
        }
        // The full layout follows the cursors itself.
        if !lay_full
            && scene::follow_cursors(
                &mut self.rt.world.borrow_mut(),
                &mut self.fonts.borrow_mut(),
                item,
            )
        {
            self.rt.settle();
        }
        {
            // Scrolling moves a Flickable's content without relayout; only its edge flags follow.
            let mut wd = self.rt.world.borrow_mut();
            let mut sc = std::mem::take(&mut wd.scrolled);
            sc.sort_unstable();
            sc.dedup();
            for f in sc {
                if !wd.dead.contains(&f) {
                    scene::flick_flags(&mut wd, f);
                }
            }
        }
        let tm = std::time::Instant::now();
        let rects = scene::mask_rects(&mut self.rt, item);
        if rects.is_some() && rects != self.wins[k].region {
            if qml::log_on!("ri::ptr") {
                tracing::debug!(target: "ri::ptr", "region win{k} {rects:?}");
            }
            if let Ok(r) = Region::new(&self.compositor) {
                let f = self.wins[k].scale.factor;
                for &[x, y, rw, rh] in rects.iter().flatten() {
                    let (x0, y0) = (to_surface(x, f), to_surface(y, f));
                    r.add(
                        x0,
                        y0,
                        to_surface(x + rw, f) - x0,
                        to_surface(y + rh, f) - y0,
                    );
                }
                self.wins[k]
                    .surf
                    .wl_surface()
                    .set_input_region(Some(r.wl_region()));
            }
            self.wins[k].region = rects;
        }
        let tf = std::time::Instant::now();
        self.forget_touched(item);
        let t1 = std::time::Instant::now();
        if qml::log_on!("ri::stats") && (t1 - t0).as_millis() > 4 {
            tracing::debug!(
                target: "ri::stats",
                "  phases pre {}us mask {}us forget {}us",
                (tm - t0).as_micros(),
                (tf - tm).as_micros(),
                (t1 - tf).as_micros()
            );
        }
        let wls = self.wins[k].surf.wl_surface().clone();
        let dpr = self.win_scale(k);
        if self.gpu.is_some() {
            self.apply_scale(k, dpr);
        }
        let prev_frame = self.wins[k].last.take();
        let driven = self.frame_driven() || self.in_frame_run(k);
        let mut veils = std::mem::take(&mut self.wins[k].veils);
        let mut spare = std::mem::take(&mut self.wins[k].spare);
        let veil_scale = (self.wins[k].scale.buf, self.wins[k].scale.dest);
        let dev = self.wins[k].device_size();
        if let Some(gpu) = self.gpu.as_mut() {
            if self.wins[k].gl.is_none() {
                let (dw, dh) = dev;
                self.wins[k].gl = gpu.surface(self.wins[k].surf.wl_surface(), dw as i32, dh as i32);
            }
            if let Some(gs) = self.wins[k].gl.as_mut() {
                let mut fr = shell::gpu::Frame::scaled(w as usize, h as usize, dpr).onto(dev);
                scene::paint_window(
                    &mut self.rt,
                    &mut self.fonts.borrow_mut(),
                    item,
                    &mut scene::Target::Gpu(&mut fr),
                );
                let anim_now = scene::take_animating();
                let t2 = std::time::Instant::now();
                gpu.may_skip = !anim_now;
                gpu.clock = Some(anim_instant(self.start, &self.rt));
                // A theme or page change crossfades from the window's previous frame.
                gs.crossfade.observe(true);
                // The start frame went onto a new veil this frame (`hold`).
                let mut hold = false;
                if let Some(d) = gs
                    .crossfade
                    .take(|| prev_frame.as_ref().map(|p| !p.draws_same(&fr)))
                    && let Some(old) = prev_frame.as_ref()
                {
                    tracing::debug!(target: "ri::fade", "win{k} {w}x{h} fade {}ms", d.as_millis());
                    // Each change fades out on its own veil, below the veils of earlier changes
                    // still fading: the screen shows the same pixels the moment it starts and
                    // every earlier fade keeps its curve. An in-window fade already running (or
                    // too many veils) restarts from the shown pixels instead (`begin_fade`).
                    // A change of a part of the window (a page of the settings window) fades
                    // inside the window, over that part only: a veil covers the whole window,
                    // and the compositor blends all of it every frame of the fade.
                    let part = fr
                        .changed_part(old)
                        .filter(|_| !gs.fading() && veils.is_empty());
                    // A veil over a window kept unseen until the previous image's windows
                    // leave would show alone: the alpha is the window's, not its subsurfaces'.
                    let cloaked = self.cloaks.iter().any(|(s, _)| *s == wls);
                    let mgr = self.veil_mgr.as_ref().filter(|_| {
                        part.is_none() && !gs.fading() && veils.len() < MAX_VEILS && !cloaked
                    });
                    let mut veiled = false;
                    if let Some(m) = mgr {
                        // A spare veil has its EGL surface and buffers from an earlier fade;
                        // the start frame is the window's last, whose triangles it draws from
                        // the window's uploaded vertex buffers (`share_buffers`).
                        let mut v = spare.pop().unwrap_or_else(|| {
                            tracing::debug!(target: "ri::fade", "win{k} veil new");
                            m.veil(&self.compositor, &self.qh, &wls)
                        });
                        v.arm(self.viewporter.as_ref(), &self.qh, veil_scale, d);
                        match veils.last() {
                            Some(below) => v.sub.place_below(&below.surf),
                            None => v.sub.place_above(&wls),
                        }
                        let (vw, vh) = gs.size();
                        if v.gl.is_none() {
                            v.gl = gpu.surface(&v.surf, vw, vh);
                        }
                        if let Some(vgl) = v.gl.as_mut() {
                            vgl.share_buffers(gs);
                            // The veil was unmapped: it needs a buffer even if its last one
                            // already holds this frame.
                            vgl.reswap();
                            gpu.may_skip = false;
                            if gpu.render(vgl, &mut old.clone(), || {}) {
                                v.mapped = true;
                                tracing::debug!(
                                    target: "ri::fade",
                                    "win{k} veil {vw}x{vh} {}ms uploads {}",
                                    d.as_millis(),
                                    gpu.last_uploads
                                );
                                veils.push(v);
                                veiled = true;
                                hold = true;
                            } else {
                                spare.push(v);
                            }
                        } else {
                            spare.push(v);
                        }
                    }
                    if !veiled {
                        gpu.begin_fade(gs, old, d, part.as_deref());
                    }
                }
                let (dw, dh) = dev;
                if veils
                    .iter()
                    .any(|v| v.gl.as_ref().map(|g| g.size()) != Some((dw as i32, dh as i32)))
                {
                    // Resized: the start frames no longer line up with the window.
                    for mut v in veils.drain(..) {
                        v.hide();
                        spare.push(v);
                    }
                    hold = false;
                }
                // A switch draws two full frames per window, the start frame and the new one:
                // drawing both at once on every window doubles the frame the switch lands in.
                // So the frame that put the start frame on its new veil commits the window
                // unchanged, with the veil still transparent (the screen shows exactly the old
                // frame), and the new frame is drawn on the next, where the fade starts.
                let mut fr = match prev_frame {
                    Some(old) if hold => old,
                    _ => fr,
                };
                let now = gpu.clock.unwrap_or_else(Instant::now);
                let held = if hold { veils.len() - 1 } else { usize::MAX };
                let veil_alpha: Vec<u32> = veils
                    .iter_mut()
                    .enumerate()
                    .map(|(i, v)| if i == held { 0 } else { v.advance(now) })
                    .collect();
                let callback = driven || anim_now || gs.fading() || !veils.is_empty();
                // Every veil step commits the window, which applies the synchronized subsurface.
                // Any other frame that changes no pixel (an item of a running animation moving
                // while its opacity is 0) sends the compositor nothing, neither a commit nor a
                // frame request: the next driver step comes by the vsync timer.
                gpu.may_skip = veils.is_empty() && !self.clock.late;
                let qh = &self.qh;
                let retiring = &mut self.retiring;
                let pres = self.presentation.as_ref();
                let swapped = gpu.render(gs, &mut fr, || {
                    if let Some(item) = retire {
                        retire_replaced(retiring, item);
                    }
                    if callback {
                        wls.frame(qh, FrameCallbackData(wls.clone()));
                    }
                    for (v, &a) in veils.iter_mut().zip(&veil_alpha) {
                        v.alpha.set_multiplier(a);
                        if v.done() {
                            v.hide();
                        } else {
                            v.surf.commit();
                        }
                    }
                    log_presented(pres, &wls, qh);
                });
                for mut v in veils.extract_if(.., |v| v.done()) {
                    tracing::debug!(target: "ri::fade", "win{k} veil end");
                    v.hide();
                    spare.push(v);
                }
                let fading = gs.fading() || !veils.is_empty();
                if swapped {
                    mark_shown(&mut self.wins[k], callback);
                    self.clock.shown();
                    committed(&self.display, qh, &mut self.wins[k]);
                }
                self.clock.sent(swapped, leads);
                if qml::log_on!("ri::stats") {
                    let d = gpu.last_damage;
                    let dl = scene::take_dl_stats();
                    tracing::debug!(
                        target: "ri::stats",
                        "  dl replay {} record {} nocache {} entries {} paint {}us",
                        dl.0,
                        dl.1,
                        dl.2,
                        dl.3,
                        (t2 - t1).as_micros()
                    );
                    tracing::debug!(
                        target: "ri::stats",
                        "draw win{k} {w}x{h} layout {}ms paint {}ms gpu {}ms damage {}x{} swap {}ms",
                        (t1 - t0).as_millis(),
                        (t2 - t1).as_millis(),
                        t2.elapsed().as_millis(),
                        d[2] - d[0],
                        d[3] - d[1],
                        gpu.swap_start.map_or(0, |s| s.elapsed().as_millis())
                    );
                }
                self.wins[k].dirty = hold;
                self.wins[k].anim = anim_now || fading;
                self.wins[k].veils = veils;
                self.wins[k].spare = spare;
                self.wins[k].last = Some(fr);
                self.wins[k].swapped = false;
                self.frames += 1;
                if let Some(failure) = gpu.take_failure() {
                    self.gpu_failed(k, failure);
                }
                return;
            }
        }
        self.wins[k].last = prev_frame;
        // The CPU rasterizer draws at scale 1; the compositor scales the buffer up (to a
        // screen's factor only through a viewport).
        self.apply_scale(k, 1.0);
        let mut cv = qsg::raster::Canvas::new(w as usize, h as usize, [0, 0, 0, 0]);
        scene::paint_window(
            &mut self.rt,
            &mut self.fonts.borrow_mut(),
            item,
            &mut scene::Target::Cpu(&mut cv),
        );
        self.wins[k].anim = scene::take_animating();
        if qml::log_on!("ri::stats") {
            tracing::debug!(
                target: "ri::stats",
                "draw win{k} {w}x{h} layout {}ms paint {}ms",
                (t1 - t0).as_millis(),
                t1.elapsed().as_millis()
            );
        }
        let callback = self.frame_driven() || self.wins[k].anim || self.in_frame_run(k);
        let damage = match &self.wins[k].shm_prev {
            Some(p) if (p.w, p.h) == (cv.w, cv.h) => {
                shell::region::diff_pixels(&p.px, &cv.px, cv.w, cv.h)
            }
            _ => vec![[0, 0, w as i64, h as i64]],
        };
        if damage.is_empty() && !self.clock.late {
            // The surface already shows this frame: nothing is sent, neither a commit nor a
            // frame request, and the next step of a running animation comes by the vsync timer.
            // It is the frame that shows an animation at its start all the same.
            self.wins[k].dirty = false;
            self.clock.shown();
            self.clock.sent(false, leads);
            return;
        }
        // The rasterizer's premultiplied RGBA bytes are DRM's ABGR8888: copied as they are
        // where the compositor takes it, else swizzled to the always supported ARGB8888.
        let format = if self.shm.formats().contains(&wl_shm::Format::Abgr8888) {
            wl_shm::Format::Abgr8888
        } else {
            wl_shm::Format::Argb8888
        };
        let stride = w as i32 * 4;
        let (buffer, canvas) = match self.pool.create_buffer(w as i32, h as i32, stride, format) {
            Ok(b) => b,
            Err(e) => {
                elog::eprintln!("shm buffer {w}x{h}: {e}");
                return;
            }
        };
        if format == wl_shm::Format::Abgr8888 {
            canvas[..cv.px.len() * 4].copy_from_slice(cv.px.as_flattened());
        } else {
            for (dst, p) in canvas.as_chunks_mut::<4>().0.iter_mut().zip(&cv.px) {
                dst.copy_from_slice(&[p[2], p[1], p[0], p[3]]);
            }
        }
        if let Some(item) = retire {
            retire_replaced(&mut self.retiring, item);
        }
        let win = &mut self.wins[k];
        let s = win.surf.wl_surface();
        for r in &damage {
            s.damage_buffer(
                r[0] as i32,
                r[1] as i32,
                (r[2] - r[0]) as i32,
                (r[3] - r[1]) as i32,
            );
        }
        if let Err(e) = buffer.attach_to(s) {
            elog::eprintln!("attach: {e:?}");
            return;
        }
        if callback {
            s.frame(&self.qh, FrameCallbackData(s.clone()));
        }
        log_presented(self.presentation.as_ref(), s, &self.qh);
        s.commit();
        mark_shown(win, callback);
        self.clock.shown();
        win.dirty = false;
        self.frames += 1;
        committed(&self.display, &self.qh, win);
        self.clock.sent(true, leads);
        if qml::log_on!("ri::stats") && self.frames <= 2 {
            tracing::debug!(target: "ri::stats", "frame {} {w}x{h}", self.frames);
            if let Ok(png) = cv.png() {
                shell::ipc::write_debug_file("impulse-shell-frame.png", &png);
            }
        }
        self.wins[k].shm_prev = Some(cv);
    }
}

/// Quickshell's IPC type name for a QML type (`IpcValueSlot::typeName`), `None` if unsupported.
fn ipc_type(ty: &str) -> Option<&'static str> {
    Some(match ty.trim() {
        "string" | "QString" => "string",
        "int" => "int",
        "bool" => "bool",
        "real" | "double" => "real",
        "color" | "QColor" => "color",
        _ => return None,
    })
}

/// `name: type` (or old-style `type name`) parameters; `None` if one is untyped or unsupported.
fn ipc_params(params: &str) -> Option<Vec<(String, String)>> {
    params
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (n, t) = p
                .split_once(':')
                .or_else(|| p.split_once(' ').map(|(t, n)| (n, t)))?;
            Some((n.trim().to_string(), ipc_type(t)?.to_string()))
        })
        .collect()
}

/// Declared type of `property alias name: id.prop`.
fn ipc_alias_type(w: &qml::runtime::World, item: usize, name: &str) -> Option<&'static str> {
    let a = w.items[item].props.get(name)?.alias.as_deref()?;
    let [id, prop] = a.path.as_slice() else {
        return None;
    };
    let t = &w.items[w.find_id(a.ctx, id)?];
    let p = t.props.get(prop);
    if p.is_some_and(|p| p.alias.is_some()) {
        return None;
    }
    // Assigned built-ins are stored as `var`: the declared type comes from the qmltypes.
    if let Some(i) = t.prop_info(prop) {
        return (!i.list).then_some(i.ty.as_str());
    }
    if prop == "objectName" {
        return Some("string");
    }
    p.map(|p| p.ty.as_str())
}

/// `qs ipc show`: the enabled `IpcHandler` targets with the members quickshell exposes for them
/// (`IpcHandler::onPostReload`): typed functions, supported properties, ≤1-argument signals
/// and every declared property's change signal.
fn ipc_targets(rt: &qml::runtime::Runtime, only: &str) -> Vec<shell::ipc::TargetDef> {
    ipc_target_items(rt, only)
        .into_iter()
        .map(|(_, t)| t)
        .collect()
}

/// [`ipc_targets`] with the `IpcHandler` serving each target.
fn ipc_target_items(rt: &qml::runtime::Runtime, only: &str) -> Vec<(usize, shell::ipc::TargetDef)> {
    use qml::runtime::IpcDecl;
    use shell::ipc::{FnDef, TargetDef};
    let mut w = rt.world.borrow_mut();
    let mut out: Vec<(usize, TargetDef)> = Vec::new();
    for h in w.of_type("IpcHandler") {
        if w.dead.contains(&h) || w.read(h, "enabled").is_some_and(|e| !e.truthy()) {
            continue;
        }
        let name = w
            .read(h, "target")
            .map(|t| t.to_js_string())
            .unwrap_or_default();
        if name.is_empty()
            || (!only.is_empty() && name != only)
            || out.iter().any(|(_, t)| t.name == name)
        {
            continue;
        }
        let mut t = TargetDef {
            name,
            functions: Vec::new(),
            properties: Vec::new(),
            signals: Vec::new(),
        };
        for d in w.ipc_decls.get(&h).into_iter().flatten() {
            match d {
                IpcDecl::Function { name, params, ret } => {
                    let ret = match ret.as_str() {
                        "" | "var" | "variant" | "void" => Some("void"),
                        r => ipc_type(r),
                    };
                    if let (Some(ret), Some(args)) = (ret, ipc_params(params))
                        && args.len() <= 10
                    {
                        t.functions.push(FnDef {
                            name: name.clone(),
                            ret: ret.to_string(),
                            args,
                        });
                    }
                }
                IpcDecl::Property { name, ty } => {
                    let ty = match ty.as_str() {
                        "alias" => ipc_alias_type(&w, h, name),
                        ty => Some(ty),
                    };
                    if let Some(ty) = ty.and_then(ipc_type) {
                        t.properties.push((name.clone(), ty.to_string()));
                    }
                    t.signals
                        .push((format!("{name}Changed"), String::new(), String::new()));
                }
                IpcDecl::Signal { name, params } => match ipc_params(params).as_deref() {
                    Some([]) => t.signals.push((name.clone(), String::new(), String::new())),
                    Some([(n, ty)]) => t.signals.push((name.clone(), n.clone(), ty.clone())),
                    _ => {}
                },
            }
        }
        out.push((h, t));
    }
    out
}

/// Test harness: every visible, enabled MouseArea with its window-relative rect and a label.
/// A remote `qs ipc listen|wait` on one handler's signal; `ty` is its argument's IPC type.
struct IpcListener {
    item: usize,
    signal: String,
    ty: Option<String>,
    stream: shell::ipc::Listener,
}

/// `IpcValueSlot::toString` of `v` as IPC type `ty`.
fn ipc_value_string(ty: &str, v: &Value) -> String {
    match ty {
        "int" => {
            let n = v.to_number();
            let n = if n.is_finite() {
                n.trunc() as i64 as i32
            } else {
                0
            };
            n.to_string()
        }
        "bool" => if v.truthy() { "true" } else { "false" }.to_string(),
        "real" => qml::builtins::fmt_g6(v.to_number()),
        "color" => {
            let c = match v {
                Value::Color(c) => *c,
                v => qml::builtins::parse_color(&v.to_js_string()),
            };
            let [r, g, b, a] = c.rgba8();
            format!("#{a:02x}{r:02x}{g:02x}{b:02x}")
        }
        _ => match v {
            Value::Undefined | Value::Null => String::new(),
            v => v.to_js_string(),
        },
    }
}

/// `IpcValueSlot::setString`: `s` as an argument of IPC type `ty`, `None` if it doesn't parse.
fn ipc_parse_arg(ty: &str, s: &str) -> Option<Value> {
    let int = |s: &str| s.trim().parse::<i32>().ok();
    match ty {
        "string" => Some(Value::str(s)),
        "int" => int(s).map(|n| Value::Num(n.into())),
        "bool" => match s {
            "true" | "false" => Some(Value::Bool(s == "true")),
            s => int(s).map(|n| Value::Bool(n != 0)),
        },
        "real" => s.trim().parse::<f64>().ok().map(Value::Num),
        "color" => {
            let valid = |c: &qml::color::Color| c.spec != qml::color::Spec::Invalid;
            let mut c = qml::builtins::parse_color(s);
            if !valid(&c) && !s.starts_with('#') {
                c = qml::builtins::parse_color(&format!("#{s}"));
            }
            valid(&c).then_some(Value::Color(c))
        }
        _ => None,
    }
}

/// Answer an IPC command on the matching enabled `IpcHandler`, as quickshell's
/// `IpcHandlerRegistry` does: only the typed members `qs ipc show` lists are reachable,
/// arguments parse by their declared type, and a throwing handler is reported as an error.
fn ipc_call(
    rt: &mut qml::runtime::Runtime,
    c: &shell::ipc::Call,
    listeners: &mut Vec<IpcListener>,
) -> shell::ipc::CallResult {
    use shell::ipc::{CallResult, Op};
    let target = match &c.op {
        Op::Query => return CallResult::Targets(ipc_targets(rt, &c.target)),
        Op::Kill => {
            rt.world.borrow_mut().quit.get_or_insert(0);
            return CallResult::Done(None);
        }
        _ => ipc_target_items(rt, &c.target).into_iter().next(),
    };
    let Some((h, t)) = target else {
        return CallResult::TargetNotFound;
    };
    match &c.op {
        Op::Prop => {
            let Some((name, ty)) = t.properties.iter().find(|p| p.0 == c.function) else {
                return CallResult::EntryNotFound;
            };
            let v = rt
                .world
                .borrow_mut()
                .read(h, name)
                .unwrap_or(Value::Undefined);
            CallResult::Value(ipc_value_string(ty, &v))
        }
        Op::Listen(stream) => {
            if !stream.alive() {
                return CallResult::Listening;
            }
            let Some((name, _, ty)) = t.signals.iter().find(|s| s.0 == c.function) else {
                return CallResult::EntryNotFound;
            };
            listeners.push(IpcListener {
                item: h,
                signal: name.clone(),
                ty: (!ty.is_empty()).then(|| ty.clone()),
                stream: stream.clone(),
            });
            rt.world.borrow_mut().ipc_taps.insert(h);
            // Quickshell's `quickshell.ipc` debug line: from here on the client gets emissions.
            tracing::debug!(
                target: "ri::ipc",
                "Remote listener created for {:?} {name:?}",
                c.target
            );
            CallResult::Listening
        }
        _ => {
            let Some(f) = t.functions.iter().find(|f| f.name == c.function) else {
                return CallResult::EntryNotFound;
            };
            if f.args.len() != c.args.len() {
                return CallResult::ArgParse(f.clone(), true, 0);
            }
            let mut args = Vec::with_capacity(c.args.len());
            for (i, (a, (_, ty))) in c.args.iter().zip(&f.args).enumerate() {
                match ipc_parse_arg(ty, a) {
                    Some(v) => args.push(v),
                    None => return CallResult::ArgParse(f.clone(), false, i as u8),
                }
            }
            match rt.call_method(h, &c.function, args) {
                None => CallResult::EntryNotFound,
                Some(Ok(_)) if f.ret == "void" => CallResult::Done(None),
                Some(Ok(v)) => CallResult::Done(Some(ipc_value_string(&f.ret, &v))),
                Some(Err(e)) => {
                    elog::eprintln!("ipc {}.{}: {e}", c.target, c.function);
                    CallResult::Error(e)
                }
            }
        }
    }
}

/// Forward tapped `IpcHandler` emissions to their remote listeners (`IpcSignalListener`:
/// "void" for no argument, else the argument as its IPC type). Listeners whose client went
/// away are dropped, and those of a destroyed handler get their connection closed, so taps
/// never outlive their last listener.
fn ipc_forward(rt: &qml::runtime::Runtime, listeners: &mut Vec<IpcListener>) {
    if listeners.is_empty() && rt.world.borrow().ipc_taps.is_empty() {
        return;
    }
    let fired = std::mem::take(&mut rt.world.borrow_mut().ipc_fired);
    for (item, sig, args) in fired {
        listeners.retain(|l| {
            if l.item != item || l.signal != sig {
                return true;
            }
            let v = match &l.ty {
                None => "void".to_string(),
                Some(ty) => ipc_value_string(ty, args.first().unwrap_or(&Value::Undefined)),
            };
            l.stream.send(&v)
        });
    }
    let mut w = rt.world.borrow_mut();
    listeners.retain(|l| {
        if w.dead.contains(&l.item) {
            l.stream.close();
            return false;
        }
        l.stream.alive()
    });
    let w = &mut *w;
    w.ipc_taps
        .retain(|h| listeners.iter().any(|l| l.item == *h));
}

/// PanelWindow `anchors` bits (top, bottom, left, right) as layer-shell anchors.
fn dump_clickables(rt: &qml::runtime::Runtime, root: usize, ns: &str, out: &mut String) {
    use std::fmt::Write;
    let mut stack = vec![(root, 0.0f64, 0.0f64, None::<[f64; 4]>)];
    while let Some((i, px, py, clip)) = stack.pop() {
        let (kids, types) = {
            let w = rt.world.borrow();
            (w.items[i].children.clone(), w.items[i].types.clone())
        };
        let vis = rt.get(i, "visible");
        if matches!(vis, Value::Bool(false)) || rt.get(i, "opacity").to_number() == 0.0 {
            continue;
        }
        let (x, y) = if i == root {
            (0.0, 0.0)
        } else {
            (
                px + rt.get(i, "x").to_number(),
                py + rt.get(i, "y").to_number(),
            )
        };
        if types.iter().any(|t| t == "MouseArea")
            && !matches!(rt.get(i, "enabled"), Value::Bool(false))
        {
            let (wd, ht) = (
                rt.get(i, "width").to_number(),
                rt.get(i, "height").to_number(),
            );
            // Visible part only: ancestors with `clip` (list views) hide the rest.
            let r = [x, y, x + wd, y + ht];
            let r = clip.map_or(r, |c: [f64; 4]| {
                [
                    r[0].max(c[0]),
                    r[1].max(c[1]),
                    r[2].min(c[2]),
                    r[3].min(c[3]),
                ]
            });
            let (x, y, wd, ht) = (r[0], r[1], r[2] - r[0], r[3] - r[1]);
            if wd > 0.0 && ht > 0.0 {
                let mut label = String::new();
                let mut chain = Vec::new();
                let mut cur = Some(i);
                while let Some(c) = cur {
                    let w = rt.world.borrow();
                    if let Some(t) = w.items[c].types.first() {
                        chain.push(*t);
                    }
                    cur = w.items[c].parent;
                    drop(w);
                    if label.is_empty() {
                        for k in ["buttonText", "text", "name", "buttonIcon", "toolTipText"] {
                            let v = rt.get(c, k);
                            if let Value::Str(s) = v
                                && !s.is_empty()
                            {
                                label = s.chars().take(40).collect();
                                break;
                            }
                        }
                    }
                    if chain.len() >= 4 {
                        break;
                    }
                }
                let _ = writeln!(
                    out,
                    "{ns}\t{i}\t{:.0}\t{:.0}\t{:.0}\t{:.0}\t{}\t{}",
                    x,
                    y,
                    wd,
                    ht,
                    qml::runtime::join_syms(&chain, "<"),
                    label.replace(['\t', '\n'], " ")
                );
            }
        }
        let clip = if i != root && rt.get(i, "clip").truthy() {
            let r = [
                x,
                y,
                x + rt.get(i, "width").to_number(),
                y + rt.get(i, "height").to_number(),
            ];
            Some(clip.map_or(r, |c: [f64; 4]| {
                [
                    r[0].max(c[0]),
                    r[1].max(c[1]),
                    r[2].min(c[2]),
                    r[3].min(c[3]),
                ]
            }))
        } else {
            clip
        };
        for c in kids {
            stack.push((c, x, y, clip));
        }
    }
}

fn dump_tree(rt: &qml::runtime::Runtime, item: usize, depth: usize, out: &mut String) {
    use std::fmt::Write;
    let (types, kids) = {
        let w = rt.world.borrow();
        (w.items[item].types.clone(), w.items[item].children.clone())
    };
    let g = |k: &str| rt.get(item, k).to_number();
    let inscene = {
        let mut w = rt.world.borrow_mut();
        let pv = w.items[item].props.contains_key("\x01propval");
        let par = w.items[item].parent;
        let vk = match par {
            Some(p) => scene::visual_kids(&mut w, p).contains(&item),
            None => true,
        };
        format!("{vk} p={par:?} pv={pv}")
    };
    let ci = rt.get(item, "contentItem");
    let _ = writeln!(
        out,
        "{:indent$}{item} {} x={:.1} y={:.1} w={:.1} h={:.1} iw={:.1} ih={:.1} vis={} op={:.2} c={} sc={inscene} ci={ci:?} {}",
        "",
        qml::runtime::join_syms(&types, "/"),
        g("x"),
        g("y"),
        g("width"),
        g("height"),
        g("implicitWidth"),
        g("implicitHeight"),
        rt.get(item, "visible").truthy(),
        g("opacity"),
        rt.get(item, "color").to_js_string(),
        rt.get(item, "text")
            .to_js_string()
            .chars()
            .take(30)
            .collect::<String>(),
        indent = depth * 2
    );
    for c in kids {
        dump_tree(rt, c, depth + 1, out);
    }
}

/// Type names a standalone app reaches from its file through the config's QML files. Qt creates
/// singletons on first use, so the rest (ConflictKiller, FirstRunExperience, ...) must not start.
fn app_singletons(
    root: &std::path::Path,
    app: &std::path::Path,
) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    let mut types: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "qml")
                && let Some(n) = p.file_stem().and_then(|n| n.to_str())
            {
                types.entry(n.to_string()).or_default().push(p);
            }
        }
    }
    let (mut used, mut seen) = (HashSet::new(), HashSet::new());
    let mut work = vec![app.to_path_buf()];
    while let Some(f) = work.pop() {
        if !seen.insert(f.clone()) {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        let (code, lits) = split_qml_literals(&text);
        // Page and Loader sources (`component: "modules/settings/QuickConfig.qml"`).
        for l in lits.iter().filter(|l| l.ends_with(".qml")) {
            let dir = f.parent().unwrap_or(root);
            if let Some(p) = [dir, root].iter().map(|b| b.join(l)).find(|p| p.is_file()) {
                work.push(p);
            }
        }
        for tok in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if tok.starts_with(|c: char| c.is_ascii_uppercase())
                && let Some(fs) = types.get(tok)
                && used.insert(tok.to_string())
            {
                work.extend(fs.iter().cloned());
            }
        }
    }
    used
}

/// QML source without comments and string literals (Qt only creates a singleton its code
/// touches; `Translation.tr("Brightness")` must not pull in `Brightness`), plus the literals.
/// Template literals stay in the code too: `${...}` holds expressions.
fn split_qml_literals(t: &str) -> (String, Vec<String>) {
    let (mut code, mut lits) = (String::with_capacity(t.len()), Vec::new());
    let mut it = t.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '/' if t[i..].starts_with("//") => {
                while it.peek().is_some_and(|&(_, c)| c != '\n') {
                    it.next();
                }
            }
            '/' if t[i..].starts_with("/*") => {
                it.next();
                let mut prev = ' ';
                for (_, c) in it.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
            }
            '"' | '\'' | '`' => {
                let start = i + 1;
                let mut end = t.len();
                while let Some((j, d)) = it.next() {
                    if d == '\\' {
                        it.next();
                    } else if d == c || (d == '\n' && c != '`') {
                        end = j;
                        break;
                    }
                }
                let lit = &t[start..end];
                if c == '`' {
                    code.push_str(lit);
                }
                code.push(' ');
                lits.push(lit.to_string());
            }
            _ => code.push(c),
        }
    }
    (code, lits)
}

/// How long the end of the process waits for the jobs that were refused a thread, when there
/// are any: as long as `qml::notifier::flush` gives a bus that does not answer.
const HELD: std::time::Duration = std::time::Duration::from_secs(2);

/// Writes out what the process image holds and its end (`exit`, or `Rustshell.reload()`'s
/// `exec`) would discard: Config.qml's delayed save of an option set just before, the
/// `HyprlandConfigFile` edits still queued for the `hyprconf` thread (the script they replace
/// ran detached and outlived the shell), the jobs that still wait for a thread the system
/// refused (a request to logind, to NetworkManager, to Hyprland: given [`HELD`] to run), the
/// notifications `Notify.send` has not put on the session bus yet (the `notify-send` they
/// replace outlived the shell too), and the log lines still queued for the `qs-log` thread.
fn flush_for_exit(world: &mut qml::runtime::World) {
    // One end for every service that may not answer: their waits do not add up.
    let until = std::time::Instant::now() + qml::notifier::FLUSH;
    qml::native::save_pending(world);
    qml::native::flush_detached();
    qml::bluez::flush_until(until);
    qml::hyprconf::flush();
    // Within the same end: a thread the system still refuses takes no time of its own.
    workpool::finish_held(HELD.min(until.saturating_duration_since(std::time::Instant::now())));
    qml::notifier::flush_until(until);
    shell::rilog::flush();
}

#[cfg(test)]
mod flush_tests {
    /// An edit queued right before the exit is on disk when `flush_for_exit` returns. The file
    /// is large, so the writer thread is still working on it when a caller that does not wait
    /// goes on to end the process.
    #[test]
    fn hyprconf_edit_queued_before_exit_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("custom.conf");
        let filler = "# a line of an existing hyprland config file\n".repeat(400_000);
        std::fs::write(&file, &filler).unwrap();
        qml::hyprconf::submit(qml::hyprconf::Edit {
            file: file.to_str().unwrap().to_owned(),
            set: vec![("general:gaps_in".into(), "7".into())],
            reset: Vec::new(),
        });
        let rt = qml::runtime::Runtime::new(None);
        super::flush_for_exit(&mut rt.world.borrow_mut());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.len() > filler.len(), "the edit was not written");
        assert!(
            text[filler.len()..].contains("gaps_in"),
            "{}",
            &text[filler.len()..]
        );
    }
}

/// Env marker telling the re-executed shell that it is the result of `Rustshell.reload()`.
const RELOADED_ENV: &str = "RI_RELOADED";

/// `Rustshell.reload()`: hand the `Process` children over and re-execute this binary with
/// the same command line, which loads the QML again from disk (a hard reload).
///
/// Nothing of the running shell goes before all that can refuse the next one has passed
/// ([`reload_refused`]): a refused reload leaves the tree as it is, with no
/// `Component.onDestruction`, and tells it `Rustshell.reloadFailed`, as Quickshell does for
/// a configuration that does not load. Then the handlers of the running tree run, as at an
/// exit, and `exec` follows.
fn reload_shell(
    root: &std::path::Path,
    app_file: Option<&std::path::Path>,
    app: &mut App,
    documents: &[usize],
    ipc: Option<&shell::ipc::Server>,
) {
    use std::os::unix::process::CommandExt;
    let App {
        rt,
        conn,
        shortcuts,
        shortcuts_rev,
        shortcuts_seen,
        wins,
        start_panels,
        cloaks,
        predecessor,
        ..
    } = app;
    // Quit (Qt.quit() or a termination signal read this turn) wins over a reload in the same
    // turn: the re-executed shell would not know it was asked to end.
    if rt.world.borrow().quit.is_some() {
        return;
    }
    // The path the shell was started as ($PREFIX/bin/impulse-shell), not /proc/self/exe: the reload
    // runs the release installed since and keeps the comm (`impulse-shell`) pidof/killall look for.
    let exe = qml::native::launch_path();
    let newer = std::fs::canonicalize(&exe).ok() != std::env::current_exe().ok();
    if let Some(why) = reload_refused(root, app_file, &exe, newer) {
        reload_failed(rt, &why);
        return;
    }
    let exec = |env: &[(&str, String)]| {
        // The windows stay on the screen until the next image has drawn its own
        // (`shell::reload::keep_surfaces`). The shortcuts go now: Hyprland ends a client
        // that registers one another client still has, and the next image registers these.
        for (sc, _) in shortcuts.values() {
            sc.destroy();
        }
        // So do the windows the next image does not start with (the sidebar the reload was
        // asked from, a menu, a popup): nothing of the next image takes their place, and
        // they would stay, frozen, until it has drawn. Should the `exec` fail, the next
        // sync makes them again.
        // A popup goes before the window it is on.
        wins.retain(|w| !matches!(w.surf, Surf::Popup { .. }));
        wins.retain(|w| match &w.surf {
            Surf::Panel { key, .. } => {
                let at_start = (start_panels.as_ref()).is_none_or(|s| s.contains(&key.namespace));
                !shell::reload::leaves_before_exec(at_start, key.geom.zone, key.layer)
            }
            Surf::Popup { .. } | Surf::Lock(_) | Surf::Toplevel(_) => true,
        });
        // A reload asked while the image before this one still has its windows on the
        // screen: those end at the `exec`, and this image's, kept unseen so far, are what
        // stays under the next one.
        if predecessor.is_some() {
            take_predecessors_place(cloaks, wins);
        }
        let _ = conn.flush();
        let surfaces = shell::reload::keep_surfaces(conn.backend().poll_fd());
        let kept = surfaces.as_ref().map(|fd| {
            let fd = std::os::fd::AsRawFd::as_raw_fd(fd).to_string();
            (shell::reload::SURFACES_ENV, fd)
        });
        // The IPC socket stays open too, with the instance's lock: a call made while the
        // process changes its image waits for the next one instead of finding no shell.
        let calls = ipc.and_then(|s| s.keep_across_exec());
        let named = [shell::shutdown::INHERITED_ENV, shell::rilog::INHERITED_ENV];
        let told = env.iter().filter(|(k, _)| named.contains(k));
        let keep: Vec<i32> = told
            .chain(&kept)
            .chain(&calls)
            .flat_map(|(_, fds)| fds.split(','))
            .filter_map(|fd| fd.parse().ok())
            .collect();
        shell::reload::close_on_exec_except(&shell::reload::open_descriptors(), &keep);
        let mut args = std::env::args_os();
        let arg0 = args.next().unwrap_or_default();
        let err = workpool::userenv::command(&exe)
            .arg0(arg0)
            .args(args)
            .env(RELOADED_ENV, shell::ipc::reload_marker())
            .envs(env.iter().map(|(k, v)| (k, v)))
            .envs(kept)
            .envs(calls)
            .exec();
        if let Some(s) = ipc {
            s.close_on_exec();
        }
        // The shell goes on: its shortcuts are registered again at the next sync.
        shortcuts.clear();
        *shortcuts_rev = None;
        *shortcuts_seen = Default::default();
        err
    };
    let refused = {
        let rt = std::cell::RefCell::new(&mut *rt);
        let flush = || {
            let mut rt = rt.borrow_mut();
            rt.destroy_at_exit(documents);
            flush_for_exit(&mut rt.world.borrow_mut());
        };
        let check_stale = !newer && cfg!(not(feature = "interp"));
        shell::reload::reload(root, app_file, check_stale, flush, exec)
    };
    match refused {
        shell::reload::Refused::Stale(differing) => {
            reload_failed(rt, &shell::reload::stale_text(root, &differing));
        }
        shell::reload::Refused::Exec(err) => {
            // The handlers ran; the tree is still there and goes on.
            rt.world.borrow_mut().exiting = false;
            reload_failed(rt, &format!("Cannot run {}: {err}", exe.display()));
        }
    }
}

/// Why a reload would leave no shell, found before anything of the running one goes: the
/// binary is not there to run, the binary installed since does not run the tree (a build
/// without the interpreter; its own tree it checks in [`shell::reload::reload`]), or no
/// compositor answers where the next image would connect.
fn reload_refused(
    root: &std::path::Path,
    app_file: Option<&std::path::Path>,
    exe: &std::path::Path,
    newer: bool,
) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let runs =
        std::fs::metadata(exe).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if !runs {
        return Some(format!("Cannot run {}: not a program", exe.display()));
    }
    if newer && cfg!(not(feature = "interp")) {
        // Another binary: it, not this one, says whether it runs the tree.
        let checked = workpool::userenv::command(exe)
            .arg("--check-tree")
            .arg("-p")
            .arg(app_file.unwrap_or(root))
            .stdin(std::process::Stdio::null())
            .output();
        if !checked.as_ref().is_ok_and(|o| o.status.success()) {
            let why = match &checked {
                Ok(o) => String::from_utf8_lossy(&o.stderr).into_owned(),
                Err(e) => format!("cannot run {}: {e}\n", exe.display()),
            };
            return Some(format!(
                "{why}The running shell keeps its current code.\nInstall again: {}",
                shell::reload::REBUILD_HINT
            ));
        }
    }
    // Quickshell reloads inside its process and keeps its connection; the next image opens
    // its own. A compositor that goes between this and the `exec` still ends the shell.
    if let Err(e) = Connection::connect_to_env() {
        return Some(format!(
            "Cannot connect to the Wayland compositor: {e}\nThe running shell keeps its \
             current code."
        ));
    }
    None
}

/// `Rustshell.reloadFailed(error)` to the running tree, which is what `ReloadPopup.qml`
/// shows. Twice: Quickshell emits it twice for one reload (`RootWrapper::reloadGraph`,
/// src/core/rootwrapper.cpp, has the `emit` before and after it decides on its own popup).
fn reload_failed(rt: &mut qml::runtime::Runtime, error: &str) {
    for line in error.lines() {
        elog::eprintln!("ERROR: {line}");
    }
    shell::rilog::log(shell::rilog::Level::Critical, "default", error);
    let qs = rt.world.borrow().singletons.get("Rustshell").copied();
    if let Some(qs) = qs {
        for _ in 0..2 {
            rt.emit(qs, "reloadFailed", &["error"], vec![Value::str(error)]);
        }
        rt.settle();
    }
}

// The shipped shell (a release build without `interp`) has no JavaScript interpreter. Cargo
// unifies features across the packages of one invocation, so `cargo build --release` of the
// workspace, or `-p shell` next to a package that enables `qml/interp` (rigen, aotcheck), would
// otherwise produce an `impulse-shell` with the interpreter that looks like the release one. Tooling
// builds that want it ask for it: `--features shell/interp`.
#[cfg(all(not(feature = "interp"), not(debug_assertions)))]
const _: () = assert!(
    !qml::INTERP,
    "qml/interp is enabled by feature unification: build the release shell alone (`cargo build \
     --release -p shell`) or ask for the interpreter (`--features shell/interp`)"
);

// The shipped shell names the commit it was built from (`impulse-shell --version`), and a build
// of that commit is the same file wherever it is made: tools/build.sh names the commit and
// keeps this machine's directories out of the binary. A build that cannot tell its commit
// does not compile, rather than say `commit unknown`.
#[cfg(all(not(feature = "interp"), not(debug_assertions)))]
const _: () = assert!(
    !env!(
        "RI_COMMIT",
        "the shipped impulse-shell is built with tools/build.sh (tools/install.sh runs it): it \
         names the commit (RI_COMMIT) and keeps this machine's directories out of the binary"
    )
    .is_empty(),
    "RI_COMMIT is empty: the shipped impulse-shell is built with tools/build.sh"
);

/// A build without the interpreter cannot run scripts that differ from the compiled QML tree,
/// nor check a changed file it cannot read or parse: rather than start with those bindings and
/// handlers missing, say which files differ and exit.
#[cfg(not(feature = "interp"))]
fn refuse_stale_tree(
    root: &std::path::Path,
    app_file: Option<&std::path::Path>,
) -> Result<(), Fatal> {
    let stale = shell::reload::differing_tree(root, app_file);
    if stale.is_empty() {
        return Ok(());
    }
    elog::eprintln!(
        "impulse-shell: {} differs from the tree this binary was compiled from and this build has no \
         interpreter to run the changed code:",
        root.display()
    );
    for s in &stale {
        elog::eprintln!("  {s}");
    }
    elog::eprintln!(
        "impulse-shell: install again: {}",
        shell::reload::REBUILD_HINT
    );
    Err(Fatal(None))
}

/// A startup failure the user can act on (a refused tree, no compositor, a compositor
/// without layer-shell): [`run`] returns it, which drops the instance registration, and
/// [`main`] prints it and exits with status 1 instead of a panic and backtrace.
struct Fatal(Option<String>);

fn fatal<E: std::fmt::Display>(what: &str) -> impl FnOnce(E) -> Fatal + '_ {
    move |e| Fatal(Some(format!("impulse-shell: {what}: {e}")))
}

/// The environment pragmas in the root file's header, as Quickshell's `launch()`
/// (src/launch/launch.cpp) applies them to the shell's own environment: `QT_STYLE_OVERRIDE`
/// is removed and `QT_QUICK_CONTROLS_STYLE` set to `Fusion` unless the file has
/// `//@ pragma RespectSystemStyle`, then `//@ pragma DefaultEnv NAME=VALUE` sets each variable
/// the environment lacks, then `//@ pragma Env NAME=VALUE` sets its variables. The
/// environment as it was before is kept as the user's ([`workpool::userenv`]): the programs
/// the shell starts get that one, so none of this reaches them.
/// Its `//@ pragma NativeTextRendering` makes native the render type of texts that set none.
fn apply_env_pragmas(f: &std::path::Path) {
    debug_assert!(
        shell::shutdown::single_threaded(),
        "env pragmas must be applied before any thread starts"
    );
    workpool::userenv::capture();
    let pragmas = std::fs::read_to_string(f)
        .map(|src| shell::pragmas::Env::parse(&src))
        .unwrap_or_default();
    if pragmas.native_text {
        shell::scene::set_native_text_rendering(true);
    }
    for (name, value) in pragmas.changes(|name| std::env::var_os(name).is_some()) {
        #[expect(
            clippy::disallowed_methods,
            reason = "before the threads that read the environment start"
        )]
        match value {
            // SAFETY: called before any thread starts (asserted above): `run` applies the
            // pragmas right after parsing the command line, before jemalloc's purging
            // thread, logging, IPC or the XWayland watcher start their threads.
            Some(value) => unsafe { std::env::set_var(name, value) },
            // SAFETY: as above, no other thread exists yet.
            None => unsafe { std::env::remove_var(name) },
        }
    }
}

/// Wakes the main loop when the Wayland reader queues events for the shell.
struct PingWaker(calloop::ping::Ping);

impl std::task::Wake for PingWaker {
    fn wake(self: Arc<Self>) {
        self.0.ping();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.ping();
    }
}

struct Unpark(std::thread::Thread);

impl std::task::Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// The shell is ending with an `unlock_and_destroy` the compositor has not answered: waits for
/// the answers to the syncs `App::end_lock` made (ext-session-lock-v1: a client that exits
/// right after unlocking "must use the wl_display.sync request to ensure the server receives
/// and processes the unlock_and_destroy request"; a compositor that finds the client gone
/// first keeps the session locked). Nothing else of the shell runs meanwhile. Returns why the
/// connection broke, if it did before they came.
fn await_unlocks(
    app: &mut App,
    queue: &mut wayland_client::EventQueue<App>,
    conn: &Connection,
    failure: &shell::wlread::Failure,
) -> Option<String> {
    if qml::log_on!("ri::sched") {
        tracing::debug!(target: "ri::sched", "the exit waits for the unlock");
    }
    let waker = std::task::Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    loop {
        match conn.flush() {
            Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // The socket buffer is full: go on once it drains.
                let mut fd = libc::pollfd {
                    fd: std::os::fd::AsRawFd::as_raw_fd(&conn.backend().poll_fd()),
                    events: libc::POLLOUT,
                    revents: 0,
                };
                // SAFETY: one valid pollfd; `conn` keeps the socket open.
                unsafe { libc::poll(&mut fd, 1, -1) };
                continue;
            }
            Err(e) => return Some(e.to_string()),
            Ok(()) => {}
        }
        // Registers the waker: the reader thread wakes it for the shell's events.
        if let std::task::Poll::Ready(Err(e)) = queue.poll_dispatch_pending(&mut cx, app) {
            return Some(e.to_string());
        }
        if app.unlocks_in_flight == 0 {
            return None;
        }
        if let Some(e) = failure.lock().unwrap_or_else(|p| p.into_inner()).take() {
            return Some(e.to_string());
        }
        std::thread::park();
    }
}

/// One-shot calloop source that wakes the loop once the Wayland socket is writable again.
fn watch_writable(
    handle: &calloop::LoopHandle<'static, App>,
    conn: &Connection,
    pending: &std::rc::Rc<std::cell::Cell<bool>>,
) -> Result<(), String> {
    let fd = conn
        .backend()
        .poll_fd()
        .try_clone_to_owned()
        .map_err(|e| e.to_string())?;
    let source =
        calloop::generic::Generic::new(fd, calloop::Interest::WRITE, calloop::Mode::OneShot);
    let done = pending.clone();
    handle
        .insert_source(source, move |_, _, _| {
            done.set(false);
            Ok(calloop::PostAction::Remove)
        })
        .map(drop)
        .map_err(|e| e.to_string())
}

fn kb_interactivity(mode: u32) -> KeyboardInteractivity {
    match mode {
        1 => KeyboardInteractivity::Exclusive,
        2 => KeyboardInteractivity::OnDemand,
        _ => KeyboardInteractivity::None,
    }
}

thread_local! {
    /// The warnings of the engine printed so far (bounded).
    static WARNED: std::cell::RefCell<std::collections::HashSet<String>> = Default::default();
}

/// Prints a warning of the engine inside the call that raised it (`qml::warning::SINK`), so it
/// stands among the `console:` lines where Qt prints it; each distinct line once.
fn log_warning(line: &str) {
    let fresh = WARNED.with_borrow_mut(|said| {
        if said.len() > 4096 {
            said.clear();
        }
        said.insert(line.to_owned())
    });
    if fresh {
        elog::eprintln!("qml: {line}");
    }
}

/// Gives the C library's `stderr` a line buffer, so a line a C library prints there
/// (libwayland's WAYLAND_DEBUG, Mesa) reaches the descriptor whole, in one write(2), as the
/// shell's own lines do (elog). Unbuffered, as it starts, glibc writes such a line in pieces
/// of 128 bytes, and a line another thread writes meanwhile lands between two of them.
///
/// The buffer is the shell's: the one the library allocates is 4096 bytes on a pipe, and a
/// longer line would go out in two writes. A Wayland message is at most 4096 bytes, so its
/// WAYLAND_DEBUG line, the message's text behind a time and a name, can be longer than that.
fn line_buffer_c_stderr() {
    const LINE: usize = 1 << 16;
    unsafe extern "C" {
        #[link_name = "stderr"]
        static C_STDERR: *mut libc::FILE;
    }
    // Never freed: the stream uses it until the process ends.
    let buffer = Box::leak(vec![0u8; LINE].into_boxed_slice());
    // SAFETY: the C library's `stderr`, set before `main` and assigned by nothing since.
    let stderr = unsafe { C_STDERR };
    // SAFETY: setvbuf(3) on an open stream, before any thread that could be writing to it,
    // with a buffer of `LINE` bytes that lives as long as the stream.
    unsafe { libc::setvbuf(stderr, buffer.as_mut_ptr().cast(), libc::_IOLBF, LINE) };
}

fn main() {
    line_buffer_c_stderr();
    shell::panics::log_whole();
    let Err(Fatal(msg)) = run();
    if let Some(msg) = msg {
        elog::eprintln!("{msg}");
        // In the instance log too: `impulse-shell log` shows why this instance did not start.
        let reason = msg.strip_prefix("impulse-shell: ").unwrap_or(&msg);
        shell::rilog::log(shell::rilog::Level::Critical, "default", reason);
    }
    // `exit` ends the `qs-log` thread wherever it is: wait until it wrote what is queued.
    shell::rilog::close();
    std::process::exit(1);
}

/// Makes the main loop watch `fd`, the runtime's epoll set of the `Process` pipes and pidfds,
/// through a descriptor of its own. Without it nothing wakes the loop for a child's output or
/// exit, so a failure refuses the start, as for the loop's other sources.
fn watch_process_fd(
    handle: &calloop::LoopHandle<'static, App>,
    fd: std::os::fd::RawFd,
) -> Result<(), Fatal> {
    use std::os::fd::{FromRawFd, OwnedFd};
    // SAFETY: F_DUPFD_CLOEXEC on a descriptor only creates a new one, or fails.
    let dup = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if dup < 0 {
        return Err(fatal("cannot watch child processes")(
            std::io::Error::last_os_error(),
        ));
    }
    let src = calloop::generic::Generic::new(
        // SAFETY: `dup` is the open fd fcntl just created; nothing else owns it.
        unsafe { OwnedFd::from_raw_fd(dup) },
        calloop::Interest::READ,
        calloop::Mode::Level,
    );
    handle
        .insert_source(src, |_, _, app: &mut App| {
            app.rt.process_fd_ready();
            app.pinged = true;
            Ok(calloop::PostAction::Continue)
        })
        .map(drop)
        .map_err(fatal("cannot watch child processes"))
}

fn run() -> Result<std::convert::Infallible, Fatal> {
    shell::shutdown::default_sigchld();
    qml::native::install_shutdown_handlers();
    // Fixed now, while the path the shell was started as still leads to this binary.
    qml::native::launch_path();
    // Set by the image that re-executed itself into this one: this image goes on as its
    // instance, as a Quickshell reload stays the same instance.
    let predecessor = std::env::var(RELOADED_ENV).ok();
    let reloaded = predecessor.is_some();
    // Its IPC socket and instance lock, open across the exec.
    let inherited_ipc = shell::ipc::inherited();
    let resumed = predecessor
        .is_some_and(|marker| shell::ipc::resume_instance(&marker, inherited_ipc.as_ref()));
    #[expect(
        clippy::disallowed_methods,
        reason = "before the threads that read the environment start"
    )]
    // SAFETY: no other thread exists yet.
    unsafe {
        std::env::remove_var(RELOADED_ENV)
    };
    // A reload's exec left SIGTERM/SIGINT/SIGHUP blocked (see `shell::shutdown`): one the
    // previous image caught and did not read, or one sent since, ends this image below.
    let terminated_at_exec = reloaded.then(shell::shutdown::pending_after_exec).flatten();
    let handed_over = std::env::var(qml::native::HANDED_OVER_ENV).unwrap_or_default();
    // The previous image's windows, on the screen until this image has drawn its own.
    let predecessor_surfaces = std::env::var(shell::reload::SURFACES_ENV)
        .ok()
        .and_then(|fd| shell::reload::inherited_surfaces(&fd))
        .map(|fd| (fd, None));
    // The previous image's log file, open and locked across the exec.
    let inherited_log = shell::rilog::inherited();
    for var in [
        qml::native::HANDED_OVER_ENV,
        shell::reload::SURFACES_ENV,
        shell::shutdown::INHERITED_ENV,
        shell::rilog::INHERITED_ENV,
        shell::ipc::INHERITED_ENV,
    ] {
        #[expect(
            clippy::disallowed_methods,
            reason = "before the threads that read the environment start"
        )]
        // SAFETY: no other thread exists yet.
        unsafe {
            std::env::remove_var(var)
        };
    }
    // Taken before this image starts a child; reaped once threads may start (the reaper is one).
    let inherited = if reloaded {
        qml::native::inherited_children()
    } else {
        Vec::new()
    };
    // The previous image's `Process` groups end here, as their destruction does on a
    // Quickshell reload.
    qml::native::end_handed_over(&shell::reload::handed_over(&handed_over), &inherited);
    if let Some(sig) = terminated_at_exec {
        // The previous image's exec unregistered nothing: the instance this image goes on as
        // leaves here, as at an orderly exit.
        shell::shutdown::log_ending(sig);
        if resumed {
            shell::ipc::remove_resumed_instance();
        }
        shell::shutdown::exit_by(sig);
    }
    if reloaded {
        // Before any thread or child, which would inherit the blocked mask. From here to the
        // main loop a termination signal ends the shell at once, as during any startup.
        shell::shutdown::unblock();
    }
    qml::debug::init_log();
    qml::debug::log_code_errors();
    let (root, app_file) = shell::ricli::dispatch();
    workpool::require_pidfd().map_err(|e| Fatal(Some(e)))?;
    apply_env_pragmas(&app_file.clone().unwrap_or_else(|| root.join("shell.qml")));
    shell::heap::start_background_thread();
    qml::native::reap_inherited(inherited);
    if app_file.is_none() {
        shell::ipc::start_logging(&root.join("shell.qml"), resumed, inherited_log);
    }
    // Registered before anything slow (the tree check, the Wayland connection, the GPU, the
    // QML), as Quickshell does: `qs ipc call` right after the launch reaches this instance and
    // waits for its answer. `qs -p app.qml` registers no IPC instance of its own that the original uses.
    let mut ipc = if app_file.is_some() {
        None
    } else {
        shell::ipc::start(&root.join("shell.qml"), inherited_ipc.filter(|_| resumed))
            .inspect_err(|e| elog::eprintln!("ipc: could not register quickshell instance: {e}"))
            .ok()
    };
    #[cfg(not(feature = "interp"))]
    refuse_stale_tree(&root, app_file.as_deref())?;
    // A shell that was killed left its marker: no such shell holds the session lock.
    drop_lock_marker_of(|pid| !shell::shutdown::process_exists(pid));
    let conn =
        Connection::connect_to_env().map_err(fatal("cannot connect to the Wayland compositor"))?;
    let (globals, mut queue) =
        registry_queue_init::<App>(&conn).map_err(fatal("cannot read the Wayland globals"))?;
    let qh = queue.handle();
    {
        // The clipboard source lives on this connection and queue, with the data device.
        let (ext, wlr) = globals.contents().with_list(|l| {
            let name = |i: &str| l.iter().find(|g| g.interface == i).map(|g| g.name);
            (
                name("ext_data_control_manager_v1"),
                name("zwlr_data_control_manager_v1"),
            )
        });
        qml::ports::clipsrc::host(&conn, &qh, globals.registry(), ext, wlr);
    }
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(fatal("the compositor has no wl_compositor"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(fatal(
        "the compositor does not support wlr-layer-shell (zwlr_layer_shell_v1)",
    ))?;
    let shm = Shm::bind(&globals, &qh).map_err(fatal("the compositor has no wl_shm"))?;
    let pool =
        SlotPool::new(1920 * 64 * 4, &shm).map_err(fatal("cannot create a shared memory pool"))?;
    let _ = qml::warning::SINK.set(log_warning);
    let mut rt = qml::runtime::Runtime::new(Some(root.clone()));
    rt.world
        .borrow_mut()
        .open_layout(qml::layout_model::default_path());
    rt.world.borrow_mut().track_fresh = !qml::debug::on(qml::debug::Switch::NOFRESH);
    rt.world.borrow_mut().frame_paced = !qml::debug::on(qml::debug::Switch::NOPOLISH);
    rt.world.borrow_mut().posted_yield = true;
    rt.world.borrow_mut().started = Some(Vec::new());
    if !cfg!(feature = "interp") || !shell::env_flag!("RI_INTERP") {
        rt.eng()
            .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&root))));
    }
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().pre_complete = Some(Box::new(move |w, n0| {
            fonts
                .try_borrow_mut()
                .is_ok_and(|mut f| scene::measure_new(w, &mut f, n0))
        }));
    }
    scene::loaded_layout_hook(&mut rt.world.borrow_mut(), &fonts);
    scene::image_hook(&mut rt.world.borrow_mut(), &fonts);
    shell::canvas2d::install();
    scene::text_hooks(&mut rt.world.borrow_mut(), &fonts);
    {
        let fonts = fonts.clone();
        rt.world.borrow_mut().popup_measure = Some(Box::new(move |w, i| {
            if let Ok(mut f) = fonts.try_borrow_mut() {
                scene::measure_popup(w, &mut f, i);
            }
        }));
    }
    let mut singleton_dirs = vec![
        root.join("modules/common"),
        root.join("modules/common/functions"),
        root.join("modules/common/utils"),
        root.join("services"),
        root.clone(),
    ];
    // Module-local singletons (e.g. overlay/OverlayContext.qml, waffle/looks/Looks.qml).
    for fam in ["modules/rusty", "modules/waffle"] {
        if let Ok(rd) = std::fs::read_dir(root.join(fam)) {
            let mut d: Vec<_> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            d.sort();
            singleton_dirs.extend(d);
        }
    }
    match &app_file {
        Some(f) => {
            let used = app_singletons(&root, f);
            rt.load_singletons_where(&singleton_dirs, &|n| used.contains(n));
        }
        None => rt.load_singletons_lazy(&singleton_dirs),
    }
    // Declared before `app` so it (and the wayland connection it owns) drops after it:
    // EGL surfaces must be destroyed while the display is still connected.
    let mut ev: EventLoop<App> =
        EventLoop::try_new().map_err(fatal("cannot create the event loop"))?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        shm,
        pool,
        factors: shell::hidpi::Factors::from_env(),
        gpu: if std::env::var("RI_RENDER").as_deref() == Ok("cpu") {
            None
        } else {
            // SAFETY: `conn` is declared before `app` and dropped after it.
            unsafe { shell::gpu::Gpu::new(conn.backend().display_ptr() as *mut _) }
        },
        rt,
        fonts: fonts.clone(),
        wins: Vec::new(),
        start: Instant::now(),
        clock: AnimClock::new(),
        prelaid: std::collections::HashSet::new(),
        prelaid_at: None,
        prelayout_idle: false,
        clean: std::collections::HashMap::new(),
        parked: Default::default(),
        stale_hidden: std::collections::HashMap::new(),
        popup_refused: std::collections::HashSet::new(),
        retiring: Vec::new(),
        off_screen: Vec::new(),
        unshown: Vec::new(),
        unshown_ids: 0,
        closing: Vec::new(),
        hypr_close: Default::default(),
        self_ipc: Vec::new(),
        roots: std::collections::HashSet::new(),
        frames: 0,
        seat: SeatState::new(&globals, &qh),
        pointer: None,
        ptr_focus: None,
        ptr: Pointer::default(),
        axis_merge: AxisCompression::default(),
        motion_merge: Default::default(),
        frame_time: None,
        motion_unanswered: false,
        motion_this_frame: false,
        frame_at: Instant::now(),
        motion_early: false,
        frame_has_motion: false,
        motion_cost: 2.0,
        motion_took: 0.0,
        ptr_frames: Default::default(),
        anim_attr: Default::default(),
        wheel_deltas: shell::input::WheelDeltas::default(),
        touch: None,
        touch_win: None,
        last_rev: u64::MAX,
        seen_bind: Default::default(),
        output_infos: Default::default(),
        sync_key: (u64::MAX, 0, 0, None, 0),
        outputs_gen: 0,
        head_mgr: globals
            .bind::<ZwlrOutputManagerV1, _, _>(&qh, 2..=4, ())
            .ok(),
        heads: Default::default(),
        caret: None,
        quant: Default::default(),
        grab_mgr: globals
            .bind::<HyprlandFocusGrabManagerV1, _, _>(&qh, 1..=1, ())
            .ok(),
        grabs: Default::default(),
        grab_cleared: Vec::new(),
        grab_flushed: false,
        late_close: Vec::new(),
        frac_mgr: globals
            .bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ())
            .ok(),
        viewporter: globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok(),
        presentation: globals.bind::<WpPresentation, _, _>(&qh, 1..=1, ()).ok(),
        output_power: globals
            .bind::<ZwlrOutputPowerManagerV1, _, _>(&qh, 1..=1, ())
            .ok(),
        output_powers: Default::default(),
        sight: Sight::default(),
        veil_mgr: globals
            .bind::<wl_subcompositor::WlSubcompositor, _, _>(&qh, 1..=1, ())
            .ok()
            .zip(globals.bind::<WpAlphaModifierV1, _, _>(&qh, 1..=1, ()).ok())
            .map(|(sub, alpha)| VeilMgr { sub, alpha }),
        idle_mgr: globals
            .bind::<ZwpIdleInhibitManagerV1, _, _>(&qh, 1..=1, ())
            .ok(),
        ti_mgr: globals
            .bind::<ZwpTextInputManagerV3, _, _>(&qh, 1..=1, ())
            .ok(),
        ti: None,
        ti_surface: None,
        ti_enabled: false,
        ti_sent: None,
        ti_key: None,
        ti_pending: TextInputPending::default(),
        ti_reset: false,
        im_key: None,
        inhibitors: Default::default(),
        predecessor: predecessor_surfaces,
        cloaks: Vec::new(),
        start_panels: None,
        shortcuts_rev: None,
        shortcuts_seen: Default::default(),
        shortcut_mgr: globals
            .bind::<HyprlandGlobalShortcutsManagerV1, _, _>(&qh, 1..=1, ())
            .ok(),
        export_mgr: globals
            .bind::<HyprlandToplevelExportManagerV1, _, _>(&qh, 1..=2, ())
            .ok(),
        dmabuf: if qml::debug::on(qml::debug::Switch::NODMABUF) {
            None
        } else {
            globals.bind::<ZwpLinuxDmabufV1, _, _>(&qh, 3..=5, ()).ok()
        },
        dma_feedback: None,
        dma_params: Default::default(),
        dma_params_next: Default::default(),
        dma_epoch: 0,
        gbm: None,
        dma_off: false,
        dma_ok: false,
        dma_keep: Default::default(),
        dma_grave: Vec::new(),
        dma_ready: Vec::new(),
        dma_wait: Vec::new(),
        pinged: true,
        unlocks_in_flight: 0,
        sc: None,
        due_at: 0.0,
        poll_due: 0.0,
        owed_rev: u64::MAX,
        animating: false,
        poll_rev: u64::MAX,
        panels_made: 0,
        cap_grave: Vec::new(),
        frames_unanswered: Unanswered(Vec::new()),
        caps: Default::default(),
        screen_caps: Default::default(),
        screen_cap_cursor: Default::default(),
        cap_cache: Default::default(),
        cap_seq: 0,
        shortcuts: Default::default(),
        keyboard: None,
        kbd: Default::default(),
        kb_modes: Default::default(),
        xkb: Default::default(),
        key_repeat: shell::keys::KeyRepeat::default(),
        held_syms: std::collections::HashMap::new(),
        key_serial: 0,
        primary: PrimarySelectionManagerState::bind(&globals, &qh).ok(),
        primary_dev: None,
        primary_src: None,
        data_mgr: DataDeviceManagerState::bind(&globals, &qh).ok(),
        data_dev: None,
        paste: shell::selection::PasteRead::new(ev.handle()),
        script_paste: shell::selection::PasteRead::new(ev.handle()),
        offer_text: Default::default(),
        offer_read: shell::selection::PasteRead::new(ev.handle()),
        clip_writes: Default::default(),
        loop_handle: ev.handle(),
        cur_shape: -1,
        cursor_missing: false,
        xdg: XdgShell::bind(&globals, &qh).map_err(fatal("the compositor has no xdg_wm_base"))?,
        layer_shell,
        compositor,
        lock_state: SessionLockState::new(&globals, &qh),
        session: None,
        lock_denied: None,
        lock_serial: 0,
        kbd_act: Default::default(),
        qh: qh.clone(),
        conn: conn.clone(),
        display: conn.display(),
        started: false,
    };
    {
        // A script's `paste()` inserts inside the call unless an earlier one still waits for
        // its text: the calls insert in call order.
        let reading = app.script_paste.busy();
        let offer = app.offer_text.clone();
        app.rt.world.borrow_mut().paste_now = Some(std::rc::Rc::new(move |rt, item| {
            let offer = offer.borrow().clone();
            !reading() && shell::keys::paste_in_call(rt, item, offer)
        }));
    }
    app.ptr.fonts = app.fonts.clone();
    app.kbd.fonts = app.fonts.clone();
    // Capture buffers are no surface's content, so the default feedback (not a surface's).
    app.dma_feedback = app
        .dmabuf
        .as_ref()
        .filter(|d| d.version() >= 4)
        .map(|d| d.get_default_feedback(&qh, ()));
    queue
        .roundtrip(&mut app)
        .map_err(fatal("lost the Wayland connection at startup"))?;
    let outs: Vec<(wl_output::WlOutput, String, i32, i32, i32, i32, f64)> = app
        .outputs
        .outputs()
        .filter_map(|o| {
            let (name, x, y, w, h, dpr) =
                output_screen(&app.outputs.info(&o)?, |n| app.screen_factor(Some(n)))?;
            Some((o, name, x, y, w, h, dpr))
        })
        .collect();
    let screens: Vec<_> = outs
        .iter()
        .map(|o| (o.1.clone(), o.2, o.3, o.4, o.5, o.6))
        .collect();
    for seat in app.seat.seats() {
        app.bind_selection_devices(&qh, &seat);
    }
    app.rt.set_screens_dpr(&screens);
    app.publish_outputs(None);
    if let Some(v) = primary_vsync(&app.outputs) {
        app.clock.vsync = v;
    }
    // shell.qml with its PanelFamilyLoaders, like `qs -c impulse-shell`; RI_MODULES=a.qml,b.qml loads only
    // those modules/rusty files plus the ShellRoot (Component.onCompleted, IpcHandler, GlobalShortcut).
    qml::native::hold_app_windows();
    // The root objects of the documents loaded, for `Component.onDestruction` at the exit.
    let mut documents = vec![];
    let loaded = match (&app_file, std::env::var("RI_MODULES")) {
        (Some(f), _) => app.rt.load_file(f).map(|r| documents.push(r)),
        (None, Ok(modules)) => {
            for m in modules.split(',').filter(|m| !m.is_empty()) {
                match app.rt.load_file(&root.join("modules/rusty").join(m)) {
                    Ok(r) => documents.push(r),
                    Err(e) => elog::eprintln!("load {m}: {e}"),
                }
            }
            app.rt
                .load_file_without(&root.join("shell.qml"), &["PanelFamilyLoader"])
                .map(|r| documents.push(r))
        }
        (None, Err(_)) => app
            .rt
            .load_file(&root.join("shell.qml"))
            .map(|r| documents.push(r)),
    };
    if let Err(e) = loaded {
        let f = app_file.clone().unwrap_or_else(|| root.join("shell.qml"));
        elog::eprintln!("load {}: {e}", f.display());
        if app_file.is_some() {
            std::process::exit(255);
        }
    } else {
        shell::rilog::log(shell::rilog::Level::Info, "default", "Configuration Loaded");
    }
    if reloaded {
        let qs = app.rt.world.borrow().singletons.get("Rustshell").copied();
        if let Some(qs) = qs {
            app.rt.emit(qs, "reloadCompleted", &[], vec![]);
        }
    }
    let mut ipc_listeners: Vec<IpcListener> = Vec::new();
    quantize_colors(&mut app);
    // An app's windows exist (and are laid out) before the first Process output is read.
    if app_file.is_some() {
        app.sync_toplevels();
    }
    // Until quiescent: what a poll brings in runs handlers and bindings whose writes the next
    // poll serves (a Process whose output starts another), all before the first frame.
    loop {
        let rev = app.rt.revision();
        app.run_window_methods();
        if !app.rt.poll_native() && app.rt.revision() == rev {
            break;
        }
    }
    app.sync_toplevels();
    app.started = true;
    app.log_errors();
    let (wl_ping, wl_ping_src) = smithay_client_toolkit::reexports::calloop::ping::make_ping()
        .map_err(fatal("cannot create the Wayland wakeup source"))?;
    ev.handle()
        .insert_source(wl_ping_src, |_, _, _: &mut App| {})
        .map_err(fatal("cannot watch the Wayland connection"))?;
    let wl_waker = std::task::Waker::from(Arc::new(PingWaker(wl_ping.clone())));
    let main_thread = std::thread::current();
    let wl_failure = shell::wlread::spawn(conn.clone(), move || {
        wl_ping.ping();
        // `await_unlocks` parks instead of running the loop.
        main_thread.unpark();
    })
    .map_err(fatal("cannot start the Wayland reader"))?;
    match &app.gpu {
        Some(g) => elog::eprintln!("renderer: GPU (GLES2, {})", g.renderer),
        None => elog::eprintln!("renderer: CPU raster (wl_shm)"),
    }
    let (ping, ping_src) = smithay_client_toolkit::reexports::calloop::ping::make_ping()
        .map_err(fatal("cannot create the wakeup source"))?;
    app.rt.world.borrow().waker.set_ping(move || ping.ping());
    // IPC clients: one epoll fd for the socket and every connection, served below.
    let ipc_ready = std::rc::Rc::new(std::cell::Cell::new(true));
    if let Some(ipc) = &ipc {
        let fd = ipc
            .fd()
            .try_clone_to_owned()
            .map_err(fatal("cannot watch the IPC socket"))?;
        let src = calloop::generic::Generic::new(fd, calloop::Interest::READ, calloop::Mode::Level);
        let ready = ipc_ready.clone();
        ev.handle()
            .insert_source(src, move |_, _, _: &mut App| {
                ready.set(true);
                Ok(calloop::PostAction::Continue)
            })
            .map_err(fatal("cannot watch the IPC socket"))?;
    }
    ev.handle()
        .insert_source(ping_src, |_, _, app: &mut App| app.pinged = true)
        .map_err(fatal("cannot watch the wakeup source"))?;
    // SIGTERM/SIGINT/SIGHUP end the loop like Qt.quit(); the shell then exits through the same
    // orderly path and dies by the signal. Caught only from here, where the loop reads them:
    // startup blocks on the compositor's answers, and a signal that only waited in the pipe
    // meanwhile would not stop a shell whose compositor never answers. Until here one ends
    // the shell at once (`qml::native::install_shutdown_handlers` ends its `Process` groups).
    let terminated = std::rc::Rc::new(std::cell::Cell::new(None::<libc::c_int>));
    {
        let signals = shell::shutdown::SignalPipe::install()
            .map_err(fatal("cannot install signal handlers"))?;
        let fd = signals
            .fd()
            .try_clone()
            .map_err(fatal("cannot watch termination signals"))?;
        let src = calloop::generic::Generic::new(fd, calloop::Interest::READ, calloop::Mode::Level);
        let terminated = terminated.clone();
        ev.handle()
            .insert_source(src, move |_, _, app: &mut App| {
                while let Some(sig) = signals.read() {
                    // Said before the shutdown starts: a shutdown that hangs is then in the
                    // log as one a signal asked for.
                    if terminated.replace(Some(sig)).is_none() {
                        shell::shutdown::log_ending(sig);
                    }
                    app.rt.world.borrow_mut().quit.get_or_insert(0);
                }
                Ok(calloop::PostAction::Continue)
            })
            .map_err(fatal("cannot watch termination signals"))?;
    }
    // Hyprland's close animations: its settings from the thread that reads them, and what it
    // closed from its event socket.
    {
        let (tx, settings) = calloop::channel::channel();
        ev.handle()
            .insert_source(settings, |ev, _, app: &mut App| {
                if let calloop::channel::Event::Msg(reading) = ev {
                    app.hypr_close.settings(reading);
                }
            })
            .map_err(|e| fatal("cannot watch Hyprland's settings")(e.error))?;
        let dir = qml::hypr::socket_dir();
        app.hypr_close = shell::hyprclose::Watch::start(dir.as_deref(), move |reading| {
            let _ = tx.send(reading);
        });
        if let Some(fd) = app.hypr_close.fd() {
            let src =
                calloop::generic::Generic::new(fd, calloop::Interest::READ, calloop::Mode::Level);
            ev.handle()
                .insert_source(src, |_, _, app: &mut App| {
                    app.hypr_closed();
                    Ok(match app.hypr_close.alive() {
                        true => calloop::PostAction::Continue,
                        false => calloop::PostAction::Remove,
                    })
                })
                .map_err(fatal("cannot watch Hyprland's event socket"))?;
        }
    }
    // Process stdout/stderr/exit: one epoll fd for every child, read at the service poll.
    if let Some(fd) = app.rt.process_fd() {
        watch_process_fd(&ev.handle(), fd)?;
    }
    let frame = move |app: &mut App| -> bool {
        let now = app.start.elapsed().as_secs_f64() * 1000.0;
        // A wakeup by Wayland traffic alone (screencopy frames, buffer releases) while every
        // window that would redraw still waits for its frame callback: install the frames and
        // re-arm captures; timers, services and drawing run at their deadline or frame callback.
        if stats_on() {
            thread_local!(static WHY: std::cell::RefCell<([u32; 7], f64)> = const { std::cell::RefCell::new(([0; 7], 0.0)) });
            let r = [
                app.pinged,
                now >= app.due_at,
                app.rt.revision() != app.last_rev,
                !app.rt.world.borrow().effects.is_empty(),
                app.caret.is_some(),
                (0..app.wins.len()).any(|k| {
                    let w = &app.wins[k];
                    (w.dirty || w.swapped || w.anim) && app.can_draw(k)
                }),
            ];
            WHY.with_borrow_mut(|(c, at)| {
                for (n, b) in c.iter_mut().zip(r) {
                    *n += b as u32;
                }
                c[6] += 1;
                if now - *at >= 1000.0 {
                    tracing::debug!(
                        target: "ri::stats",
                        "why ping {} poll {} rev {} fx {} caret {} draw {} of {}",
                        c[0], c[1], c[2], c[3], c[4], c[5], c[6]
                    );
                    *c = [0; 7];
                    *at = now;
                }
            });
        }
        if !app.pinged
            && app.grab_cleared.is_empty()
            && !app.grab_flushed
            && app.sync_key() == app.sync_key
            && now < app.due_at
            && app.rt.revision() == app.last_rev
            && app.rt.world.borrow().effects.is_empty()
            && app
                .caret
                .is_none_or(|c| c.1 == shell::scene::caret_phase(&app.rt.world.borrow()).0)
            && !(0..app.wins.len()).any(|k| {
                let w = &app.wins[k];
                (w.dirty || w.swapped || w.anim) && app.can_draw(k)
            })
            // The next animation step is due (Qt advances its animation driver every vsync),
            // even before the next deadline.
            && !(animations_running(app) && app.clock.step_due(now, app.awaiting_frame()))
        {
            app.apply_screencopies();
            if app.rt.revision() == app.last_rev {
                app.sync_screencopy(false);
                return true;
            }
        }
        // Behaviors started by the events polled below (a Hyprland global toggling a panel)
        // begin now, not at the last tick before an idle sleep: QUnifiedTimer restarts its
        // clock when an animation registers, so the first frames are not skipped.
        app.step_started();
        let running = animations_running(app);
        let awaiting = app.awaiting_frame();
        app.clock.one_window = app.one_window();
        app.read_wall(now);
        let at = app
            .clock
            .advance(now, running, animations_since(app), awaiting);
        {
            let mut w = app.rt.world.borrow_mut();
            w.now = at;
            w.frame_wall = Some(now);
        }
        let stats = qml::log_on!("ri::stats");
        // Loop timings are only reported with ri::stats: a clock read costs ~1 µs on an HPET
        // clocksource, and the loop runs at every wakeup.
        let slow0 = stats.then(std::time::Instant::now);
        let lap = || slow0.map_or(std::time::Duration::ZERO, |t| t.elapsed());
        let poll = app.pinged
            || app.rt.revision() != app.poll_rev
            || !app.rt.world.borrow().effects.is_empty()
            || now >= app.poll_due;
        let mut pn = poll && {
            app.pinged = false;
            // Writes made while handling this poll (e.g. a `Process` started by an event
            // handler) get the next wakeup's poll.
            app.poll_rev = app.rt.revision();
            app.run_window_methods();
            app.rt.poll_native() | quantize_colors(app)
        };
        let slow_pn = lap();
        app.panels_made();
        let tk = app.rt.tick(at);
        let tk = app.step_started().unwrap_or(tk);
        // With none or several windows exposed Qt's driver is stepped by a timer, whose
        // first tick is one interval after the driver's start whatever started it.
        if std::mem::take(&mut app.rt.world.borrow_mut().timer_started_driver)
            && !app.clock.running
            && app.clock.one_window
            && animations_running(app)
        {
            app.start_late();
        }
        let slow_tk = lap();
        // Timer/animation handlers may have queued service calls (MPRIS, Hyprland dispatch);
        // send them now instead of at the next wakeup, which can be a second away.
        if !app.rt.world.borrow().effects.is_empty() {
            app.run_window_methods();
            pn |= app.rt.poll_native();
        }
        app.rt.world.borrow_mut().trace_writes = false;
        let anim = app.wins.iter().any(|w| w.anim);
        app.animating = tk || anim;
        for w in app.wins.iter_mut().filter(|w| w.anim) {
            w.dirty = true;
        }
        let busy = pn | tk | anim;
        let t0 = stats.then(std::time::Instant::now);
        if stats && (busy || app.rt.revision() != app.last_rev) {
            tracing::debug!(
                target: "ri::stats",
                "wake native={pn} tick={tk} rev+{}",
                app.rt.revision().wrapping_sub(app.last_rev)
            );
        }
        app.run_closed();
        // The sync passes scan the item tree; they only have work after one of the changes
        // `sync_key` tracks or a compositor-cleared focus grab.
        app.flush_grab_cleared();
        let cleared = std::mem::take(&mut app.grab_flushed);
        if cleared || app.sync_key() != app.sync_key {
            app.sync_popups();
            app.sync_session_lock();
            app.sync_kb_modes();
        }
        app.apply_screencopies();
        let slow_pre = lap();
        if !qml::debug::on(qml::debug::Switch::NOPOLISH) {
            app.rt.set_polish_wins(app.wins.iter().map(|w| w.item));
        }
        // Output held for a frame is released, or waits, once the set of windows that can
        // draw one changes (shown, configured, hidden).
        if app
            .rt
            .set_frame_wins(app.wins.iter().filter(|w| draws_frames(w)).map(|w| w.item))
        {
            app.pinged = true;
        }
        app.rt.settle();
        let slow_settle = lap();
        let full = cleared || app.sync_key() != app.sync_key;
        if full {
            if qml::log_on!("ri::sync") {
                tracing::debug!(target: "ri::sync", "sync pass t={now:.0}");
            }
            app.sync_grabs();
            app.sync_idle_inhibitors();
            app.sync_shortcuts();
        }
        app.close_late();
        app.sync_screencopy(full);
        app.sync_key = app.sync_key();
        app.retarget_kbd();
        app.sync_input_method();
        app.sync_text_input();
        let slow_post = lap();
        if stats && lap().as_millis() >= 30 {
            tracing::debug!(
                target: "ri::stats",
                "slow t={now:.0} native {:?} tick {:?} pre {:?} settle {:?} total {:?}",
                slow_pn,
                slow_tk - slow_pn,
                slow_pre - slow_tk,
                slow_settle - slow_pre,
                lap()
            );
        }
        let (hit, fetched) = shell::scene::take_finished_images(&mut app.rt.world.borrow_mut());
        {
            let w = app.rt.world.borrow();
            for &i in &hit {
                let mut cur = Some(i);
                while let Some(x) = cur {
                    if let Some(win) = app.wins.iter_mut().find(|win| win.item == x) {
                        win.dirty = true;
                        break;
                    }
                    cur = w.items.get(x).and_then(|it| it.parent);
                }
            }
        }
        if fetched {
            for w in app.wins.iter_mut() {
                w.dirty = true;
            }
        }
        let caret = app
            .kbd
            .active_editor(&app.rt)
            .map(|e| (e, shell::scene::caret_phase(&app.rt.world.borrow()).0));
        if caret != app.caret {
            let mut w = app.rt.world.borrow_mut();
            w.touched
                .extend(caret.iter().chain(app.caret.iter()).map(|c| c.0));
            drop(w);
            app.caret = caret;
        }
        let slow_misc = lap();
        for r in app.rt.held_windows() {
            if let Some(w) = app.wins.iter_mut().find(|w| w.item == r) {
                w.dirty = true;
            }
        }
        for w in app.wins.iter_mut() {
            if app.rt.world.borrow().frame_wanted.contains(&w.item) {
                w.dirty = true;
            }
        }
        app.mark_dirty();
        // Output waiting for the next frame of any window: draw one when none is due.
        if app.rt.world.borrow().frame_wanted.contains(&usize::MAX)
            && !app.wins.iter().any(|w| w.dirty && draws_frames(w))
            && let Some(k) = (0..app.wins.len())
                .filter(|&k| draws_frames(&app.wins[k]))
                .min_by_key(|&k| !app.can_draw(k))
        {
            app.wins[k].dirty = true;
        }
        // Crossfade requests made this turn go to the windows its changes redraw.
        for w in app.wins.iter_mut() {
            if let Some(gs) = w.gl.as_mut() {
                gs.crossfade.observe(w.dirty);
            }
        }
        let slow_md = lap();
        // Decided once for the turn: the frames held back are drawn in a later one, after the
        // lock surfaces' own have gone to the compositor.
        let lock_first = app.lock_surfaces_draw_first();
        for k in 0..app.wins.len() {
            if app.wins[k].dirty && !app.can_draw(k) && qml::log_on!("ri::frames") {
                tracing::debug!(target: "ri::frames", "skip win{k} pending");
            }
            if app.holds_for_screen_cap(app.wins[k].item)
                || (lock_first && !matches!(app.wins[k].surf, Surf::Lock(_)))
            {
                continue;
            }
            if app.wins[k].dirty && app.can_draw(k) {
                app.draw(k);
            } else if app.wins[k].swapped && !app.wins[k].dirty && app.can_draw(k) {
                app.swap_draw(k);
            }
        }
        app.clock.answered = false;
        let slow_draw = lap();
        // Layout writes (focus of the current SwipeView page, sizes) fire change handlers.
        app.rt.settle();
        if stats {
            thread_local! {
                static LS: std::cell::RefCell<(u32, [f64; 10], Option<std::time::Instant>)> =
                    const { std::cell::RefCell::new((0, [0.0; 10], None)) };
            }
            let marks = [
                slow_pn,
                slow_tk,
                slow_pre,
                slow_settle,
                slow_post,
                slow_misc,
                slow_md,
                slow_draw,
                lap(),
            ];
            LS.with_borrow_mut(|(n, acc, last)| {
                *n += 1;
                let mut prev = std::time::Duration::ZERO;
                for (a, m) in acc.iter_mut().zip(marks) {
                    *a += (m - prev).as_secs_f64() * 1000.0;
                    prev = m;
                }
                let l = *last.get_or_insert_with(std::time::Instant::now);
                if l.elapsed().as_secs_f64() >= 1.0 {
                    let f = |x: f64| format!("{x:.0}");
                    tracing::debug!(
                        target: "ri::stats",
                        "loopstat iters {n} pn {} tick {} pre {} settle {} post {} misc {} mark {} draw {} settle2 {}",
                        f(acc[0]), f(acc[1]), f(acc[2]), f(acc[3]), f(acc[4]), f(acc[5]), f(acc[6]), f(acc[7]), f(acc[8])
                    );
                    tracing::debug!(target: "ri::stats", "wakes {}", qml::native::take_wake_stats());
                    tracing::debug!(
                        target: "ri::stats",
                        "delegators read again {}",
                        qml::runtime::take_deleg_scans()
                    );
                    tracing::debug!(target: "ri::stats", "scdiff {:?}", shell::gpu::take_diff_stats());
                    *n = 0;
                    *acc = [0.0; 10];
                    *last = Some(std::time::Instant::now());
                }
            });
        }
        app.ptr
            .keep_windows(app.wins.len(), |w| app.wins.iter().any(|x| x.item == w));
        app.ptr.hover_timer(Instant::now());
        if app.ptr.rehover(&mut app.rt) {
            app.update_cursor_shape();
        }
        if let Some(t0) = t0
            && t0.elapsed().as_millis() > 5
        {
            tracing::debug!(target: "ri::stats", "frame {}ms", t0.elapsed().as_millis());
        }
        if app.rt.revision() != app.last_rev {
            app.log_errors();
        }
        app.last_rev = app.rt.revision();
        app.rt.world.borrow_mut().trace_writes = stats && qml::log_on!("ri::tracew");
        if qml::log_on!("ri::stats") && app.frames % 60 == 1 {
            tracing::debug!(target: "ri::stats", "frames {} t {:.1}s busy {busy}", app.frames, now / 1000.0);
            let w = app.rt.world.borrow();
            for r in w.runs.iter().take(8) {
                let p = w.items[r.anim].parent;
                tracing::debug!(
                    target: "ri::stats",
                    "  run {} {:?} parent {:?} {:?} prop {:?} loops {}",
                    r.anim,
                    w.items[r.anim].types.first(),
                    p,
                    p.and_then(|p| w.items[p].types.first().cloned()),
                    w.items[r.anim]
                        .props
                        .get("property")
                        .map(|v| v.value.clone()),
                    r.loops
                );
            }
        }
        busy
    };
    // Sleep until input, a helper thread, the next timer or clock tick, or the next animation frame.
    let mut timeout = Some(Duration::ZERO);
    let flush_pending = std::rc::Rc::new(std::cell::Cell::new(false));
    // `Rustshell.reload()` was called and the shell has not re-executed yet: it waits for a
    // held session lock to be released (`App::lock_holds_reload`).
    let mut reload_wanted = false;
    // The session lock holds that reload back.
    let mut reload_held = false;
    // Why the Wayland connection broke, which ends the loop.
    let mut broke: Option<String> = None;
    loop {
        app.release_retiring();
        app.drop_unwanted_paste();
        app.script_pastes();
        app.order_self_ipc();
        match conn.flush() {
            Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // wl_display_flush: the socket buffer is full and requests are still queued.
                // Wake when it drains (POLLOUT) so the next iteration flushes them, instead of
                // sleeping on input only while the compositor waits for those requests.
                if !flush_pending.replace(true)
                    && let Err(e) = watch_writable(&ev.handle(), &conn, &flush_pending)
                {
                    broke = Some(e);
                    break;
                }
            }
            Err(WaylandError::Io(e)) => {
                broke = Some(e.to_string());
                break;
            }
            _ => {}
        }
        if let Err(e) = ev.dispatch(timeout, &mut app) {
            broke = Some(e.to_string());
            break;
        }
        if let Some(e) = wl_failure.lock().unwrap_or_else(|p| p.into_inner()).take() {
            broke = Some(e.to_string());
            break;
        }
        let mut cx = std::task::Context::from_waker(&wl_waker);
        if let std::task::Poll::Ready(Err(e)) = queue.poll_dispatch_pending(&mut cx, &mut app) {
            broke = Some(e.to_string());
            break;
        }
        // Quickshell runs with quitOnLastWindowClosed(false): losing every window (all outputs
        // unplugged, no panels enabled) must not end the shell; only Qt.quit()/Qt.exit() do.
        if app.rt.world.borrow().quit.is_some() {
            break;
        }
        // A grab cleared in this dispatch comes before the pointer frames read with it:
        // Quickshell runs `cleared` as it reads the event, Qt delivers the input afterwards.
        // A press outside the grab arrives as leave, enter of the surface under the pointer,
        // then `cleared`; a hover handler of that surface (the screen corner that toggles a
        // sidebar) finds the grab's panels closed and their surfaces gone, as a QWindow's is
        // once `visible` is false, and the panel it shows again gets a new one.
        if !app.grab_cleared.is_empty() {
            app.flush_grab_cleared();
            app.sync_popups();
        }
        app.flush_pointer_read();
        app.fire_key_repeat();
        // Animations started by the events handled below begin now, not at the last frame
        // before an idle sleep (QUnifiedTimer restarts its clock when it wakes).
        let wall = app.start.elapsed().as_secs_f64() * 1000.0;
        app.rt.world.borrow_mut().now = app.clock.stamp(wall);
        app.ptr.tick(&mut app.rt);
        if let Some(ipc) = ipc.as_ref().filter(|_| ipc_ready.replace(false)) {
            loop {
                let reqs = ipc.requests();
                if reqs.is_empty() {
                    break;
                }
                for req in reqs {
                    let r = if is_dump_call(&app.rt, &req.call) {
                        shell::ipc::CallResult::Done(Some(write_dump(&mut app).join("\n")))
                    } else {
                        ipc_call(&mut app.rt, &req.call, &mut ipc_listeners)
                    };
                    ipc.reply(req, r);
                }
            }
        }
        for (target, function) in std::mem::take(&mut app.self_ipc) {
            // An app window (settings.qml, welcome.qml) has no handlers of the shell: the
            // call goes to the shell running this config.
            if app_file.is_some() {
                shell::ricli::call_running_shell(&root, target, function);
                continue;
            }
            let c = shell::ipc::Call {
                target,
                function,
                args: Vec::new(),
                op: shell::ipc::Op::Call,
            };
            match ipc_call(&mut app.rt, &c, &mut ipc_listeners) {
                shell::ipc::CallResult::TargetNotFound => {
                    elog::eprintln!("ipc: no such target {}", c.target)
                }
                shell::ipc::CallResult::EntryNotFound => {
                    elog::eprintln!("ipc: no {}.{}", c.target, c.function)
                }
                _ => {}
            }
        }
        // `qs kill` (or a handler calling Qt.quit()) ends the shell now, not after the next wakeup.
        if app.rt.world.borrow().quit.is_some() {
            break;
        }
        let t_frame = Instant::now();
        let mut busy = frame(&mut app);
        if app.started && qml::native::app_windows_held() && app.exclusive_zones_mapped() {
            qml::native::release_app_windows();
        }
        app.note_start_panels();
        app.release_predecessor();
        let asked = qml::native::take_reload();
        reload_wanted |= asked;
        // Said when the lock begins to hold a reload (asked for under the lock, or the lock
        // came with a call served first, below) and for every reload asked for under it.
        let held = reload_wanted && app.lock_holds_reload();
        if held && (asked || !reload_held) {
            elog::eprintln!("reload: the session is locked; reloading once it is unlocked");
        }
        reload_held = held;
        if reload_wanted && !app.lock_holds_reload() && ipc.as_ref().is_some_and(|s| s.mid_call()) {
            // A call that is arriving comes first (it may be the one that locks the
            // session): served by the next turn, which decides again.
            ipc_ready.set(true);
        } else if reload_wanted && !app.lock_holds_reload() {
            reload_wanted = false;
            reload_shell(
                &root,
                app_file.as_deref(),
                &mut app,
                &documents,
                ipc.as_ref(),
            );
            // It came back: the reload was refused and the tree was told so in this turn.
            // What its handlers changed is laid out and drawn now, not at the next event.
            busy |= frame(&mut app);
        }
        if stats_on() {
            thread_local!(static LOOP: std::cell::Cell<(u32, u128, Option<Instant>)> = const { std::cell::Cell::new((0, 0, None)) });
            LOOP.with(|l| {
                let (n, us, at) = l.get();
                let (n, us) = (n + 1, us + t_frame.elapsed().as_micros());
                let at = at.unwrap_or(t_frame);
                if at.elapsed().as_secs_f64() >= 1.0 {
                    tracing::debug!(target: "ri::stats", "loopall calls {n} frame {us}us");
                    l.set((0, 0, Some(Instant::now())));
                } else {
                    l.set((n, us, Some(at)));
                }
            });
        }
        if !busy && std::mem::take(&mut app.prelayout_idle) && app.prelayout_pending() {
            app.prelayout_hidden();
        } else if !busy || app.only_hidden_changed() {
            app.note_hidden_changes();
            app.relayout_stale_hidden();
        }
        ipc_forward(&app.rt, &mut ipc_listeners);
        let now = app.start.elapsed().as_secs_f64() * 1000.0;
        let q = app.deadlines(now, busy);
        app.due_at = q.head();
        app.poll_due = q
            .iter()
            .find(|&(_, why)| matches!(why, Due::Timers | Due::Poll))
            .map_or(f64::INFINITY, |(t, _)| t);
        if qml::log_on!("ri::sched") {
            tracing::debug!(target: "ri::sched", "t={now:.1} next {:?}", q.next());
        }
        // Nothing due: block until an event, like Qt's idle event loop.
        timeout = q.timeout(now);
    }
    if broke.is_none() {
        app.unlock_before_exit();
        if app.unlocks_in_flight > 0 {
            broke = await_unlocks(&mut app, &mut queue, &conn, &wl_failure);
        }
    }
    // QWaylandDisplay::checkWaylandError: a broken connection ends the shell with a warning
    // and `exit(-1)`, status 255, here after what is pending was written out and the
    // children were ended.
    let code = match &broke {
        Some(e) => {
            elog::eprintln!(
                "WARN: The Wayland connection broke. Did the Wayland compositor die? ({e})"
            );
            255
        }
        None => {
            // Qt.quit(), Qt.exit() and the `kill` request delete the root object in Quickshell
            // (EngineGeneration::destroy): `Component.onDestruction` runs, and a handler
            // calling Qt.exit() sets the status. A termination signal ends Quickshell where it
            // stands and so does a broken Wayland connection: no handler runs.
            if terminated.get().is_none() {
                app.rt.destroy_at_exit(&documents);
            }
            app.rt.world.borrow().quit.unwrap_or(0)
        }
    };
    // The answer to an unlock never came (the connection broke): `UnlockSync` did not take the
    // marker away. A lock still held stays named: the session is left with a dead lockscreen.
    if !app.session.as_ref().is_some_and(|s| s.locked) {
        drop_lock_marker_of(|pid| pid == std::process::id());
    }
    flush_for_exit(&mut app.rt.world.borrow_mut());
    drop(app);
    if let Some(ipc) = ipc.take() {
        ipc.unregister(code != 0 && terminated.get().is_none());
    }
    shell::rilog::close();
    if let Some(sig) = terminated.get() {
        qml::native::terminate_children();
        shell::shutdown::exit_by(sig);
    }
    std::process::exit(code);
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        s: &wl_surface::WlSurface,
        factor: i32,
    ) {
        let Some(k) = self.wins.iter().position(|w| w.surf.wl_surface() == s) else {
            return;
        };
        self.wins[k].scale.int = factor.max(1);
        self.scale_reported(k);
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        s: &wl_surface::WlSurface,
        time: u32,
    ) {
        let Some(k) = self.wins.iter().position(|w| w.surf.wl_surface() == s) else {
            return;
        };
        self.wins[k].pending = false;
        self.wins[k].asked = None;
        if animations_running(self) {
            self.clock.callback(k);
        }
        // The pointer frames read with this callback came before it: the frame it starts shows
        // where they left the pointer. Asked only after them, a drag would find nothing to draw
        // here and draw past the callback, a refresh late. (A cleared grab goes first, in `run`.)
        // The windows of one refresh (callbacks with one time) show one step of a drag: the
        // motion read while the first of them drew waits for the next refresh.
        if self.grab_cleared.is_empty() {
            if self.frame_time.replace(time) == Some(time) {
                self.flush_pointer_read();
            } else {
                self.frame_at = Instant::now();
                if std::mem::take(&mut self.motion_early) {
                    // The drag's step for this refresh is in the scene already: what was
                    // read with the callback is the next one's.
                    self.frame_has_motion = true;
                    self.flush_pointer_read();
                    self.frame_has_motion = false;
                } else {
                    self.motion_this_frame = false;
                    self.flush_pointer();
                }
            }
        }
        // A click delivered above may have closed windows, this one among them.
        let Some(k) = self.wins.iter().position(|w| w.surf.wl_surface() == s) else {
            return;
        };
        self.mark_dirty();
        if qml::log_on!("ri::frames") {
            tracing::debug!(target: "ri::frames", "cb win{k} dirty={}", self.wins[k].dirty);
        }
        if self.wins[k].dirty {
            self.draw(k);
        }
    }

    /// A toplevel on another screen has that screen's factor: Qt keeps the surface's size
    /// and the window gets the size that is in the new screen's pixels.
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        s: &wl_surface::WlSurface,
        o: &wl_output::WlOutput,
    ) {
        let Some(k) = self.wins.iter().position(|w| w.surf.wl_surface() == s) else {
            return;
        };
        if !matches!(self.wins[k].surf, Surf::Toplevel(_)) {
            return;
        }
        self.wins[k].scale.on = self.outputs.info(o).and_then(|i| i.name);
        let f = self.win_factor(k);
        let w = &mut self.wins[k];
        if w.scale.factor == f {
            return;
        }
        let surface = w.surface_size();
        w.scale.factor = f;
        w.set_surface_size(surface);
        w.dirty = true;
        let (item, size) = (w.item, w.size);
        self.rt.set(item, "width", Value::Num(size.0 as f64));
        self.rt.set(item, "height", Value::Num(size.1 as f64));
        self.scale_changed(k);
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, o: wl_output::WlOutput) {
        self.outputs_gen += 1;
        if let Some(m) = &self.output_power {
            let p = m.get_output_power(&o, &self.qh, PowerOn(o.id()));
            self.output_powers.insert(o.id(), (p, true));
        }
        self.sync_unseen();
        self.output_changed(&o);
        self.refresh_screens(None);
        self.lock_add_output(&o);
        self.reswap_all();
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, o: wl_output::WlOutput) {
        // Hyprland re-announces every output on each config reload with nothing changed;
        // QScreen then emits no change and Qt redraws nothing.
        if !self.output_changed(&o) {
            return;
        }
        self.outputs_gen += 1;
        self.refresh_screens(None);
        self.reswap_all();
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, o: wl_output::WlOutput) {
        self.outputs_gen += 1;
        self.output_infos.remove(&o.id());
        if let Some((p, _)) = self.output_powers.remove(&o.id()) {
            p.destroy();
        }
        self.sync_unseen();
        self.lock_remove_output(&o);
        self.close_panels_on(&o);
        self.probes_answered(|p| p.on == o);
        self.refresh_screens(Some(&o));
        self.reswap_all();
    }
}

/// A wl_shm screencopy buffer the compositor offered holds `height` rows of `width` 4-byte
/// pixels `stride` bytes apart, in a pool size that fits in i32.
fn shm_geometry_ok(width: u32, height: u32, stride: u32) -> bool {
    width > 0
        && height > 0
        && u64::from(stride) >= u64::from(width) * 4
        && u64::from(stride) * u64::from(height) <= i32::MAX as u64
}

/// The output a layer surface for screen `want` goes on: `Some(None)` lets the compositor pick
/// (no screen set), `None` means the named screen is not connected.
fn panel_wl_output<O>(
    want: Option<&str>,
    mut outputs: impl Iterator<Item = (O, Option<String>)>,
) -> Option<Option<O>> {
    match want {
        None => Some(None),
        Some(n) => outputs
            .find(|(_, name)| name.as_deref() == Some(n))
            .map(|(o, _)| Some(o)),
    }
}

fn edge_anchor(e: u32) -> PAnchor {
    match (e & 1 != 0, e & 8 != 0, e & 2 != 0, e & 4 != 0) {
        (true, _, true, _) => PAnchor::TopLeft,
        (true, _, _, true) => PAnchor::TopRight,
        (_, true, true, _) => PAnchor::BottomLeft,
        (_, true, _, true) => PAnchor::BottomRight,
        (true, ..) => PAnchor::Top,
        (_, true, ..) => PAnchor::Bottom,
        (_, _, true, _) => PAnchor::Left,
        (_, _, _, true) => PAnchor::Right,
        _ => PAnchor::None,
    }
}

fn edge_gravity(e: u32) -> Gravity {
    match edge_anchor(e) {
        PAnchor::TopLeft => Gravity::TopLeft,
        PAnchor::TopRight => Gravity::TopRight,
        PAnchor::BottomLeft => Gravity::BottomLeft,
        PAnchor::BottomRight => Gravity::BottomRight,
        PAnchor::Top => Gravity::Top,
        PAnchor::Bottom => Gravity::Bottom,
        PAnchor::Left => Gravity::Left,
        PAnchor::Right => Gravity::Right,
        _ => Gravity::None,
    }
}

impl App {
    /// Wanted geometry of every visible `PopupWindow` anchored to one of our mapped windows:
    /// like Quickshell (ProxyPopupWindow::completeWindow makes any backing window of
    /// `anchor.window` the transient parent), a panel, a toplevel or another popup.
    fn popup_keys(&mut self) -> Vec<(usize, PopupKey)> {
        let parents: Vec<usize> = self
            .wins
            .iter()
            .filter(|w| !matches!(w.surf, Surf::Lock(_)) && w.configured)
            .map(|w| w.item)
            .collect();
        // A lock surface has no xdg_surface or layer surface to parent an xdg_popup to.
        let locks: Vec<usize> = self
            .wins
            .iter()
            .filter(|w| matches!(w.surf, Surf::Lock(_)))
            .map(|w| w.item)
            .collect();
        let mut w = self.rt.world.borrow_mut();
        let mut out = Vec::new();
        for i in w.of_type("PopupWindow") {
            if w.dead.contains(&i)
                || w.items[i].types[0] != "PopupWindow"
                    && !w.items[i].types.iter().any(|t| t == "PopupWindow")
            {
                continue;
            }
            if !w.read(i, "visible").is_some_and(|v| v.truthy()) {
                continue;
            }
            let Some(Value::Item(parent)) = w.read(i, "anchor.window") else {
                continue;
            };
            if !parents.contains(&parent) {
                if locks.contains(&parent) && self.popup_refused.insert(i) {
                    elog::eprintln!(
                        "PopupWindow: anchor.window is a session lock surface, which cannot have popups; the popup is not shown."
                    );
                }
                continue;
            }
            let n = |w: &mut qml::runtime::World, it: usize, k: &str| {
                w.read(it, k)
                    .map(|v| v.to_number())
                    .filter(|x| x.is_finite())
                    .unwrap_or(0.0)
            };
            let size = (
                n(&mut w, i, "implicitWidth").trunc().max(1.0) as i32,
                n(&mut w, i, "implicitHeight").trunc().max(1.0) as i32,
            );
            let r = [
                n(&mut w, i, "anchor.rect.x"),
                n(&mut w, i, "anchor.rect.y"),
                n(&mut w, i, "anchor.rect.width"),
                n(&mut w, i, "anchor.rect.height"),
            ];
            let rect = match w.read(i, "anchor.item") {
                Some(Value::Item(it)) => {
                    let (x, y) = w.scene_pos(it, Some(parent));
                    let (iw, ih) = (n(&mut w, it, "width"), n(&mut w, it, "height"));
                    let (rw, rh) = if r[2] > 0.0 || r[3] > 0.0 {
                        (r[2], r[3])
                    } else {
                        (iw, ih)
                    };
                    (x + r[0], y + r[1], rw, rh)
                }
                _ => (r[0], r[1], r[2], r[3]),
            };
            let flags = |w: &mut qml::runtime::World, k: &str, d: u32| match w.read(i, k) {
                Some(Value::Num(v)) if v > 0.0 => v as u32,
                _ => d,
            };
            out.push((
                i,
                PopupKey {
                    parent,
                    size,
                    rect: (
                        rect.0.round() as i32,
                        rect.1.round() as i32,
                        (rect.2.round() as i32).max(1),
                        (rect.3.round() as i32).max(1),
                    ),
                    edges: flags(&mut w, "anchor.edges", 1 | 2),
                    gravity: flags(&mut w, "anchor.gravity", 8 | 4),
                    adjust: scene::popup_adjustment(w.read(i, "anchor.adjustment")),
                },
            ));
        }
        out
    }

    /// Wanted layer surface of every visible `PanelWindow`.
    fn panel_keys(&mut self) -> Vec<(usize, PanelKey)> {
        let mut w = self.rt.world.borrow_mut();
        let mut out = Vec::new();
        for i in w.of_type("PanelWindow") {
            if w.dead.contains(&i)
                || !w.items[i].types.iter().any(|t| t == "PanelWindow")
                || !w.read(i, "visible").is_none_or(|v| v.truthy())
            {
                continue;
            }
            let n = |w: &mut qml::runtime::World, k: &str| {
                w.read(i, k)
                    .map(|v| v.to_number())
                    .filter(|x| x.is_finite())
                    .unwrap_or(0.0)
            };
            let mut anchor = 0;
            for (b, k) in [
                "anchors.top",
                "anchors.bottom",
                "anchors.left",
                "anchors.right",
            ]
            .iter()
            .enumerate()
            {
                if w.read(i, k).is_some_and(|v| v.truthy()) {
                    anchor |= 1 << b;
                }
            }
            let namespace = match w.read(i, "WlrLayershell.namespace") {
                Some(Value::Str(s)) => s.to_string(),
                _ => "rustshell".into(),
            };
            let layer = match w.read(i, "WlrLayershell.layer") {
                Some(Value::Num(v)) => v as u32,
                _ => 2,
            };
            let exclusion = Exclusion::from_qml(
                w.read(i, "exclusionMode")
                    .map(|v| v.to_number())
                    .filter(|x| x.is_finite()),
            );
            let size = (
                n(&mut w, "implicitWidth").trunc().max(1.0) as u32,
                n(&mut w, "implicitHeight").trunc().max(1.0) as u32,
            );
            let masked = w
                .read(i, "mask")
                .is_some_and(|m| !matches!(m, Value::Null | Value::Undefined));
            let output = shell::scene::panel_output(&mut w, i);
            out.push((
                i,
                PanelKey {
                    namespace,
                    size,
                    geom: Panel {
                        anchor,
                        margins: (
                            n(&mut w, "margins.top").round() as i32,
                            n(&mut w, "margins.right").round() as i32,
                            n(&mut w, "margins.bottom").round() as i32,
                            n(&mut w, "margins.left").round() as i32,
                        ),
                        size,
                        exclusion,
                        zone: n(&mut w, "exclusiveZone") as i32,
                        edge: shell::layer::from_edges(
                            n(&mut w, "WlrLayershell.exclusiveEdge") as u32
                        ),
                    }
                    .geometry()
                    .scaled(self.screen_factor(output.as_deref())),
                    layer,
                    masked,
                    output,
                },
            ));
        }
        out
    }

    /// What the draw of a shown window does for the hidden ones, in a turn without a draw:
    /// the queued resizes and layout changes inside a laid out hidden window make it stale.
    /// Without this a window that grows while hidden (the cheatsheet) went stale only when
    /// some shown window happened to draw.
    fn note_hidden_changes(&mut self) {
        if self.clean.is_empty() {
            return;
        }
        self.take_resized(usize::MAX);
        self.drain_lrev(None);
    }

    /// A turn that changed something left every shown window as it was: what changed is in
    /// a hidden one (the cheatsheet making its rows a few per frame). Its layout then follows
    /// in pieces during the making, not in one pass after it.
    fn only_hidden_changed(&self) -> bool {
        !self.wins.iter().any(|w| w.anim || w.dirty)
    }

    /// Qt lays out a hidden window's items too, so content a binding creates from geometry
    /// (ListView delegates, size-gated Loaders) exists before the first show, and a Behavior
    /// animates a change made while the window is still hidden or as it is shown. Lay out one
    /// hidden window per idle wake at its predicted size, whenever it was made; the configure
    /// on show then has little to do.
    /// Lays out again one hidden window whose layout went stale at least 300 ms ago (a
    /// burst of changes costs one pass), at the size it was laid out at.
    fn relayout_stale_hidden(&mut self) {
        let now = self.start.elapsed().as_secs_f64() * 1000.0;
        let wins = &self.wins;
        let dead = |i: &usize| self.rt.world.borrow().dead.contains(i);
        self.stale_hidden
            .retain(|i, _| !dead(i) && !wins.iter().any(|w| w.item == *i));
        let Some((&i, &(size, _))) = self
            .stale_hidden
            .iter()
            .find(|&(_, &(_, t))| shell::sched::stale_relayout_due(t, now))
        else {
            return;
        };
        self.stale_hidden.remove(&i);
        let t = Instant::now();
        scene::layout_window(
            &mut self.rt,
            &mut self.fonts.borrow_mut(),
            i,
            [size.0 as f64, size.1 as f64],
        );
        self.drain_lrev(Some(i));
        self.clean.insert(i, size);
        self.roots.insert(i);
        // The pass settled the local resizes kept for the window's first draw too.
        self.parked.remove(&i);
        if qml::log_on!("ri::stats") {
            tracing::debug!(target: "ri::stats", "relayout hidden {i} {size:?} {}ms", t.elapsed().as_millis());
        }
    }

    fn prelayout_hidden(&mut self) {
        let screen = self
            .wins
            .iter()
            .filter(|w| w.configured)
            .fold((0, 0), |a, w| (a.0.max(w.size.0), a.1.max(w.size.1)));
        let state = self.prelayout_state();
        let next = {
            let mut w = self.rt.world.borrow_mut();
            let (done, wins) = (&mut self.prelaid, &self.wins);
            // A window that is shown has its layout: it is not laid out again once hidden.
            done.extend(wins.iter().map(|x| x.item));
            // A panel can stretch between two edges of a screen whose size no window tells yet.
            let all: Vec<usize> = (PRELAID_WINDOWS.iter())
                .flat_map(|ty| w.of_type(ty))
                .collect();
            (all.into_iter()).find(|&i| !done.contains(&i) && (screen.0 > 0 || !is_panel(&w, i)))
        };
        let Some(i) = next else {
            self.prelaid_at = Some(state);
            return;
        };
        self.prelaid.insert(i);
        let panel = is_panel(&self.rt.world.borrow(), i);
        // Exclusive zones of the mapped panels shrink what the compositor will configure
        // (wlroots arrange_layers: zone plus the margin on the reserved edge).
        let out_name = {
            let mut w = self.rt.world.borrow_mut();
            match w.read(i, "screen") {
                Some(Value::Item(sc)) => match w.read(sc, "name") {
                    Some(Value::Str(n)) => Some(n.to_string()),
                    _ => None,
                },
                _ => None,
            }
        };
        // (top, bottom, left, right)
        let mut res = [0.0f64; 4];
        for win in self.wins.iter().filter(|w| w.configured) {
            let Surf::Panel { key, .. } = &win.surf else {
                continue;
            };
            if out_name.is_some() && key.output.is_some() && key.output != out_name {
                continue;
            }
            let (t, r, b, l) = shell::layer::reserved(&key.geom);
            for (e, v) in [t, b, l, r].into_iter().enumerate() {
                res[e] += v;
            }
        }
        let size = {
            let mut w = self.rt.world.borrow_mut();
            if w.read(i, "exclusionMode")
                .is_some_and(|v| v.to_number() == 1.0)
            {
                res = [0.0; 4];
            }
            let n = |w: &mut qml::runtime::World, k: &str| {
                w.read(i, k)
                    .map(|v| v.to_number())
                    .filter(|x| x.is_finite())
                    .unwrap_or(0.0)
            };
            let on =
                |w: &mut qml::runtime::World, k: &str| w.read(i, k).is_some_and(|v| v.truthy());
            let (l, r, t, b) = (
                n(&mut w, "margins.left"),
                n(&mut w, "margins.right"),
                n(&mut w, "margins.top"),
                n(&mut w, "margins.bottom"),
            );
            // A window that is no panel has the size written to it, else its implicit size.
            let own = |w: &mut qml::runtime::World, k: &str, implicit: &str| match n(w, k) {
                x if x > 0.0 && !panel => x,
                _ => n(w, implicit),
            };
            let iw = own(&mut w, "width", "implicitWidth");
            let ih = own(&mut w, "height", "implicitHeight");
            let sw = on(&mut w, "anchors.left") && on(&mut w, "anchors.right");
            let sh = on(&mut w, "anchors.top") && on(&mut w, "anchors.bottom");
            [
                if sw {
                    screen.0 as f64 - res[2] - res[3] - l - r
                } else {
                    iw.trunc()
                }
                .max(1.0),
                if sh {
                    screen.1 as f64 - res[0] - res[1] - t - b
                } else {
                    ih.trunc()
                }
                .max(1.0),
            ]
        };
        let t = Instant::now();
        scene::layout_window(&mut self.rt, &mut self.fonts.borrow_mut(), i, size);
        self.drain_lrev(Some(i));
        self.clean.insert(i, (size[0] as u32, size[1] as u32));
        self.roots.insert(i);
        if qml::log_on!("ri::stats") {
            tracing::debug!(
                target: "ri::stats",
                "prelayout {i} {:?} {res:?} {}ms",
                size,
                t.elapsed().as_millis()
            );
        }
    }

    /// Every GPU window swaps once (`shell::gpu::Surface::reswap`).
    fn reswap_all(&mut self) {
        for w in &mut self.wins {
            if let Some(gs) = w.gl.as_mut() {
                gs.reswap();
                w.dirty = true;
            }
        }
    }

    /// Records the output's current info; whether it differs from the last recorded.
    fn output_changed(&mut self, o: &wl_output::WlOutput) -> bool {
        let Some(info) = self.outputs.info(o).map(|i| OutputSnapshot::of(&i)) else {
            return true;
        };
        self.output_infos.insert(o.id(), info.clone()) != Some(info)
    }

    /// Output hotplug: republish `Rustshell.screens`; `Variants` then adds or drops the
    /// per-screen panels and `sync_panels` maps them onto their outputs.
    fn refresh_screens(&mut self, gone: Option<&wl_output::WlOutput>) {
        // QScreen::devicePixelRatio(): wl_output.scale times the screen's factor.
        let scales = self.outputs.outputs().filter(|o| Some(o) != gone);
        let scales = scales.filter_map(|o| {
            let i = self.outputs.info(&o)?;
            Some(i.scale_factor as f64 * self.screen_factor(i.name.as_deref()))
        });
        scene::set_screens_ratio(scales.fold(0.0, f64::max));
        if !self.started {
            return;
        }
        let screens: Vec<_> = self
            .outputs
            .outputs()
            .filter(|o| Some(o) != gone)
            .filter_map(|o| output_screen(&self.outputs.info(&o)?, |n| self.screen_factor(Some(n))))
            .collect();
        if let Some(v) = primary_vsync(&self.outputs) {
            self.clock.vsync = v;
        }
        self.rt.set_screens_dpr(&screens);
        self.publish_outputs(gone);
        self.log_errors();
    }

    /// Tells the layout which monitors are connected, by connector and EDID identity: the
    /// wlr-output-management head's make/model/serial, or without that protocol `wl_output`'s
    /// make/model (which carry no serial).
    fn publish_outputs(&mut self, gone: Option<&wl_output::WlOutput>) {
        let ids: Vec<_> = self
            .outputs
            .outputs()
            .filter(|o| Some(o) != gone)
            .filter_map(|o| {
                let i = self.outputs.info(&o)?;
                let connector = i.name.clone().unwrap_or_default();
                let edid = match &self.head_mgr {
                    Some(_) => self.heads.edid(&connector).cloned(),
                    None => Some(qml::layout_model::Edid {
                        make: i.make.clone(),
                        model: i.model.clone(),
                        serial: String::new(),
                    }),
                };
                Some(qml::layout_model::MonitorId { connector, edid })
            })
            .collect();
        self.rt.set_outputs(&ids);
    }

    /// Drops the warnings the runtime kept since the last call: `log_warning` printed each when
    /// it was raised. A binding error that went away is printed again when it comes back.
    fn log_errors(&mut self) {
        let (_, bind) = self.rt.take_errors();
        WARNED.with_borrow_mut(|said| {
            for gone in self.seen_bind.iter().filter(|e| !bind.contains(e)) {
                said.remove(gone);
            }
        });
        self.seen_bind = bind.into_iter().collect();
    }

    fn sync_panels(&mut self) {
        let mut want = self.panel_keys();
        let over_predecessor = self.predecessor.is_some();
        // Panels reserving space map first (the bar on startup), so the others are configured
        // inside its exclusive zone.
        want.sort_by_key(|(_, k)| k.geom.zone <= 0);
        // Only a namespace or screen change needs a new layer surface (a layer surface's output
        // is fixed at creation; Quickshell hides, moves and shows the window again); recreating
        // one for a size/mask/zone change would drop keyboard focus and make Hyprland clear a
        // focus grab that holds it.
        let same =
            |k: &PanelKey, key: &PanelKey| k.namespace == key.namespace && k.output == key.output;
        let mut hidden = Vec::new();
        let gone: Vec<usize> = self
            .wins
            .iter()
            .filter(|win| match &win.surf {
                Surf::Panel { key, .. } => {
                    !want.iter().any(|(i, k)| *i == win.item && same(k, key))
                }
                _ => false,
            })
            .map(|win| win.item)
            .collect();
        // A replaced panel that reserves space keeps its surface until its successor's first
        // frame ([`Retiring`]).
        let retires = |win: &Win| {
            win.mapped == Some(true)
                && matches!(&win.surf, Surf::Panel { key, .. } if key.geom.zone > 0)
        };
        let closing = self.wins.iter().filter(|w| !retires(w)).map(|w| w.item);
        let closing: Vec<usize> = closing.filter(|i| gone.contains(i)).collect();
        self.blank_closing(&closing);
        // A popup goes before the layer surface it is parented to.
        self.drop_popups_of(&gone);
        for win in std::mem::take(&mut self.wins) {
            let keep = match &win.surf {
                Surf::Panel { key, .. } => want.iter().any(|(i, k)| *i == win.item && same(k, key)),
                _ => true,
            };
            if !keep && !want.iter().any(|(i, _)| *i == win.item) {
                hidden.push((win.item, win.size));
            }
            if keep {
                self.wins.push(win);
            } else if retires(&win) {
                self.retiring.push(Retiring { win, succ: None });
            }
        }
        let dead = |i: usize| self.rt.world.borrow().dead.contains(&i);
        // A panel hidden with changes since its last frame is laid out again in idle, so the
        // next show does not pay a full layout.
        let now = self.start.elapsed().as_secs_f64() * 1000.0;
        for (i, sz) in hidden {
            if !dead(i) && sz.0 > 0 && sz.1 > 0 && !self.clean.contains_key(&i) {
                self.stale_hidden.entry(i).or_insert((sz, now)).0 = sz;
            }
        }
        for (item, key) in want {
            if let Some(win) = self.wins.iter_mut().find(|w| w.item == item) {
                if let Surf::Panel {
                    layer: ls,
                    key: old,
                    ..
                } = &mut win.surf
                    && *old != key
                {
                    if old.layer != key.layer {
                        ls.set_layer(match key.layer {
                            0 => Layer::Background,
                            1 => Layer::Bottom,
                            3 => Layer::Overlay,
                            _ => Layer::Top,
                        });
                    }
                    if old.geom != key.geom {
                        ls.set_geometry(&sent_geometry(&key.geom, over_predecessor));
                    }
                    if key.masked != old.masked {
                        if key.masked {
                            if let Ok(r) = Region::new(&self.compositor) {
                                ls.wl_surface().set_input_region(Some(r.wl_region()));
                            }
                        } else {
                            ls.wl_surface().set_input_region(None);
                        }
                        win.region = None;
                    }
                    if win.mapped.is_some() {
                        log_presented(self.presentation.as_ref(), ls.wl_surface(), &self.qh);
                    }
                    ls.commit();
                    *old = key;
                    win.dirty = true;
                }
                continue;
            }
            let layer = match key.layer {
                0 => Layer::Background,
                1 => Layer::Bottom,
                3 => Layer::Overlay,
                _ => Layer::Top,
            };
            let named = self.outputs.outputs().map(|o| {
                let name = self.outputs.info(&o).and_then(|i| i.name);
                (o, name)
            });
            // A window for a screen that is gone (unplugged, not announced yet) waits for it:
            // without an output the compositor would put it on the focused monitor.
            let Some(out) = panel_wl_output(key.output.as_deref(), named) else {
                continue;
            };
            let surface = self.compositor.create_surface(&self.qh);
            let ls = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                layer,
                key.namespace.clone(),
                out.as_ref(),
            );
            ls.set_geometry(&sent_geometry(&key.geom, over_predecessor));
            // A window opened once this image has its first frames (a menu asked for while
            // the previous image's windows are still there) shows as it draws.
            if over_predecessor
                && self.start_panels.is_none()
                && let Some(m) = &self.veil_mgr
            {
                let cloak = m.alpha.get_surface(ls.wl_surface(), &self.qh, ());
                cloak.set_multiplier(0);
                self.cloaks.push((ls.wl_surface().clone(), cloak));
            }
            // zwlr_layer_surface_v1: double-buffered state belongs in the initial commit, so the
            // compositor never maps the surface with the wrong keyboard interactivity.
            let kb_mode = self.kb_mode(item);
            ls.set_keyboard_interactivity(kb_interactivity(kb_mode));
            self.kb_modes
                .insert(ls.wl_surface().id().protocol_id(), kb_mode);
            if key.masked
                && let Ok(r) = Region::new(&self.compositor)
            {
                ls.wl_surface().set_input_region(Some(r.wl_region()));
            }
            ls.commit();
            self.wins.push(Win {
                gl: None,
                item,
                size: key.size,
                surf: Surf::Panel {
                    layer: ls,
                    key,
                    on: out,
                },
                configured: false,
                dirty: true,
                anim: false,
                pending: false,
                shown: None,
                committed: None,
                asked: None,
                laid: (u64::MAX, (0, 0)),
                unsettled: 0,
                region: None,
                last: None,
                shm_prev: None,
                veils: Vec::new(),
                spare: Vec::new(),
                swapped: false,
                scale: WinScale::default(),
                mapped: None,
            });
            self.ensure_scaler(self.wins.len() - 1);
        }
        self.publish_window_rects();
        self.release_retiring();
    }

    /// A replaced panel that reserves space (the bar moved between a horizontal and a vertical
    /// edge, or to another namespace) stays mapped until its own successor draws its first
    /// frame, and is destroyed right before that frame is committed: both zone changes then
    /// reach the compositor together, so other windows are resized once instead of growing
    /// into the gap left by the old bar and shrinking again when the new one maps. Each
    /// replaced panel waits for the successor it is paired with ([`pair_successors`]): with
    /// the bar and a dock replaced on one output, the bar's first frame must not take the
    /// dock's zone away before the new dock has one.
    ///
    /// Returns the item of `k` when it is a panel reserving space about to draw its first
    /// frame that replaces others: they go with that commit ([`retire_replaced`]).
    fn first_zone_frame(&self, k: usize) -> Option<usize> {
        let w = &self.wins[k];
        match &w.surf {
            Surf::Panel { key, .. }
                if w.mapped.is_none()
                    && key.geom.zone > 0
                    && self.retiring.iter().any(|r| r.succ == Some(w.item)) =>
            {
                Some(w.item)
            }
            _ => None,
        }
    }

    /// Pairs each replaced panel with its successor ([`App::first_zone_frame`]) and drops the
    /// ones no successor waits on any more: the bar was hidden rather than moved, or its
    /// successor was closed or lost its output.
    fn release_retiring(&mut self) {
        if self.retiring.is_empty() {
            return;
        }
        let panels: Vec<PanelState> = self
            .wins
            .iter()
            .filter_map(|w| match &w.surf {
                Surf::Panel { key, .. } => {
                    Some((w.item, key.output.clone(), key.geom.zone, w.mapped))
                }
                _ => None,
            })
            .collect();
        let replaced: Vec<(usize, Option<String>, Option<usize>)> = self
            .retiring
            .iter()
            .map(|r| match &r.win.surf {
                Surf::Panel { key, .. } => (r.win.item, key.output.clone(), r.succ),
                _ => (r.win.item, None, None),
            })
            .collect();
        for (r, succ) in self
            .retiring
            .iter_mut()
            .zip(pair_successors(&replaced, &panels))
        {
            r.succ = succ;
        }
        for mut r in std::mem::take(&mut self.retiring) {
            if r.succ.is_some() {
                self.retiring.push(r);
            } else if self.off_screen_waits(r.win.item) {
                self.blank(&mut r.win);
            }
        }
    }

    /// Every window that reserves an exclusive zone is mapped, so the compositor has applied
    /// the zones (app windows opened from here are placed in what is left). Mapping needs no
    /// presented frame: an output off by DPMS at login maps its bar all the same.
    fn exclusive_zones_mapped(&self) -> bool {
        zones_mapped(self.wins.iter().map(|w| {
            let zone = match &w.surf {
                Surf::Panel { key, .. } => key.geom.zone,
                _ => 0,
            };
            (zone, w.mapped == Some(true))
        }))
    }

    /// `WlrLayershell.keyboardFocus` (WlrKeyboardFocus None/Exclusive/OnDemand) per layer surface.
    /// Keys go to the focused surface unless it never takes keyboard focus itself (a bar inside a
    /// focus grab, which Hyprland may refocus to); then to the grab's newest popup (a tray menu
    /// over its bar or overflow popup), else the newest shown surface that does.
    fn retarget_kbd(&mut self) {
        let mode = |app: &Self, k: usize| {
            if matches!(app.wins[k].surf, Surf::Lock(_)) {
                return 1;
            }
            app.wins[k]
                .surf
                .layer()
                .and_then(|ls| {
                    app.kb_modes
                        .get(&ls.wl_surface().id().protocol_id())
                        .copied()
                })
                .unwrap_or(0)
        };
        let raw = self
            .kbd_act
            .active()
            .and_then(|s| self.wins.iter().position(|w| w.surf.wl_surface() == s));
        let wins: Vec<KbdWin> = (0..self.wins.len())
            .map(|k| KbdWin {
                configured: self.wins[k].configured,
                mode: mode(self, k),
                toplevel: matches!(self.wins[k].surf, Surf::Toplevel(_)),
                grabbed: matches!(self.wins[k].surf, Surf::Popup { .. })
                    && raw.is_some_and(|f| {
                        let f = self.wins[f].surf.wl_surface();
                        let s = self.wins[k].surf.wl_surface();
                        self.grabs
                            .values()
                            .any(|(_, ss)| ss.contains(f) && ss.contains(s))
                    }),
            })
            .collect();
        let target = kbd_target(raw, &wins).map(|k| self.wins[k].item);
        if target != self.kbd.win {
            if qml::log_on!("ri::keys") {
                tracing::debug!(target: "ri::keys", "kbd target {:?} -> {target:?}", self.kbd.win);
            }
            self.kbd.win = target;
        }
        self.kbd.sync(&mut self.rt);
    }

    /// The keyboard mode (0 None, 1 Exclusive, 2 OnDemand) a PanelWindow asks for.
    /// `focusable` is the same setting: the runtime keeps the two in step on every write.
    fn kb_mode(&self, item: usize) -> u32 {
        let m = self
            .rt
            .world
            .borrow_mut()
            .read(item, "WlrLayershell.keyboardFocus")
            .map_or(0.0, |v| v.to_number());
        if m.is_finite() { (m as u32).min(2) } else { 0 }
    }

    fn sync_kb_modes(&mut self) {
        let mut live = Vec::new();
        for win in &self.wins {
            let Some(ls) = win.surf.layer() else { continue };
            let id = ls.wl_surface().id().protocol_id();
            live.push(id);
            let m = self.kb_mode(win.item);
            if self.kb_modes.get(&id) != Some(&m) {
                if qml::log_on!("ri::keys") {
                    tracing::debug!(target: "ri::keys", "kb mode item {} -> {m}", win.item);
                }
                ls.set_keyboard_interactivity(kb_interactivity(m));
                if win.mapped.is_some() {
                    log_presented(self.presentation.as_ref(), ls.wl_surface(), &self.qh);
                }
                ls.commit();
                self.kb_modes.insert(id, m);
            }
        }
        self.kb_modes.retain(|k, _| live.contains(k));
    }

    /// Rustshell.Hyprland `GlobalShortcut` over hyprland_global_shortcuts_v1 (`global, appid:name`
    /// binds in hyprland.conf).
    fn sync_shortcuts(&mut self) {
        let Some(mgr) = self.shortcut_mgr.clone() else {
            return;
        };
        // Reads only the world: nothing to change without a write since the last sync.
        let rev = self.rt.revision();
        if self.shortcuts_rev == Some(rev) {
            return;
        }
        self.shortcuts_rev = Some(rev);
        let mut want: std::collections::HashMap<(String, String), (Vec<usize>, String, String)> =
            Default::default();
        {
            let mut w = self.rt.world.borrow_mut();
            let items = w.of_type("GlobalShortcut");
            let chg = items.iter().map(|&i| w.items[i].chg).max().unwrap_or(0);
            if self.shortcuts_seen.0 == items && self.shortcuts_seen.1 == chg {
                return;
            }
            self.shortcuts_seen = (items.clone(), chg);
            for i in items {
                if w.dead.contains(&i) {
                    continue;
                }
                let s = |w: &mut qml::runtime::World, k: &str| match w.read(i, k) {
                    Some(Value::Str(s)) => s.to_string(),
                    _ => String::new(),
                };
                let name = s(&mut w, "name");
                if name.is_empty() {
                    continue;
                }
                let appid = Some(s(&mut w, "appid"))
                    .filter(|a| !a.is_empty())
                    .unwrap_or_else(|| "rustshell".into());
                let e = want.entry((appid, name)).or_insert_with(|| {
                    (
                        Vec::new(),
                        s(&mut w, "description"),
                        s(&mut w, "triggerDescription"),
                    )
                });
                e.0.push(i);
            }
        }
        self.shortcuts.retain(|k, (sc, items)| match want.get(k) {
            Some((it, ..)) => {
                *items = it.clone();
                true
            }
            None => {
                sc.destroy();
                false
            }
        });
        for (k, (items, desc, trig)) in want {
            if self.shortcuts.contains_key(&k) {
                continue;
            }
            let sc =
                mgr.register_shortcut(k.1.clone(), k.0.clone(), desc, trig, &self.qh, k.clone());
            self.shortcuts.insert(k, (sc, items));
        }
    }

    /// The toplevel's dmabuf copy target for a `fourcc`×w×h frame; `None` falls back to shm.
    fn dma_slot(&mut self, handle: u32, key: (u32, u32, u32)) -> Option<WlBuffer> {
        if self.dma_off {
            return None;
        }
        if let Some(s) = self.dma_keep.get(&handle) {
            if s.key == key && s.epoch == self.dma_epoch {
                return Some(s.buf.clone());
            }
            let s = self.dma_keep.remove(&handle)?;
            self.free_slot(s);
        }
        let dmabuf = self.dmabuf.clone()?;
        self.gpu.as_ref()?;
        if self.gbm.is_none() {
            // The compositor's main device (it must import the buffer), else the first node.
            self.gbm = self
                .dma_params
                .main_device
                .and_then(shell::gbm::Gbm::open_device)
                .or_else(shell::gbm::Gbm::open)
                .map(std::rc::Rc::new);
        }
        let slot = self.gbm.clone().and_then(|gbm| {
            let (fourcc, w, h) = key;
            let on_dev = |d| gbm.is_device(d);
            let usable = self.gpu.as_ref()?.dmabuf_modifiers(fourcc);
            let bo = self
                .dma_params
                .modifier_tranches(fourcc, &on_dev, &usable)
                .iter()
                .find_map(|mods| shell::gbm::Bo::with_modifiers(&gbm, w, h, fourcc, mods))
                .or_else(|| {
                    let mut bo = shell::gbm::Bo::new(&gbm, w, h, fourcc)?;
                    bo.modifier =
                        self.dma_params
                            .implicit_modifier(fourcc, bo.modifier, &on_dev)?;
                    Some(bo)
                })?;
            let fds = bo.plane_fds()?;
            let planes: Vec<(i32, u32, u32)> = fds
                .iter()
                .zip(&bo.planes)
                .map(|(fd, &(offset, stride))| {
                    (std::os::fd::AsRawFd::as_raw_fd(fd), offset, stride)
                })
                .collect();
            let (tex, img) =
                self.gpu
                    .as_mut()?
                    .import_dmabuf(&planes, w, h, fourcc, bo.modifier)?;
            let params = dmabuf.create_params(&self.qh, ());
            for (i, (fd, &(offset, stride))) in fds.iter().zip(&bo.planes).enumerate() {
                params.add(
                    std::os::fd::AsFd::as_fd(fd),
                    i as u32,
                    offset,
                    stride,
                    (bo.modifier >> 32) as u32,
                    bo.modifier as u32,
                );
            }
            let buf = params.create_immed(
                w as i32,
                h as i32,
                fourcc,
                zwp_linux_buffer_params_v1::Flags::empty(),
                &self.qh,
                DmaBuf,
            );
            params.destroy();
            Some(DmaSlot {
                _bo: bo,
                buf,
                tex,
                img,
                key,
                epoch: self.dma_epoch,
                views: Vec::new(),
                copy: None,
            })
        });
        let Some(slot) = slot else {
            elog::eprintln!("screencopy: no dmabuf target for {key:x?}, using wl_shm");
            self.dma_off = true;
            return None;
        };
        let buf = slot.buf.clone();
        self.dma_keep.insert(handle, slot);
        Some(buf)
    }

    /// New dmabuf parameters: the compositor re-sends them when buffers made with the old ones
    /// are sub-optimal, so each capture target is made again (on the new main device) at its
    /// next frame, never under a copy in flight.
    fn dma_feedback_done(&mut self) {
        let next = self.dma_params_next.done();
        if next == self.dma_params {
            return;
        }
        self.dma_params = next;
        self.dma_epoch += 1;
        self.gbm = None;
        self.dma_off = false;
    }

    fn free_slot(&mut self, s: DmaSlot) {
        if s.views.iter().any(|v| v.strong_count() > 0) {
            self.dma_grave.push(s);
        } else {
            self.destroy_slot(s);
        }
    }

    fn destroy_slot(&mut self, s: DmaSlot) {
        s.buf.destroy();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.free_dmabuf(s.tex, s.img);
            gpu.free_frame_copy(s.copy);
        }
    }

    fn sc_convert(&mut self, m: ScMsg) {
        if self.sc.is_none() {
            let waker = self.rt.world.borrow().waker.handle(qml::svc::Svc::Host);
            // Refused by the system (`EAGAIN`): this frame is not shown; the next one
            // starts the worker again.
            self.sc = ScWorker::start(waker);
        }
        let unsent = match &self.sc {
            Some(sc) => sc.send(m).err(),
            None => Some(m),
        };
        if let Some(m) = unsent {
            sc_drop(&mut self.caps, m);
        }
    }

    /// Install toplevel frames converted off the main thread by `sc_convert`.
    fn apply_screencopies(&mut self) {
        let mut done: Vec<_> = self
            .sc
            .as_ref()
            .map(|sc| sc.done.try_iter().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|out| match out {
                ScOut::Frame(f) => Some(f),
                ScOut::Same(i) => {
                    if let Some(c) = self.caps.get_mut(&i) {
                        c.landing = false;
                    }
                    None
                }
            })
            .map(|(i, h, sz, tex, thumb)| {
                let tex = std::rc::Rc::new(tex);
                let thumb = thumb.map_or_else(|| tex.clone(), std::rc::Rc::new);
                (i, h, sz, tex, thumb)
            })
            .collect();
        let (dead, live): (Vec<_>, Vec<_>) = std::mem::take(&mut self.dma_grave)
            .into_iter()
            .partition(|s| s.views.iter().all(|v| v.strong_count() == 0));
        self.dma_grave = live;
        for s in dead {
            self.destroy_slot(s);
        }
        // Frames compared at an earlier pass: their answers once the GPU has them, or now when
        // a newer frame of the same toplevel needs its copy.
        let mut ready = Vec::new();
        let mut changed = Vec::new();
        if let Some(g) = self.gpu.as_mut()
            && !self.dma_wait.is_empty()
        {
            let clash = (self.dma_ready.iter()).any(|r| self.dma_wait.iter().any(|q| q.1 == r.1));
            if clash || g.compare_ready() {
                ready = std::mem::take(&mut self.dma_wait);
                changed = vec![true; ready.len()];
                let mut queued: Vec<(usize, u32)> = ready.iter().map(|r| r.1).enumerate().collect();
                read_compares(g, &mut self.dma_keep, &mut queued, &mut changed);
            }
        }
        let mut waiting = Vec::new();
        if self.dma_wait.is_empty() {
            let base = ready.len();
            ready.append(&mut self.dma_ready);
            // Compared at the view's size, so pixels the preview never samples don't count.
            changed.resize(ready.len(), true);
            if let Some(g) = self.gpu.as_mut() {
                let mut queued = Vec::new();
                for (k, &(_, h, (w, ht), _, view)) in ready.iter().enumerate().skip(base) {
                    if queued.iter().any(|&(_, q)| q == h) || g.compare_full() {
                        read_compares(g, &mut self.dma_keep, &mut queued, &mut changed);
                    }
                    let Some(slot) = self.dma_keep.get_mut(&h) else {
                        continue;
                    };
                    let view = (view.0 as i32, view.1 as i32);
                    match g.frame_compare(slot.tex, w as i32, ht as i32, view, &mut slot.copy) {
                        Some(c) => changed[k] = c,
                        None => queued.push((k, h)),
                    }
                }
                // A window that redraws for another frame of this batch shows the queued ones
                // too: their answers would not save a draw, only cost a readback.
                if !queued.is_empty() {
                    let mut owner = rustc_hash::FxHashMap::default();
                    let mut texs = Vec::new();
                    for (k, w) in self.wins.iter().enumerate() {
                        if let Some(f) = w.last.as_ref() {
                            f.shared_texs(&mut texs);
                        }
                        for t in texs.drain(..) {
                            owner.entry(t).or_insert(k);
                        }
                    }
                    let win_of = |i: usize| {
                        let t = scene::screencopy(i)?;
                        owner.get(&t.key.get()).copied()
                    };
                    let drawn: Vec<usize> = ready
                        .iter()
                        .enumerate()
                        .filter(|&(k, r)| {
                            changed[k]
                                && !queued.iter().any(|q| q.0 == k)
                                && self.dma_keep.contains_key(&r.1)
                        })
                        .filter_map(|(_, r)| win_of(r.0))
                        .collect();
                    if !drawn.is_empty()
                        && queued
                            .iter()
                            .all(|&(k, _)| win_of(ready[k].0).is_some_and(|w| drawn.contains(&w)))
                    {
                        g.compare_forget();
                        for (_, h) in std::mem::take(&mut queued) {
                            if let Some(slot) = self.dma_keep.get_mut(&h) {
                                shell::gpu::Gpu::compare_drop(&mut slot.copy);
                            }
                        }
                    }
                }
                // The rest are answered at the pass their fence's sync file wakes (`dma_wait`),
                // not by stalling on the GPU now; without one they are read now.
                let watched = g.compare_submit().is_some_and(|fd| {
                    let src = calloop::generic::Generic::new(
                        fd,
                        calloop::Interest::READ,
                        calloop::Mode::OneShot,
                    );
                    (self.loop_handle)
                        .insert_source(src, |_, _, _: &mut App| Ok(calloop::PostAction::Remove))
                        .is_ok()
                });
                if !watched {
                    read_compares(g, &mut self.dma_keep, &mut queued, &mut changed);
                }
                waiting = queued.into_iter().map(|q| q.0).collect();
            }
        }
        for (k, r) in ready.into_iter().enumerate() {
            if waiting.contains(&k) {
                self.dma_wait.push(r);
                continue;
            }
            let (i, h, (w, ht), inv, _) = r;
            if let Some(c) = self.caps.get_mut(&i) {
                c.landing = false;
            }
            let Some(slot) = self.dma_keep.get_mut(&h) else {
                continue;
            };
            // Hyprland re-delivers unchanged frames on every output render; redrawing for them
            // makes the next render deliver them again.
            slot.views.retain(|v| v.strong_count() > 0);
            if !changed[k] && !slot.views.is_empty() {
                continue;
            }
            let tex = std::rc::Rc::new(qsg::raster::Canvas {
                w,
                h: ht,
                px: Vec::new(),
                key: Default::default(),
            });
            shell::gpu::register_ext(&tex, slot.tex, inv);
            slot.views.retain(|v| v.strong_count() > 0);
            slot.views.push(std::rc::Rc::downgrade(&tex));
            done.push((i, Some(h), (w, ht), tex.clone(), tex));
        }
        for (i, handle, (tw, th), tex, thumb) in done {
            if qml::log_on!("ri::stats") {
                tracing::debug!(target: "ri::stats", "screencopy ready {i} {tw}x{th}");
            }
            if qml::log_on!("ri::scdump")
                && !tex.px.is_empty()
                && let Ok(png) = tex.png()
            {
                shell::ipc::write_debug_file(&format!("sc-{i}.png"), &png);
            }
            if let Some(h) = handle {
                self.cap_seq += 1;
                self.cap_cache.insert(h, (self.cap_seq, thumb));
                if self.cap_cache.len() > CAP_CACHE_MAX {
                    let old = self.cap_cache.iter().min_by_key(|e| e.1.0).map(|e| *e.0);
                    self.cap_cache.retain(|k, _| Some(*k) != old);
                    let busy = self
                        .caps
                        .values()
                        .any(|c| c.handle == old && c.frame.is_some());
                    if let Some(o) = old.filter(|_| !busy)
                        && let Some(s) = self.dma_keep.remove(&o)
                    {
                        self.free_slot(s);
                    }
                }
            }
            let Some(cap) = self.caps.get_mut(&i).filter(|c| c.handle == handle) else {
                continue;
            };
            cap.landing = false;
            cap.painted = false;
            let old = scene::screencopy(i);
            scene::set_screencopy(i, Some(tex.clone()));
            let mut w = self.rt.world.borrow_mut();
            if !w.dead.contains(&i) {
                // A new frame of the same size only changes pixels: reshow the windows' last
                // frames with the texture swapped instead of repainting them.
                let same = scene::screencopy_size(i) == Some((tw as u32, th as u32));
                if same
                    && let Some(old) = old
                    && w.read(i, "hasContent").is_some_and(|v| v.truthy())
                    && w.read(i, "sourceSize").is_some()
                {
                    let mut found = false;
                    for win in self.wins.iter_mut() {
                        if let Some(fr) = win.last.as_mut()
                            && fr.swap_tex(&old, &tex)
                        {
                            win.swapped = true;
                            found = true;
                        }
                    }
                    if found {
                        continue;
                    }
                }
                if w.read(i, "hasContent").map(|v| v.truthy()) != Some(true) {
                    w.write(i, "hasContent", Value::Bool(true));
                }
                let fresh = scene::set_screencopy_size(i, (tw as u32, th as u32));
                if fresh || w.read(i, "sourceSize").is_none() {
                    let sz = qml::eval::new_obj();
                    qml::eval::set_own(&sz, "width", Value::Num(tw as f64));
                    qml::eval::set_own(&sz, "height", Value::Num(th as f64));
                    w.write(i, "sourceSize", Value::Obj(sz));
                }
                w.touched.push(i);
            }
        }
    }

    /// Whether window `win` waits for a screen capture of its own: the frozen frame must show
    /// the screen without the window, so it may not commit a frame before the copy is taken.
    /// (Capturing after the window's first frame, as Qt effectively does, sometimes freezes
    /// the region selector's own dim layer into the image: a doubly darkened screen.)
    fn holds_for_screen_cap(&self, win: usize) -> bool {
        holds_for_screen_cap(&self.screen_caps, win)
    }

    /// Show screen captures that landed; like a non-live QQuickScreencopyView they stay frozen.
    fn poll_screen_caps(&mut self) {
        for (i, c) in poll_screen_caps(&mut self.screen_caps) {
            scene::set_screencopy(i, Some(std::rc::Rc::new(c)));
            let mut w = self.rt.world.borrow_mut();
            if !w.dead.contains(&i) {
                w.write(i, "hasContent", Value::Bool(true));
                w.touched.push(i);
            }
        }
    }

    /// Start a toplevel capture for every visible `ScreencopyView` with a source that has no
    /// frame in flight: once, or continuously while `live` (later frames wait for damage).
    fn sync_screencopy(&mut self, full: bool) {
        self.poll_screen_caps();
        // A screen is copied over wlr-screencopy; only a toplevel needs the export manager.
        let mgr = self.export_mgr.clone();
        self.cap_grave.retain(Capture::busy);
        if !full {
            for (&i, cap) in self.caps.iter_mut() {
                if let Some(mgr) = &mgr
                    && let Some(handle) = cap.handle.filter(|_| cap.wants_frame())
                {
                    cap.frame =
                        Some(mgr.capture_toplevel(cap.overlay_cursor(), handle, &self.qh, i));
                    cap.seen = false;
                }
            }
            return;
        }
        let items = self.rt.world.borrow_mut().of_type("ScreencopyView");
        let mut live_items = std::collections::HashSet::new();
        let mut screen_items = std::collections::HashSet::new();
        for i in items {
            let (handle, live, screen, cursor) = {
                let mut w = self.rt.world.borrow_mut();
                if w.dead.contains(&i) {
                    continue;
                }
                // Quickshell ScreencopyView.paintCursor (false by default): the capture's
                // `overlay_cursor`.
                let cursor = w.read(i, "paintCursor").is_some_and(|v| v.truthy());
                let ht = match w.read(i, "captureSource") {
                    Some(Value::Item(t)) => match w.read(t, "HyprlandToplevel") {
                        Some(Value::Item(ht)) => Some(ht),
                        _ => w.items[t]
                            .types
                            .iter()
                            .any(|t| t == "HyprlandToplevel")
                            .then_some(t),
                    },
                    _ => None,
                };
                let handle = ht.and_then(|ht| {
                    let a = w.read(ht, "address")?.to_js_string();
                    u64::from_str_radix(a.trim_start_matches("0x"), 16).ok()
                });
                // QQuickItem::updatePaintNode re-arms a live capture, so hidden views stay frozen.
                let mut shown = true;
                let mut cur = Some(i);
                while let Some(x) = cur {
                    if w.read(x, "visible").is_some_and(|v| !v.truthy()) {
                        shown = false;
                        break;
                    }
                    cur = w.items.get(x).and_then(|it| it.parent);
                }
                let screen = match w.read(i, "captureSource") {
                    Some(Value::Item(t)) if w.items[t].types.iter().any(|t| t == "ShellScreen") => {
                        let mut root = i;
                        while !self.wins.iter().any(|x| x.item == root) {
                            match w.items.get(root).and_then(|it| it.parent) {
                                Some(p) => root = p,
                                None => break,
                            }
                        }
                        w.read(t, "name").map(|n| (n.to_js_string(), root, shown))
                    }
                    _ => None,
                };
                (
                    handle.map(|h| h as u32),
                    shown && w.read(i, "live").is_some_and(|v| v.truthy()),
                    screen,
                    cursor,
                )
            };
            if let Some((name, win, shown)) = screen {
                screen_items.insert(i);
                // ScreencopyView::setPaintCursors captures again.
                if self.screen_cap_cursor.get(&i).is_some_and(|c| *c != cursor) {
                    self.screen_caps.remove(&i);
                }
                if shown && !self.screen_caps.contains_key(&i) {
                    let h = self.rt.world.borrow().waker.handle(qml::svc::Svc::Host);
                    let rx = capture_screen(name, cursor, h);
                    self.screen_caps.insert(i, ScreenCap::Busy(rx, win));
                    self.screen_cap_cursor.insert(i, cursor);
                }
                continue;
            }
            let (Some(handle), Some(mgr)) = (handle, &mgr) else {
                continue;
            };
            live_items.insert(i);
            let cap = self.caps.entry(i).or_default();
            if cap.handle != Some(handle) || cap.cursor != cursor {
                if let Some(f) = cap.frame.take()
                    && let Some(f) = self.frames_unanswered.retire(f, cap.seen)
                {
                    f.destroy();
                }
                let old = std::mem::replace(
                    cap,
                    Capture {
                        handle: Some(handle),
                        cursor,
                        ..Default::default()
                    },
                );
                if old.busy() {
                    self.cap_grave.push(old);
                }
                if let Some((_, tex)) = self.cap_cache.get(&handle) {
                    scene::set_screencopy(i, Some(tex.clone()));
                    let mut w = self.rt.world.borrow_mut();
                    w.write(i, "hasContent", Value::Bool(true));
                    w.touched.push(i);
                }
            }
            cap.live = live;
            if cap.wants_frame() {
                cap.frame = Some(mgr.capture_toplevel(cap.overlay_cursor(), handle, &self.qh, i));
                cap.seen = false;
            }
        }
        let gone: Vec<usize> = self
            .screen_caps
            .keys()
            .filter(|i| !screen_items.contains(i))
            .copied()
            .collect();
        for i in gone {
            self.screen_caps.remove(&i);
            self.screen_cap_cursor.remove(&i);
            scene::set_screencopy(i, None);
        }
        let gone: Vec<usize> = self
            .caps
            .keys()
            .filter(|i| !live_items.contains(i))
            .copied()
            .collect();
        for i in gone {
            let Some(mut cap) = self.caps.remove(&i) else {
                continue;
            };
            self.sc_convert(ScMsg::Forget(i));
            if let Some(f) = cap.frame.take()
                && let Some(f) = self.frames_unanswered.retire(f, cap.seen)
            {
                f.destroy();
            }
            if let Some(e) = cap.handle.and_then(|h| self.cap_cache.get_mut(&h)) {
                self.cap_seq += 1;
                e.0 = self.cap_seq;
            }
            if cap.busy() {
                self.cap_grave.push(cap);
            }
            scene::set_screencopy(i, None);
            let mut w = self.rt.world.borrow_mut();
            if !w.dead.contains(&i) {
                w.write(i, "hasContent", Value::Bool(false));
                w.touched.push(i);
            }
        }
    }

    /// Rustshell.Wayland `IdleInhibitor` over zwp_idle_inhibit_manager_v1: while `enabled`, an
    /// inhibitor lives on `window`'s surface. Like Quickshell, an inhibitor without a mapped
    /// `window` is inactive: zwp_idle_inhibitor_v1 only inhibits while its own surface is
    /// visible, so borrowing another window's surface would tie it to that window's visibility.
    fn sync_idle_inhibitors(&mut self) {
        let Some(mgr) = self.idle_mgr.clone() else {
            return;
        };
        let items = self.rt.world.borrow_mut().of_type("IdleInhibitor");
        let mut want: std::collections::HashMap<usize, wl_surface::WlSurface> = Default::default();
        for i in items {
            let (enabled, win) = {
                let mut w = self.rt.world.borrow_mut();
                if w.dead.contains(&i) {
                    continue;
                }
                let enabled = w.read(i, "enabled").is_none_or(|v| v.truthy());
                let win = match w.read(i, "window") {
                    Some(Value::Item(c)) => Some(c),
                    _ => None,
                };
                (enabled, win)
            };
            if !enabled {
                continue;
            }
            let surf = win
                .and_then(|it| self.wins.iter().find(|w| w.item == it && w.configured))
                .map(|w| w.surf.wl_surface().clone());
            if let Some(s) = surf {
                want.insert(i, s);
            }
        }
        self.inhibitors.retain(|i, (inh, s)| {
            let keep = want.get(i) == Some(s);
            if !keep {
                inh.destroy();
            }
            keep
        });
        for (i, s) in want {
            if !self.inhibitors.contains_key(&i) {
                let inh = mgr.create_inhibitor(&s, &self.qh, ());
                if qml::log_on!("ri::stats") {
                    tracing::debug!(target: "ri::stats", "idle inhibitor on for {i}");
                }
                self.inhibitors.insert(i, (inh, s));
            }
        }
    }

    /// Deliver compositor `cleared` events and run their handlers now, so the panels they
    /// hide are unmapped by this pass's `sync_popups` (Qt runs `onCleared` synchronously)
    /// instead of at a later wakeup, which can be a timer up to a second away.
    fn flush_grab_cleared(&mut self) {
        if self.grab_cleared.is_empty() {
            return;
        }
        self.grab_flushed = true;
        for i in std::mem::take(&mut self.grab_cleared) {
            if let Some((g, _)) = self.grabs.remove(&i) {
                g.destroy();
            }
            let mut w = self.rt.world.borrow_mut();
            if !w.dead.contains(&i) {
                w.write(i, "active", Value::Bool(false));
                qml::native::emit(&mut w, i, "cleared", &[], Vec::new());
            }
        }
        self.rt.settle();
    }

    /// Rustshell.Hyprland `HyprlandFocusGrab` over hyprland_focus_grab_v1: while `active`, a click
    /// outside `windows` clears the grab (`active` = false, `cleared()`).
    fn sync_grabs(&mut self) {
        let Some(mgr) = self.grab_mgr.clone() else {
            if qml::log_on!("ri::grab") {
                tracing::debug!(target: "ri::grab", "grab: no hyprland_focus_grab_manager_v1");
            }
            return;
        };
        let items = self.rt.world.borrow_mut().of_type("HyprlandFocusGrab");
        let mut live = std::collections::HashSet::new();
        for i in items {
            let (active, wins) = {
                let mut w = self.rt.world.borrow_mut();
                if w.dead.contains(&i) {
                    continue;
                }
                let active = w.read(i, "active").is_some_and(|v| v.truthy());
                let wins: Vec<usize> = match w.read(i, "windows") {
                    Some(Value::Obj(o)) => o
                        .borrow()
                        .array
                        .iter()
                        .flatten()
                        .filter_map(|x| match x {
                            Value::Item(c) => Some(*c),
                            _ => None,
                        })
                        .collect(),
                    Some(Value::Item(c)) => vec![c],
                    _ => Vec::new(),
                };
                (active, wins)
            };
            let surfaces: Vec<wl_surface::WlSurface> = wins
                .iter()
                .filter_map(|&it| {
                    self.wins
                        .iter()
                        .find(|w| w.item == it)
                        .map(|w| w.surf.wl_surface().clone())
                })
                .collect();
            if qml::log_on!("ri::grab") {
                tracing::debug!(
                    target: "ri::grab",
                    "grab {i} active={active} wins={wins:?} surfaces={}",
                    surfaces.len()
                );
            }
            if !active || surfaces.is_empty() {
                continue;
            }
            live.insert(i);
            // Hyprland clears a grab as soon as keyboard focus lands outside it, so a grab must
            // not go live before every shown window it whitelists has a surface. Like Quickshell
            // (QWaylandWindow::surfaceCreated), a surface joins before its first buffer: an
            // exclusive-keyboard layer focuses on map, which would clear a grab that adds it later.
            let pending = wins.iter().any(|&it| {
                !self.wins.iter().any(|w| w.item == it)
                    && self
                        .rt
                        .world
                        .borrow_mut()
                        .read(it, "visible")
                        .is_some_and(|v| v.truthy())
            });
            if pending && !self.grabs.contains_key(&i) {
                continue;
            }
            let entry = self
                .grabs
                .entry(i)
                .or_insert_with(|| (mgr.create_grab(&self.qh, i), Vec::new()));
            if entry.1 == surfaces {
                continue;
            }
            for s in &entry.1 {
                if !surfaces.contains(s) {
                    entry.0.remove_surface(s);
                }
            }
            for s in &surfaces {
                if !entry.1.contains(s) {
                    entry.0.add_surface(s);
                }
            }
            entry.0.commit();
            entry.1 = surfaces;
        }
        self.grabs.retain(|i, (g, _)| {
            let keep = live.contains(i);
            if !keep {
                g.destroy();
            }
            keep
        });
    }

    fn drop_toplevels(&mut self, gone: &[usize]) {
        if gone.is_empty() {
            return;
        }
        self.blank_closing(gone);
        // A popup goes before the toplevel it is parented to.
        self.drop_popups_of(gone);
        self.wins.retain(|win| !gone.contains(&win.item));
        self.leave_gone_focus();
    }

    fn close_late(&mut self) {
        let gone = std::mem::take(&mut self.late_close);
        self.drop_toplevels(&gone);
    }

    /// Shown `ApplicationWindow`/`Window`/`FloatingWindow` items get an xdg_toplevel; hidden
    /// ones lose it.
    fn sync_toplevels(&mut self) {
        self.track_off_screen();
        let want: Vec<usize> = {
            let mut w = self.rt.world.borrow_mut();
            let mut v: Vec<usize> = w.of_type("ApplicationWindow");
            v.extend(w.of_type("Window"));
            v.extend(w.of_type("FloatingWindow"));
            v.sort_unstable();
            v.dedup();
            v.retain(|&i| is_app_window(&w, i) && w.read(i, "visible").is_some_and(|v| v.truthy()));
            v
        };
        let gone: Vec<usize> = self
            .wins
            .iter()
            .filter(|win| matches!(win.surf, Surf::Toplevel(_)) && !want.contains(&win.item))
            .map(|win| win.item)
            .collect();
        // QObject::deleteLater: a window deleted with its Loader keeps its surface until the
        // event loop runs again, after the windows and focus grabs the same handler made. A
        // hidden one loses it at once.
        let (late, gone): (Vec<usize>, Vec<usize>) = {
            let w = self.rt.world.borrow();
            gone.into_iter().partition(|i| w.dead.contains(i))
        };
        for i in late {
            if !self.late_close.contains(&i) {
                self.late_close.push(i);
            }
        }
        self.drop_toplevels(&gone);
        for item in want {
            if self.wins.iter().any(|w| w.item == item) {
                continue;
            }
            let num = |rt: &qml::runtime::Runtime, n: &str| {
                let v = rt.get(item, n).to_number();
                if v.is_finite() && v > 0.0 {
                    v.round() as u32
                } else {
                    0
                }
            };
            let surface = self.compositor.create_surface(&self.qh);
            let t = self
                .xdg
                .create_window(surface, WindowDecorations::RequestServer, &self.qh);
            // QtWayland titles an untitled window with the application name.
            let title = self.rt.get(item, "title").to_js_string();
            t.set_title(if title.is_empty() {
                "rustshell".into()
            } else {
                title
            });
            t.set_app_id(APP_ID);
            let (minw, minh) = (
                num(&self.rt, "minimumWidth"),
                num(&self.rt, "minimumHeight"),
            );
            // On the first screen, where Qt creates a window.
            let f = self.screen_factor(None);
            let px = |v: u32| to_surface(v as i32, f) as u32;
            if minw > 0 || minh > 0 {
                t.set_min_size(Some((px(minw), px(minh))));
            }
            let (maxw, maxh) = (
                num(&self.rt, "maximumWidth"),
                num(&self.rt, "maximumHeight"),
            );
            if maxw > 0 && maxh > 0 && maxw < 16_777_215 && maxh < 16_777_215 {
                t.set_max_size(Some((px(maxw), px(maxh))));
            }
            t.commit();
            let size = (
                num(&self.rt, "width").max(1),
                num(&self.rt, "height").max(1),
            );
            if qml::log_on!("ri::appdebug") {
                tracing::debug!(target: "ri::appdebug", "toplevel {item} size {size:?} min {minw}x{minh}");
            }
            self.wins.push(Win {
                gl: None,
                item,
                size,
                surf: Surf::Toplevel(t),
                configured: false,
                dirty: true,
                anim: false,
                pending: false,
                shown: None,
                committed: None,
                asked: None,
                laid: (u64::MAX, (0, 0)),
                unsettled: 0,
                region: None,
                last: None,
                shm_prev: None,
                veils: Vec::new(),
                spare: Vec::new(),
                swapped: false,
                scale: WinScale::default(),
                mapped: None,
            });
            self.ensure_scaler(self.wins.len() - 1);
            // Qt polishes the content at the declared size before any later event (an async
            // Process result, the compositor's configure) arrives; lay it out now as well.
            let lsize = [size.0 as f64, size.1 as f64];
            scene::layout_window(&mut self.rt, &mut self.fonts.borrow_mut(), item, lsize);
            self.drain_lrev(Some(item));
            self.clean.insert(item, size);
            self.roots.insert(item);
        }
    }

    /// QWindow::close(): `closing(CloseEvent)`, then hide unless a handler refused.
    fn close_window(&mut self, item: usize) {
        let ev = qml::eval::new_obj();
        qml::eval::set_own(&ev, "accepted", Value::Bool(true));
        self.rt
            .emit(item, "closing", &["close"], vec![Value::Obj(ev.clone())]);
        self.rt.settle();
        if ev.borrow().props.get("accepted").is_none_or(|v| v.truthy()) {
            self.rt.set(item, "visible", Value::Bool(false));
            self.rt.settle();
            self.sync_toplevels();
        }
    }

    /// Window methods (`root.close()`) queued by handlers as runtime effects.
    fn run_window_methods(&mut self) {
        let calls: Vec<(usize, String)> = {
            let mut w = self.rt.world.borrow_mut();
            let mut out = Vec::new();
            let mut effects = std::mem::take(&mut w.effects);
            effects.retain(|e| {
                let qml::svc::Effect::Method(a) = e else {
                    return true;
                };
                match (a.first(), a.get(1)) {
                    (Some(Value::Item(i)), Some(Value::Str(m)))
                        if is_app_window(&w, *i)
                            && matches!(&**m, "close" | "show" | "hide" | "showNormal") =>
                    {
                        out.push((*i, m.to_string()));
                        false
                    }
                    _ => {
                        if qml::log_on!("ri::appdebug") {
                            tracing::debug!(target: "ri::appdebug", "effect method {:?}", a.get(1));
                        }
                        true
                    }
                }
            });
            effects.append(&mut w.effects);
            w.effects = effects;
            out
        };
        for (i, m) in calls {
            if m == "close" {
                self.close_window(i);
            } else {
                self.rt.set(i, "visible", Value::Bool(m != "hide"));
                self.rt.settle();
                self.sync_toplevels();
            }
        }
    }

    /// Ends the focus grabs that went inactive (or whose item died) before `sync_popups` unmaps
    /// their windows: Hyprland hands keyboard focus back (to a popup's parent layer or the last
    /// window) from the grab's focused surface, which must still be mapped, as it is when the
    /// compositor clears the grab itself. A grab outliving its menu leaves the keyboard nowhere.
    fn release_grabs(&mut self) {
        if self.grabs.is_empty() {
            return;
        }
        let mut w = self.rt.world.borrow_mut();
        self.grabs.retain(|&i, (g, _)| {
            let keep = !w.dead.contains(&i) && w.read(i, "active").is_some_and(|v| v.truthy());
            if !keep {
                g.destroy();
            }
            keep
        });
    }

    /// Takes the runtime's new off-screen work (`World::off_screen`) and notes the outputs the
    /// window of each waiting work is on. Before the passes that close windows, while such a
    /// window still has its surface: the outputs it closes on are the ones to wait for, and
    /// it may have entered them after the work was asked for, or close in the pass that is
    /// the first to see the work.
    fn track_off_screen(&mut self) {
        let new = std::mem::take(&mut self.rt.world.borrow_mut().off_screen);
        self.off_screen
            .extend(new.into_iter().map(|(item, run)| OffScreen {
                item,
                outputs: Vec::new(),
                close: None,
                run,
            }));
        if self.session.is_some() {
            self.cancel_off_screen("the session is locked");
            return;
        }
        let closing = self.retiring.iter().map(|r| &r.win);
        let wins = self.wins.iter().chain(closing);
        for o in &mut self.off_screen {
            if let Some(w) = wins.clone().find(|w| w.item == o.item) {
                o.outputs = entered(w.surf.wl_surface());
            }
        }
    }

    /// Arms the off-screen work whose window's surfaces are gone ([`App::show_without`]).
    fn sync_off_screen(&mut self) {
        self.track_off_screen();
        let (gone, waiting) = std::mem::take(&mut self.off_screen)
            .into_iter()
            .partition::<Vec<_>, _>(|o| {
                !self
                    .wins
                    .iter()
                    .chain(self.retiring.iter().map(|r| &r.win))
                    .any(|w| w.item == o.item)
            });
        self.off_screen = waiting;
        for o in gone {
            let Some(mut close) = o.close else {
                self.show_without(o.outputs, o.run);
                continue;
            };
            if close.due.is_none() && !self.hypr_close.alive() {
                // The event socket was lost since: nothing says when the animation began. It
                // has by now.
                close.due = Some(Instant::now() + Duration::from_secs_f64(close.ms / 1000.0));
            }
            self.unshown_ids += 1;
            let id = self.unshown_ids;
            self.closing.push(Closing {
                id,
                item: o.item,
                close,
                outputs: o.outputs,
                run: o.run,
            });
            self.display.sync(&self.qh, OffScreenShown::Closed(id));
        }
        self.run_closed();
    }

    /// The close animations whose beginning Hyprland has not told: of the layers the shell is
    /// about to close ([`App::keep_last_frame`]) and of the ones it closed, with their windows.
    fn closes_awaited(&mut self) -> impl Iterator<Item = (usize, &mut Close)> {
        let kept = self.off_screen.iter_mut();
        let kept = kept.filter_map(|o| Some((o.item, o.close.as_mut()?)));
        let closed = self.closing.iter_mut().map(|c| (c.item, &mut c.close));
        closed.chain(kept).filter(|(_, c)| c.due.is_none())
    }

    /// Reads what Hyprland closed: the animation of a layer work waits on began, and ends its
    /// length later. A window has one layer, whatever work waits on it.
    fn hypr_closed(&mut self) {
        let closed = self.hypr_close.closed();
        let now = Instant::now();
        let began = |c: &mut Close| c.due = Some(now + Duration::from_secs_f64(c.ms / 1000.0));
        match closed {
            Ok(namespaces) => {
                for n in namespaces {
                    let named = self.closes_awaited().find(|(_, c)| c.namespace == n);
                    let Some(item) = named.map(|(item, _)| item) else {
                        continue;
                    };
                    let of_item = self.closes_awaited().filter(|(i, _)| *i == item);
                    of_item.for_each(|(_, c)| began(c));
                }
            }
            // Nothing will say when it began: it has by now, for a layer already closed.
            Err(shell::hyprclose::Lost) => self.closing.iter_mut().for_each(|c| {
                if c.close.due.is_none() {
                    began(&mut c.close)
                }
            }),
        }
        self.run_closed();
    }

    /// Hyprland handled the shell's requests up to the one that closed the layer of
    /// [`Closing`] `id`. It wrote `closelayer` before that if it had the layer mapped
    /// (`CLayerSurface::onDestroy`); one it did not have mapped it does not animate.
    fn closing_synced(&mut self, id: u64) {
        self.hypr_closed();
        let now = Instant::now();
        let unmapped = |c: &&mut Closing| c.id == id && c.close.due.is_none();
        if let Some(c) = self.closing.iter_mut().find(unmapped) {
            c.close.due = Some(now);
        }
        self.run_closed();
    }

    /// Hands the work whose layer Hyprland has finished animating away to
    /// [`App::show_without`]: the first frame shown from now on is without the layer.
    fn run_closed(&mut self) {
        let now = Instant::now();
        let ended = |c: &Closing| c.close.due.is_some_and(|d| d <= now);
        let (ended, closing) = std::mem::take(&mut self.closing)
            .into_iter()
            .partition::<Vec<_>, _>(ended);
        self.closing = closing;
        for c in ended {
            self.show_without(c.outputs, c.run);
        }
    }

    /// Runs `run` once the compositor has shown the screen without a window whose surfaces were
    /// just destroyed: on each of `outputs`, the ones it was on, the shell maps a surface of
    /// its own for this ([`Probe`]), and `run` runs when the first frame of each is presented
    /// (wp_presentation_feedback.presented: the repaint that applied the destruction committed
    /// before it is on screen), or without wp_presentation when the compositor asks each for
    /// its next frame. A window that was on no output was not shown: `run` runs once the
    /// compositor has processed the destruction (wl_display.sync).
    ///
    /// The shell's other windows are not asked when a commit of theirs is shown: a compositor
    /// answers only for a surface it draws, and it does not draw a bar under a fullscreen
    /// window or a window on a hidden workspace, nor repaint for a commit that changes nothing.
    ///
    /// Nor does it draw a probe on an output that is powered off, until it is switched on
    /// again: `run` is dropped then, with a line in the log (zwlr_output_power_v1, where the
    /// compositor has it). The same goes for a locked session, which shows only its lock
    /// surfaces ([`App::cancel_off_screen`]).
    fn show_without(&mut self, outputs: Vec<wl_output::WlOutput>, run: Box<dyn FnOnce() + Send>) {
        self.unshown_ids += 1;
        let id = self.unshown_ids;
        let plugged: Vec<wl_output::WlOutput> = self.outputs.outputs().collect();
        let probes: Vec<Probe> = outputs
            .into_iter()
            .filter(|o| plugged.contains(o))
            .map(|on| {
                let surface = self.compositor.create_surface(&self.qh);
                let layer = self.layer_shell.create_layer_surface(
                    &self.qh,
                    surface,
                    Layer::Overlay,
                    "impulse-shell-shown".to_string(),
                    Some(&on),
                );
                layer.set_geometry(&PROBE);
                layer.set_keyboard_interactivity(KeyboardInteractivity::None);
                if let Ok(r) = Region::new(&self.compositor) {
                    layer.wl_surface().set_input_region(Some(r.wl_region()));
                }
                layer.commit();
                Probe {
                    layer,
                    on,
                    buffer: None,
                }
            })
            .collect();
        if probes.is_empty() {
            self.display.sync(&self.qh, OffScreenShown::Sync(id));
        }
        let off = |p: &Probe| self.output_powers.get(&p.on.id()).is_some_and(|p| !p.1);
        let off = probes.iter().find(|p| off(p)).map(|p| p.on.id());
        self.unshown.push(Unshown { id, probes, run });
        if let Some(o) = off {
            self.probe_output_off(&o);
        }
    }

    /// The compositor configured the probe `layer` (acknowledged already). The commit after
    /// its first configure carries its buffer and asks when that is shown: mapping the probe
    /// makes the compositor repaint. A repeat configure gets no commit. The probe has nothing
    /// new to show, and a compositor does not repaint for a commit that changes nothing, so it
    /// may never answer one that asks; and a commit takes the place of the one before, whose
    /// feedback the compositor then discards without having composed a frame.
    fn probe_configured(&mut self, layer: &ZwlrLayerSurfaceV1) {
        let probes = self.unshown.iter_mut().flat_map(|u| &mut u.probes);
        let Some(p) = probes.into_iter().find(|p| p.layer.is(layer)) else {
            return;
        };
        if p.buffer.is_some() {
            return;
        }
        let s = p.layer.wl_surface();
        let n = PROBE_SIDE;
        let buffer = self
            .pool
            .create_buffer(n, n, n * 4, wl_shm::Format::Argb8888)
            .map_err(|e| e.to_string())
            .and_then(|(b, px)| {
                px.fill(0);
                b.attach_to(s).map_err(|e| format!("{e:?}"))?;
                Ok(b)
            });
        match buffer {
            Ok(b) => {
                s.damage_buffer(0, 0, n, n);
                p.buffer = Some(b);
            }
            Err(e) => {
                // Without a buffer the probe is never mapped and nothing answers for it.
                elog::eprintln!("off screen: no buffer to show: {e}");
                let s = s.clone();
                self.probes_answered(|p| *p.layer.wl_surface() == s);
                return;
            }
        }
        match &self.presentation {
            Some(pres) => {
                pres.feedback(s, &self.qh, OffScreenShown::Probe(s.clone()));
            }
            None => {
                s.frame(&self.qh, OffScreenShown::Probe(s.clone()));
            }
        }
        s.commit();
    }

    /// The probes `of` matches have their answer: the compositor showed their first frame, or
    /// composed it without them ([`App::probe_discarded`]), or closed them, or their output is
    /// gone, where nothing is shown any more. They are destroyed, and the work whose last
    /// probes they were runs.
    fn probes_answered(&mut self, of: impl Fn(&Probe) -> bool) {
        let mut done = Vec::new();
        for u in &mut self.unshown {
            let before = u.probes.len();
            u.probes.retain(|p| !of(p));
            if before > 0 && u.probes.is_empty() {
                done.push(u.id);
            }
        }
        for id in done {
            self.off_screen_shown(id);
        }
    }

    fn off_screen_answered(&mut self, shown: &OffScreenShown) {
        match shown {
            OffScreenShown::Sync(id) => self.off_screen_shown(*id),
            OffScreenShown::Closed(id) => self.closing_synced(*id),
            OffScreenShown::Probe(s) => self.probes_answered(|p| p.layer.wl_surface() == s),
        }
    }

    /// wp_presentation_feedback.discarded for the probe with surface `s`, whose one commit
    /// with a buffer is the only one that asks ([`App::probe_configured`]): the compositor
    /// composed a frame, after that commit, that does not show it (Hyprland never draws a
    /// surface it rounds to one pixel, [`PROBE_SIDE`]). Nothing more is coming for this probe,
    /// and that frame came after the destruction the probe followed: the probe has its answer.
    fn probe_discarded(&mut self, s: &wl_surface::WlSurface) {
        self.probes_answered(|p| p.layer.wl_surface() == s);
    }

    /// The output `o` is powered off: it shows no frame until it is switched on again, so the
    /// work that waits for one of a probe there is dropped, not kept for then.
    fn probe_output_off(&mut self, o: &wayland_client::backend::ObjectId) {
        let of = |u: &Unshown| u.probes.iter().any(|p| p.on.id() == *o);
        while let Some(k) = self.unshown.iter().position(of) {
            let u = self.unshown.remove(k);
            let on = u.probes.iter().find(|p| p.on.id() == *o);
            let name = on
                .and_then(|p| self.outputs.info(&p.on))
                .and_then(|i| i.name)
                .unwrap_or_default();
            elog::eprintln!(
                "off screen: dropped the work of a closed window: its output {name} is powered off"
            );
        }
    }

    /// Drops the work waiting for a window to be off screen, with its probes, saying `why` in
    /// the log: what it would start (hyprpicker) must not start later, when `why` has passed.
    fn cancel_off_screen(&mut self, why: &str) {
        if self.off_screen.is_empty() && self.closing.is_empty() && self.unshown.is_empty() {
            return;
        }
        self.off_screen.clear();
        self.closing.clear();
        self.unshown.clear();
        elog::eprintln!("off screen: dropped the work of a closed window: {why}");
    }

    /// Commits an empty last frame for `win`, which loses its surface next with work waiting
    /// for it to be off screen. A compositor that animates a closed window away (Hyprland's
    /// `layersOut` and `fadeLayersOut`) keeps showing the surface's last frame until the
    /// animation ends, and tells no client when it has; an empty frame has nothing to show, so
    /// the screen is without the window from the repaint that applies the destruction on.
    fn blank(&mut self, win: &mut Win) {
        let (w, h) = win.size;
        if win.mapped.is_none() || w == 0 || h == 0 {
            return;
        }
        // Subsurfaces of the window first: they are part of what is shown of it.
        win.veils.clear();
        win.spare.clear();
        // A compositor applies a GPU frame's commit when its rendering is done, and drops the
        // commits still queued behind one when the surface is destroyed (Hyprland): nothing of
        // this window is being rendered when the empty frame is committed.
        if let (Some(_), Some(gpu)) = (&win.gl, &self.gpu) {
            gpu.finish();
        }
        let s = win.surf.wl_surface();
        // One pixel stretched over the window where the surface has a viewport.
        let (bw, bh) = match &win.scale.obj {
            Some((_, vp)) => {
                s.set_buffer_scale(1);
                vp.set_destination(w as i32, h as i32);
                (1, 1)
            }
            None => {
                let k = win.scale.buf.max(1);
                (w as i32 * k, h as i32 * k)
            }
        };
        let format = wl_shm::Format::Argb8888;
        match self.pool.create_buffer(bw, bh, bw * 4, format) {
            Ok((b, px)) => {
                px.fill(0);
                if b.attach_to(s).is_ok() {
                    s.damage_buffer(0, 0, bw, bh);
                    s.commit();
                }
            }
            Err(e) => elog::eprintln!("off screen: no buffer for an empty last frame: {e}"),
        }
    }

    /// Whether work waits for the window of `item` to be off screen.
    fn off_screen_waits(&self, item: usize) -> bool {
        self.off_screen.iter().any(|o| o.item == item)
    }

    /// Whether the window of `item`, which work waits on and the shell closes next, keeps its
    /// last frame: a layer on a Hyprland whose close animation has a known length, noted on
    /// the work ([`OffScreen::close`]). The panel then leaves the screen the way the user's
    /// Hyprland animates it, as it does for the original, and the work runs after that.
    fn keep_last_frame(&mut self, item: usize) -> bool {
        let layer = |w: &&Win| w.item == item && w.mapped.is_some();
        let namespace = match self.wins.iter().find(layer).map(|w| &w.surf) {
            Some(Surf::Panel { key, .. }) => key.namespace.clone(),
            _ => return false,
        };
        // What Hyprland closed before this layer is not about it.
        self.hypr_closed();
        let Some(ms) = self.hypr_close.close_ms() else {
            return false;
        };
        for o in self.off_screen.iter_mut().filter(|o| o.item == item) {
            let namespace = namespace.clone();
            let due = None;
            o.close = Some(Close { namespace, ms, due });
        }
        true
    }

    /// Before the shell destroys the surfaces of the windows `items`: an empty last frame
    /// ([`App::blank`]) for those work waits on, and for the popups anchored to them, which
    /// close with them and are animated away the same. A layer Hyprland animates away for a
    /// known time keeps its frame ([`App::keep_last_frame`]); its popups, which Hyprland fades
    /// on their own (`fadePopupsOut`), do not.
    fn blank_closing(&mut self, items: &[usize]) {
        let waited = items.iter().copied().filter(|i| self.off_screen_waits(*i));
        let mut family: Vec<usize> = waited.collect();
        if family.is_empty() {
            return;
        }
        let mut kept = Vec::new();
        for i in family.clone() {
            if self.keep_last_frame(i) {
                kept.push(i);
            }
        }
        let mut wins = std::mem::take(&mut self.wins);
        let mut k = 0;
        while k < family.len() {
            for w in &wins {
                if matches!(&w.surf, Surf::Popup { key, .. } if key.parent == family[k])
                    && !family.contains(&w.item)
                {
                    family.push(w.item);
                }
            }
            k += 1;
        }
        family.retain(|i| !kept.contains(i));
        for w in wins.iter_mut().filter(|w| family.contains(&w.item)) {
            self.blank(w);
        }
        self.wins = wins;
    }

    /// The compositor answered [`Unshown`] `id`: the screen is shown without its window.
    fn off_screen_shown(&mut self, id: u64) {
        if let Some(k) = self.unshown.iter().position(|u| u.id == id) {
            tracing::debug!(target: "ri::frames", "off screen: running");
            (self.unshown.remove(k).run)();
        }
    }

    /// Sends the `ShellControl.ipcCall` calls of the turn that just ended (`World::self_ipc`)
    /// round the compositor: their handlers run in the turn that its answer (wl_display.sync)
    /// starts, when it has handled the shell's requests up to here, with the surfaces brought
    /// in line with the windows first. The original made these calls with `qs ipc call`, a process of
    /// its own: by the time its call arrived, Qt had destroyed the surface of a window the
    /// caller closed and the compositor had it off the screen, so a handler that copies the
    /// screen (the region selector opened by a button of the overlay, which closes the overlay
    /// first) never had that window on the copy. Run in the caller's turn, the handler came
    /// before the frame that destroys the surface.
    fn order_self_ipc(&mut self) {
        if self.rt.world.borrow().self_ipc.is_empty() {
            return;
        }
        self.sync_popups();
        let calls = std::mem::take(&mut self.rt.world.borrow_mut().self_ipc);
        self.display.sync(&self.qh, SelfIpc(calls));
    }

    /// Create, move and destroy popup surfaces to follow the QML `PopupWindow`s, after the
    /// passes for the windows they are anchored to.
    fn sync_popups(&mut self) {
        self.track_off_screen();
        self.release_grabs();
        self.sync_toplevels();
        self.sync_panels();
        self.sync_off_screen();
        let want = self.popup_keys();
        // Like Quickshell (WaylandPopupPositioner::reposition), a shown popup whose size or anchor
        // changes is moved with xdg_popup.reposition (v3+) instead of being remapped: a new
        // surface would leave a HyprlandFocusGrab holding the old one, which clears it.
        for win in &mut self.wins {
            let Surf::Popup { popup, key } = &mut win.surf else {
                continue;
            };
            let Some((_, k)) = want.iter().find(|(i, _)| *i == win.item) else {
                continue;
            };
            if k == key || k.parent != key.parent || popup.xdg_popup().version() < 3 {
                continue;
            }
            let Some(pos) = popup_positioner(&self.xdg, k, win.scale.factor) else {
                continue;
            };
            popup.reposition(&pos, 0);
            *key = k.clone();
            win.dirty = true;
        }
        let gone: Vec<usize> = self
            .wins
            .iter()
            .filter(|win| match &win.surf {
                Surf::Popup { key, .. } => !want.iter().any(|(i, k)| *i == win.item && k == key),
                Surf::Panel { .. } | Surf::Lock(_) | Surf::Toplevel(_) => false,
            })
            .map(|win| win.item)
            .collect();
        // The popups anchored to a popup that goes are mapped again once it is. One at a time:
        // a popup of `gone` may be anchored to another one.
        self.blank_closing(&gone);
        for item in gone {
            self.drop_popups_of(&[item]);
            self.wins.retain(|win| win.item != item);
        }
        for (item, key) in want {
            if self.wins.iter().any(|w| w.item == item) {
                continue;
            }
            let Some(parent) = self.wins.iter().find(|w| w.item == key.parent) else {
                continue;
            };
            let Some(pos) = popup_positioner(&self.xdg, &key, parent.scale.factor) else {
                continue;
            };
            // xdg_wm_base.get_popup takes the parent's xdg_surface; a layer surface is not
            // one and adopts the popup with zwlr_layer_surface_v1.get_popup instead.
            let xdg_parent = match &parent.surf {
                Surf::Toplevel(t) => Some(t.xdg_surface()),
                Surf::Popup { popup, .. } => Some(popup.xdg_surface()),
                Surf::Panel { .. } => None,
                Surf::Lock(_) => continue,
            };
            let surface = self.compositor.create_surface(&self.qh);
            let Ok(popup) = Popup::from_surface(xdg_parent, &pos, &self.qh, surface, &self.xdg)
            else {
                continue;
            };
            let on_layer = parent.surf.layer().is_some();
            if let Some(layer) = parent.surf.layer() {
                layer.get_popup(popup.xdg_popup());
            }
            if self
                .rt
                .world
                .borrow_mut()
                .read(item, "mask")
                .is_some_and(|m| !matches!(m, Value::Null | Value::Undefined))
                && let Ok(r) = Region::new(&self.compositor)
            {
                popup.wl_surface().set_input_region(Some(r.wl_region()));
            }
            // A layer surface is the compositor's to end (`closed`, when its screen goes), and
            // one that had ended when the compositor read `get_popup` adopts nothing: wlroots
            // answers the commit of the popup left without a parent with a protocol error,
            // which ends the shell. So the first commit waits for the compositor to have read
            // `get_popup`: `closed`, if there is one, comes before that answer and takes the
            // popup away uncommitted.
            if on_layer {
                let adopted = PopupAdopted(popup.wl_surface().clone());
                self.display.sync(&self.qh, adopted);
            } else {
                popup.wl_surface().commit();
            }
            self.wins.push(Win {
                gl: None,
                item,
                size: (key.size.0 as u32, key.size.1 as u32),
                surf: Surf::Popup { popup, key },
                configured: false,
                dirty: true,
                anim: false,
                pending: false,
                shown: None,
                committed: None,
                asked: None,
                laid: (u64::MAX, (0, 0)),
                unsettled: 0,
                region: None,
                last: None,
                shm_prev: None,
                veils: Vec::new(),
                spare: Vec::new(),
                swapped: false,
                scale: WinScale::default(),
                mapped: None,
            });
            self.ensure_scaler(self.wins.len() - 1);
        }
        self.leave_gone_focus();
    }
}

/// `key` for a popup of a window on a screen of factor `f`.
fn popup_positioner(xdg: &XdgShell, key: &PopupKey, f: f64) -> Option<XdgPositioner> {
    let pos = XdgPositioner::new(xdg).ok()?;
    let px = |v: i32| to_surface(v, f);
    pos.set_size(px(key.size.0).max(1), px(key.size.1).max(1));
    let (x, y, w, h) = key.rect;
    pos.set_anchor_rect(px(x), px(y), px(w).max(1), px(h).max(1));
    pos.set_anchor(edge_anchor(key.edges));
    pos.set_gravity(edge_gravity(key.gravity));
    pos.set_constraint_adjustment(ConstraintAdjustment::from_bits_truncate(key.adjust));
    // xdg_positioner.set_reactive (since v3): the compositor re-runs the constraint
    // adjustment when the parent moves or resizes, for new and repositioned popups alike.
    if xdg.xdg_wm_base().version() >= 3 {
        pos.set_reactive();
    }
    Some(pos)
}

impl PopupHandler for App {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        popup: &Popup,
        c: PopupConfigure,
    ) {
        if let Some(k) = self
            .wins
            .iter()
            .position(|w| w.surf.wl_surface() == popup.wl_surface())
        {
            let w = &mut self.wins[k];
            if c.width > 0 && c.height > 0 {
                w.set_surface_size((c.width as u32, c.height as u32));
            }
            w.configured = true;
            self.draw(k);
        }
    }
    /// The compositor dismissed the popup: like QWindow::close() on xdg_popup.done, the
    /// PopupWindow turns invisible (its `visible` binding stays and can show it again);
    /// otherwise the next sync would map it right back.
    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, popup: &Popup) {
        let items: Vec<usize> = self
            .wins
            .iter()
            .filter(|w| w.surf.wl_surface() == popup.wl_surface())
            .map(|w| w.item)
            .collect();
        // QtWayland closes a closed window's popups with it.
        let mut closed = self.drop_popups_of(&items);
        self.wins
            .retain(|w| w.surf.wl_surface() != popup.wl_surface());
        self.leave_gone_focus();
        closed.extend(items);
        for i in closed {
            self.window_closed(i);
        }
    }
}

impl App {
    /// zwlr_layer_surface_v1.closed: the compositor will not show this surface again. Its
    /// xdg_popups go first (xdg_shell: destroying a parent before its popups is a protocol
    /// error), then the surface. Quickshell's LayerSurface closes the QWindow here, so the
    /// window turns invisible and emits `closed` (ProxyWindowBase::onVisibleChanged); its
    /// `visible` binding stays and can show it again.
    fn layer_closed(&mut self, layer: &ZwlrLayerSurfaceV1) {
        self.probes_answered(|p| p.layer.is(layer));
        self.retiring
            .retain(|r| !r.win.surf.layer().is_some_and(|l| l.is(layer)));
        let closed = self
            .wins
            .iter()
            .find(|w| w.surf.layer().is_some_and(|l| l.is(layer)))
            .map(|w| w.item);
        if let Some(item) = closed {
            self.close_panel(item);
        }
    }

    /// The panels created for output `o`, which is gone, close as on
    /// `zwlr_layer_surface_v1.closed`. The compositor sends that for a layer surface on a
    /// removed output, but not for one whose `get_layer_surface` it read after removing the
    /// output: that request names a dead wl_output, so the compositor picks an output itself
    /// and the panel would stay there for good.
    fn close_panels_on(&mut self, o: &wl_output::WlOutput) {
        let on = |w: &Win| matches!(&w.surf, Surf::Panel { on: Some(x), .. } if x == o);
        self.retiring.retain(|r| !on(&r.win));
        let closed: Vec<usize> = self.wins.iter().filter(|w| on(w)).map(|w| w.item).collect();
        for item in closed {
            self.close_panel(item);
        }
    }

    /// Destroys panel `item`'s layer surface and its popups, and closes the windows.
    fn close_panel(&mut self, item: usize) {
        let popups = self.drop_popups_of(&[item]);
        self.wins.retain(|w| w.item != item);
        self.leave_gone_focus();
        // QtWayland closes a closed window's popups with it.
        for p in popups {
            self.window_closed(p);
        }
        self.window_closed(item);
    }

    /// Destroys the popups anchored to the windows `parents`, directly or through other popups,
    /// each before the popup it is anchored to (xdg_popup: a client destroys its popups from
    /// the topmost down, and a parent only after them). Returns their items, in that order.
    fn drop_popups_of(&mut self, parents: &[usize]) -> Vec<usize> {
        let mut family = parents.to_vec();
        let mut k = 0;
        while k < family.len() {
            for w in &self.wins {
                if matches!(&w.surf, Surf::Popup { key, .. } if key.parent == family[k])
                    && !family.contains(&w.item)
                {
                    family.push(w.item);
                }
            }
            k += 1;
        }
        // `family` has every popup after the window it is anchored to.
        let popups: Vec<usize> = family
            .into_iter()
            .rev()
            .filter(|i| !parents.contains(i))
            .collect();
        for i in &popups {
            self.wins.retain(|w| w.item != *i);
        }
        popups
    }

    /// Windows lost their surfaces: the pointer and keyboard focus they held end with them.
    fn leave_gone_focus(&mut self) {
        self.leave_gone_pointer_focus();
        self.leave_gone_keyboard_focus();
    }

    /// The window holding keyboard focus or activation lost its surface (closed, hidden, or
    /// replaced by a new surface while the old one retires): it is deactivated at once, as
    /// QtWayland's QWaylandInputDevice::handleWindowDestroyed and
    /// QWaylandDisplay::handleWindowDestroyed have it, so the item's next surface is not
    /// focused before its own wl_keyboard.enter.
    fn leave_gone_keyboard_focus(&mut self) {
        let wins = &self.wins;
        if !self
            .kbd_act
            .retain(|s| wins.iter().any(|w| w.surf.wl_surface() == s))
        {
            return;
        }
        if qml::log_on!("ri::keys") {
            tracing::debug!(target: "ri::keys", "kbd focus surface gone");
        }
        self.key_repeat.stop();
        self.held_syms.clear();
        qml::ports::clip::keyboard_focus(&self.rt.world.borrow().waker, false);
        self.paste.cancel();
        self.forget_offer();
    }

    /// The window holding pointer focus lost its surface (closed, hidden, or replaced by a new
    /// surface while the old one retires): the pointer left it, as QtWayland's
    /// QWaylandInputDevice::handleWindowDestroyed and QGuiApplication's leave for a
    /// destroyed window have it. No wl_pointer.leave reaches it: the compositor sends that
    /// to the old surface, which no window owns any more.
    fn leave_gone_pointer_focus(&mut self) {
        let Some(s) = &self.ptr_focus else {
            return;
        };
        if self.wins.iter().any(|w| w.surf.wl_surface() == s) {
            return;
        }
        self.ptr_focus = None;
        // Motion is only sent to the focused surface: a pending one was for the gone surface.
        self.motion_merge.take();
        self.ptr.leave(&mut self.rt);
    }

    /// ProxyWindowBase::onVisibleChanged after the platform window was closed.
    fn window_closed(&mut self, item: usize) {
        let mut w = self.rt.world.borrow_mut();
        w.set_internal(item, "visible", Value::Bool(false));
        qml::native::emit(&mut w, item, "closed", &[], Vec::new());
        drop(w);
        self.rt.settle();
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: <ZwlrLayerShellV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for App {
    fn event(
        app: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let is = |w: &Win| w.surf.layer().is_some_and(|l| l.is(layer));
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer.ack_configure(serial);
                match app.wins.iter().position(is) {
                    Some(k) => app.layer_configure(k, (width, height)),
                    None => app.probe_configured(layer),
                }
            }
            zwlr_layer_surface_v1::Event::Closed => app.layer_closed(layer),
            _ => {}
        }
    }
}

impl App {
    fn layer_configure(&mut self, k: usize, new_size: (u32, u32)) {
        {
            let w = &mut self.wins[k];
            let before = (w.configured, w.size);
            let cur = w.surface_size();
            let pick = |new: u32, cur: u32| if new > 0 { new } else { cur };
            w.set_surface_size((pick(new_size.0, cur.0), pick(new_size.1, cur.1)));
            w.configured = true;
            if qml::log_on!("ri::stats") {
                tracing::debug!(target: "ri::stats", "configure {new_size:?} -> {:?}", w.size);
            }
            // A repeat configure at the shown size only needs its ack committed; the frame on
            // screen is still current (the next change draws as usual).
            if before == (true, w.size) && w.last.is_some() && !w.dirty && !w.anim {
                let s = w.surf.wl_surface();
                log_presented(self.presentation.as_ref(), s, &self.qh);
                s.commit();
                return;
            }
            self.publish_window_rects();
            self.draw(k);
        }
    }

    /// Where each shown layer surface sits on its output, for edit-mode hit-testing
    /// (`World::window_rects`): wlroots/Hyprland arrange_layers places exclusive surfaces in
    /// mapping order, each inside the space the earlier ones left, then the others inside the
    /// space all exclusive zones leave; a surface ignoring exclusive zones gets the whole output.
    fn publish_window_rects(&mut self) {
        let outputs: Vec<(String, (f64, f64))> = self
            .outputs
            .outputs()
            .filter_map(|o| {
                let i = self.outputs.info(&o)?;
                let (w, h) = i.logical_size.unwrap_or((1920, 1080));
                Some((i.name.clone().unwrap_or_default(), (w as f64, h as f64)))
            })
            .collect();
        let panels: Vec<(usize, &PanelKey, (u32, u32))> = self
            .wins
            .iter()
            .filter(|w| w.configured)
            .filter_map(|w| match &w.surf {
                Surf::Panel { key, .. } => Some((w.item, key, w.size)),
                _ => None,
            })
            .collect();
        let mut rects: std::collections::HashMap<usize, qml::edit_mode::WinRect, _> =
            Default::default();
        for (n, &(item, key, size)) in panels.iter().enumerate() {
            let Some((name, (ow, oh))) = (match &key.output {
                Some(o) => outputs.iter().find(|(n, _)| n == o),
                None => outputs.first(),
            }) else {
                continue;
            };
            let mut area = (0.0, 0.0, *ow, *oh);
            if key.geom.zone >= 0 {
                for (m, &(_, other, _)) in panels.iter().enumerate() {
                    let before = other.geom.zone > 0 && (key.geom.zone <= 0 || m < n);
                    if m == n || !before || other.output.as_ref().is_some_and(|o| o != name) {
                        continue;
                    }
                    let (t, r, b, l) = shell::layer::reserved(&other.geom);
                    area = (area.0 + l, area.1 + t, area.2 - l - r, area.3 - t - b);
                }
            }
            let (x, y) = shell::layer::place(key.geom.anchor, key.geom.margins, size, area);
            rects.insert(
                item,
                qml::edit_mode::WinRect {
                    connector: name.clone(),
                    x,
                    y,
                    w: size.0 as f64,
                    h: size.1 as f64,
                },
            );
        }
        let mut w = self.rt.world.borrow_mut();
        if w.window_rects != rects {
            w.window_rects = rects;
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

const CAP_CACHE_MAX: usize = 32;
/// `ipc call ri-debug dump` under `RI_DEBUG=dump` while no `IpcHandler` claims that target
/// (it is not listed by `ipc show`).
fn is_dump_call(rt: &qml::runtime::Runtime, c: &shell::ipc::Call) -> bool {
    shell::ipc::is_debug_dump(c, qml::debug::opts()) && {
        let mut w = rt.world.borrow_mut();
        !w.of_type("IpcHandler").into_iter().any(|h| {
            !w.dead.contains(&h)
                && w.read(h, "target")
                    .is_some_and(|t| t.to_js_string() == c.target)
        })
    }
}

/// Item tree, clickables, live item counts and workspace/icon state, to `ri-tree.txt`,
/// `ri-click.txt`, `ri-live.txt` and `ri-ws.txt` in the instance's run directory; the paths
/// written.
fn write_dump(app: &mut App) -> Vec<String> {
    let mut files = vec![];
    let mut put = |name: &str, out: String| {
        if let Some(p) = shell::ipc::write_debug_file(name, out.as_bytes()) {
            files.push(p.display().to_string());
        }
    };
    let mut out = String::new();
    for w in &app.wins {
        dump_tree(&app.rt, w.item, 0, &mut out);
    }
    put("ri-tree.txt", out);
    let mut out = String::new();
    for w in &app.wins {
        let ns = match &w.surf {
            Surf::Panel { key, .. } => key.namespace.clone(),
            Surf::Popup { .. } => format!("popup:{}", w.item),
            Surf::Lock(_) => format!("lock:{}", w.item),
            Surf::Toplevel(_) => format!("toplevel:{}", w.item),
        };
        dump_clickables(&app.rt, w.item, &ns, &mut out);
    }
    put("ri-click.txt", out);
    {
        let w = app.rt.world.borrow();
        let mut by: std::collections::BTreeMap<&str, usize> = Default::default();
        for i in w.items.ids().filter(|i| !w.dead.contains(i)) {
            *by.entry(w.items[i].types.first().map_or("?", |t| t.as_str()))
                .or_default() += 1;
        }
        let mut v: Vec<_> = by.into_iter().collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.1));
        let out: String = v.iter().map(|(t, n)| format!("{n} {t}\n")).collect();
        put(
            "ri-live.txt",
            format!(
                "live {} slots {}\n{out}",
                w.items.len() - w.dead.len(),
                w.items.len()
            ),
        );
    }
    let mut out = String::new();
    let wsm = app.rt.world.borrow_mut().of_type("WorkspaceModel");
    for i in wsm {
        let occ = app.rt.get(i, "occupied").to_js_string();
        let bw = match app.rt.get(i, "biggestWindow") {
            Value::Obj(o) => o
                .borrow()
                .array
                .clone()
                .unwrap_or_default()
                .iter()
                .map(|v| v.to_js_string())
                .collect::<Vec<_>>()
                .join(","),
            v => format!("{v:?}"),
        };
        out.push_str(&format!("{i} occupied=[{occ}] biggest=[{bw}]\n"));
    }
    {
        let w = app.rt.world.borrow();
        for (i, it) in w.items.iter().enumerate() {
            if !it.types.iter().any(|t| t == "Kirigami.Icon") || w.dead.contains(&i) {
                continue;
            }
            let pw = it.props.get("width").map(|p| {
                (
                    format!("{:?}", p.value),
                    p.binding.is_some(),
                    p.alias.is_some(),
                )
            });
            let src = it.props.get("source").map(|p| p.value.to_js_string());
            out.push_str(&format!(
                "icon {i} width={pw:?} assigned={} src={src:?}\n",
                w.is_assigned(i, "width")
            ));
        }
    }
    put("ri-ws.txt", out);
    files
}

/// When the earliest running animation (wheel flick, SwipeView move) started.
fn animations_since(app: &App) -> f64 {
    let w = app.rt.world.borrow();
    w.runs
        .iter()
        .map(|r| r.start)
        .chain(
            w.flicks
                .values()
                .chain(w.flicks_x.values())
                .filter(|f| f.active)
                .map(|f| f.started()),
        )
        .chain(
            w.swipes
                .values()
                .filter(|s| !s.settled)
                .map(|s| s.started()),
        )
        .chain(
            w.view_hl
                .values()
                .filter(|h| !h.settled)
                .map(|h| h.started()),
        )
        .chain(w.frame_anims_ticking().map(|(_, start)| start))
        .fold(f64::INFINITY, f64::min)
}

/// The animation clock's current time (`World::now`, ms from `start`) as an `Instant`.
fn anim_instant(start: Instant, rt: &qml::runtime::Runtime) -> Instant {
    start + Duration::from_secs_f64(rt.world.borrow().now.max(0.0) / 1000.0)
}

/// Animations (Behaviors, transitions, flicks, SwipeView moves) are registered with the animation
/// clock. Endless animations that show nothing and FrameAnimations out of sight
/// (`App::animating_windows`) do not keep it running: Timers go on following the wall clock
/// while they wait.
fn animations_running(app: &App) -> bool {
    {
        let w = app.rt.world.borrow();
        if w.flicks_active()
            || w.swipes_active()
            || w.highlights_active()
            || w.view_tracks_moving().next().is_some()
        {
            return true;
        }
        // A theme crossfade (`shell::gpu::Fade`) runs on this clock like any animation.
        if app
            .wins
            .iter()
            .any(|win| win.gl.as_ref().is_some_and(|g| g.fading()))
        {
            return true;
        }
        // A FrameAnimation is a job of the animation driver (QFrameAnimationJob).
        if w.runs.is_empty() && w.frame_anims_ticking().next().is_none() {
            return false;
        }
    }
    !app.only_unseen()
}

struct ScJob {
    item: usize,
    handle: Option<u32>,
    w: usize,
    h: usize,
    opaque: bool,
    has_tex: bool,
    src: ShmView,
    /// Displayed size when smaller than the frame: only that is sampled and uploaded.
    dst: Option<(usize, usize)>,
}

/// A capture's shm buffer, read by the worker in place. The capture keeps the mapping alive
/// and requests no new frame into it until the worker clears `busy`.
struct ShmView {
    ptr: *const u8,
    len: usize,
    stride: usize,
    inv: bool,
    busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

// SAFETY: see `ShmView`; the memory outlives the job and is not written while `busy`.
unsafe impl Send for ShmView {}

enum ScMsg {
    Frame(ScJob),
    Forget(usize),
}

/// What the worker made of an item's frame.
enum ScOut {
    Frame(ScDone),
    /// Same pixels as the texture shown already.
    Same(usize),
}

/// Item, toplevel handle, frame size, texture to show, and the ≤480 px copy kept in
/// `cap_cache` for reopening.
type ScDone = (
    usize,
    Option<u32>,
    (usize, usize),
    qsg::raster::Canvas,
    Option<qsg::raster::Canvas>,
);

/// The frame as the GPU shows it drawn into a `dw`×`dh` rect with GL_LINEAR filtering
/// (what Qt's ScreencopyView texture gets), sampled straight from the BGRA rows.
fn sc_sample(
    raw: &[u8],
    (sw, sh): (usize, usize),
    opaque: bool,
    (dw, dh): (usize, usize),
) -> qsg::raster::Canvas {
    let axis = |n: usize, d: usize| -> Vec<(usize, usize, f32)> {
        (0..d)
            .map(|x| {
                let u = ((x as f32 + 0.5) * n as f32 / d as f32 - 0.5).max(0.0);
                let i0 = (u as usize).min(n - 1);
                (i0, (i0 + 1).min(n - 1), u - i0 as f32)
            })
            .collect()
    };
    let (xs, ys) = (axis(sw, dw), axis(sh, dh));
    let mut cv = qsg::raster::Canvas::new(dw, dh, [0, 0, 0, 0]);
    let px = |x: usize, y: usize| -> [f32; 4] {
        let s = &raw[(y * sw + x) * 4..][..4];
        [s[2] as f32, s[1] as f32, s[0] as f32, s[3] as f32]
    };
    for (y, &(y0, y1, fy)) in ys.iter().enumerate() {
        for (x, &(x0, x1, fx)) in xs.iter().enumerate() {
            let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
            let mut o = [0u8; 4];
            for k in 0..4 {
                let top = a[k] + (b[k] - a[k]) * fx;
                let bot = c[k] + (d[k] - c[k]) * fx;
                o[k] = (top + (bot - top) * fy).round().clamp(0.0, 255.0) as u8;
            }
            if opaque {
                o[3] = 255;
            }
            cv.px[y * dw + x] = o;
        }
    }
    cv
}
const THUMB_W: usize = 480;
/// The App's screencopy worker: dedup + BGRA→RGBA conversion off the main thread, which only
/// copies the rows of the frames it reports on `done`.
struct ScWorker {
    /// `None` only inside `drop`.
    tx: Option<std::sync::mpsc::Sender<ScMsg>>,
    done: std::sync::mpsc::Receiver<ScOut>,
    /// Set by `drop`: the worker ends before the next frame instead of converting the queue.
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// The worker reads the captures' buffers in place (`ShmView`), so it is gone before whoever
/// drops it goes on to free them: `drop` returns once the thread ended.
impl Drop for ScWorker {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        // Disconnects the channel, which wakes a worker waiting for a frame.
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // Before any field goes: the worker may be reading a capture's buffer (`ShmView`).
        self.sc = None;
    }
}

/// Gives up the frame of `m` that no worker took: its buffer is free for the next frame
/// and its capture waits for none to land.
fn sc_drop(caps: &mut rustc_hash::FxHashMap<usize, Capture>, m: ScMsg) {
    let ScMsg::Frame(j) = m else { return };
    j.src
        .busy
        .store(false, std::sync::atomic::Ordering::Release);
    if let Some(c) = caps.get_mut(&j.item) {
        c.landing = false;
    }
}

impl ScWorker {
    /// Starts the worker; none where the system refuses its thread (`EAGAIN`), which is said
    /// once in the log.
    fn start(waker: qml::svc::Handle) -> Option<ScWorker> {
        let (tx, rx) = std::sync::mpsc::channel::<ScMsg>();
        let (done_tx, done) = waker.channel::<ScOut>();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = stop.clone();
        let spawned = std::thread::Builder::new()
            .name("screencopy".into())
            .spawn(move || {
                // Hyprland re-delivers unchanged frames; keep the uploaded texture.
                let mut store = std::collections::HashMap::<usize, (usize, usize, Vec<u8>)>::new();
                for m in rx {
                    if stopped.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    let j = match m {
                        ScMsg::Forget(i) => {
                            store.remove(&i);
                            continue;
                        }
                        ScMsg::Frame(j) => j,
                    };
                    let (w, h) = (j.w, j.h);
                    let st = store.entry(j.item).or_insert_with(|| (0, 0, Vec::new()));
                    let mut changed = (st.0, st.1) != (w, h);
                    if changed {
                        *st = (w, h, vec![0; w * h * 4]);
                    }
                    {
                        // SAFETY: `ShmView` contract.
                        let data = unsafe { std::slice::from_raw_parts(j.src.ptr, j.src.len) };
                        for y in 0..h {
                            let sy = if j.src.inv { h - 1 - y } else { y };
                            let Some(s) =
                                data.get(sy * j.src.stride..).and_then(|r| r.get(..w * 4))
                            else {
                                break;
                            };
                            let d = &mut st.2[y * w * 4..][..w * 4];
                            if d != s {
                                d.copy_from_slice(s);
                                changed = true;
                            }
                        }
                    }
                    j.src
                        .busy
                        .store(false, std::sync::atomic::Ordering::Release);
                    if stopped.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    // Hyprland re-delivers unchanged frames; keep the uploaded texture.
                    if !changed && j.has_tex {
                        let _ = done_tx.send(ScOut::Same(j.item));
                        continue;
                    }
                    let raw = &store[&j.item].2;
                    let cv = match j.dst {
                        Some(d) => sc_sample(raw, (j.w, j.h), j.opaque, d),
                        None => {
                            let mut cv = qsg::raster::Canvas::new(j.w, j.h, [0, 0, 0, 0]);
                            for (d, s) in cv.px.iter_mut().zip(raw.as_chunks::<4>().0) {
                                *d = [s[2], s[1], s[0], if j.opaque { 255 } else { s[3] }];
                            }
                            cv
                        }
                    };
                    let thumb = (cv.w > THUMB_W).then(|| {
                        let h = (cv.h * THUMB_W / cv.w).max(1);
                        qsg::image::smooth_scale(&cv, THUMB_W, h, true)
                    });
                    let _ = done_tx.send(ScOut::Frame((j.item, j.handle, (j.w, j.h), cv, thumb)));
                }
            });
        let thread = match spawned {
            Ok(thread) => thread,
            Err(e) => {
                static SAID: std::sync::Once = std::sync::Once::new();
                SAID.call_once(|| elog::eprintln!("screencopy: cannot start its thread: {e}"));
                return None;
            }
        };
        Some(ScWorker {
            tx: Some(tx),
            done,
            stop,
            thread: Some(thread),
        })
    }

    /// Queues `m` for the worker; gives it back if the worker is gone.
    fn send(&self, m: ScMsg) -> Result<(), ScMsg> {
        match &self.tx {
            Some(tx) => tx.send(m).map_err(|e| e.0),
            None => Err(m),
        }
    }
}

#[derive(Default)]
struct Capture {
    handle: Option<u32>,
    frame: Option<HyprlandToplevelExportFrameV1>,
    /// shm format, width, height, stride offered for the frame in flight.
    shm: Option<(wl_shm::Format, i32, i32, i32)>,
    buffer: Option<(smithay_client_toolkit::shm::slot::Buffer, wl_shm::Format)>,
    y_invert: bool,
    got: bool,
    /// A frame failed: like Quickshell's stopped context, no frames until the source changes.
    stopped: bool,
    /// A copied frame is being converted or compared and not installed yet.
    landing: bool,
    /// A window presented the installed frame since it was installed.
    painted: bool,
    /// `ScreencopyView.live` and shown, as of the last full sync.
    live: bool,
    /// Own pool so the worker can read the buffer in place (`ShmView`).
    pool: Option<SlotPool>,
    busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// dmabuf fourcc, width, height offered for the frame in flight.
    dma: Option<(u32, u32, u32)>,
    /// The frame in flight copies into the toplevel's `DmaSlot`.
    dma_used: bool,
    /// The frame in flight got an event, so Hyprland created it.
    seen: bool,
    /// `ScreencopyView.paintCursor`: the frames are asked for with the cursor on them.
    cursor: bool,
}

/// The pool a capture's frames are copied into, created for its first frame; `None` when the
/// system refuses its memory (no descriptor left, no address space).
fn frame_pool<'a>(
    pool: &'a mut Option<SlotPool>,
    len: usize,
    shm: &Shm,
) -> Option<&'a mut SlotPool> {
    if pool.is_none() {
        match SlotPool::new(len, shm) {
            Ok(p) => *pool = Some(p),
            Err(e) => elog::eprintln!("screencopy: no memory for a window frame: {e}"),
        }
    }
    pool.as_mut()
}

/// Export frames dropped before their first event. Hyprland's captureToplevel creates no frame
/// for a window that is already gone, so any request on it (even `destroy`) is a protocol error
/// that kills the connection; a created frame gets `buffer_done` or `failed` at once, and is
/// destroyed when that arrives.
struct Unanswered<T>(Vec<T>);

impl<T: PartialEq> Unanswered<T> {
    /// `f` to destroy now, or `None` while Hyprland has not answered it.
    fn retire(&mut self, f: T, seen: bool) -> Option<T> {
        if seen {
            return Some(f);
        }
        self.0.push(f);
        None
    }

    /// An event for `f`: it to destroy if it was retired unanswered.
    fn answered(&mut self, f: &T) -> Option<T> {
        let p = self.0.iter().position(|o| o == f)?;
        Some(self.0.swap_remove(p))
    }
}

struct DmaSlot {
    _bo: shell::gbm::Bo,
    buf: WlBuffer,
    tex: glow::Texture,
    img: usize,
    key: (u32, u32, u32),
    /// `App::dma_epoch` it was made under.
    epoch: u64,
    /// Frames showing `tex`; the slot outlives them (saved frames and layers redraw them).
    views: Vec<std::rc::Weak<qsg::raster::Canvas>>,
    /// Last frame, to drop re-delivered unchanged ones.
    copy: Option<shell::gpu::FrameCopy>,
}

/// Reads back the `queued` (index into the batch, slot) screencopy compares into `changed`.
fn read_compares(
    g: &mut shell::gpu::Gpu,
    keep: &mut rustc_hash::FxHashMap<u32, DmaSlot>,
    queued: &mut Vec<(usize, u32)>,
    changed: &mut [bool],
) {
    for ((k, h), c) in std::mem::take(queued).into_iter().zip(g.compare_read()) {
        changed[k] = c;
        if let Some(slot) = keep.get_mut(&h) {
            shell::gpu::Gpu::compare_apply(&mut slot.copy, c);
        }
    }
}

/// User data of dmabuf `wl_buffer`s.
struct DmaBuf;

impl Capture {
    fn busy(&self) -> bool {
        self.busy.load(std::sync::atomic::Ordering::Acquire)
    }

    /// The same capture with none of its frames: what is left of it when the GPU context its
    /// frames were textures of is lost.
    fn restarted(&self) -> Capture {
        Capture {
            handle: self.handle,
            cursor: self.cursor,
            stopped: self.stopped,
            live: self.live,
            ..Default::default()
        }
    }

    /// `overlay_cursor` of hyprland_toplevel_export_manager_v1.capture_toplevel.
    fn overlay_cursor(&self) -> i32 {
        i32::from(self.cursor)
    }

    /// The next frame is due: the first one, then, while live, each once a window presented
    /// the last (QQuickItem::updatePaintNode re-arms a live ScreencopyView; the compositor
    /// answers that copy once the toplevel is damaged).
    fn wants_frame(&self) -> bool {
        self.handle.is_some()
            && !self.stopped
            && self.frame.is_none()
            && !self.busy()
            && !self.landing
            && (!self.got || (self.live && self.painted))
    }
}

impl Dispatch<ZwpLinuxDmabufV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ZwpLinuxDmabufV1,
        e: zwp_linux_dmabuf_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // v3 only: v4+ compositors send feedback instead.
        if let zwp_linux_dmabuf_v1::Event::Modifier {
            format,
            modifier_hi,
            modifier_lo,
        } = e
        {
            let m = (u64::from(modifier_hi) << 32) | u64::from(modifier_lo);
            app.dma_params.add_v3_modifier(format, m);
        }
    }
}

impl Dispatch<ZwpLinuxDmabufFeedbackV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ZwpLinuxDmabufFeedbackV1,
        e: zwp_linux_dmabuf_feedback_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwp_linux_dmabuf_feedback_v1::Event;
        let b = &mut app.dma_params_next;
        match e {
            Event::FormatTable { fd, size } => b.format_table(&fd, size),
            Event::MainDevice { device } => b.main_device(&device),
            Event::TrancheTargetDevice { device } => b.tranche_target_device(&device),
            Event::TrancheFlags { flags } => b.tranche_flags(match flags {
                wayland_client::WEnum::Value(f) => f.bits(),
                wayland_client::WEnum::Unknown(f) => f,
            }),
            Event::TrancheFormats { indices } => b.tranche_formats(&indices),
            Event::TrancheDone => b.tranche_done(),
            Event::Done => app.dma_feedback_done(),
            _ => {}
        }
    }
}

impl Dispatch<ZwpLinuxBufferParamsV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ZwpLinuxBufferParamsV1,
        e: zwp_linux_buffer_params_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_linux_buffer_params_v1::Event::Failed = e {
            elog::eprintln!("screencopy: dmabuf import failed, using wl_shm");
            app.dma_off = true;
        }
    }
}

impl Dispatch<WlBuffer, DmaBuf> for App {
    fn event(
        _: &mut Self,
        _: &WlBuffer,
        _: <WlBuffer as Proxy>::Event,
        _: &DmaBuf,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<HyprlandToplevelExportManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &HyprlandToplevelExportManagerV1,
        _: <HyprlandToplevelExportManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<HyprlandToplevelExportFrameV1, usize> for App {
    fn event(
        app: &mut Self,
        frame: &HyprlandToplevelExportFrameV1,
        event: hyprland_toplevel_export_frame_v1::Event,
        item: &usize,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use hyprland_toplevel_export_frame_v1::Event;
        let i = *item;
        if let Some(f) = app.frames_unanswered.answered(frame) {
            f.destroy();
            return;
        }
        if let Some(c) = app.caps.get_mut(&i)
            && c.frame.as_ref() == Some(frame)
        {
            c.seen = true;
        }
        let shown = if matches!(event, Event::Ready { .. }) {
            let mut w = app.rt.world.borrow_mut();
            let mut n = |k| w.read(i, k).map_or(0.0, |v| v.to_number());
            // Sampled once at the texels of the item's Qt 6.12 layer texture (rounded size).
            (n("width").round() as usize, n("height").round() as usize)
        } else {
            (0, 0)
        };
        if matches!(event, Event::BufferDone) {
            let want = app
                .caps
                .get(&i)
                .filter(|c| c.frame.as_ref() == Some(frame))
                .and_then(|c| Some((c.handle?, c.dma?, c.got)));
            if let Some((h, key, got)) = want
                && let Some(buf) = app.dma_slot(h, key)
            {
                frame.copy(&buf, if got { 0 } else { 1 });
                if let Some(c) = app.caps.get_mut(&i) {
                    c.dma_used = true;
                }
                return;
            }
        }
        let shm = &app.shm;
        let Some(cap) = app.caps.get_mut(&i) else {
            frame.destroy();
            return;
        };
        if cap.frame.as_ref() != Some(frame) {
            return;
        }
        match event {
            Event::Buffer {
                format: wayland_client::WEnum::Value(f),
                width,
                height,
                stride,
            } if cap.shm.is_none()
                && matches!(f, wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888)
                && shm_geometry_ok(width, height, stride) =>
            {
                cap.shm = Some((f, width as i32, height as i32, stride as i32));
            }
            Event::LinuxDmabuf {
                format,
                width,
                height,
            } if cap.dma.is_none() => cap.dma = Some((format, width, height)),
            Event::Ready { .. } if cap.dma_used => {
                if let (Some(h), Some((_, w, ht))) = (cap.handle, cap.dma) {
                    app.dma_ready
                        .push((i, h, (w as usize, ht as usize), cap.y_invert, shown));
                    cap.landing = true;
                }
                app.dma_ok = true;
                frame.destroy();
                cap.frame = None;
                cap.shm = None;
                cap.dma = None;
                cap.dma_used = false;
                cap.got = true;
            }
            Event::Flags { flags } => {
                cap.y_invert = match flags {
                    wayland_client::WEnum::Value(f) => {
                        f.contains(hyprland_toplevel_export_frame_v1::Flags::YInvert)
                    }
                    wayland_client::WEnum::Unknown(b) => b & 1 != 0,
                };
            }
            Event::BufferDone => {
                // No buffer type this client can use: Quickshell ends the capture too.
                let Some((f, w, h, stride)) = cap.shm else {
                    frame.destroy();
                    cap.frame = None;
                    app.stop_capture(i);
                    return;
                };
                // No memory for the frame: this capture ends, the shell goes on.
                let Some(pool) = frame_pool(&mut cap.pool, (stride * h) as usize, shm) else {
                    frame.destroy();
                    cap.frame = None;
                    app.stop_capture(i);
                    return;
                };
                let reuse = cap
                    .buffer
                    .as_ref()
                    .is_some_and(|(b, bf)| b.height() == h && b.stride() == stride && *bf == f);
                if !reuse {
                    cap.buffer = None;
                    cap.buffer = pool.create_buffer(w, h, stride, f).ok().map(|b| (b.0, f));
                }
                match &cap.buffer {
                    Some((b, _)) => frame.copy(b.wl_buffer(), if cap.got { 0 } else { 1 }),
                    None => {
                        frame.destroy();
                        cap.frame = None;
                        app.stop_capture(i);
                    }
                }
            }
            Event::Ready { .. } => {
                let mut job = None;
                let inv = cap.y_invert;
                if let (Some((f, w, h, stride)), Some((b, _)), Some(pool)) =
                    (cap.shm, &cap.buffer, cap.pool.as_mut())
                    && let Some(data) = pool.raw_data_mut(&b.slot()).get(..(stride * h) as usize)
                {
                    let (w, h, stride) = (w as usize, h as usize, stride as usize);
                    let dst =
                        (shown.0 > 0 && shown.1 > 0 && shown.0 < w && shown.1 < h).then_some(shown);
                    cap.busy.store(true, std::sync::atomic::Ordering::Release);
                    job = Some(ScJob {
                        item: i,
                        handle: cap.handle,
                        w,
                        h,
                        opaque: f == wl_shm::Format::Xrgb8888,
                        has_tex: scene::screencopy(i)
                            .is_some_and(|t| (t.w, t.h) == dst.unwrap_or((w, h))),
                        src: ShmView {
                            ptr: data.as_ptr(),
                            len: data.len(),
                            stride,
                            inv,
                            busy: cap.busy.clone(),
                        },
                        dst,
                    });
                }
                frame.destroy();
                cap.frame = None;
                cap.shm = None;
                cap.got = true;
                if let Some(j) = job {
                    cap.landing = true;
                    app.sc_convert(ScMsg::Frame(j));
                }
            }
            Event::Failed => {
                // A first dmabuf copy failing means the compositor can't fill our dmabufs:
                // fall back to wl_shm and ask again at once. Any other failure is final, as
                // the protocol leaves it (the client destroys the frame) and Quickshell does.
                let fallback = cap.dma_used && !app.dma_ok;
                frame.destroy();
                cap.frame = None;
                cap.shm = None;
                cap.dma = None;
                cap.dma_used = false;
                let again = (app.export_mgr.clone())
                    .zip(cap.handle)
                    .filter(|_| fallback);
                if let Some((mgr, h)) = again {
                    elog::eprintln!("screencopy: dmabuf copy failed, using wl_shm");
                    app.dma_off = true;
                    cap.frame = Some(mgr.capture_toplevel(cap.overlay_cursor(), h, qh, i));
                    cap.seen = false;
                } else {
                    app.stop_capture(i);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<HyprlandGlobalShortcutsManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &HyprlandGlobalShortcutsManagerV1,
        _: <HyprlandGlobalShortcutsManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<HyprlandGlobalShortcutV1, (String, String)> for App {
    fn event(
        app: &mut Self,
        _: &HyprlandGlobalShortcutV1,
        event: hyprland_global_shortcut_v1::Event,
        key: &(String, String),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let (sig, down) = match event {
            hyprland_global_shortcut_v1::Event::Pressed { .. } => ("pressed", true),
            hyprland_global_shortcut_v1::Event::Released { .. } => ("released", false),
            _ => return,
        };
        let items = app
            .shortcuts
            .get(key)
            .map(|s| s.1.clone())
            .unwrap_or_default();
        {
            let mut w = app.rt.world.borrow_mut();
            // Behaviors these handlers start (a panel's `visible` flipping) count from the press,
            // not from the last tick before the loop went idle.
            w.now = app.clock.stamp(app.start.elapsed().as_secs_f64() * 1000.0);
            for i in items {
                if w.dead.contains(&i) {
                    continue;
                }
                w.write(i, "pressed", Value::Bool(down));
                qml::native::emit(&mut w, i, sig, &[], Vec::new());
            }
        }
        app.rt.settle();
    }
}

/// zwp_text_input_v3 events between two `done`s.
#[derive(Default)]
struct TextInputPending {
    /// zwp_text_input_v3.preedit_string: text, cursor_begin, cursor_end.
    preedit: Option<(String, i32, i32)>,
    commit: Option<String>,
    delete: (u32, u32),
}

impl Dispatch<ZwpTextInputManagerV3, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputManagerV3,
        _: <ZwpTextInputManagerV3 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTextInputV3, ()> for App {
    fn event(
        app: &mut Self,
        ti: &ZwpTextInputV3,
        e: zwp_text_input_v3::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwp_text_input_v3::Event;
        match e {
            Event::Enter { surface } => {
                app.ti_surface = Some(surface);
                app.ti_enabled = false;
                app.ti_sent = None;
                app.ti_key = None;
                app.sync_text_input();
            }
            Event::Leave { .. } => {
                if app.ti_enabled {
                    ti.disable();
                    ti.commit();
                }
                app.ti_surface = None;
                app.ti_enabled = false;
                app.ti_pending = TextInputPending::default();
                if app.kbd.ime_preedit(&mut app.rt, None) {
                    app.respond_to_input();
                }
            }
            Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => app.ti_pending.preedit = text.map(|t| (t, cursor_begin, cursor_end)),
            Event::CommitString { text } => app.ti_pending.commit = text,
            Event::DeleteSurroundingText {
                before_length,
                after_length,
            } => app.ti_pending.delete = (before_length, after_length),
            Event::Done { .. } => {
                // text-input-v3 `done` order: drop the preedit, delete around the cursor,
                // insert the commit string, then show the new preedit.
                let p = std::mem::take(&mut app.ti_pending);
                app.kbd.ime_preedit(&mut app.rt, None);
                app.kbd.ime_commit(
                    &mut app.rt,
                    p.delete.0,
                    p.delete.1,
                    p.commit.as_deref().unwrap_or(""),
                );
                app.kbd.ime_preedit(
                    &mut app.rt,
                    p.preedit.as_ref().map(|(t, b, e)| (t.as_str(), *b, *e)),
                );
                app.respond_to_input();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpIdleInhibitManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwpIdleInhibitManagerV1,
        _: <ZwpIdleInhibitManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpIdleInhibitorV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwpIdleInhibitorV1,
        _: <ZwpIdleInhibitorV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpFractionalScaleV1, wl_surface::WlSurface> for App {
    fn event(
        app: &mut Self,
        _: &WpFractionalScaleV1,
        ev: wp_fractional_scale_v1::Event,
        s: &wl_surface::WlSurface,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = ev else {
            return;
        };
        let Some(k) = app.wins.iter().position(|w| w.surf.wl_surface() == s) else {
            return;
        };
        app.wins[k].scale.frac = scale;
        app.scale_reported(k);
    }
}

wayland_client::delegate_noop!(App: ignore WpFractionalScaleManagerV1);
wayland_client::delegate_noop!(App: ignore WpViewporter);
wayland_client::delegate_noop!(App: ignore WpPresentation);
wayland_client::delegate_noop!(App: ignore ZwlrOutputPowerManagerV1);
wayland_client::delegate_noop!(App: ignore wl_subcompositor::WlSubcompositor);
wayland_client::delegate_noop!(App: ignore wl_subsurface::WlSubsurface);
wayland_client::delegate_noop!(App: ignore WpAlphaModifierV1);
wayland_client::delegate_noop!(App: ignore WpAlphaModifierSurfaceV1);
// A veil's wl_surface: it takes no input, and its scale follows its window's.
wayland_client::delegate_noop!(App: ignore wl_surface::WlSurface);

/// Under ri::present, asks for wp_presentation feedback on the commit `s` is about to make: the
/// `presented win{k}` line it logs means that content update is on screen. Every commit of a
/// shown window asks, also one that carries no new frame (a repeat configure's ack): it
/// supersedes the frame committed before it, whose feedback a compositor that has not
/// repainted yet answers `discarded`, and its own feedback is then the one for that frame.
fn log_presented(pres: Option<&WpPresentation>, s: &wl_surface::WlSurface, qh: &QueueHandle<App>) {
    if let Some(p) = pres
        && qml::log_on!("ri::present")
    {
        p.feedback(s, qh, s.clone());
    }
}

impl Dispatch<WpPresentationFeedback, wl_surface::WlSurface> for App {
    fn event(
        app: &mut Self,
        _: &WpPresentationFeedback,
        ev: wp_presentation_feedback::Event,
        s: &wl_surface::WlSurface,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(k) = app.wins.iter().position(|w| w.surf.wl_surface() == s) else {
            return;
        };
        match ev {
            wp_presentation_feedback::Event::Presented {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
                refresh,
                seq_hi,
                seq_lo,
                ..
            } => {
                let seq = (u64::from(seq_hi) << 32) | u64::from(seq_lo);
                let sec = (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo);
                tracing::debug!(
                    target: "ri::present",
                    "presented win{k} seq={seq} t={sec}.{tv_nsec:09} refresh={refresh}ns"
                );
            }
            wp_presentation_feedback::Event::Discarded => {
                tracing::debug!(target: "ri::present", "discarded win{k}");
            }
            _ => {}
        }
    }
}
wayland_client::delegate_noop!(App: ignore WpViewport);

/// What of an output's state the shell reacts to; a change re-lays out the screens.
#[derive(Clone, PartialEq)]
struct OutputSnapshot {
    name: Option<String>,
    description: Option<String>,
    make: String,
    model: String,
    location: (i32, i32),
    physical_size: (i32, i32),
    transform: wl_output::Transform,
    scale_factor: i32,
    logical_position: Option<(i32, i32)>,
    logical_size: Option<(i32, i32)>,
    /// (dimensions, refresh mHz, current, preferred) per wl_output.mode.
    modes: Vec<((i32, i32), i32, bool, bool)>,
}

impl OutputSnapshot {
    fn of(i: &smithay_client_toolkit::output::OutputInfo) -> Self {
        OutputSnapshot {
            name: i.name.clone(),
            description: i.description.clone(),
            make: i.make.clone(),
            model: i.model.clone(),
            location: i.location,
            physical_size: i.physical_size,
            transform: i.transform,
            scale_factor: i.scale_factor,
            logical_position: i.logical_position,
            logical_size: i.logical_size,
            modes: i
                .modes
                .iter()
                .map(|m| (m.dimensions, m.refresh_rate, m.current, m.preferred))
                .collect(),
        }
    }
}

/// A Quickshell screen (name, logical x, y, width, height, devicePixelRatio) for an output.
/// The name (wl_output v4 / xdg_output) is the screen's identity, so an output without one is
/// not a screen yet; SCTK announces outputs only once their initial `done` arrived.
/// Without xdg_output's logical geometry it follows from wl_output alone (xdg-output-v1: the
/// current mode, transformed, over the integer scale, at the compositor-space location).
/// `factor` is the screen's (`QT_SCALE_FACTOR`, `QT_SCREEN_SCALE_FACTORS`): Qt divides the
/// size by it, not the position, and multiplies the ratio.
fn output_screen(
    i: &smithay_client_toolkit::output::OutputInfo,
    factor: impl Fn(&str) -> f64,
) -> Option<(String, i32, i32, i32, i32, f64)> {
    let Some(name) = i.name.clone().filter(|n| !n.is_empty()) else {
        tracing::warn!(
            "wl_output {} has no name (wl_output v4 / xdg_output); not a screen",
            i.id
        );
        return None;
    };
    let (x, y) = i.logical_position.unwrap_or(i.location);
    let (w, h) = match i.logical_size {
        Some(s) => s,
        None => {
            let Some(m) = i.modes.iter().find(|m| m.current) else {
                tracing::warn!("output {name} has neither a logical size nor a current mode");
                return None;
            };
            mode_logical_size(m.dimensions, i.transform, i.scale_factor)
        }
    };
    if w <= 0 || h <= 0 {
        tracing::warn!("output {name} has an empty logical size {w}x{h}");
        return None;
    }
    let f = factor(&name);
    let px = |v: i32| {
        if f == 1.0 {
            v
        } else {
            (v as f64 / f).round() as i32
        }
    };
    // QWaylandScreen::devicePixelRatio: wl_output's integer scale, 2 on a 1.5 output.
    let dpr = i.scale_factor.max(1) as f64 * f;
    Some((name, x, y, px(w), px(h), dpr))
}

/// A wl_output mode's size in the compositor's logical space (xdg-output-v1's derivation for
/// integer-scaled outputs).
fn mode_logical_size((mw, mh): (i32, i32), t: wl_output::Transform, scale: i32) -> (i32, i32) {
    let (w, h) = if transform_swaps(t) {
        (mh, mw)
    } else {
        (mw, mh)
    };
    let scale = scale.max(1);
    (w / scale, h / scale)
}

fn transform_swaps(t: wl_output::Transform) -> bool {
    matches!(
        t,
        wl_output::Transform::_90
            | wl_output::Transform::_270
            | wl_output::Transform::Flipped90
            | wl_output::Transform::Flipped270
    )
}

/// wl_display.sync after a wl_keyboard enter/leave: the window activation takes effect here.
struct KbdSync;

impl Dispatch<wl_callback::WlCallback, KbdSync> for App {
    fn event(
        app: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &KbdSync,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let win = app.kbd_act.synced().and_then(|s| {
            app.wins
                .iter()
                .find(|w| w.surf.wl_surface() == s)
                .map(|w| w.item)
        });
        if qml::log_on!("ri::keys") {
            tracing::debug!(target: "ri::keys", "kbd active {win:?}");
        }
        app.retarget_kbd();
    }
}

impl Dispatch<ZwlrOutputManagerV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ZwlrOutputManagerV1,
        e: zwlr_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            zwlr_output_manager_v1::Event::Done { .. } => {
                if app.heads.done() && app.started {
                    app.publish_outputs(None);
                }
            }
            zwlr_output_manager_v1::Event::Finished => app.head_mgr = None,
            _ => {}
        }
    }

    wayland_client::event_created_child!(App, ZwlrOutputManagerV1, [
        zwlr_output_manager_v1::EVT_HEAD_OPCODE => (ZwlrOutputHeadV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputHeadV1, ()> for App {
    fn event(
        app: &mut Self,
        head: &ZwlrOutputHeadV1,
        e: zwlr_output_head_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use shell::output_heads::HeadEvent;
        let ev = match e {
            zwlr_output_head_v1::Event::Name { name } => HeadEvent::Name(name),
            zwlr_output_head_v1::Event::Make { make } => HeadEvent::Make(make),
            zwlr_output_head_v1::Event::Model { model } => HeadEvent::Model(model),
            zwlr_output_head_v1::Event::SerialNumber { serial_number } => {
                HeadEvent::Serial(serial_number)
            }
            zwlr_output_head_v1::Event::Finished => {
                app.heads.remove(&head.id());
                if head.version() >= 3 {
                    head.release();
                }
                return;
            }
            _ => return,
        };
        app.heads.event(head.id(), ev);
    }

    wayland_client::event_created_child!(App, ZwlrOutputHeadV1, [
        zwlr_output_head_v1::EVT_MODE_OPCODE => (ZwlrOutputModeV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputModeV1, ()> for App {
    fn event(
        _: &mut Self,
        mode: &ZwlrOutputModeV1,
        e: zwlr_output_mode_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_output_mode_v1::Event::Finished = e
            && mode.version() >= 3
        {
            mode.release();
        }
    }
}

impl Dispatch<HyprlandFocusGrabManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &HyprlandFocusGrabManagerV1,
        _: <HyprlandFocusGrabManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<HyprlandFocusGrabV1, usize> for App {
    fn event(
        app: &mut Self,
        _: &HyprlandFocusGrabV1,
        event: hyprland_focus_grab_v1::Event,
        item: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let hyprland_focus_grab_v1::Event::Cleared = event {
            if qml::log_on!("ri::grab") {
                tracing::debug!(target: "ri::grab", "grab {item} cleared by compositor");
            }
            app.grab_cleared.push(*item);
        }
    }
}

delegate_dispatch2!(App);

/// `$XDG_RUNTIME_DIR/ri-native.locked` exists while this shell holds the session lock, so
/// tooling can tell that stopping the shell would leave the session with a dead lockscreen.
fn lock_marker(on: bool) {
    let Some(p) = lock_marker_path() else {
        return;
    };
    // The file first: whoever reads the log line finds the marker in the state it names.
    let _ = if on {
        std::fs::write(&p, std::process::id().to_string())
    } else {
        std::fs::remove_file(&p)
    };
    // One write each: libwayland's WAYLAND_DEBUG lines land between the pieces of a formatted
    // eprintln and break "session lock: locked" for tests reading the log.
    if on {
        elog::eprintln!("session lock: locked");
    } else {
        elog::eprintln!("session lock: released");
    }
}

fn lock_marker_path() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(std::path::Path::new(&dir).join("ri-native.locked"))
}

/// Removes the marker when `gone` says so of the shell it names (its pid): one that no longer
/// holds the lock and will not get to say so.
fn drop_lock_marker_of(gone: impl Fn(u32) -> bool) {
    let Some(p) = lock_marker_path() else {
        return;
    };
    let named = std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| s.trim().parse().ok());
    if named.is_some_and(gone) {
        let _ = std::fs::remove_file(&p);
    }
}

/// wl_display.sync after `ext_session_lock_manager_v1.lock`, with [`App::lock_serial`] of that
/// request: the compositor has read it.
struct LockSync(u64);

impl Dispatch<wl_callback::WlCallback, LockSync> for App {
    fn event(
        app: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        serial: &LockSync,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        app.lock_answered(serial.0);
    }
}

/// wl_display.sync after `unlock_and_destroy`: the compositor has unlocked the session.
struct UnlockSync {
    /// The compositor had confirmed the lock. One it never confirmed has no marker; the file
    /// there may be that of another shell which holds the lock.
    held: bool,
}

impl Dispatch<wl_callback::WlCallback, UnlockSync> for App {
    fn event(
        app: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        sync: &UnlockSync,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        app.unlocks_in_flight = app.unlocks_in_flight.saturating_sub(1);
        // Not before: until the compositor has read the unlock, a shell that goes away leaves
        // the session locked.
        if sync.held {
            lock_marker(false);
        }
    }
}

impl SessionLockHandler for App {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        s.locked = true;
        let lock_item = s.item;
        lock_marker(true);
        self.rt
            .world
            .borrow_mut()
            .write(lock_item, "secure", Value::Bool(true));
        self.sync_unseen();
        self.sync_session_lock();
    }
    /// ext-session-lock-v1 `finished`: after `locked` the lock may only go by unlock_and_destroy
    /// (`destroy` is then a protocol error), before it only by `destroy`.
    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, lock: SessionLock) {
        let item = self.session.as_ref().map(|s| s.item);
        let refused = !lock.is_locked();
        if refused {
            elog::eprintln!("Failed to acquire session lock.");
            self.lock_denied = item;
        } else {
            elog::eprintln!("The compositor ended the session lock.");
        }
        self.end_lock(!refused);
        // Quickshell's WlSessionLock::unlock: `locked` reads false, so the lock is not retaken.
        // Its QSWaylandSessionLock::unlock emits `secureChanged` for a refused lock too, whose
        // `secure` never was true.
        if let Some(i) = item {
            let mut w = self.rt.world.borrow_mut();
            if !w.dead.contains(&i) {
                w.write(i, "locked", Value::Bool(false));
                if refused {
                    w.notify(i, "secure");
                }
            }
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: SessionLockSurface,
        c: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        if let Some(k) = self
            .wins
            .iter()
            .position(|w| w.surf.wl_surface() == surface.wl_surface())
        {
            let w = &mut self.wins[k];
            w.set_surface_size(c.new_size);
            w.configured = true;
            w.dirty = true;
            self.draw(k);
        }
    }
}
delegate_registry!(App);

impl App {
    /// QWaylandClipboard::setMimeData(QClipboard::Selection) with the button serial; Qt offers
    /// the same types in this order.
    fn set_primary(&mut self, qh: &QueueHandle<Self>, text: String, serial: u32) {
        let (Some(m), Some(d)) = (&self.primary, &self.primary_dev) else {
            return;
        };
        let src = m.create_selection_source(qh, ["text/plain", "text/plain;charset=utf-8"]);
        src.set_selection(d, serial);
        let sends = shell::selection::Transfers::new(self.loop_handle.clone());
        self.primary_src = Some((src, text.into(), sends));
    }

    /// QWaylandInputDevice creates its data device and primary selection device as soon as the
    /// seat exists.
    fn bind_selection_devices(&mut self, qh: &QueueHandle<Self>, seat: &wl_seat::WlSeat) {
        if self.primary_dev.is_none() {
            self.primary_dev = self
                .primary
                .as_ref()
                .map(|m| m.get_selection_device(qh, seat));
        }
        if self.data_dev.is_none() {
            self.data_dev = self.data_mgr.as_ref().map(|m| m.get_data_device(qh, seat));
        }
        qml::ports::clipsrc::seat(seat);
    }

    /// Selection side effects of a key, click or touch: export a TextEdit selection as the
    /// primary selection and start the paste it asked for.
    fn selection_requests(&mut self, qh: &QueueHandle<Self>, serial: u32) {
        if let Some(t) = shell::keys::take_primary_out() {
            self.set_primary(qh, t, serial);
        }
        if let Some(p) = shell::keys::take_paste(&self.rt) {
            self.start_paste(p);
        }
    }

    /// Serves the `paste()` calls of scripts that found no clipboard text in the process
    /// (`shell::keys::paste_in_call`), in call order, each once the one before has its text.
    /// Qt inserts inside the call; here that text comes once the selection's owner has
    /// written it (docs/qt-differences.md).
    fn script_pastes(&mut self) {
        loop {
            let mut pasted = false;
            while self.script_paste.pending().is_none()
                && let Some(p) = shell::keys::take_script_paste(&self.rt)
            {
                pasted |= shell::keys::paste_wanted(&self.rt, &p) && self.start_paste(p);
            }
            if !pasted {
                return;
            }
            // The handlers of the pasted text run now and may paste again.
            self.respond_to_input();
        }
    }

    /// `QClipboard::text(mode)` for a paste, read like `gdk_clipboard_read_text_async`: the
    /// current offer of the data device (clipboard) or primary selection device, in the type
    /// QMimeData::text reads, its pipe read from the loop until the owner closes it. Without an
    /// offer with text, Qt's text() is "". True if the paste was applied here, with no read
    /// left pending.
    fn start_paste(&mut self, p: shell::keys::Paste) -> bool {
        // QWaylandClipboard::mimeData: the process's own selection source answers before any
        // offer, and its offer is never read. A script's `copy()` then `paste()` gets the
        // copied text although the compositor has not announced that selection yet. A paste
        // key's is the read superpaste waits for.
        if !p.primary
            && let Some(text) = qml::ports::clip::own_text()
        {
            shell::keys::apply_paste(&mut self.rt, &p, &text);
            if !p.by_script() {
                qml::ports::clipsrc::took();
            }
            return true;
        }
        let read = if p.by_script() {
            &self.script_paste
        } else {
            &self.paste
        };
        let pipe = if p.primary {
            self.primary_dev
                .as_ref()
                .and_then(|d| d.data().selection_offer())
                .and_then(|o| {
                    let m = o.with_mime_types(shell::selection::text_mime)?;
                    Some(o.receive(m.into()))
                })
        } else {
            self.data_dev
                .as_ref()
                .and_then(|d| d.data().selection_offer())
                .and_then(|o| {
                    let m = o.with_mime_types(shell::selection::text_mime)?;
                    Some(o.receive(m.into()).map_err(std::io::Error::other))
                })
        };
        let started = match pipe {
            None => Ok(false),
            Some(pipe) => pipe.and_then(|pipe| {
                read.start(p, pipe.into(), |app: &mut App, p, got| {
                    app.finish_paste(p, got)
                })
                .map(|()| true)
            }),
        };
        match started {
            Ok(true) => return false,
            Ok(false) => {}
            Err(e) => tracing::warn!("paste: cannot read the selection: {e}"),
        }
        shell::keys::apply_paste(&mut self.rt, &p, "");
        true
    }

    /// The owner closed the pipe: the text goes to the editor if the paste is still wanted.
    fn finish_paste(&mut self, p: shell::keys::Paste, got: std::io::Result<Vec<u8>>) {
        if !shell::keys::paste_wanted(&self.rt, &p) {
            return;
        }
        let text = match got {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => {
                tracing::warn!("paste: reading the selection failed: {e}");
                String::new()
            }
        };
        shell::keys::apply_paste(&mut self.rt, &p, &text);
        self.respond_to_input();
    }

    /// Drops a pending paste once its editor died or, for a key's or click's, lost the
    /// keyboard focus.
    fn drop_unwanted_paste(&mut self) {
        for read in [&self.paste, &self.script_paste] {
            if read
                .pending()
                .is_some_and(|p| !shell::keys::paste_wanted(&self.rt, &p))
            {
                read.cancel();
            }
        }
    }

    /// A new selection replaces the offer a pending paste key or click reads. A script's
    /// `paste()` keeps reading the owner it asked, as Qt's does inside the call.
    fn selection_changed(&mut self, primary: bool) {
        if self.paste.pending().is_some_and(|p| p.primary == primary) {
            self.paste.cancel();
        }
    }

    /// `wl_data_device.selection`: the text of the new clipboard offer is asked from its owner
    /// at once and read from the loop, so that a script's `paste()` finds it in the process,
    /// as QClipboard::text() gives it inside the call. The event, the read and the script
    /// are on this one connection, in its order: until the owner has written the text
    /// `offer_text` is `None`, never the text of the selection before. An offer without text
    /// and no offer read as "" (QClipboard::text()).
    ///
    /// While the process's own source is the selection its offer is not read: a paste takes
    /// that text from the source (`own_text`). The source is on this connection too, and the
    /// compositor cancels it before it announces the selection that replaced it, so an offer
    /// that comes while the source lives is the source's own or one the source replaces.
    fn read_offer(&mut self) {
        self.forget_offer();
        if qml::ports::clipsrc::owns() {
            return;
        }
        let pipe = self
            .data_dev
            .as_ref()
            .and_then(|d| d.data().selection_offer())
            .and_then(|o| {
                let m = o.with_mime_types(shell::selection::text_mime)?;
                Some(o.receive(m.into()).map_err(std::io::Error::other))
            });
        let Some(pipe) = pipe else {
            *self.offer_text.borrow_mut() = Some(String::new());
            return;
        };
        let started = pipe.and_then(|pipe| {
            self.offer_read.start((), pipe.into(), |app: &mut App, (), got| {
                match got {
                    Ok(b) => {
                        if qml::log_on!("ri::keys") {
                            tracing::debug!(target: "ri::keys", "selection text read: {} bytes", b.len());
                        }
                        let text = String::from_utf8_lossy(&b).into_owned();
                        *app.offer_text.borrow_mut() = Some(text);
                    }
                    // Left unknown: a paste reads the offer itself.
                    Err(e) => tracing::warn!("reading the selection failed: {e}"),
                }
            })
        });
        if let Err(e) = started {
            tracing::warn!("cannot read the selection: {e}");
        }
    }

    /// The clipboard offer is no longer the selection (a new one, or the keyboard focus that
    /// it is valid under is gone): its text is not the clipboard's.
    fn forget_offer(&mut self) {
        self.offer_read.cancel();
        *self.offer_text.borrow_mut() = None;
    }
}

impl qml::ports::clipsrc::Host for App {
    fn watch(&mut self, read: u64, pipe: std::os::fd::OwnedFd) {
        let src =
            calloop::generic::Generic::new(pipe, calloop::Interest::WRITE, calloop::Mode::Level);
        let added = self
            .loop_handle
            .insert_source(src, move |_, _, app: &mut App| {
                if !qml::ports::clipsrc::writable(read) {
                    return Ok(calloop::PostAction::Continue);
                }
                app.clip_writes.remove(&read);
                Ok(calloop::PostAction::Remove)
            });
        match added {
            Ok(t) => {
                self.clip_writes.insert(read, t);
            }
            // The read stays unanswered until its source is cancelled.
            Err(e) => tracing::warn!("cannot watch a clipboard reader's pipe: {}", e.error),
        }
    }

    fn unwatch(&mut self, read: u64) {
        if let Some(t) = self.clip_writes.remove(&read) {
            self.loop_handle.remove(t);
        }
    }
}

/// The events of the clipboard source's objects go to `qml::ports::clipsrc::Clip`.
macro_rules! clipboard_source_dispatch {
    ($data:ident: $($i:ident),*) => {$(
        impl smithay_client_toolkit::dispatch2::Dispatch2<qml::ports::clipsrc::$i, App>
            for qml::ports::clipsrc::$data
        {
            fn event(
                &self,
                app: &mut App,
                p: &qml::ports::clipsrc::$i,
                e: <qml::ports::clipsrc::$i as wayland_client::Proxy>::Event,
                conn: &Connection,
                qh: &QueueHandle<App>,
            ) {
                <qml::ports::clipsrc::Clip as Dispatch<_, _, App>>::event(app, p, e, self, conn, qh)
            }

            fn event_created_child(
                opcode: u16,
                qh: &QueueHandle<App>,
            ) -> std::sync::Arc<dyn wayland_client::backend::ObjectData> {
                <qml::ports::clipsrc::Clip as Dispatch<qml::ports::clipsrc::$i, Self, App>>::event_created_child(opcode, qh)
            }
        }
    )*};
}
clipboard_source_dispatch!(Tag: ExtManager, ExtDevice, ExtOffer, ExtSource, WlrManager, WlrDevice, WlrOffer, WlrSource);
clipboard_source_dispatch!(SyncNo: SyncCallback);

impl PrimarySelectionDeviceHandler for App {
    fn selection(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ZwpPrimarySelectionDeviceV1,
    ) {
        self.selection_changed(true);
    }
}

/// Only the selection is read; drag and drop onto the shell is not supported.
impl DataDeviceHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        self.selection_changed(false);
        self.read_offer();
    }
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
}

impl DataOfferHandler for App {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

impl PrimarySelectionSourceHandler for App {
    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        s: &ZwpPrimarySelectionSourceV1,
        _: String,
        pipe: WritePipe,
    ) {
        // Written from the loop, so a reader that stops reading (`wl-paste -p | sleep 60`)
        // freezes neither the shell, as it does Qt, nor a thread.
        if let Some((src, t, sends)) = &self.primary_src
            && src.inner() == s
            && let Err(e) = sends.send(t.clone(), pipe.into())
        {
            elog::eprintln!("WARN: cannot send the primary selection: {e}");
        }
    }
    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        s: &ZwpPrimarySelectionSourceV1,
    ) {
        if self
            .primary_src
            .as_ref()
            .is_some_and(|(src, ..)| src.inner() == s)
        {
            self.primary_src = None;
        } else {
            s.destroy();
        }
    }
}

impl App {
    /// Serves `Qt.inputMethod`'s requests and hands it the focused editor's rectangles.
    fn sync_input_method(&mut self) {
        use qml::inputmethod::Request;
        let reqs = qml::inputmethod::take_requests(&mut self.rt.world.borrow_mut());
        for r in reqs {
            match r {
                Request::Update(_) => {
                    self.ti_sent = None;
                    self.ti_key = None;
                    self.im_key = None;
                }
                Request::Reset => self.reset_input_method(false),
                Request::Commit => self.reset_input_method(true),
                // No input panel to show (text-input-v3 v1 has no show/hide_input_panel) and no
                // actions on the composition (Qt's own v3 backend ignores invokeAction too).
                Request::Show | Request::Hide | Request::InvokeAction(..) => {}
            }
        }
        let key = (self.rt.revision(), self.kbd.ime_editor(&self.rt));
        if self.im_key == Some(key) {
            return;
        }
        self.im_key = Some(key);
        let g = self.kbd.im_geometry(&self.rt);
        let changed = {
            let mut w = self.rt.world.borrow_mut();
            let changed = w.input_method.geometry != g;
            qml::inputmethod::set_geometry(&mut w, g);
            changed
        };
        if changed {
            self.rt.settle();
            self.mark_dirty();
        }
    }

    /// `Qt.inputMethod.reset()` / `commit()`: the composition is dropped, or inserted as typed
    /// text, and the input method starts over.
    fn reset_input_method(&mut self, commit: bool) {
        self.ti_pending = TextInputPending::default();
        let text = commit.then(|| self.kbd.ime_preedit_text()).flatten();
        let mut dirty = self.kbd.ime_preedit(&mut self.rt, None);
        if let Some(t) = text {
            dirty |= self.kbd.ime_commit(&mut self.rt, 0, 0, &t);
        }
        if dirty {
            self.rt.settle();
            self.mark_dirty();
        }
        if self.ti_enabled {
            self.ti_reset = true;
        }
    }

    /// QWaylandTextInputv3 enable/disable/updateState: enabled while the entered surface holds
    /// keyboard focus with an editable TextInput/TextEdit focused, its state re-sent whenever
    /// it changes.
    fn sync_text_input(&mut self) {
        let Some(ti) = self.ti.clone() else { return };
        let focused = self.ti_surface.as_ref().is_some_and(|s| {
            self.wins
                .iter()
                .find(|w| w.surf.wl_surface() == s)
                .is_some_and(|w| self.kbd.win == Some(w.item))
        });
        let editor = focused.then(|| self.kbd.ime_editor(&self.rt)).flatten();
        let Some(editor) = editor else {
            if self.ti_enabled {
                ti.disable();
                ti.commit();
                self.ti_enabled = false;
                self.ti_reset = false;
                self.ti_sent = None;
                self.ti_key = None;
                if self.kbd.ime_preedit(&mut self.rt, None) {
                    self.mark_dirty();
                }
            }
            return;
        };
        let key = (self.rt.revision(), Some(editor));
        if self.ti_enabled && !self.ti_reset && self.ti_key == Some(key) {
            return;
        }
        self.ti_key = Some(key);
        let Some(st) = self.kbd.ime_state(&self.rt) else {
            return;
        };
        // text-input-v3: `enable` also resets the input method's state for the surface.
        if !self.ti_enabled || std::mem::take(&mut self.ti_reset) {
            ti.enable();
            self.ti_enabled = true;
            self.ti_sent = None;
        } else if self.ti_sent.as_ref() == Some(&st) {
            return;
        }
        ti.set_surrounding_text(st.text.clone(), st.cursor, st.anchor);
        ti.set_content_type(
            zwp_text_input_v3::ContentHint::from_bits_truncate(st.hint),
            zwp_text_input_v3::ContentPurpose::try_from(st.purpose)
                .unwrap_or(zwp_text_input_v3::ContentPurpose::Normal),
        );
        let f = (self.ti_surface.as_ref())
            .and_then(|s| self.wins.iter().find(|w| w.surf.wl_surface() == s))
            .map_or(1.0, |w| w.scale.factor);
        let (x, y, w, h) = st.rect;
        ti.set_cursor_rectangle(
            to_surface(x, f),
            to_surface(y, f),
            to_surface(w, f),
            to_surface(h, f),
        );
        ti.commit();
        self.ti_sent = Some(st);
    }

    /// Deliver the held key again if its client-side repeat is due.
    fn fire_key_repeat(&mut self) {
        let now = self.start.elapsed().as_secs_f64() * 1000.0;
        if let Some(k) = self.key_repeat.fire(now) {
            self.kbd.auto_repeat(&mut self.rt, k.sym, k.utf8.as_deref());
            let qh = self.qh.clone();
            self.selection_requests(&qh, self.key_serial);
        }
    }
}

/// wl_keyboard on the one xkb state in `App::xkb` (QWaylandInputDevice::Keyboard).
impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        app: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use wl_keyboard::{Event, KeyState, KeymapFormat};
        match event {
            Event::Keymap {
                format: WEnum::Value(KeymapFormat::XkbV1),
                fd,
                size,
            } => {
                if !app.xkb.keymap_fd(fd, size) {
                    tracing::warn!("wl_keyboard: the compositor's xkb keymap does not compile");
                }
            }
            Event::Keymap { .. } => tracing::warn!("wl_keyboard: keymap is not xkb_v1"),
            Event::Enter { surface, .. } => app.kbd_enter(conn, qh, &surface),
            Event::Leave { surface, .. } => app.kbd_leave(conn, qh, &surface),
            Event::Key {
                serial,
                key,
                state: WEnum::Value(state),
                ..
            } => {
                app.flush_pointer();
                match state {
                    KeyState::Pressed => app.press_key(qh, serial, key),
                    KeyState::Released => app.release_key(key),
                    KeyState::Repeated => app.repeat_key(qh, serial, key),
                    _ => {}
                }
            }
            Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                // The clicks read before this have the modifiers that were down then.
                app.flush_pointer();
                app.kbd.mods = app
                    .xkb
                    .modifiers(mods_depressed, mods_latched, mods_locked, group);
                app.ptr.mods = app.kbd.mods;
            }
            // A rate of 0 disables client-side repeat; only then may a v10 compositor send
            // `repeated` keys (wayland.xml wl_keyboard.key), so the two never both repeat.
            Event::RepeatInfo { rate, delay } => {
                app.key_repeat.info(rate.max(0) as u32, delay.max(0) as u32)
            }
            _ => {}
        }
    }
}

impl App {
    fn kbd_enter(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
    ) {
        let win = self
            .wins
            .iter()
            .find(|w| w.surf.wl_surface() == surface)
            .map(|w| w.item);
        if qml::log_on!("ri::keys") {
            tracing::debug!(target: "ri::keys", "kbd enter {win:?}");
        }
        qml::ports::clip::keyboard_focus(&self.rt.world.borrow().waker, win.is_some());
        if win.is_some() && self.kbd_act.enter(surface.clone()) {
            conn.display().sync(qh, KbdSync);
        }
    }

    fn kbd_leave(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
    ) {
        if qml::log_on!("ri::keys") {
            tracing::debug!(target: "ri::keys", "kbd leave");
        }
        self.key_repeat.stop();
        self.held_syms.clear();
        qml::ports::clip::keyboard_focus(&self.rt.world.borrow().waker, false);
        // Selection offers are only valid while the keyboard focus lasts.
        self.paste.cancel();
        self.forget_offer();
        // Leave ends the focus Enter gave the surface, whether a window still owns it or not.
        if self.kbd_act.leave(surface) {
            conn.display().sync(qh, KbdSync);
        }
    }

    fn press_key(&mut self, qh: &QueueHandle<Self>, serial: u32, key: u32) {
        let Some((sym, utf8)) = self.xkb.press(key) else {
            return;
        };
        let now = self.start.elapsed().as_secs_f64() * 1000.0;
        self.key_repeat.press(
            shell::keys::RepeatedKey {
                code: key,
                sym,
                utf8: utf8.clone(),
            },
            self.xkb.repeats(key),
            now,
        );
        self.key_serial = serial;
        self.held_syms.insert(key, sym);
        self.kbd
            .key(&mut self.rt, sym, utf8.as_deref(), true, false);
        // A TextEdit keyboard selection (keys::edit) sets the primary selection; a paste key reads one.
        self.selection_requests(qh, serial);
    }

    /// wl_keyboard v10 compositor repeat (QWaylandInputDevice::Keyboard::keyboard_key with
    /// WL_KEYBOARD_KEY_STATE_REPEATED): an auto-repeat release+press pair.
    fn repeat_key(&mut self, qh: &QueueHandle<Self>, serial: u32, key: u32) {
        let Some((sym, utf8)) = self.xkb.repeat(key) else {
            return;
        };
        self.kbd.auto_repeat(&mut self.rt, sym, utf8.as_deref());
        // A TextEdit keyboard selection (keys::edit) sets the primary selection; a paste key reads one.
        self.selection_requests(qh, serial);
    }

    fn release_key(&mut self, key: u32) {
        self.key_repeat.release(key);
        let Some(sym) = self.held_syms.remove(&key).or_else(|| self.xkb.sym(key)) else {
            return;
        };
        self.kbd.key(&mut self.rt, sym, None, false, false);
    }
}
/// QWaylandInputDevice::Touch, with the first touch point delivered as the left mouse button
/// (Qt Quick's touch-to-mouse synthesis; the original has no touch-specific handlers).
impl TouchHandler for App {
    fn down(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: u32,
        time: u32,
        surface: wl_surface::WlSurface,
        id: i32,
        (x, y): (f64, f64),
    ) {
        let Some((item, f)) = self
            .wins
            .iter()
            .find(|w| w.surf.wl_surface() == &surface)
            .map(|w| (w.item, 1.0 / w.scale.factor))
        else {
            return;
        };
        self.flush_pointer();
        self.ptr.time = time as f64;
        if self.ptr.touch_down(&mut self.rt, item, id, x * f, y * f) {
            self.touch_win = Some(item);
            self.respond_to_input();
        }
    }
    fn up(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        serial: u32,
        time: u32,
        id: i32,
    ) {
        self.flush_pointer();
        self.ptr.time = time as f64;
        if self.ptr.touch_up(&mut self.rt, id) {
            self.touch_win = None;
            self.selection_requests(qh, serial);
            self.respond_to_input();
        }
    }
    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        time: u32,
        id: i32,
        (x, y): (f64, f64),
    ) {
        if let Some(item) = self.touch_win {
            self.flush_pointer();
            let f =
                (self.wins.iter().find(|w| w.item == item)).map_or(1.0, |w| 1.0 / w.scale.factor);
            self.ptr.time = time as f64;
            self.ptr.touch_motion(&mut self.rt, id, x * f, y * f);
            self.respond_to_input();
        }
    }
    fn shape(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: i32,
        _: f64,
        _: f64,
    ) {
    }
    fn orientation(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_touch::WlTouch,
        _: i32,
        _: f64,
    ) {
    }
    fn cancel(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_touch::WlTouch) {
        self.flush_pointer();
        if self.ptr.touch_cancel(&mut self.rt) {
            self.touch_win = None;
            self.respond_to_input();
        }
    }
}

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, t: &Window) {
        if let Some(item) = self
            .wins
            .iter()
            .find(|w| w.surf.wl_surface() == t.wl_surface())
            .map(|w| w.item)
        {
            self.close_window(item);
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        t: &Window,
        c: WindowConfigure,
        _: u32,
    ) {
        let Some(k) = self
            .wins
            .iter()
            .position(|w| w.surf.wl_surface() == t.wl_surface())
        else {
            return;
        };
        let w = &mut self.wins[k];
        let before = (w.configured, w.size);
        let cur = w.surface_size();
        w.set_surface_size((
            c.new_size.0.map_or(cur.0, |x| x.get()),
            c.new_size.1.map_or(cur.1, |y| y.get()),
        ));
        let geometry = w.surface_size();
        w.configured = true;
        let (item, size) = (w.item, w.size);
        if qml::log_on!("ri::appdebug") {
            tracing::debug!(target: "ri::appdebug", "toplevel configure {:?} -> {size:?}", c.new_size);
        }
        if before == (true, size) && w.last.is_some() && !w.dirty && !w.anim {
            let s = w.surf.wl_surface();
            log_presented(self.presentation.as_ref(), s, &self.qh);
            s.commit();
            return;
        }
        // Qt sets the window geometry; Hyprland sizes a new floating window from it.
        t.xdg_surface()
            .set_window_geometry(0, 0, geometry.0 as i32, geometry.1 as i32);
        self.rt.set(item, "width", Value::Num(size.0 as f64));
        self.rt.set(item, "height", Value::Num(size.1 as f64));
        self.rt.settle();
        self.draw(k);
    }
}

/// Qt sets the toplevel's app_id from QGuiApplication::desktopFileName, which Quickshell sets.
const APP_ID: &str = "org.impulse-shell";

fn is_app_window(w: &qml::runtime::World, i: usize) -> bool {
    let has = |ty: &str| w.items[i].types.iter().any(|t| t == ty);
    !w.dead.contains(&i)
        && (has("FloatingWindow")
            || (has("ApplicationWindow") || has("Window")) && !has("ProxyWindowBase"))
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.bind_selection_devices(qh, &seat);
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Keyboard {
            // new_seat runs while the globals are still being bound, before ti_mgr exists.
            if let Some(m) = &self.ti_mgr
                && self.ti.is_none()
            {
                self.ti = Some(m.get_text_input(&seat, qh, ()));
            }
            if shell::keys::seat_cap(
                &mut self.keyboard,
                true,
                || Some(seat.get_keyboard(qh, ())),
                |_| {},
            ) {
                // QWaylandInputDevice::Keyboard's repeat defaults, for wl_keyboard below v4
                // (no repeat_info event); a later repeat_info replaces them.
                self.key_repeat.info(25, 400);
            }
        }
        if cap == Capability::Touch && self.touch.is_none() {
            self.touch = self.seat.get_touch(qh, &seat).ok();
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            // With its wp_cursor_shape_device_v1 where the compositor has the protocol, else
            // with a surface for the cursor theme's images (`update_cursor_shape`).
            let cursor = self.compositor.create_surface(qh);
            self.pointer = self
                .seat
                .get_pointer_with_theme::<App, ()>(
                    qh,
                    &seat,
                    self.shm.wl_shm(),
                    cursor,
                    ThemeSpec::System,
                )
                .ok();
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            self.cur_shape = -1;
            self.ptr_frames.clear();
            self.motion_merge = Default::default();
            self.axis_merge = AxisCompression::default();
            self.wheel_deltas = shell::input::WheelDeltas::default();
            self.ptr_focus = None;
            self.ptr.pointer_lost(&mut self.rt);
            // Releases the wl_pointer and destroys what belongs to it: the cursor-shape device
            // (or the next pointer binds a second one and the old object leaks) and the
            // cursor surface.
            drop(p);
        }
        if cap == Capability::Touch
            && let Some(t) = self.touch.take()
        {
            self.ptr.touch_cancel(&mut self.rt);
            self.touch_win = None;
            t.release();
        }
        // Without a keyboard capability (last keyboard unplugged, a virtual keyboard closed)
        // the old wl_keyboard is dead; drop it so the next new_capability binds a new one.
        if cap == Capability::Keyboard
            && shell::keys::seat_cap(&mut self.keyboard, false, || None, |k| k.release())
        {
            self.kbd_act.lost();
            self.key_repeat.stop();
            qml::ports::clip::keyboard_focus(&self.rt.world.borrow().waker, false);
            self.paste.cancel();
            self.forget_offer();
            self.retarget_kbd();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

/// Wayland axis -> Qt angleDelta (120 per notch, positive = away from the user).
impl App {
    /// Deliver the axis frames merged while the Wayland queue was drained.
    fn flush_axis(&mut self) {
        if let Some(a) = self.axis_merge.take() {
            self.ptr.wheel_axis(&mut self.rt, &a);
        }
    }

    /// Deliver the motion that waits, the last of the drained queue, and answer the motions
    /// delivered since the last answer in this frame. A frame of motions queued behind a busy
    /// turn (a 1000 Hz mouse while a frame draws) would each pay a hit test, hover update and
    /// binding pass for positions nobody sees.
    fn flush_motion(&mut self) {
        self.answer_motion(true);
    }

    /// The editor holds a widget (a button is down on it), this refresh has had its motion and
    /// a window's frame is with the compositor: the widget is drawn again only once that
    /// frame's callback comes, so the motions read until then are one step of the drag, its
    /// last position. Each handled as it is read (a hit test, the editor's frames and a
    /// binding pass a motion) would hold the callback's frame back past the refresh it is
    /// for. The first motion of a refresh is not one of them: a slow drag's lone step, read
    /// while another window's frame is out (the editor's outline over the desktop), would
    /// wait a whole refresh for a callback that has nothing to add. What QML itself drags (a
    /// Flickable, a DragHandler) gets its motions as Qt delivers them, as they are read.
    fn drag_awaits_frame(&self) -> bool {
        self.drag_holds_motions() && !self.motion_ahead()
    }

    fn drag_holds_motions(&self) -> bool {
        self.ptr.held()
            && self.motion_this_frame
            && (self.frame_has_motion
                || (0..self.wins.len()).any(|k| self.wins[k].pending && !self.unexposed(k)))
            && self
                .rt
                .world
                .try_borrow()
                .is_ok_and(|w| w.edit.gesture.is_some())
    }

    /// When the motion the editor's drag holds is handled ahead of the callback it waits
    /// for, in the loop's ms: early enough to be in the scene when the callback comes. The
    /// callback's frame is then drawn at once, and every window the drag changes (the
    /// desktop under the widget, the editor's outline over it) is with the compositor
    /// before the refresh; handled after the callback, the motion took that much of the
    /// refresh from them and the last drawn missed it, shown every second refresh.
    fn motion_ahead_at(&self) -> f64 {
        self.loop_ms(self.frame_at) + self.clock.vsync - self.motion_cost - MOTION_AHEAD_MARGIN
    }

    /// It is time for that (`motion_ahead_at`), and this refresh has not had it.
    fn motion_ahead(&self) -> bool {
        !self.motion_early && self.loop_ms(Instant::now()) >= self.motion_ahead_at()
    }

    /// A motion is handled, `early` for the editor's drag (`motion_ahead`) or not, since `at`.
    fn motion_handled(&mut self, early: bool, at: Instant) {
        self.motion_unanswered = true;
        self.motion_this_frame = true;
        self.motion_early |= early;
        self.motion_took += at.elapsed().as_secs_f64() * 1000.0;
    }

    /// `flush_motion`, with the motion that waits delivered only if `held`.
    fn answer_motion(&mut self, held: bool) {
        let early = self.drag_holds_motions() && self.motion_ahead();
        if held
            && let Some((item, (x, y), time)) = self.motion_merge.take()
            && self.wins.iter().any(|w| w.item == item)
        {
            let at = Instant::now();
            self.ptr.time = time as f64;
            self.ptr.motion(&mut self.rt, item, x, y);
            self.motion_handled(early, at);
            if early && qml::log_on!("ri::frames") {
                tracing::debug!(
                    target: "ri::frames",
                    "motion ahead of the callback, handling one takes {:.1}ms",
                    self.motion_cost
                );
            }
        }
        if std::mem::take(&mut self.motion_unanswered) {
            let at = Instant::now();
            self.update_cursor_shape();
            self.respond_to_input();
            let took = std::mem::take(&mut self.motion_took) + at.elapsed().as_secs_f64() * 1000.0;
            self.motion_cost = ((self.motion_cost + took) / 2.0).min(self.clock.vsync / 2.0);
        }
    }

    /// Everything the pointer queued in this dispatch, before input of another kind.
    fn flush_pointer(&mut self) {
        while let Some((at, events)) = self.ptr_frames.pop_front() {
            self.handle_pointer_frame(at, &events);
        }
        self.flush_axis();
        self.flush_motion();
    }

    /// `flush_pointer` at the end of a dispatch: a drag's motion stays held while a frame is
    /// with the compositor (`drag_awaits_frame`), for that frame's callback to deliver.
    fn flush_pointer_read(&mut self) {
        while let Some((at, events)) = self.ptr_frames.pop_front() {
            self.handle_pointer_frame(at, &events);
        }
        self.flush_axis();
        self.answer_motion(!self.drag_awaits_frame());
    }

    /// Show the cursor of the item under the pointer. `wl_pointer.enter` leaves the image
    /// undefined until the client sets one: through wp_cursor_shape_v1 where the compositor
    /// has it, else `wl_pointer.set_cursor` with the image of the XCURSOR theme (the
    /// `XCURSOR_THEME` and `XCURSOR_SIZE` Qt reads too), at the cursor surface's scale.
    fn update_cursor_shape(&mut self) {
        let shape = self.ptr.cursor_shape(&self.rt);
        if shape == self.cur_shape {
            return;
        }
        let Some(p) = &self.pointer else {
            return;
        };
        let icon = qt_cursor(shape);
        let set = match p.set_cursor(&self.conn, icon) {
            // A theme without this image: the arrow, as for a shape Qt has no image for.
            Err(PointerThemeError::CursorNotFound) if icon != CursorIcon::Default => {
                p.set_cursor(&self.conn, CursorIcon::Default)
            }
            r => r,
        };
        match set {
            Ok(()) => {}
            // No `enter` yet: set once it comes.
            Err(PointerThemeError::MissingEnterSerial) => return,
            Err(e) => {
                if !std::mem::replace(&mut self.cursor_missing, true) {
                    elog::eprintln!("Unable to set the {} cursor: {e}", icon.name());
                }
            }
        }
        self.cur_shape = shape;
    }
}

/// wl_pointer axis_value120, or v5-7 axis_discrete steps scaled to it.
fn value120(a: &AxisScroll) -> i32 {
    if a.value120 != 0 {
        a.value120
    } else {
        a.discrete * 120
    }
}

/// The cursor of a Qt::CursorShape, by its CSS name (wp_cursor_shape_v1 and cursor themes).
fn qt_cursor(s: i32) -> CursorIcon {
    match s {
        2 => CursorIcon::Crosshair,
        3 => CursorIcon::Wait,
        4 => CursorIcon::Text,
        5 => CursorIcon::NsResize,
        6 => CursorIcon::EwResize,
        7 => CursorIcon::NeswResize,
        8 => CursorIcon::NwseResize,
        9 | 20 => CursorIcon::Move,
        11 => CursorIcon::RowResize,
        12 => CursorIcon::ColResize,
        13 => CursorIcon::Pointer,
        14 => CursorIcon::NotAllowed,
        15 => CursorIcon::Help,
        16 => CursorIcon::Progress,
        17 => CursorIcon::Grab,
        18 => CursorIcon::Grabbing,
        19 => CursorIcon::Copy,
        21 => CursorIcon::Alias,
        _ => CursorIcon::Default,
    }
}

impl PointerHandler for App {
    /// Qt handles what it read from the socket after it has read all of it
    /// (QWindowSystemInterface queues the events): a press handler that takes long does not
    /// run between two frames of one read, and what comes meanwhile is the next read.
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        self.ptr_frames.push_back((Instant::now(), events.to_vec()));
    }
}

impl App {
    /// A pointer frame read at `at` reaches the scene.
    fn handle_pointer_frame(&mut self, at: Instant, events: &[PointerEvent]) {
        let qh = &self.qh.clone();
        let merge = self.axis_merge.frame(at);
        // QWaylandInputDevice::Pointer::pointer_frame holds a motion back when its frame is
        // read within 100 µs of the previous one or while a motion waits. The first of a
        // burst is not held.
        let hold = self.motion_merge.frame(at);
        let mut delivered = false;
        for e in events {
            if !matches!(e.kind, PointerEventKind::Axis { .. }) {
                self.flush_axis();
            }
            if !matches!(e.kind, PointerEventKind::Motion { .. }) {
                self.flush_motion();
            }
            // Leave ends the focus Enter gave the surface, whether a window still owns it or not.
            if let PointerEventKind::Leave { .. } = e.kind {
                if self.ptr_focus.as_ref() == Some(&e.surface) {
                    self.ptr_focus = None;
                    self.ptr.leave(&mut self.rt);
                    delivered = true;
                }
                continue;
            }
            let Some(k) = self
                .wins
                .iter()
                .position(|w| w.surf.wl_surface() == &e.surface)
            else {
                if qml::log_on!("ri::ptr") {
                    tracing::debug!(target: "ri::ptr", "ptr event on unknown surface: {:?}", e.kind);
                }
                if let PointerEventKind::Release { button, .. } = e.kind {
                    self.ptr.release_lost(&mut self.rt, button);
                    delivered = true;
                }
                continue;
            };
            if tracing::enabled!(target: "ri::ptr", tracing::Level::TRACE)
                || qml::log_on!("ri::ptr") && !matches!(e.kind, PointerEventKind::Motion { .. })
            {
                tracing::debug!(
                    target: "ri::ptr",
                    "ptr win {k} t={:.0} {:?} at {:?}",
                    self.start.elapsed().as_secs_f64() * 1000.0,
                    e.kind,
                    e.position
                );
            }
            let item = self.wins[k].item;
            // QML's pixels: the surface's over the factor of the window's screen.
            // (Qt multiplies by the inverse, QHighDpi::fromNativePixels.)
            let f = 1.0 / self.wins[k].scale.factor;
            let (x, y) = (e.position.0 * f, e.position.1 * f);
            if let PointerEventKind::Motion { time } = e.kind {
                // A held-back motion replaces the one that waits (Qt's setFrameEvent), with a
                // button down or not. The first motion of a burst reaches the scene at once:
                // a drag starts from it, not from the burst's last position.
                if hold || self.drag_awaits_frame() {
                    self.motion_merge.hold((item, (x, y), time));
                } else {
                    let early = self.drag_holds_motions();
                    let at = Instant::now();
                    self.ptr.time = time as f64;
                    self.ptr.motion(&mut self.rt, item, x, y);
                    self.motion_handled(early, at);
                }
                continue;
            }
            delivered = true;
            if let PointerEventKind::Press { time, .. }
            | PointerEventKind::Release { time, .. }
            | PointerEventKind::Axis { time, .. } = &e.kind
            {
                self.ptr.time = *time as f64;
            }
            match &e.kind {
                PointerEventKind::Enter { .. } => {
                    self.cur_shape = -1;
                    self.ptr_focus = Some(e.surface.clone());
                    self.ptr.enter(&mut self.rt, item, x, y)
                }
                PointerEventKind::Motion { .. } | PointerEventKind::Leave { .. } => {}
                PointerEventKind::Press { button, .. } => {
                    self.ptr.motion(&mut self.rt, item, x, y);
                    self.ptr.press(&mut self.rt, *button)
                }
                PointerEventKind::Release { button, serial, .. } => {
                    self.ptr.release(&mut self.rt, *button);
                    self.selection_requests(qh, *serial);
                }
                PointerEventKind::Axis {
                    time,
                    horizontal,
                    vertical,
                    source,
                } => {
                    // Qt reports pixelDelta only for finger/continuous sources, not wheel clicks.
                    let wheel = shell::input::wheel_source(*source);
                    let inverted = [horizontal, vertical].iter().any(|a| {
                        a.relative_direction == Some(wl_pointer::AxisRelativeDirection::Inverted)
                    });
                    let deltas = self.wheel_deltas.frame(
                        (value120(horizontal), value120(vertical)),
                        (horizontal.absolute, vertical.absolute),
                        wheel,
                    );
                    if horizontal.stop || vertical.stop {
                        self.wheel_deltas.end();
                    }
                    let Some((angle, pixel)) = deltas else {
                        continue;
                    };
                    // Flicks run on the animation clock, which pauses while idle.
                    let now = self
                        .clock
                        .stamp(self.start.elapsed().as_secs_f64() * 1000.0);
                    let a = Axis {
                        angle,
                        pixel,
                        time: (*time as f64, now),
                        synthesized: !wheel,
                        inverted,
                    };
                    if merge {
                        self.axis_merge.merge(a);
                    } else {
                        self.ptr.wheel_axis(&mut self.rt, &a)
                    }
                }
            }
        }
        if delivered {
            self.motion_unanswered = false;
            self.update_cursor_shape();
            self.respond_to_input();
        }
    }
}

impl App {
    /// Handlers that ran may have started animations, Timers or FrameAnimations: on a driver
    /// that follows the wall and stepped last more than 50 ms ago, the running ones step to
    /// the current time first (`AnimClock::catch_up`). Returns what that tick answered.
    /// `AnimClock::read_wall` for the Timer due first.
    fn read_wall(&mut self, wall: f64) {
        let due = self.rt.world.borrow().timer_due();
        self.clock.read_wall(wall, due);
    }

    /// The handler of a Timer whose end started the driver started the animations that run
    /// (`AnimClock::late`): the windows that show them send a frame now, as Qt's do for the
    /// started driver, changed or not, and its callback is the driver's first step. One whose
    /// last frame is still with the compositor has that callback to come.
    fn start_late(&mut self) {
        self.clock.late = true;
        let shows = self.with_anim_attr(|animating, _, _| animating.to_vec());
        for (win, shows) in self.wins.iter_mut().zip(shows) {
            win.dirty |= shows;
        }
    }

    fn step_started(&mut self) -> Option<bool> {
        if !self.rt.animations_started() {
            return None;
        }
        let wall = self.start.elapsed().as_secs_f64() * 1000.0;
        let at = self.clock.catch_up(wall);
        if at.is_some() {
            self.rt.world.borrow_mut().frame_wall = Some(wall);
        }
        self.rt.step_started(at)
    }

    /// Run the effects of the input just delivered and draw them in the same frame instead
    /// of waiting for the next timer tick.
    fn respond_to_input(&mut self) {
        self.sync_popups();
        let wall = self.start.elapsed().as_secs_f64() * 1000.0;
        self.rt.world.borrow_mut().frame_wall = Some(wall);
        self.read_wall(wall);
        let mut now = self.clock.stamp(wall);
        // What the handlers started joins a driver that is up to date: one that stopped behind
        // the wall clock steps to it first, as in the loop's own turn (`step_started`), not
        // with the first step of the driver they start.
        let caught = if self.clock.running {
            None
        } else {
            self.step_started()
        };
        if caught.is_some() {
            now = self.rt.world.borrow().now;
        }
        if !self.clock.running && animations_running(self) {
            // The frame answering an event that started the first animation shows it at its
            // start: Qt starts its animation driver from the event loop after the handler,
            // and advances it after a sync (QSGThreadedRenderLoop::polishAndSync). The first
            // step, one vsync on, is the next pass's.
            now = self.clock.begin(wall, animations_since(self));
        }
        let busy = self.rt.tick(now);
        let busy = self.step_started().unwrap_or(busy);
        if busy || self.rt.revision() != self.last_rev {
            self.mark_dirty();
            for k in 0..self.wins.len() {
                if self.wins[k].dirty && self.can_draw(k) {
                    self.draw(k);
                }
            }
            self.last_rev = self.rt.revision();
        }
        if !self.wins.iter().any(|w| w.dirty) {
            // No window has a frame to draw that would show the animations at their start,
            // so none is waited for: the first step comes by the vsync timer.
            self.clock.shown();
        }
    }
}

/// Quickshell ColorQuantizer: median-cut buckets over the image scaled to `rescaleSize`.
/// The original only uses depth 0 (one bucket), so the result is the mean colour. Quickshell decodes and
/// scales on the GUI thread, freezing the shell for a full 4K/8K decode on every wallpaper
/// change; here the decode runs on the work pool at a reduced DCT scale and the result is
/// applied on a later turn, dropped when `source` or `rescaleSize` changed meanwhile.
fn quantize_colors(app: &mut App) -> bool {
    let items = app.rt.world.borrow_mut().of_type("ColorQuantizer");
    for i in items {
        let (src, size) = {
            let mut w = app.rt.world.borrow_mut();
            (
                w.read(i, "source")
                    .map(|v| v.to_js_string())
                    .unwrap_or_default(),
                w.read(i, "rescaleSize").map_or(0.0, |v| v.to_number()),
            )
        };
        if !src.is_empty() {
            let h = &app.rt.world.borrow().waker.handle(qml::svc::Svc::Host);
            app.quant.request(i, &src, size, h);
        }
    }
    let mut changed = false;
    for (i, col) in app.quant.ready() {
        app.rt.world.borrow_mut().write(
            i,
            "colors",
            qml::value::Value::array(vec![qml::value::Value::Color(col)]),
        );
        changed = true;
    }
    changed
}

/// A quantizer result: (item, source, colour).
type Quantized = (usize, String, Option<qml::color::Color>);

#[derive(Default)]
struct Quantizer {
    want: std::collections::HashMap<usize, String>,
    chan: Option<(
        qml::svc::Sender<Quantized>,
        std::sync::mpsc::Receiver<Quantized>,
    )>,
}

impl Quantizer {
    fn request(&mut self, item: usize, src: &str, size: f64, h: &qml::svc::Handle) {
        let key = format!("{src}@{size}");
        if self.want.get(&item) == Some(&key) {
            return;
        }
        self.want.insert(item, key.clone());
        let tx = self.chan.get_or_insert_with(|| h.channel()).0.clone();
        // ColorQuantizer reads `source.toLocalFile()`.
        let path = qml::native::url_to_path(src);
        workpool::run(move || {
            let _ = tx.send((item, key, mean_color(&path, size)));
        });
    }

    /// Finished results whose request is still the current one.
    fn ready(&mut self) -> Vec<(usize, qml::color::Color)> {
        let Some((_, rx)) = &self.chan else {
            return Vec::new();
        };
        rx.try_iter()
            .filter(|(i, key, _)| self.want.get(i) == Some(key))
            .filter_map(|(i, _, c)| Some((i, c?)))
            .collect()
    }
}

fn mean_color(path: &str, size: f64) -> Option<qml::color::Color> {
    let (mut img, alpha, (w, h)) = qsg::image::load_scaled(path, size).ok()?;
    if size > 0.0 && w > 0 && h > 0 {
        let (dw, dh) = qsg::image::fit_long_side(w, h, size);
        img = qsg::image::smooth_scale(&img, dw, dh, alpha);
    }
    // QImage::scaled to an empty size (rescaleSize in (0, 1)) is a null image: no colour.
    if img.px.is_empty() {
        return None;
    }
    let n = img.px.len() as f64;
    let mut sum = [0.0f64; 3];
    for p in &img.px {
        for c in 0..3 {
            sum[c] += p[c] as f64;
        }
    }
    Some(qml::color::Color::from_rgb_f(
        (sum[0] / n / 255.0) as f32,
        (sum[1] / n / 255.0) as f32,
        (sum[2] / n / 255.0) as f32,
        1.0,
    ))
}

fn stats_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| qml::log_on!("ri::stats"))
}

enum ScreenCap {
    /// The capture in flight and the window root item it holds back until it ends.
    Busy(
        std::sync::mpsc::Receiver<Option<qsg::raster::Canvas>>,
        usize,
    ),
    Done,
}

/// Ends every capture whose worker answered (or died); the canvases of those that copied.
fn poll_screen_caps(
    caps: &mut rustc_hash::FxHashMap<usize, ScreenCap>,
) -> Vec<(usize, qsg::raster::Canvas)> {
    let mut got = Vec::new();
    for (&i, cap) in caps.iter_mut() {
        if let ScreenCap::Busy(r, _) = cap {
            match r.try_recv() {
                Ok(c) => {
                    *cap = ScreenCap::Done;
                    got.extend(c.map(|c| (i, c)));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => *cap = ScreenCap::Done,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
    }
    got
}

fn holds_for_screen_cap(caps: &rustc_hash::FxHashMap<usize, ScreenCap>, win: usize) -> bool {
    caps.values()
        .any(|c| matches!(c, ScreenCap::Busy(_, w) if *w == win))
}

/// One wlr-screencopy frame of output `name` on a worker, waking the loop when it ends. It
/// always ends: copied, failed, output removed or switched off, the compositor did not answer
/// within `qml::ports::CAPTURE_LIMIT`, the worker panicked, or the system refused the worker's
/// thread (the receiver is disconnected then, as by a worker that died).
fn capture_screen(
    name: String,
    cursor: bool,
    h: qml::svc::Handle,
) -> std::sync::mpsc::Receiver<Option<qsg::raster::Canvas>> {
    let (tx, rx) = h.channel();
    let capture = move || {
        let got = std::panic::catch_unwind(|| qml::ports::capture_output(&name, cursor))
            .unwrap_or_else(|_| Err("capture panicked".into()));
        let tex = match got {
            Ok((w, h, px)) => Some(qsg::raster::Canvas {
                w: w as usize,
                h: h as usize,
                px: px
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| [p[0], p[1], p[2], p[3]])
                    .collect(),
                key: Default::default(),
            }),
            Err(e) => {
                tracing::warn!("screencopy {name}: {e}");
                None
            }
        };
        let _ = tx.send(tex);
    };
    if let Err(e) = std::thread::Builder::new()
        .name("screen-capture".into())
        .spawn(capture)
    {
        // Not `tracing`: the default `RI_LOG` filter shows only the `ri` targets.
        elog::eprintln!("screencopy: cannot start a thread: {e}");
    }
    rx
}

/// `w` draws a frame when one is requested: it is configured with a size (`App::draw`).
fn draws_frames(w: &Win) -> bool {
    w.configured && w.size.0 > 0 && w.size.1 > 0
}

/// Destroys the panels `item` replaces right before its first commit, in the same flush: EGL
/// surface setup for that frame does roundtrips, which would send it alone.
fn retire_replaced(retiring: &mut Vec<Retiring>, item: usize) {
    retiring.retain(|r| r.succ != Some(item));
}

/// A panel's (item, output, exclusive zone, mapped).
type PanelState = (usize, Option<String>, i32, Option<bool>);

/// The successor of each replaced panel (item, output, successor paired so far): a panel
/// reserving space on the same output that has not committed its first frame, a different one
/// per replaced panel. A pairing holds while its successor still waits; otherwise the same item
/// on its new surface (a namespace change) comes first, then any other waiting panel (the bar
/// replaced by the vertical bar). None: no successor waits for it.
fn pair_successors(
    replaced: &[(usize, Option<String>, Option<usize>)],
    panels: &[PanelState],
) -> Vec<Option<usize>> {
    let waiting = |p: &PanelState, out: &Option<String>| p.1 == *out && p.2 > 0 && p.3.is_none();
    let waits =
        |item: usize, out: &Option<String>| panels.iter().any(|p| p.0 == item && waiting(p, out));
    let mut succ: Vec<Option<usize>> = Vec::with_capacity(replaced.len());
    for (_, out, s) in replaced {
        let s = s.filter(|&s| waits(s, out) && !succ.contains(&Some(s)));
        succ.push(s);
    }
    for (k, (item, out, _)) in replaced.iter().enumerate() {
        if succ[k].is_none() && waits(*item, out) && !succ.contains(&Some(*item)) {
            succ[k] = Some(*item);
        }
    }
    for (k, (_, out, _)) in replaced.iter().enumerate() {
        if succ[k].is_none() {
            succ[k] = (panels.iter())
                .find(|p| waiting(p, out) && !succ.contains(&Some(p.0)))
                .map(|p| p.0);
        }
    }
    succ
}

/// Shows the panels kept unseen while the previous image's windows were on the screen and
/// reserves their exclusive zones, in commits the caller writes together.
fn take_predecessors_place(
    cloaks: &mut Vec<(wl_surface::WlSurface, WpAlphaModifierSurfaceV1)>,
    wins: &[Win],
) {
    // The alpha goes back with the commit below (wp_alpha_modifier_surface_v1.destroy is
    // double-buffered), on the buffer the window holds: nothing is drawn again.
    let cloaked: Vec<_> = (cloaks.drain(..))
        .map(|(surf, cloak)| {
            cloak.destroy();
            surf
        })
        .collect();
    for win in wins {
        let Surf::Panel { layer, key, .. } = &win.surf else {
            continue;
        };
        let shown = cloaked.contains(layer.wl_surface());
        if shown {
            layer.wl_surface().damage_buffer(0, 0, i32::MAX, i32::MAX);
        }
        if key.geom.zone > 0 {
            layer.set_geometry(&key.geom);
        }
        if shown || key.geom.zone > 0 {
            layer.commit();
        }
    }
}

/// The geometry a panel sends for `geom`. While the previous image's windows are still on
/// the screen after a reload (`over_predecessor`) they reserve their zones: a second zone
/// would put this image's bar below the old one and squeeze the app windows until the old
/// one goes, so a zone is sent as -1, which puts the window over the old one
/// (wlr-layer-shell `set_exclusive_zone`). [`App::release_predecessor`] sends the zone.
fn sent_geometry(geom: &shell::layer::Geometry, over_predecessor: bool) -> shell::layer::Geometry {
    let mut sent = *geom;
    if over_predecessor && sent.zone > 0 {
        sent.zone = -1;
    }
    sent
}

fn zones_mapped(wins: impl IntoIterator<Item = (i32, bool)>) -> bool {
    wins.into_iter().all(|(zone, mapped)| zone <= 0 || mapped)
}

/// Work waiting for window `item` to leave the screen (`World::off_screen`).
struct OffScreen {
    item: usize,
    /// The outputs the window is on: a frame without it is presented on them.
    outputs: Vec<wl_output::WlOutput>,
    /// Set for a layer that keeps its last frame as the shell closes it
    /// ([`App::keep_last_frame`]); without it the window gives an empty last frame.
    close: Option<Close>,
    run: Box<dyn FnOnce() + Send>,
}

/// Hyprland's close animation of a layer the shell closes with its last frame on.
struct Close {
    /// What Hyprland calls the layer in `closelayer`.
    namespace: String,
    /// How long the animation runs, ms.
    ms: f64,
    /// When it ends; not known until Hyprland says it closed the layer.
    due: Option<Instant>,
}

/// Work whose layer the shell closed with its last frame on, for Hyprland to animate away
/// ([`shell::hyprclose`]).
struct Closing {
    id: u64,
    item: usize,
    close: Close,
    outputs: Vec<wl_output::WlOutput>,
    run: Box<dyn FnOnce() + Send>,
}

/// The outputs surface `s` is on (wl_surface.enter without a leave).
fn entered(s: &wl_surface::WlSurface) -> Vec<wl_output::WlOutput> {
    s.data::<SurfaceData<()>>()
        .map(|d| d.outputs().collect())
        .unwrap_or_default()
}

/// Work waiting for the compositor to show the screen without a gone window
/// ([`App::show_without`]).
struct Unshown {
    id: u64,
    /// The probes not answered yet, one per output the gone window was on; none from the
    /// start for a window that was on no output, when wl_display.sync is the answer.
    probes: Vec<Probe>,
    run: Box<dyn FnOnce() + Send>,
}

/// A surface the shell maps on an output only to hear when the compositor has shown that
/// output's next frame: a transparent square in a corner of the overlay layer, which takes no
/// input and reserves nothing. The compositor draws the overlay layer whatever else is on the
/// output (over a fullscreen window too), and mapping a surface makes it repaint, so the
/// first frame of this one is presented, which no commit of a window already there is sure of.
struct Probe {
    layer: LayerSurface,
    on: wl_output::WlOutput,
    /// Its buffer, attached by the commit that follows the first configure.
    buffer: Option<smithay_client_toolkit::shm::slot::Buffer>,
}

/// The zwlr_output_power_v1 the shell keeps for the output with this id
/// ([`App::sync_unseen`], [`App::probe_output_off`]).
struct PowerOn(wayland_client::backend::ObjectId);

impl Dispatch<ZwlrOutputPowerV1, PowerOn> for App {
    fn event(
        app: &mut Self,
        power: &ZwlrOutputPowerV1,
        ev: zwlr_output_power_v1::Event,
        of: &PowerOn,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwlr_output_power_v1::Event::Mode { mode } => {
                let on = mode != WEnum::Value(zwlr_output_power_v1::Mode::Off);
                if let Some(p) = app.output_powers.get_mut(&of.0) {
                    p.1 = on;
                }
                if !on {
                    app.probe_output_off(&of.0);
                }
            }
            // No power control for this output (or it is gone, which `output_destroyed`
            // handles): its state is unknown, so it counts as on, and a probe waits as usual.
            zwlr_output_power_v1::Event::Failed => {
                if app.output_powers.get(&of.0).is_some_and(|p| p.0 == *power) {
                    app.output_powers.remove(&of.0);
                }
                power.destroy();
            }
            _ => {}
        }
        app.sync_unseen();
    }
}

/// The side of a [`Probe`], in logical pixels. Hyprland does not draw a surface whose box,
/// scaled by the output's scale and rounded, is one device pixel or less on a side, and
/// answers `discarded` for it (0.57.1 src/render/ElementRenderer.cpp, `drawSurface`:
/// `windowBox.scale(scale).round()`, then `width <= 1 || height <= 1` discards). Its least
/// output scale is 0.25 (src/config/shared/monitor/Parser.cpp), where 8 is two device pixels.
const PROBE_SIDE: i32 = 8;

/// [`Probe`]'s layer-surface state: in the top left corner of what the exclusive zones leave,
/// reserving nothing itself.
const PROBE: Geometry = Geometry {
    anchor: shell::layer::TOP | shell::layer::LEFT,
    size: (PROBE_SIDE as u32, PROBE_SIDE as u32),
    margins: (0, 0, 0, 0),
    zone: 0,
    edge: 0,
};

/// An answer [`Unshown`] waits for.
enum OffScreenShown {
    /// The wl_callback of wl_display.sync, for the work of this id.
    Sync(u64),
    /// The wl_callback of wl_display.sync, for the [`Closing`] of this id.
    Closed(u64),
    /// The wp_presentation_feedback of the first frame of the probe with this surface, or
    /// without wp_presentation the wl_callback of that frame.
    Probe(wl_surface::WlSurface),
}

impl Dispatch<WpPresentationFeedback, OffScreenShown> for App {
    fn event(
        app: &mut Self,
        _: &WpPresentationFeedback,
        ev: wp_presentation_feedback::Event,
        shown: &OffScreenShown,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match (ev, shown) {
            (wp_presentation_feedback::Event::Presented { .. }, _) => {
                app.off_screen_answered(shown);
            }
            (wp_presentation_feedback::Event::Discarded, OffScreenShown::Probe(s)) => {
                app.probe_discarded(s);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, OffScreenShown> for App {
    fn event(
        app: &mut Self,
        _: &wl_callback::WlCallback,
        ev: wl_callback::Event,
        shown: &OffScreenShown,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = ev {
            app.off_screen_answered(shown);
        }
    }
}

/// wl_display.sync after the turn that made these `ShellControl.ipcCall` calls
/// ([`App::order_self_ipc`]): their handlers run once it is done.
struct SelfIpc(Vec<(String, String)>);

impl Dispatch<wl_callback::WlCallback, SelfIpc> for App {
    fn event(
        app: &mut Self,
        _: &wl_callback::WlCallback,
        ev: wl_callback::Event,
        calls: &SelfIpc,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = ev {
            app.self_ipc.extend(calls.0.iter().cloned());
        }
    }
}

/// wl_display.sync after `zwlr_layer_surface_v1.get_popup`: the compositor has read it, and
/// the popup's surface, if the shell still has it, gets its first commit.
struct PopupAdopted(wl_surface::WlSurface);

impl Dispatch<wl_callback::WlCallback, PopupAdopted> for App {
    fn event(
        app: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        s: &PopupAdopted,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if app.wins.iter().any(|w| w.surf.wl_surface() == &s.0) {
            s.0.commit();
        }
    }
}

/// wl_display.sync after a window's first buffer commit: the compositor has mapped the surface
/// (and applied a layer surface's exclusive zone) once this is done.
struct MapSync(wl_surface::WlSurface);

/// `w` just committed a buffer: the first one after its configure maps it.
/// Records a commit of `w`'s new frame, with a frame callback requested or without one.
fn mark_shown(w: &mut Win, callback: bool) {
    let now = std::time::Instant::now();
    w.pending = callback;
    w.asked = callback.then_some(now);
    w.shown = (!callback).then_some(now);
    w.committed = Some(now);
}

fn committed(display: &wl_display::WlDisplay, qh: &QueueHandle<App>, w: &mut Win) {
    if w.mapped.is_none() {
        w.mapped = Some(false);
        display.sync(qh, MapSync(w.surf.wl_surface().clone()));
    }
}

impl Dispatch<wl_callback::WlCallback, MapSync> for App {
    fn event(
        app: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        s: &MapSync,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(w) = app.wins.iter_mut().find(|w| w.surf.wl_surface() == &s.0) {
            w.mapped = Some(true);
        }
    }
}

#[cfg(test)]
mod tests {
    /// A capture of a live, shown view whose first frame is installed.
    fn live_capture() -> super::Capture {
        super::Capture {
            handle: Some(1),
            got: true,
            live: true,
            ..Default::default()
        }
    }

    #[test]
    fn live_capture_waits_for_its_frame_to_be_drawn() {
        let mut cap = live_capture();
        assert!(
            !cap.wants_frame(),
            "re-armed before any window drew the frame"
        );
        cap.painted = true;
        assert!(cap.wants_frame());
        cap.live = false;
        assert!(!cap.wants_frame(), "a non-live view keeps its first frame");
    }

    /// A lost GPU context takes every frame of a capture with it: the capture asks for its
    /// first frame again, whether or not the last one was drawn or one was landing, and stays
    /// the capture of the same toplevel with the same settings. One that failed stays stopped.
    #[test]
    fn capture_starts_over_after_a_lost_context() {
        let mut cap = live_capture();
        cap.cursor = true;
        assert!(!cap.wants_frame(), "waits for its frame to be drawn");
        let fresh = cap.restarted();
        assert!(fresh.wants_frame(), "the first frame is requested again");
        assert_eq!(
            (fresh.handle, fresh.cursor, fresh.live, fresh.got),
            (Some(1), true, true, false)
        );
        cap.landing = true;
        assert!(cap.restarted().wants_frame(), "a landing frame is dropped");
        cap.live = false;
        let fresh = cap.restarted();
        assert!(fresh.wants_frame() && !fresh.live, "a frozen view too");
        cap.stopped = true;
        assert!(!cap.restarted().wants_frame(), "a failed capture stays so");
    }

    #[test]
    fn landing_frame_holds_the_next_request() {
        let mut cap = live_capture();
        cap.painted = true;
        cap.landing = true;
        assert!(!cap.wants_frame());
    }

    #[test]
    fn failed_capture_is_not_requested_again() {
        let mut cap = super::Capture {
            handle: Some(1),
            ..Default::default()
        };
        assert!(cap.wants_frame(), "the first frame is requested");
        cap.stopped = true;
        assert!(!cap.wants_frame());
        let mut cap = live_capture();
        cap.painted = true;
        cap.stopped = true;
        assert!(!cap.wants_frame());
    }

    use super::{PanelState, pair_successors};

    #[test]
    fn replaced_bar_waits_for_successor_first_frame() {
        let dp = Some("DP-1".to_string());
        // Bar (item 1) replaced by the vertical bar (item 2); item 3 is an unrelated panel.
        let vbar: Vec<PanelState> = vec![(2, dp.clone(), 46, None), (3, dp.clone(), 0, Some(true))];
        assert_eq!(pair_successors(&[(1, dp.clone(), None)], &vbar), [Some(2)]);
        let drawn: Vec<PanelState> = vec![(2, dp.clone(), 46, Some(false))];
        assert_eq!(pair_successors(&[(1, dp.clone(), Some(2))], &drawn), [None]);
        let hdmi = Some("HDMI-A-1".to_string());
        assert_eq!(pair_successors(&[(1, hdmi, None)], &vbar), [None]);
    }

    #[test]
    fn replaced_bar_goes_when_bar_hidden() {
        let dp = Some("DP-1".to_string());
        // Only panels reserving no space are left (the bar was hidden, not moved).
        let left: Vec<PanelState> = vec![(2, dp.clone(), 0, None), (3, dp.clone(), -1, None)];
        assert_eq!(pair_successors(&[(1, dp.clone(), None)], &left), [None]);
        assert_eq!(pair_successors(&[(1, dp.clone(), None)], &[]), [None]);
    }

    #[test]
    fn replaced_bar_goes_when_successor_closed() {
        let dp = Some("DP-1".to_string());
        let mut panels: Vec<PanelState> =
            vec![(2, dp.clone(), 46, None), (3, dp.clone(), 0, Some(true))];
        assert_eq!(
            pair_successors(&[(1, dp.clone(), Some(2))], &panels),
            [Some(2)]
        );
        // layer_surface.closed (or its output gone) removes the successor before it drew.
        panels.remove(0);
        assert_eq!(
            pair_successors(&[(1, dp.clone(), Some(2))], &panels),
            [None]
        );
    }

    #[test]
    fn two_replaced_panels_each_wait_for_their_own_successor() {
        // The bar (item 1) and the dock (item 2) on DP-1 both get new surfaces: each old one
        // goes with its own successor's first frame. The bar's first frame used to drop both,
        // so the dock's zone was gone until the new dock drew.
        let dp = Some("DP-1".to_string());
        let mut replaced = vec![(1, dp.clone(), None), (2, dp.clone(), None)];
        let mut panels: Vec<PanelState> =
            vec![(2, dp.clone(), 60, None), (1, dp.clone(), 46, None)];
        let succ = pair_successors(&replaced, &panels);
        assert_eq!(
            succ,
            [Some(1), Some(2)],
            "the same item on its new surface first"
        );
        for (r, s) in replaced.iter_mut().zip(&succ) {
            r.2 = *s;
        }
        let mut retiring = vec![(1, succ[0]), (2, succ[1])];
        // The new bar commits its first frame (`retire_replaced(1)`): only the old bar goes.
        retiring.retain(|r| r.1 != Some(1));
        assert_eq!(retiring, [(2, Some(2))], "the old dock keeps its zone");
        replaced.remove(0);
        panels[1].3 = Some(false);
        assert_eq!(
            pair_successors(&replaced, &panels),
            [Some(2)],
            "still waits for the new dock"
        );
        panels[0].3 = Some(false);
        assert_eq!(pair_successors(&replaced, &panels), [None]);
    }

    #[test]
    fn replaced_panels_of_other_items_get_one_successor_each() {
        // The bar (1) and the dock (2) both replaced by new items (3, 4) on one output.
        let dp = Some("DP-1".to_string());
        let replaced = [(1, dp.clone(), None), (2, dp.clone(), None)];
        let panels: Vec<PanelState> = vec![(3, dp.clone(), 46, None), (4, dp.clone(), 60, None)];
        assert_eq!(pair_successors(&replaced, &panels), [Some(3), Some(4)]);
        // Only one successor so far: the other replaced panel has none to wait for.
        assert_eq!(pair_successors(&replaced, &panels[..1]), [Some(3), None]);
        // A pairing holds even when an earlier replaced panel could take that successor.
        let held = [(1, dp.clone(), None), (2, dp.clone(), Some(3))];
        assert_eq!(pair_successors(&held, &panels), [Some(4), Some(3)]);
    }

    #[test]
    fn mode_logical_size_follows_transform_and_scale() {
        use wl_output::Transform as T;
        assert_eq!(mode_logical_size((3840, 2160), T::Normal, 2), (1920, 1080));
        assert_eq!(mode_logical_size((2560, 1440), T::_90, 1), (1440, 2560));
        assert_eq!(
            mode_logical_size((2560, 1440), T::Flipped270, 2),
            (720, 1280)
        );
        assert_eq!(mode_logical_size((1920, 1080), T::Flipped, 0), (1920, 1080));
    }

    #[test]
    fn hidden_window_draws_only_when_its_frame_callback_comes_back() {
        // Every animating window awaits its callback (hidden, or the step is on screen): the
        // clock waits, however long, and the first callback back is the next step.
        assert!(awaits_frame([(true, true), (false, false)], false, false));
        assert!(awaits_frame([(true, true), (true, true)], false, false));
        // A hidden animating window does not stall one that can draw its animation...
        assert!(!awaits_frame([(true, true), (false, true)], false, false));
        // ...nor an animation shown in no window (a script step).
        assert!(!awaits_frame([(true, true)], true, false));
        assert!(!awaits_frame([(true, false), (false, false)], false, false));
        // An endless animation that shows nothing waits like a hidden window, unless another
        // animation can draw or is shown in no window.
        assert!(awaits_frame([(false, false)], false, true));
        assert!(!awaits_frame([(false, true)], false, true));
        assert!(!awaits_frame([(false, false)], true, true));
        let mut clock = AnimClock::new();
        clock.advance(0.0, true, 0.0, false);
        assert!(!clock.step_due(5000.0, true));
        assert!(clock.step_due(5000.0, false));
        clock.frame_due = true;
        assert!(clock.step_due(5000.0, true));
    }

    /// On the wall clock too (the driver's timer mode, after frames that came late) the
    /// animations an event started stand at their start until a frame has shown them there:
    /// a pass before that frame (the button's release, a turn that draws nothing) gave them
    /// the fraction of a ms that had passed, a first step no frame showed the start of.
    #[test]
    fn the_wall_clock_holds_the_start_until_a_frame_showed_it() {
        let mut clock = AnimClock {
            timer_mode: true,
            ..AnimClock::new()
        };
        let start = clock.begin(1000.0, 0.0);
        assert_eq!(clock.stamp(1000.5), start);
        assert_eq!(clock.advance(1001.0, true, 0.0, false), start);
        clock.shown();
        // With no frame to wait for the first step is due at once. After it a pass that is
        // no new frame (a wakeup between two ticks of the timer, a wait for a frame callback)
        // moves nothing either; a step moves by the wall time that passed.
        assert_eq!(clock.advance(1001.5, true, 0.0, true), start);
        assert_eq!(clock.advance(1002.0, true, 0.0, false), start + 2.0);
        assert_eq!(clock.advance(1003.0, true, 0.0, false), start + 2.0);
        assert_eq!(clock.advance(1017.0, true, 0.0, false), start + 17.0);
        assert_eq!(clock.stamp(1020.0), start + 17.0);
        assert_eq!(clock.advance(1025.0, true, 0.0, true), start + 17.0);
        clock.frame_due = true;
        assert_eq!(clock.advance(1030.0, true, 0.0, true), start + 30.0);
    }

    /// QQmlAnimationTimer::startAnimations starts the driver from the event loop, and
    /// QSGThreadedRenderLoop steps it right after the sync of the update it gives the window
    /// for that: an animation's first step is due as soon as it is registered, not one vsync
    /// later. The frame that shows it is followed by a second step (`App::draw_frame`), and the
    /// third comes with that frame's callback.
    #[test]
    fn the_first_step_is_due_as_soon_as_an_animation_is_registered() {
        let mut clock = AnimClock::new();
        let v = clock.vsync;
        let idle = clock.advance(1000.0, false, 0.0, false);
        assert!(clock.arm(1000.0, false, false, None, false).is_none());
        let due = clock.arm(1000.2, true, false, None, true);
        assert!(matches!(due, Some((at, Due::Frame)) if at == 1000.2));
        assert!(clock.step_due(1000.2, false));
        assert_eq!(clock.advance(1000.2, true, idle, false), idle + v.trunc());
        assert!(
            !clock.led,
            "the frame of the first step is followed by a step"
        );
        // A frame that changed no pixel is not sent and takes that step back: the second
        // step is the vsync timer's.
        clock.frame_due = true;
        clock.sent(false, true);
        let due = clock.arm(1000.3, true, false, None, true);
        assert!(matches!(due, Some((at, Due::Frame)) if at > 1000.2 + 0.75 * v));
        assert_eq!(clock.advance(1005.0, true, idle, false), idle + v.trunc());
        // A frame that was sent is followed by the step, and its callback by the next.
        clock.frame_due = true;
        clock.sent(true, true);
        assert!(clock.led);
        assert_eq!(
            clock.advance(1006.0, true, idle, true),
            idle + (2.0 * v).trunc()
        );
        assert_eq!(
            clock.advance(1007.0, true, idle, true),
            idle + (2.0 * v).trunc()
        );
        clock.callback(0);
        assert_eq!(
            clock.advance(1010.0, true, idle, true),
            idle + (3.0 * v).trunc()
        );
    }

    /// The frame of the first step was sent, and its frame callback came before the step
    /// that follows the frame was taken (a slow frame): they are two steps, both due at once.
    /// Qt's main thread took the second step as its render thread drew the frame.
    #[test]
    fn a_frame_callback_that_comes_before_the_step_after_the_first_frame_is_a_step_too() {
        let mut clock = AnimClock::new();
        let v = clock.vsync;
        let idle = clock.advance(1000.0, false, 0.0, false);
        assert_eq!(clock.advance(1000.2, true, idle, false), idle + v.trunc());
        clock.frame_due = true;
        clock.sent(true, true);
        clock.callback(0);
        assert_eq!(
            clock.advance(1012.0, true, idle, false),
            idle + (2.0 * v).trunc()
        );
        assert!(clock.step_due(1012.5, true));
        assert_eq!(
            clock.advance(1012.5, true, idle, true),
            idle + (3.0 * v).trunc()
        );
        assert!(!clock.step_due(1013.0, true));
    }

    /// With none or several windows exposed QSGThreadedRenderLoop steps the driver by a timer
    /// of one vsync (startOrStopAnimationTimer): the first step comes one interval after the
    /// start, and the frame that shows it is followed by no step before its frame callback.
    #[test]
    fn with_several_windows_the_first_step_comes_one_vsync_after_the_start() {
        let mut clock = AnimClock::new();
        let v = clock.vsync;
        clock.one_window = false;
        let idle = clock.advance(1000.0, false, 0.0, false);
        let due = clock.arm(1000.2, true, false, None, true);
        assert!(matches!(due, Some((at, Due::Frame)) if at > 1000.0 + 0.75 * v));
        assert_eq!(
            clock.advance(1000.0 + v, true, idle, false),
            idle + v.trunc()
        );
        assert!(
            clock.led,
            "the frame of the first step is followed by no extra step"
        );
        assert!(!clock.step_due(1000.5 + v, true));
        clock.callback(0);
        assert_eq!(
            clock.advance(1010.0 + v, true, idle, true),
            idle + (2.0 * v).trunc()
        );
    }

    /// The handler of the only Timer counting started the animation (`AnimClock::late`):
    /// Qt's driver was started by the Timer's end and stepped before the animation was in
    /// its list, so the first step waits for the frame callback of the frame sent meanwhile,
    /// or for the vsync timer when no window sent one.
    #[test]
    fn an_animation_started_as_the_only_timer_ends_steps_first_with_a_frame_callback() {
        let mut clock = AnimClock::new();
        let v = clock.vsync;
        let idle = clock.advance(1000.0, false, 0.0, false);
        clock.late = true;
        let due = clock.arm(1000.0, true, true, Some(1100.0), true);
        assert!(matches!(due, Some((at, Due::Frame)) if at == 1100.0));
        assert_eq!(clock.advance(1001.0, true, idle, true), idle + 1.0);
        assert!(!clock.running);
        clock.callback(0);
        assert_eq!(clock.advance(1010.0, true, idle, false), idle + v.trunc());
        assert!(clock.running && !clock.late && clock.led);

        let mut clock = AnimClock::new();
        let idle = clock.advance(1000.0, false, 0.0, false);
        clock.late = true;
        let due = clock.arm(1000.0, true, false, None, true);
        assert!(matches!(due, Some((at, Due::Frame)) if at == 1000.0 + v));
        assert_eq!(clock.advance(1005.0, true, idle, false), idle + 5.0);
        assert!(!clock.running);
        clock.arm(1005.0, true, false, None, true);
        assert_eq!(
            clock.advance(1000.0 + v, true, idle, false),
            idle + v.trunc()
        );
        assert!(clock.running && !clock.late);
    }

    /// A step by the vsync timer is out of step with the output, so the frame callback of the
    /// frame it sent comes a part of an interval later: the driver moves by that part, not by
    /// a whole interval, and an animation whose frames are now sent, now not, takes its time.
    #[test]
    fn a_frame_callback_after_a_timer_step_moves_the_driver_by_the_time_passed() {
        let mut clock = AnimClock::new();
        let v = clock.vsync;
        let t0 = clock.advance(0.0, true, 0.0, false);
        let mut wall = 0.0;
        for _ in 0..10 {
            // A step by the timer, whose frame is sent; its callback 4 ms later; then two
            // steps by the timer that send nothing.
            wall += v;
            clock.advance(wall, true, 0.0, false);
            wall += 4.0;
            clock.frame_due = true;
            clock.advance(wall, true, 0.0, true);
            for _ in 0..2 {
                wall += v;
                clock.advance(wall, true, 0.0, false);
            }
        }
        let t = clock.advance(wall, true, 0.0, false) - t0;
        assert!((t - wall).abs() < 11.0, "{t} ms of animation in {wall} ms");
        // Frame callbacks one after another are a whole interval each, however they come.
        let before = clock.advance(wall, true, 0.0, true);
        for _ in 0..10 {
            wall += 5.0;
            clock.frame_due = true;
            clock.advance(wall, true, 0.0, true);
        }
        let t = clock.advance(wall, true, 0.0, true) - before;
        assert!((t - 9.0 * v - 5.0).abs() < 2.0, "{t}");
    }

    /// With no frame to pace it (a window the compositor does not show) the driver steps on
    /// the vsync timer, one interval a tick, as QSGThreadedRenderLoop's animation timer calls
    /// QSGDefaultAnimationDriver::advance() (`m_time += m_vsync`) at each of its events. The
    /// timer keeps its grid, so a tick handled late is followed by one on time, less than an
    /// interval later, and that one is a step too: a 500 ms animation takes 500 ms.
    #[test]
    fn a_timer_tick_soon_after_a_late_one_is_a_step() {
        let mut clock = AnimClock::new();
        let t0 = clock.advance(0.0, true, 0.0, false);
        clock.arm(0.0, true, false, None, true);
        let (mut wall, mut ticks) = (0.0, 0);
        // The loop wakes 20, 20, 20 and 7 ms after its last turn: each time the timer is due.
        for late in [20.0, 20.0, 20.0, 7.0].into_iter().cycle().take(30) {
            wall += late;
            assert!(
                clock.timer.is_some_and(|at| at <= wall),
                "{:?}",
                clock.timer
            );
            assert!(clock.step_due(wall, false), "tick {ticks} at {wall} ms");
            ticks += 1;
            let t = clock.advance(wall, true, 0.0, false) - t0;
            let want = (f64::from(ticks + 1) * clock.vsync).trunc() - clock.vsync.trunc();
            assert_eq!(t, want, "after tick {ticks} at {wall} ms");
            clock.arm(wall, true, false, None, true);
        }
        assert_eq!(clock.advance(wall, true, 0.0, false) - t0, 500.0);
        assert_eq!(wall, 509.0);
        assert!(!clock.timer_mode);
        // A wakeup before the next tick (an IPC call) is no step, though most of an interval
        // has passed since the last one.
        let at = clock.timer.unwrap();
        let before = clock.advance(at, true, 0.0, false);
        clock.arm(at, true, false, None, true);
        let early = at + 0.9 * clock.vsync;
        assert!(!clock.step_due(early, false));
        assert_eq!(clock.advance(early, true, 0.0, false), before);
        clock.arm(early, true, false, None, true);
        assert_eq!(clock.timer, Some(at + clock.vsync));
        assert!(clock.step_due(at + clock.vsync, false));
        assert!(clock.advance(at + clock.vsync, true, 0.0, false) > before);
        // A step by a frame callback is not followed by the tick the timer was armed for.
        clock.arm(at + clock.vsync, true, false, None, true);
        let frame = at + 2.0 * clock.vsync - 2.0;
        clock.frame_due = true;
        let before = clock.advance(frame, true, 0.0, true);
        clock.arm(frame, true, false, None, true);
        assert_eq!(clock.timer, Some(frame + clock.vsync));
        assert_eq!(clock.advance(frame + 2.0, true, 0.0, false), before);
    }

    /// Frames far slower than vsync switch Qt's default driver to wall time; with
    /// QSG_FIXED_ANIMATION_STEP each frame still moves animations 16 ms.
    #[test]
    fn fixed_animation_step_ignores_the_wall_clock() {
        let run = |fixed| {
            let mut clock = AnimClock {
                fixed,
                ..AnimClock::new()
            };
            let t0 = clock.advance(0.0, true, 0.0, false);
            (1..=20)
                .map(|n| clock.advance(f64::from(n) * 500.0, true, 0.0, false) - t0)
                .collect::<Vec<_>>()
        };
        let fixed = run(Some(16.0));
        assert_eq!(
            fixed,
            (1..=20).map(|n| f64::from(n) * 16.0).collect::<Vec<_>>()
        );
        let default = run(None);
        assert!(
            default[19] > 1000.0,
            "the default driver kept vsync steps: {default:?}"
        );
    }

    /// A loop woken every ms (posted events that post themselves again) with nothing drawn,
    /// so no frame callback to wait for: the animation clock steps once per vsync interval.
    #[test]
    fn wakeups_without_end_step_the_clock_once_per_vsync() {
        let mut clock = AnimClock {
            fixed: None,
            ..AnimClock::new()
        };
        let mut at = clock.advance(0.0, true, 0.0, false);
        let mut steps = 0;
        for ms in 1..=1000 {
            let wall = f64::from(ms);
            let due = clock.step_due(wall, false);
            let t = clock.advance(wall, true, 0.0, false);
            assert_eq!(due, t != at, "at {ms} ms");
            steps += u32::from(t != at);
            at = t;
        }
        assert!(!clock.timer_mode);
        assert!((59..=61).contains(&steps), "{steps} steps in a second");
    }

    /// The same loop with turns that take 2 ms each, one right after the other: a tick of the
    /// vsync timer nearly always comes due while a turn runs. The turn after it handles it, as
    /// a QTimer that timed out while its thread was busy fires once the event loop is back.
    #[test]
    fn a_tick_that_comes_due_during_a_turn_is_a_step() {
        let mut clock = AnimClock {
            fixed: None,
            ..AnimClock::new()
        };
        let mut at = clock.advance(0.0, true, 0.0, false);
        clock.arm(0.0, true, false, None, true);
        let mut steps = 0;
        for turn in 0..500 {
            let wall = f64::from(turn) * 2.0 + 1.0;
            let t = clock.advance(wall, true, 0.0, false);
            steps += u32::from(t != at);
            at = t;
            clock.arm(wall + 1.9, true, false, None, true);
        }
        assert!(!clock.timer_mode);
        assert!((59..=61).contains(&steps), "{steps} steps in a second");
    }

    /// QSGThreadedRenderLoop computes a step two frames ahead of the screen. A turn that
    /// outlasts both leaves it nothing to wait for: a step is taken as soon as the turn is
    /// over, and one more at once. Measured under Quickshell (crates/shell/tests/
    /// timer_clock_equiv.rs): a step of 100 ms is followed by steps 0, 2 and 4 ms after it.
    #[test]
    fn a_turn_of_two_frames_is_followed_by_two_steps_at_once() {
        let v = AnimClock::new().vsync;
        // The frames change no pixel: steps by the vsync timer. The three ticks before the
        // end of the late turn are due at once, one a turn; the fourth an interval later.
        let mut clock = AnimClock::new();
        let t0 = clock.advance(0.0, true, 0.0, false);
        clock.arm(0.0, true, false, None, true);
        clock.advance(v, true, 0.0, false);
        clock.arm(v + 100.0, true, false, None, true);
        let mut wall = v + 100.0;
        for step in 2..=4 {
            wall += 0.1;
            assert!(clock.step_due(wall, false), "step {step}");
            let t = clock.advance(wall, true, 0.0, false) - t0;
            assert_eq!(t.round(), (f64::from(step) * v).round(), "step {step}");
            clock.arm(wall, true, false, None, true);
        }
        assert!(!clock.step_due(wall + 0.1, false));
        let next = clock.timer.unwrap();
        assert!((next - (v + 100.0 + v)).abs() < 1.0, "{next}");
        // A turn of less than two frames: the one tick it missed, and the grid goes on.
        let mut clock = AnimClock::new();
        clock.advance(0.0, true, 0.0, false);
        clock.arm(0.0, true, false, None, true);
        clock.advance(v, true, 0.0, false);
        clock.arm(v + 30.0, true, false, None, true);
        assert!(clock.step_due(v + 30.1, false));
        clock.advance(v + 30.1, true, 0.0, false);
        clock.arm(v + 30.1, true, false, None, true);
        assert!(!clock.step_due(v + 30.2, false));
        assert!((clock.timer.unwrap() - 3.0 * v).abs() < 1e-9);

        // Frames are sent: steps by their callbacks. A turn of 100 ms whose step began with a
        // frame callback outstanding or not; then the steps taken at once, the first of them
        // for the `callbacks` that came before it.
        let at_once = |flying: bool, callbacks: u32| {
            let mut clock = AnimClock::new();
            clock.advance(0.0, true, 0.0, false);
            clock.shown();
            clock.callback(0);
            clock.advance(v, true, 0.0, flying);
            clock.arm(v + 100.0, true, true, None, true);
            for _ in 0..callbacks {
                clock.callback(0);
            }
            let mut steps = 0;
            for turn in 1..=5 {
                let wall = v + 100.0 + f64::from(turn) * 0.1;
                steps += u32::from(clock.step_due(wall, true));
                clock.advance(wall, true, 0.0, true);
                clock.arm(wall, true, true, None, true);
            }
            steps
        };
        // The callback on its way when the turn began came during it: its step, and one more.
        assert_eq!(at_once(true, 1), 2);
        // The frame drawn as it came is answered before that step is taken: a step more.
        assert_eq!(at_once(true, 2), 3);
        // None was on its way: the callback that comes answers the frame sent after the turn.
        // Its step, and two more.
        assert_eq!(at_once(false, 1), 3);
        // A step taken with no callback began with the frame of the turn before it in flight.
        let mut clock = AnimClock::new();
        clock.advance(0.0, true, 0.0, false);
        clock.shown();
        clock.callback(0);
        clock.advance(v, true, 0.0, false);
        clock.arm(v + 100.0, true, true, None, true);
        clock.advance(v + 100.1, true, 0.0, true);
        clock.arm(v + 200.0, true, true, None, true);
        assert_eq!(clock.ahead, 1);
        // A turn of less than two frames is followed by the callback's step alone.
        let mut clock = AnimClock::new();
        clock.advance(0.0, true, 0.0, false);
        clock.shown();
        clock.callback(0);
        clock.advance(v, true, 0.0, true);
        clock.arm(v + 30.0, true, true, None, true);
        clock.callback(0);
        clock.advance(v + 30.1, true, 0.0, true);
        clock.arm(v + 30.1, true, true, None, true);
        assert!(!clock.step_due(v + 30.2, true));
    }

    /// QSGDefaultAnimationDriver::advance: three frames in a row that came late, more than 10
    /// vsyncs late together, put the driver on wall time (TimerMode), and more than 10 in a
    /// row that came on time put it back. On wall time a step moves the clock by what the
    /// wall moved, and between two steps the clock stands however often the loop wakes:
    /// the driver's time is read when a frame advances it.
    /// QUnifiedTimer::maybeUpdateAnimationsToCurrentTime: an animation started more than
    /// 50 ms after the last step of a driver on wall time steps the running ones to the
    /// current time; the driver's count of timely frames does not see that step.
    #[test]
    fn a_start_long_after_a_step_on_wall_time_steps_the_clock() {
        let mut clock = AnimClock::new();
        let mut wall = 0.0;
        clock.advance(wall, true, 0.0, false);
        wall += 16.0;
        clock.frame_due = true;
        clock.advance(wall, true, 0.0, false);
        assert_eq!(clock.catch_up(wall + 500.0), None, "the clock is on vsyncs");
        let mut t = 0.0;
        for _ in 0..3 {
            wall += 70.0;
            clock.frame_due = true;
            t = clock.advance(wall, true, 0.0, false);
        }
        assert!(clock.timer_mode);
        assert_eq!(clock.catch_up(wall + 50.0), None);
        assert_eq!(clock.catch_up(wall + 60.0), Some(t + 60.0));
        assert_eq!(clock.stamp(wall + 65.0), t + 60.0);
        assert_eq!(clock.catch_up(wall + 100.0), None, "40 ms after that step");
        assert_eq!(clock.catch_up(wall + 115.0), Some(t + 115.0));
        // The frame comes 120 ms after the one before it: late for the driver, and
        // animations move by what is left of it.
        clock.good = 5;
        clock.frame_due = true;
        assert_eq!(clock.advance(wall + 120.0, true, 0.0, false), t + 120.0);
        assert_eq!(clock.good, 0);
        assert_eq!(clock.catch_up(wall + 150.0), None);
        // Stopped, the clock follows the wall. The driver was behind by its three late frames
        // before it went on wall time: a start reads the wall for that, once.
        clock.advance(wall + 200.0, false, 0.0, false);
        let owed = clock.owed;
        assert!(owed > 100.0, "{owed}");
        assert_eq!(clock.catch_up(wall + 300.0), Some(t + 300.0 + owed));
        assert_eq!(clock.catch_up(wall + 400.0), None);
    }

    /// A driver that ran 19 steps, two of them 100 ms after the one before, then stopped:
    /// its time is 150 ms behind the wall time that went by (the first step is at the start).
    fn stopped_behind() -> (AnimClock, f64, f64) {
        let mut clock = AnimClock {
            fixed: None,
            ..AnimClock::new()
        };
        clock.advance(1000.0, false, 0.0, false);
        clock.advance(1000.0, true, 1000.0, false);
        let mut wall = 1000.0;
        for step in 2..=19 {
            wall += if step <= 3 { 100.0 } else { clock.vsync };
            clock.advance(wall, true, 1000.0, false);
        }
        assert!(!clock.timer_mode);
        let t = clock.advance(wall, false, 0.0, false);
        assert_eq!(t - 1000.0, 316.0, "19 steps of one vsync");
        assert!((clock.owed - (wall - 1000.0 - 316.0)).abs() < 1.0);
        assert!((clock.owed - 150.0).abs() < 2.0, "{}", clock.owed);
        (clock, t, wall)
    }

    #[test]
    fn timers_get_the_time_a_stopped_driver_was_behind_when_the_first_is_due() {
        let (mut clock, t, wall) = stopped_behind();
        let owed = clock.owed;
        // The Timer due first is due 184 ms on: until then the clock goes on from the
        // driver's time.
        clock.read_wall(wall + 100.0, t + 184.0);
        assert_eq!(clock.advance(wall + 100.0, false, 0.0, false), t + 100.0);
        clock.read_wall(wall + 183.0, t + 184.0);
        assert_eq!(clock.now(wall + 183.0), t + 183.0);
        // It fires then, and the clock reads the wall: a second Timer is that much nearer.
        clock.read_wall(wall + 184.0, t + 184.0);
        assert_eq!(clock.now(wall + 184.0), t + 184.0 + owed);
        assert_eq!(clock.owed, 0.0);
        assert_eq!(
            clock.advance(wall + 200.0, false, 0.0, false),
            t + 200.0 + owed
        );
    }

    #[test]
    fn a_stopped_driver_that_was_ahead_makes_the_first_timer_wait() {
        let mut clock = AnimClock {
            fixed: None,
            ..AnimClock::new()
        };
        // Steps 10 ms apart: the driver's time runs ahead of the wall.
        let mut wall = 0.0;
        clock.advance(wall, true, 0.0, false);
        for _ in 0..3 {
            wall += 10.0;
            clock.frame_due = true;
            clock.advance(wall, true, 0.0, true);
        }
        let t = clock.advance(wall, false, 0.0, false);
        let ahead = -clock.owed;
        assert!(ahead > 15.0, "{ahead}");
        clock.read_wall(wall + 50.0, t + 50.0);
        assert_eq!(clock.now(wall + 50.0), t + 50.0 - ahead);
    }

    #[test]
    fn with_no_timer_left_nothing_gets_what_the_stopped_driver_was_behind() {
        let (mut clock, t, wall) = stopped_behind();
        clock.read_wall(wall + 1.0, f64::INFINITY);
        assert_eq!(clock.owed, 0.0);
        clock.read_wall(wall + 300.0, t + 184.0);
        assert_eq!(clock.now(wall + 300.0), t + 300.0);
    }

    #[test]
    fn a_start_after_the_driver_stopped_behind_reads_the_wall_first() {
        // More than 50 ms of wall time since the driver's last step, with what it was behind:
        // the Timers step to the wall time before what was started joins them.
        let (mut clock, t, wall) = stopped_behind();
        let owed = clock.owed;
        assert_eq!(clock.catch_up(wall + 10.0), Some(t + 10.0 + owed));
        assert_eq!(clock.owed, 0.0);
        assert_eq!(clock.catch_up(wall + 100.0), None);
    }

    #[test]
    fn a_driver_started_after_the_wall_was_read_steps_one_vsync() {
        // An input handler starts an animation long after the driver stopped behind
        // (`respond_to_input`): the wall is read first, so the driver that starts has nothing
        // left to bring with its first step and the animation is shown from its start.
        let (mut clock, t, wall) = stopped_behind();
        let owed = clock.owed;
        let now = clock.catch_up(wall + 500.0).unwrap();
        assert_eq!(now, t + 500.0 + owed);
        let since = clock.begin(wall + 500.0, now);
        assert_eq!(since, now);
        assert_eq!(clock.owed, 0.0);
        let start = clock.advance(wall + 501.0, true, since, false);
        assert!((start - since).abs() < 1.0, "{start} {since}");
        clock.shown();
        let first = clock.advance(wall + 500.0 + clock.vsync, true, since, false);
        let step = first - since;
        assert!(
            (step - clock.vsync).abs() < 1.5,
            "a first step of {step} ms"
        );
    }

    #[test]
    fn a_driver_started_again_begins_at_the_wall_time() {
        let mut clock = AnimClock {
            fixed: None,
            ..AnimClock::new()
        };
        // One frame of 60 ms: the driver stops 27 ms behind, within the 50 ms no start reads
        // the wall for.
        clock.advance(0.0, true, 0.0, false);
        let t = clock.advance(60.0, true, 0.0, false);
        assert_eq!(clock.advance(60.0, false, 0.0, false), t);
        let owed = clock.owed;
        assert!((owed - (60.0 - 2.0 * clock.vsync)).abs() < 1.0, "{owed}");
        assert_eq!(clock.catch_up(62.0), None);
        // An animation started 2 ms later: the driver's first step is one vsync on from
        // there on the wall clock, so what runs gets what the driver was behind with it.
        let since = clock.stamp(62.0);
        assert_eq!(since, t + 2.0);
        let first = clock.advance(62.0, true, since, false);
        assert_eq!(first, since + owed + clock.vsync.trunc());
        assert_eq!(clock.owed, 0.0);
    }

    /// On wall time the clock reads whole ms, as QSGDefaultAnimationDriver::elapsed() does:
    /// steps that follow each other at once (ticks missed while a turn ran) move nothing
    /// until a ms has passed, where a fraction of a ms each was a step of every animation.
    #[test]
    fn steps_at_once_on_wall_time_move_nothing_within_a_ms() {
        let mut clock = AnimClock {
            fixed: None,
            ..AnimClock::new()
        };
        clock.advance(0.0, true, 0.0, false);
        let (mut wall, mut t) = (0.0, 0.0);
        for _ in 0..3 {
            wall += 70.0;
            t = clock.advance(wall, true, 0.0, false);
        }
        assert!(clock.timer_mode);
        for (after, moved) in [(0.1, 0.0), (0.2, 0.0), (1.3, 1.0), (1.9, 1.0), (2.0, 2.0)] {
            clock.frame_due = true;
            assert_eq!(
                clock.advance(wall + after, true, 0.0, false),
                t + moved,
                "a step {after} ms after the late one"
            );
        }
    }

    #[test]
    fn late_frames_put_the_clock_on_wall_time_and_it_stands_between_steps() {
        let mut clock = AnimClock {
            fixed: None,
            ..AnimClock::new()
        };
        let mut t = clock.advance(0.0, true, 0.0, false);
        let mut wall = 0.0;
        for late in 1..=3 {
            assert!(!clock.timer_mode, "before late frame {late}");
            wall += 70.0;
            let next = clock.advance(wall, true, 0.0, false);
            assert!(
                (next - t - clock.vsync).abs() < 1.0,
                "late frame {late} is a step of one vsync, not of {} ms",
                next - t
            );
            t = next;
        }
        assert!(clock.timer_mode);
        // A wakeup every ms: one in 13 is a step, of 13 ms.
        let (mut stepped, mut steps) = (wall, 0);
        while clock.timer_mode {
            wall += 1.0;
            let due = clock.step_due(wall, false);
            assert_eq!(
                clock.stamp(wall),
                t,
                "an event {} ms after a step",
                wall - stepped
            );
            let next = clock.advance(wall, true, 0.0, false);
            if due {
                assert_eq!(next - t, wall - stepped);
                (stepped, steps) = (wall, steps + 1);
            } else {
                assert_eq!(next, t, "a wakeup {} ms after a step", wall - stepped);
            }
            t = next;
            assert!(steps <= 11, "{steps} steps on time and still on wall time");
        }
        assert_eq!(steps, 11);
        // By the vsync again.
        let mut steps = 0;
        for _ in 0..1000 {
            wall += 1.0;
            assert_eq!(clock.stamp(wall), t);
            let next = clock.advance(wall, true, 0.0, false);
            if next != t {
                assert!(
                    (next - t - clock.vsync).abs() < 1.0,
                    "a step of {}",
                    next - t
                );
                steps += 1;
            }
            t = next;
        }
        assert!(!clock.timer_mode);
        assert!((59..=61).contains(&steps), "{steps} steps in a second");
    }

    use super::*;

    /// Remote `qs ipc listen|wait` listeners end with their client or their handler: a gone
    /// client drops its listener and tap without waiting for the next emission, a destroyed
    /// handler closes the stream, and a client gone before its request was read never registers.
    #[cfg(feature = "interp")]
    #[test]
    fn ipc_listeners_end_with_their_client_or_handler() {
        use shell::ipc::CallResult;
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        let mut rt = qml::runtime::Runtime::new(None);
        rt.load_str(
            r#"import QtQuick
import Rustshell.Io
Item {
    Loader { active: true; sourceComponent: Component { IpcHandler { target: "inner"; signal ping() } } }
    IpcHandler { target: "outer"; signal ping() }
}"#,
        )
        .unwrap();
        rt.settle();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ipc.sock");
        let server = shell::ipc::Server::bind_unregistered(&path).unwrap();
        let mut listeners = Vec::new();
        let request = |target: &str| {
            let mut b = vec![4];
            shell::ipc::put_str(&mut b, target);
            shell::ipc::put_str(&mut b, "ping");
            b
        };
        let serve = |rt: &mut qml::runtime::Runtime, listeners: &mut Vec<IpcListener>| {
            let mut served = Vec::new();
            for req in server.requests() {
                let r = ipc_call(rt, &req.call, listeners);
                served.push(matches!(r, CallResult::Listening));
                server.reply(req, r);
            }
            ipc_forward(rt, listeners);
            served
        };
        // Sleeps until the server has something to do, as the main loop does.
        let wait = || {
            use std::os::fd::AsRawFd;
            let mut p = libc::pollfd {
                fd: server.fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid pollfd, no timeout.
            while unsafe { libc::poll(&mut p, 1, -1) } < 0 {
                let e = std::io::Error::last_os_error();
                assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "poll: {e}");
            }
        };
        let settle =
            |rt: &mut qml::runtime::Runtime, listeners: &mut Vec<IpcListener>, n: usize| {
                let mut served = Vec::new();
                while served.len() < n {
                    wait();
                    served.extend(serve(rt, listeners));
                }
                served
            };
        let taps = |rt: &qml::runtime::Runtime| rt.world.borrow().ipc_taps.len();

        let mut late = UnixStream::connect(&path).unwrap();
        late.write_all(&request("outer")).unwrap();
        drop(late);
        // Served until the server is done with the connection: accepted, its request read
        // and the connection closed.
        loop {
            wait();
            assert_eq!(serve(&mut rt, &mut listeners), [false; 0]);
            if server.connections() == 0 {
                break;
            }
        }
        assert_eq!(
            (listeners.len(), taps(&rt)),
            (0, 0),
            "late listen registered"
        );

        let mut outer = UnixStream::connect(&path).unwrap();
        outer.write_all(&request("outer")).unwrap();
        let mut inner = UnixStream::connect(&path).unwrap();
        inner.write_all(&request("inner")).unwrap();
        assert_eq!(settle(&mut rt, &mut listeners, 2), [true, true]);
        assert_eq!((listeners.len(), taps(&rt)), (2, 2));

        drop(outer);
        // The hangup wakes the server; a listener kept past it is never woken for again.
        while listeners.len() > 1 {
            wait();
            serve(&mut rt, &mut listeners);
        }
        assert_eq!((listeners.len(), taps(&rt)), (1, 1));

        let loader = rt.world.borrow_mut().of_type("Loader")[0];
        rt.set(loader, "active", Value::Bool(false));
        rt.settle();
        ipc_forward(&rt, &mut listeners);
        assert_eq!(
            (listeners.len(), taps(&rt)),
            (0, 0),
            "destroyed handler kept its listener"
        );
        assert_eq!(
            inner.read(&mut [0u8; 1]).unwrap(),
            0,
            "destroyed handler left its stream open"
        );
    }

    /// Held app windows open once every exclusive-zone surface is mapped (its first buffer
    /// committed and the compositor caught up), with no frame presented and no deadline: an
    /// output off by DPMS at login sends no frame callback but maps its bar.
    #[test]
    fn app_windows_open_once_exclusive_zones_are_mapped() {
        assert!(zones_mapped([]));
        assert!(zones_mapped([(0, false), (-1, false)]));
        assert!(!zones_mapped([(40, false), (0, true)]));
        assert!(zones_mapped([(40, true), (0, false)]));
    }

    /// After a reload, while the previous image's windows are still on the screen, a panel
    /// that reserves a zone is put over the old one instead of reserving a second zone; the
    /// zone is sent as it is once they are gone, and other panels are sent as they are.
    #[test]
    fn a_zone_is_not_reserved_twice_while_the_previous_image_is_on_the_screen() {
        let geom = |zone| shell::layer::Geometry {
            anchor: 1 | 4 | 8,
            size: (0, 40),
            margins: (0, 0, 0, 0),
            zone,
            edge: 0,
        };
        assert_eq!(sent_geometry(&geom(40), true), geom(-1));
        assert_eq!(sent_geometry(&geom(40), false), geom(40));
        for zone in [0, -1] {
            assert_eq!(sent_geometry(&geom(zone), true), geom(zone));
        }
    }

    fn write_jpeg(path: &std::path::Path, w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 3]) {
        let mut px = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&f(x, y));
            }
        }
        let img = turbojpeg::Image {
            pixels: &px[..],
            width: w,
            pitch: 3 * w,
            height: h,
            format: turbojpeg::PixelFormat::RGB,
        };
        let jpg = turbojpeg::compress(img, 95, turbojpeg::Subsamp::None).unwrap();
        std::fs::write(path, &*jpg).unwrap();
    }

    /// ColorQuantizer never decodes a big wallpaper at full size: a 4000x3000 JPEG for a
    /// `rescaleSize: 10` quantizer decodes at 1/8, and the mean colour stays the one the full
    /// decode gives.
    #[test]
    fn color_quantizer_decodes_big_jpeg_downscaled() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("wall.jpg");
        write_jpeg(&p, 4000, 3000, |x, y| {
            [(x * 255 / 3999) as u8, (y * 255 / 2999) as u8, 90]
        });
        let path = p.to_str().unwrap();
        let (img, _, size) = qsg::image::load_scaled(path, 10.0).unwrap();
        assert_eq!(size, (4000, 3000));
        assert_eq!((img.w, img.h), (500, 375));
        let (full, alpha) = qsg::image::load_ex(path).unwrap();
        let full = qsg::image::smooth_scale(&full, 10, 8, alpha);
        let n = full.px.len() as f64;
        let mut want = [0.0f64; 3];
        for p in &full.px {
            for c in 0..3 {
                want[c] += p[c] as f64 / n;
            }
        }
        let got = mean_color(path, 10.0).unwrap();
        for (c, want) in want.iter().enumerate() {
            let g = got.c[c] as f64 / 257.0;
            assert!((g - want).abs() < 1.5, "channel {c}: {g} vs {want}");
        }
    }

    /// The quantizer works off the GUI thread and only its latest request for an item lands: a
    /// result for a `source` that changed meanwhile is dropped instead of flashing the old colour.
    #[test]
    fn color_quantizer_applies_only_latest_request() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.jpg"), dir.path().join("b.jpg"));
        write_jpeg(&a, 64, 64, |_, _| [250, 0, 0]);
        write_jpeg(&b, 64, 64, |_, _| [0, 0, 250]);
        let waker = qml::svc::Waker::default();
        let h = waker.handle(qml::svc::Svc::Host);
        let mut q = Quantizer::default();
        q.request(7, &format!("file://{}", a.display()), 1.0, &h);
        q.request(7, &format!("file://{}", b.display()), 1.0, &h);
        q.request(7, &format!("file://{}", b.display()), 1.0, &h);
        // Every request ran and reported once the pool is idle; their reports woke the host.
        workpool::wait_idle();
        assert!(
            waker.peek().has(qml::svc::Svc::Host),
            "no wakeup for the results"
        );
        let got = q.ready();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, 7);
        assert!(
            got[0].1.c[2] > 60000 && got[0].1.c[0] < 5000,
            "{:?}",
            got[0].1
        );
    }

    /// The frozen screen of a ScreencopyView is captured before its window's first frame: a
    /// capture taken after it sometimes froze the region selector's own dim layer into the
    /// image (double darkening, in Quickshell too).
    #[test]
    fn window_holds_its_frame_until_its_screen_capture_ends() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut caps = rustc_hash::FxHashMap::default();
        caps.insert(7, ScreenCap::Busy(rx, 3));
        assert!(holds_for_screen_cap(&caps, 3));
        assert!(!holds_for_screen_cap(&caps, 4));
        // The capture is still in flight (its sender alive, nothing sent): only its answer
        // ends the hold, never a pass of the loop.
        assert!(poll_screen_caps(&mut caps).is_empty());
        assert!(
            holds_for_screen_cap(&caps, 3),
            "a slow capture let the window draw live content before its frozen frame"
        );
        tx.send(None).unwrap();
        poll_screen_caps(&mut caps);
        assert!(
            !holds_for_screen_cap(&caps, 3),
            "a failed capture kept its window held"
        );
        let (tx, rx) = std::sync::mpsc::channel::<Option<qsg::raster::Canvas>>();
        caps.insert(7, ScreenCap::Busy(rx, 3));
        drop(tx);
        poll_screen_caps(&mut caps);
        assert!(
            !holds_for_screen_cap(&caps, 3),
            "a capture whose worker died kept its window held"
        );
    }

    /// A screen capture that cannot complete (no screencopy here) ends on its own, failed, so
    /// the window it holds draws at once.
    #[test]
    fn a_refused_capture_thread_ends_the_capture() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_capture_thread_ends_the_capture"
        );
        if !testenv::in_own_process(TEST) {
            testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        // The limit on threads is reached: no panic, and the receiver is disconnected, which
        // `poll_screen_caps` takes for a capture that ended without a frame. The refusal is
        // in the log the shell writes by default (in a process of its own: the test reads
        // what that process wrote to its stderr).
        qml::debug::init_log();
        let waker = qml::svc::Waker::default();
        testenv::refuse_new_threads();
        let (rx, log) = testenv::stderr_with(|| {
            capture_screen(
                "NO-SUCH-OUTPUT".into(),
                false,
                waker.handle(qml::svc::Svc::Host),
            )
        });
        assert!(
            log.contains("screencopy: cannot start a thread:"),
            "stderr: {log:?}"
        );
        assert!(matches!(
            rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
    }

    /// A compositor that takes the screencopy request and never answers it: the capture ends
    /// on its own, failed, and the window it holds draws (in Quickshell a window never waits
    /// for its ScreencopyView). The worker used to wait for the answer without a limit, and
    /// the window (the region selector) stayed without a frame for good.
    #[test]
    fn a_capture_the_compositor_never_answers_releases_its_window() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_capture_the_compositor_never_answers_releases_its_window"
        );
        if !testenv::in_own_process(TEST) {
            // A Wayland socket nobody serves: a client connects and writes its requests,
            // and no event ever comes back.
            let dir = tempfile::tempdir().unwrap();
            let sock = dir.path().join("wayland-mute");
            let _mute = std::os::unix::net::UnixListener::bind(&sock).unwrap();
            testenv::run_in_own_process_env(
                TEST,
                [("WAYLAND_DISPLAY", sock.as_os_str())],
                &["WAYLAND_SOCKET"],
            );
            return;
        }
        let waker = qml::svc::Waker::default();
        // The loop's wakeup: the capture pings it when it ends.
        let (ping, woken) = std::sync::mpsc::channel();
        waker.set_ping(move || {
            let _ = ping.send(());
        });
        let rx = capture_screen("DP-1".into(), false, waker.handle(qml::svc::Svc::Host));
        let mut caps = rustc_hash::FxHashMap::default();
        caps.insert(7, ScreenCap::Busy(rx, 3));
        assert!(holds_for_screen_cap(&caps, 3));
        // A capture the compositor never answers ends at its deadline: nothing else pings.
        woken.recv().expect("the capture ended without a ping");
        assert!(
            poll_screen_caps(&mut caps).is_empty(),
            "no frame was copied"
        );
        assert!(
            !holds_for_screen_cap(&caps, 3),
            "a capture the compositor never answered kept its window held"
        );
    }

    /// The descriptor of the `Process` pipes and pidfds cannot be duplicated for the main loop
    /// (here it is not open; at the limit on descriptors it is `EMFILE`): the start is refused
    /// with the reason, instead of running a shell that hears of a child's output and exit
    /// only when something else wakes it.
    #[test]
    fn an_unwatchable_process_fd_refuses_the_start() {
        let ev: calloop::EventLoop<App> = calloop::EventLoop::try_new().unwrap();
        let Err(Fatal(msg)) = watch_process_fd(&ev.handle(), -1) else {
            panic!("the main loop started without watching the child processes");
        };
        let ebadf = std::io::Error::from_raw_os_error(libc::EBADF);
        assert_eq!(
            msg,
            Some(format!(
                "impulse-shell: cannot watch child processes: {ebadf}"
            ))
        );
        // SAFETY: epoll_create1(2) takes only flags.
        let ep = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        assert!(ep >= 0);
        assert!(watch_process_fd(&ev.handle(), ep).is_ok());
        // SAFETY: close(2) of the descriptor made above; the loop watches its own copy.
        unsafe { libc::close(ep) };
    }

    /// The system refuses the screencopy worker's thread (the limit on threads is reached):
    /// `ScWorker::start` reports it in the log instead of panicking, and the frame nobody
    /// converts is given up: its buffer is free again and its capture may request the next
    /// frame. In a process of its own: the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_screencopy_worker_gives_the_frame_up() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_screencopy_worker_gives_the_frame_up"
        );
        if !testenv::in_own_process(TEST) {
            testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let waker = qml::svc::Waker::default();
        testenv::refuse_new_threads();
        let (sc, log) = testenv::stderr_with(|| ScWorker::start(waker.handle(qml::svc::Svc::Host)));
        assert!(sc.is_none());
        assert!(
            log.contains("screencopy: cannot start its thread:"),
            "stderr: {log:?}"
        );
        let busy = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let px = [0u8; 4];
        let frame = ScMsg::Frame(ScJob {
            item: 7,
            handle: None,
            w: 1,
            h: 1,
            opaque: true,
            has_tex: false,
            src: ShmView {
                ptr: px.as_ptr(),
                len: px.len(),
                stride: 4,
                inv: false,
                busy: busy.clone(),
            },
            dst: None,
        });
        let mut caps = rustc_hash::FxHashMap::default();
        caps.insert(
            7,
            Capture {
                landing: true,
                ..Default::default()
            },
        );
        sc_drop(&mut caps, frame);
        assert!(!busy.load(std::sync::atomic::Ordering::Acquire));
        assert!(!caps[&7].landing);
    }

    use wayland_client::globals::GlobalListContents;
    use wayland_client::protocol::wl_registry::WlRegistry;

    struct OnlyShm(Shm);

    impl ShmHandler for OnlyShm {
        fn shm_state(&mut self) -> &mut Shm {
            &mut self.0
        }
    }
    delegate_dispatch2!(OnlyShm);

    impl Dispatch<WlRegistry, GlobalListContents> for OnlyShm {
        fn event(
            _: &mut Self,
            _: &WlRegistry,
            _: wayland_client::protocol::wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    /// The soft limit on open descriptors, set to `n`; the previous limit to restore.
    fn limit_descriptors(n: libc::rlim_t) -> libc::rlimit {
        let mut old = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: getrlimit(2) fills `old`.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut old) }, 0);
        let new = libc::rlimit {
            rlim_cur: n,
            rlim_max: old.rlim_max,
        };
        // SAFETY: setrlimit(2) reads a valid rlimit.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &new) }, 0);
        old
    }

    /// The system refuses the memory of a window frame (here the descriptor of its memfd, at
    /// the limit on open files): the capture gets no pool, and nothing panics. With the limit
    /// lifted the same capture gets its pool.
    #[test]
    fn a_refused_frame_pool_is_reported_not_a_panic() {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            testenv::unavailable("a Wayland compositor (WAYLAND_DISPLAY)");
            return;
        }
        let conn = Connection::connect_to_env().unwrap();
        let (globals, queue) = registry_queue_init::<OnlyShm>(&conn).unwrap();
        let shm = Shm::bind(&globals, &queue.handle()).unwrap();
        let mut pool = None;
        let old = limit_descriptors(0);
        let got = frame_pool(&mut pool, 64 * 64 * 4, &shm).is_some();
        // SAFETY: setrlimit(2) reads the limit `limit_descriptors` returned.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &old) }, 0);
        assert!(!got && pool.is_none(), "a pool without a descriptor");
        assert!(frame_pool(&mut pool, 64 * 64 * 4, &shm).is_some());
        assert!(pool.is_some());
    }

    /// Anonymous memory the test maps and unmaps itself, as a capture's `SlotPool` does.
    struct Mapping(*mut libc::c_void, usize);

    impl Mapping {
        fn new(len: usize) -> Self {
            // SAFETY: mmap(2) of new anonymous memory; no existing mapping is touched.
            let p = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            assert_ne!(p, libc::MAP_FAILED, "{}", std::io::Error::last_os_error());
            Mapping(p, len)
        }
    }

    impl Drop for Mapping {
        fn drop(&mut self) {
            // SAFETY: the mapping `new` made, unmapped once.
            unsafe { libc::munmap(self.0, self.1) };
        }
    }

    /// The owner of the frame memory drops the worker and then frees the memory (the shell's
    /// exit: `App::drop`, then the captures' pools). The worker reads that memory through a
    /// raw pointer, so when `drop` returns its thread has ended: no job is left to read it.
    #[test]
    fn dropping_the_screencopy_worker_waits_for_its_thread() {
        // A frame large enough that the worker is still on it when the sender has moved on.
        let (w, h) = (4096, 4096);
        let frame = Mapping::new(w * h * 4);
        let waker = qml::svc::Waker::default();
        let sc = ScWorker::start(waker.handle(qml::svc::Svc::Host)).unwrap();
        let busy = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let job = ScMsg::Frame(ScJob {
            item: 1,
            handle: None,
            w,
            h,
            opaque: true,
            has_tex: false,
            src: ShmView {
                ptr: frame.0 as *const u8,
                len: frame.1,
                stride: w * 4,
                inv: false,
                busy: busy.clone(),
            },
            dst: None,
        });
        assert!(sc.send(job).is_ok());
        drop(sc);
        // The job holds the only other reference to `busy`: it is gone with the thread.
        assert_eq!(
            std::sync::Arc::strong_count(&busy),
            1,
            "the worker still holds the frame after it was dropped"
        );
        drop(frame);
    }

    #[test]
    fn capture_without_screencopy_ends_failed() {
        let waker = qml::svc::Waker::default();
        let rx = capture_screen(
            "NO-SUCH-OUTPUT".into(),
            false,
            waker.handle(qml::svc::Svc::Host),
        );
        assert!(matches!(rx.recv(), Ok(None)));
    }

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn floating_window_is_a_toplevel() {
        let mut rt = qml::runtime::Runtime::new(None);
        rt.load_str(
            "import QtQuick\nimport Quickshell\nScope {\n\
             FloatingWindow { objectName: \"float\" }\n\
             PanelWindow { objectName: \"panel\" }\n}",
        )
        .unwrap();
        rt.settle();
        let mut w = rt.world.borrow_mut();
        let float = w.of_type("FloatingWindow");
        let panel = w.of_type("PanelWindow");
        assert_eq!((float.len(), panel.len()), (1, 1));
        assert!(is_app_window(&w, float[0]));
        assert!(!is_app_window(&w, panel[0]));
        drop(w);
        for p in ["width", "height", "implicitWidth", "implicitHeight"] {
            assert_eq!(rt.get(float[0], p).to_number(), 100.0, "{p}");
        }
    }

    #[test]
    fn unanswered_export_frames_wait_for_an_event() {
        let mut u = Unanswered(Vec::new());
        assert_eq!(u.retire(1, true), Some(1));
        // Capture of a window closed meanwhile: no request until Hyprland answers it.
        assert_eq!(u.retire(2, false), None);
        assert_eq!(u.retire(3, false), None);
        assert_eq!(u.answered(&1), None);
        assert_eq!(u.answered(&3), Some(3));
        assert_eq!(u.answered(&3), None);
        assert_eq!(u.0, [2]);
    }

    /// A window whose screen is not connected waits for it instead of landing on the monitor
    /// the compositor picks (the focused one); only a window without a screen lets it pick.
    #[test]
    fn panel_for_a_missing_screen_is_not_created() {
        let outs = || {
            vec![
                (1, Some("DP-1".to_string())),
                (2, None),
                (3, Some("HDMI-A-1".into())),
            ]
        };
        assert_eq!(
            panel_wl_output(Some("HDMI-A-1"), outs().into_iter()),
            Some(Some(3))
        );
        assert_eq!(panel_wl_output(None, outs().into_iter()), Some(None));
        assert_eq!(panel_wl_output(Some("DP-2"), outs().into_iter()), None);
        assert_eq!(
            panel_wl_output::<i32>(Some("DP-1"), std::iter::empty()),
            None
        );
    }

    /// A screencopy buffer whose rows would not fit its stride, or whose pool would not fit an
    /// i32, is refused instead of panicking the worker (which would leave the capture busy).
    #[test]
    fn screencopy_buffer_geometry_is_checked() {
        assert!(shm_geometry_ok(1920, 1080, 7680));
        assert!(shm_geometry_ok(1919, 1080, 7680));
        assert!(!shm_geometry_ok(1920, 1080, 7676));
        assert!(!shm_geometry_ok(0, 1080, 7680));
        assert!(!shm_geometry_ok(1920, 0, 7680));
        assert!(!shm_geometry_ok(40_000, 40_000, 160_000));
        assert!(!shm_geometry_ok(u32::MAX, 1, u32::MAX));
    }
}
