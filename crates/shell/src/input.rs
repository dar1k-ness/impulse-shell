//! Pointer delivery for native windows, after QQuickDeliveryAgent and QQuickMouseArea:
//! presses/wheel go to the topmost accepting MouseArea (or Control), hover to the topmost
//! hover-enabled item and its ancestors.
use qml::runtime::{Runtime, World, is_button};
use qml::value::Value;

use crate::scene::{explicit, is, is_text, kids, num, truthy};

#[cfg(all(test, feature = "interp"))]
#[path = "input_tests.rs"]
mod pointer_tests;

/// QQuickDeliveryAgentPrivate::frameSynchronousHoverInterval: 100 ms unless
/// QT_QUICK_FRAME_SYNCHRONOUS_HOVER_INTERVAL.
fn frame_hover_interval() -> i64 {
    static INTERVAL: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *INTERVAL.get_or_init(|| {
        std::env::var("QT_QUICK_FRAME_SYNCHRONOUS_HOVER_INTERVAL")
            .ok()
            .and_then(|v| v.trim().parse::<i32>().ok())
            .map_or(100, i64::from)
    })
}

/// linux/input-event-codes.h BTN_LEFT.
const BTN_LEFT: u32 = 0x110;

#[derive(Default)]
pub struct Pointer {
    win: Option<usize>,
    pos: (f64, f64),
    hovered: Vec<usize>,
    under: Vec<usize>,
    grab: Option<(usize, u32)>,
    buttons: u32,
    rev: u64,
    /// Hover is due again at the last mouse position: `rehover` delivers it.
    framed: bool,
    /// QQuickDeliveryAgentPrivate::frameSynchronousHoverInterval in ms when it is not Qt's
    /// (`frame_hover_interval`).
    pub hover_every: Option<i64>,
    /// Each window's frameSynchronousHoverTimer: when hover was last delivered again for it.
    /// Qt starts it with the window; here the window's first frame does.
    hover_timers: std::collections::HashMap<usize, std::time::Instant>,
    /// frameSynchronousDelayTimer: the window whose frame came before its interval was over,
    /// and when that is.
    hover_delay: Option<(usize, std::time::Instant)>,
    /// Hit tests `rehover` made.
    pub hit_tests: u64,
    /// MouseArea press-and-hold timer start (QQuickMouseArea::pressAndHoldTimer).
    hold: Option<std::time::Instant>,
    /// pressAndHold fired and was accepted: the release emits no clicked().
    long_press: bool,
    /// MouseArea `drag.target` state of the current grab (QQuickMouseAreaPrivate drag fields).
    drag: Option<DragGrab>,
    /// Item with `Drag.active` and the DropArea its hot spot is over (QQuickDragAttached).
    drop: Option<(usize, usize)>,
    /// DragHandlers that took a passive grab of the press (one becomes the exclusive grabber).
    handlers: Vec<HandlerGrab>,
    /// Scene press point and button of `handlers`.
    press_pos: (f64, f64),
    handler_button: u32,
    /// wl_pointer time of the event being handled.
    pub time: f64,
    /// QMutableEventPoint velocity (Kalman-filtered) and the (time, x, y) it was last taken at.
    vel: (f64, f64),
    vel_last: Option<(f64, f64, f64)>,
    /// Flickable that filtered the press (QQuickFlickable::filterPointerEvent), and whether it
    /// holds the grab.
    flick: Option<usize>,
    flick_grab: bool,
    /// QQuickScrollBarPrivate::offset of the pressed ScrollBar.
    bar_offset: f64,
    /// Previous press for double-click detection (QGuiApplicationPrivate::processMouseEvent:
    /// mousePressButton, lastPressTimestamp, lastPressWindow, press position).
    last_press: Option<LastPress>,
    /// The grabbing MouseArea took a doubleClicked() for this press: its release emits no
    /// clicked() (QQuickMouseAreaPrivate::doubleClick).
    double_click: bool,
    /// Active WheelHandlers and when their deactivation timer (activeTimeout) started.
    wheel_active: Vec<(usize, std::time::Instant)>,
    /// SwipeView whose ListView (a Flickable) filters the pressed point.
    swipe: Option<SwipeGrab>,
    /// SwipeView flick/fixup animations of the content position after a swipe.
    swipe_anims: Vec<SwipeAnim>,
    /// Font stacks for editor and link hit tests (the app's shared cache).
    pub fonts: std::rc::Rc<std::cell::RefCell<crate::scene::Fonts>>,
    /// QQuickText `activeLink` of a press on a link (the Text is the grabber).
    link: Option<String>,
    /// Texts whose `hoveredLink` is set.
    link_hover: Vec<(usize, String)>,
    /// Keyboard modifiers (Qt flags) of the pointer events.
    pub mods: u32,
    /// TextInput/TextEdit press, double-click and drag selection state.
    text: crate::textmouse::TextMouse,
    /// QQuickTextInput::mouseMoveEvent's setKeepMouseGrab(true) once the selecting drag passed
    /// startDragDistance horizontally.
    text_keep: bool,
    /// wl_touch id of the touch point that drives the pointer (the first one down).
    touch: Option<i32>,
}

struct SwipeGrab {
    view: usize,
    press: (f64, f64),
    /// Content position at the press.
    base: f64,
    /// A move crossed the drag threshold and the view took the grab (stealMouse).
    stolen: bool,
    /// Pointer travel along the view's axis at the move after that one, which the content
    /// follows from (dragStartOffset, set again by every move until the first one that drags).
    start: Option<f64>,
    /// Event time (ms) of the last move, for the release velocity.
    last_move: f64,
}

struct SwipeAnim {
    view: usize,
    from: f64,
    to: f64,
    /// World animation time (ms) it started at: QQuickTimeLine runs on the animation driver's
    /// clock, not wall time.
    start: f64,
    /// ms
    dur: f64,
    /// QQuickTimeLine::accel (initial speed px/ms, decelerating to rest at `to`) or a
    /// QQuickTimeLine::move with InOutQuad.
    decel: bool,
}

/// QPlatformTheme FlickStartDistance, MouseDoubleClick-independent Flickable constants.
const FLICK_START_DISTANCE: f64 = 15.0;
const MINIMUM_FLICK_VELOCITY: f64 = 75.0;
const FLICK_DECELERATION: f64 = 1500.0;
/// QML_FLICK_SNAPONETHRESHOLD
const SNAP_ONE_THRESHOLD: f64 = 30.0;

struct LastPress {
    /// Event time (ms) of the press: QGuiApplicationPrivate compares the events' timestamps.
    at: f64,
    pos: (f64, f64),
    button: u32,
    win: usize,
}

/// QStyleHints::mouseDoubleClickInterval / mouseDoubleClickDistance (QPlatformTheme defaults).
const DOUBLE_CLICK_MS: f64 = 400.0;
const DOUBLE_CLICK_DISTANCE: f64 = 5.0;

/// QQuickDragHandler state for the pressed point.
struct HandlerGrab {
    handler: usize,
    /// Press point in target coordinates (m_pressTargetPos).
    press_local: Option<(f64, f64)>,
    active: bool,
    /// activeTranslation.
    trans: (f64, f64),
}

struct DragGrab {
    target: usize,
    /// Scene press point; reset to the threshold-crossing point (drag.smoothed).
    start: (f64, f64),
    /// Target position at the first move.
    target_start: Option<(f64, f64)>,
    over: bool,
    active: bool,
}

struct Hit {
    item: usize,
    x: f64,
    y: f64,
}

fn collect(
    w: &mut World,
    i: usize,
    o: (f64, f64),
    clip: Option<[f64; 4]>,
    p: (f64, f64),
    out: &mut Vec<Hit>,
) {
    if !truthy(w, i, "visible", true) {
        return;
    }
    let (x, y) = (o.0 + num(w, i, "x"), o.1 + num(w, i, "y"));
    // QQuickItem::mapFromScene goes through the item's whole transform: under a `scale` or
    // `rotation` the item and its subtree are hit where they show. The clip rectangles of the
    // ancestors are in their coordinates; the point is inside them, or this item is not reached.
    let (p, clip) = match crate::scene::item_xf(w, i) {
        Some(xf) => match xf.undo((x, y), p) {
            Some(p) => (p, None),
            // A transform without an inverse (a zero scale): Qt's delivery maps the parent's
            // point with the identity instead, so the item is hit where its parent's point
            // lies within its size, wherever the item is.
            None => ((p.0 + x - o.0, p.1 + y - o.1), None),
        },
        None => (p, clip),
    };
    let (wd, ht) = (num(w, i, "width"), num(w, i, "height"));
    let within = |r: [f64; 4]| p.0 >= r[0] && p.1 >= r[1] && p.0 < r[2] && p.1 < r[3];
    let r = [x, y, x + wd, y + ht];
    if w.item_contains(i, (p.0 - x, p.1 - y)) && clip.is_none_or(within) {
        out.push(Hit {
            item: i,
            x: p.0 - x,
            y: p.1 - y,
        });
    }
    let clip = if truthy(w, i, "clip", false) {
        Some(clip.map_or(r, |c| {
            [
                c[0].max(r[0]),
                c[1].max(r[1]),
                c[2].min(r[2]),
                c[3].min(r[3]),
            ]
        }))
    } else {
        clip
    };
    // Nothing below can be hit outside the clip.
    if clip.is_some_and(|c| !within(c)) {
        return;
    }
    for c in crate::scene::stacked_kids(w, i) {
        // Popups live in QQuickOverlay; `hits` collects them above everything else.
        if crate::scene::is_popup(w, c) {
            continue;
        }
        if crate::scene::view_culls(w, i, c) {
            continue;
        }
        collect(w, c, (x, y), clip, p, out);
    }
}

/// Scene point `p` with the `scale` and `rotation` of `i` and its ancestors undone, outermost
/// first: the point that `World::scene_pos`, which adds up positions only, maps to the same
/// place in `i` as QQuickItem::mapFromScene maps `p`.
fn unscaled(w: &mut World, i: usize, win: Option<usize>, mut p: (f64, f64)) -> (f64, f64) {
    let mut chain = vec![i];
    while let Some(a) = w.items[*chain.last().unwrap()].parent
        && Some(a) != win
    {
        chain.push(a);
    }
    let scene = p;
    for &a in chain.iter().rev() {
        if let Some(xf) = crate::scene::item_xf(w, a) {
            let at = w.scene_pos(a, win);
            match xf.undo(at, p) {
                Some(q) => p = q,
                // QQuickItem::mapFromScene inverts the whole chain at once: with one transform
                // in it that has no inverse, the scene point is the item's point.
                None => {
                    let at = w.scene_pos(i, win);
                    return (scene.0 + at.0, scene.1 + at.1);
                }
            }
        }
    }
    p
}

/// Where `i` sits in the scene, its ancestors' positions added outermost first as
/// QQuickItemPrivate::itemToWindowTransform adds them: the sum differs from
/// `World::scene_pos`'s in its last bits.
fn scene_offset(w: &mut World, i: usize, win: Option<usize>) -> (f64, f64) {
    let mut chain = vec![];
    let mut cur = Some(i);
    while let Some(a) = cur
        && Some(a) != win
    {
        let (x, y, next) = w.parent_step(a);
        chain.push((x, y));
        cur = next;
    }
    chain
        .iter()
        .rev()
        .fold((0.0, 0.0), |at, d| (at.0 + d.0, at.1 + d.1))
}

/// QQuickItem::mapFromScene: scene point `p` in `i`'s coordinates.
fn in_item(w: &mut World, i: usize, win: Option<usize>, p: (f64, f64)) -> (f64, f64) {
    let p = unscaled(w, i, win, p);
    let at = w.scene_pos(i, win);
    (p.0 - at.0, p.1 - at.1)
}

/// QQuickItem::transformOriginPoint: the point of `i` its `scale` and `rotation` keep still.
fn origin_point(w: &mut World, i: usize) -> (f64, f64) {
    let to = w.read(i, "transformOrigin").map_or(4.0, |v| v.to_number()) as i32;
    let f = |k: i32| [0.0, 0.5, 1.0][k.clamp(0, 2) as usize];
    (
        num(w, i, "width") * f(to % 3),
        num(w, i, "height") * f(to / 3),
    )
}

/// Items under `p`, topmost first.
fn hits(w: &mut World, win: usize, p: (f64, f64)) -> Vec<Hit> {
    let mut out = Vec::new();
    for c in kids(w, win) {
        collect(w, c, (0.0, 0.0), None, p, &mut out);
    }
    for c in open_popups(w, win) {
        let Some(par) = w.items[c].parent else {
            continue;
        };
        let (ox, oy) = w.scene_pos(par, Some(win));
        let o = (ox, oy + crate::scene::popup_flip_dy(w, c, win));
        collect(w, c, o, None, p, &mut out);
    }
    out.reverse();
    out
}

/// Input reaches `i`: it and every item it is in are enabled. A MouseArea's `enabled` is the
/// area's own (qml::types::own_enabled): off, it stops that area and not the items in it.
fn enabled(w: &mut World, i: usize) -> bool {
    truthy(w, i, "enabled", true) && item_enabled(w, i)
}

/// QQuickItem::isEnabled of `i`: the item state, which a MouseArea's own `enabled` leaves on.
fn item_enabled(w: &mut World, mut i: usize) -> bool {
    if !qml::types::own_enabled(&w.items[i].types) && !truthy(w, i, "enabled", true) {
        return false;
    }
    while let Some(p) = w.items[i].parent {
        if !qml::types::own_enabled(&w.items[p].types) && !truthy(w, p, "enabled", true) {
            return false;
        }
        i = p;
    }
    true
}

fn mouse_area(w: &World, i: usize) -> bool {
    is(w, i, "MouseArea")
}

fn handlers_of(w: &World, i: usize, ty: &str) -> Vec<usize> {
    w.items[i]
        .children
        .iter()
        .copied()
        .filter(|&c| !w.dead.contains(&c) && is(w, c, ty))
        .collect()
}

fn drag_handlers(w: &World, i: usize) -> Vec<usize> {
    handlers_of(w, i, "DragHandler")
}

/// QQuickWheelHandler.activeTimeout in ms (default 0.1 s).
fn wheel_timeout(w: &mut World, h: usize) -> f64 {
    if explicit(w, h, "activeTimeout") {
        (num(w, h, "activeTimeout") * 1000.0).round()
    } else {
        100.0
    }
}

/// QQuickPointerHandler::target: the parent item unless set (`target: null` disables moving).
fn handler_target(w: &mut World, h: usize) -> Option<usize> {
    if !explicit(w, h, "target") {
        return w.items[h].parent;
    }
    match w.read(h, "target") {
        Some(Value::Item(t)) if !w.dead.contains(&t) => Some(t),
        _ => None,
    }
}

/// QQuickDragAxis `minimum`/`maximum` (default -DBL_MAX/DBL_MAX) and `enabled`.
fn drag_axis(w: &mut World, h: usize, axis: &str) -> (bool, f64, f64) {
    let lim = |w: &mut World, n: &str, d: f64| {
        let n = format!("{axis}.{n}");
        if explicit(w, h, &n) {
            w.read(h, &n).map_or(d, |v| v.to_number())
        } else {
            d
        }
    };
    let en = truthy(w, h, &format!("{axis}.enabled"), true);
    (en, lim(w, "minimum", f64::MIN), lim(w, "maximum", f64::MAX))
}

/// An item grabber's keepMouseGrab, which vetoes a pointer handler's takeover
/// (QQuickPointerHandler::approveGrabTransition).
fn keeps_grab(w: &mut World, g: usize, dragging: bool) -> bool {
    if mouse_area(w, g) {
        dragging || truthy(w, g, "preventStealing", false)
    } else {
        slider(w, g) || is(w, g, "ScrollBar")
    }
}

fn button(w: &World, i: usize) -> bool {
    is_button(&w.items[i].types)
}

/// Visible QtQuick.Controls `Popup`s inside window `win`, in creation order.
pub fn open_popups(w: &mut World, win: usize) -> Vec<usize> {
    let mut ps: Vec<usize> = ["Popup", "Menu", "Dialog", "Drawer"]
        .iter()
        .flat_map(|t| w.of_type(t))
        .collect();
    ps.retain(|&i| crate::scene::is_popup(w, i));
    ps.sort_unstable();
    ps.dedup();
    ps.retain(|&i| truthy(w, i, "visible", false) && is_ancestor(w, win, i));
    ps
}

/// QQuickPopup::closePolicy; QQuickComboBox forces CloseOnEscape | CloseOnPressOutsideParent.
pub fn close_policy(w: &mut World, i: usize) -> u32 {
    if w.items[i]
        .props
        .get("closePolicy")
        .is_some_and(|p| p.binding.is_some() || !matches!(p.value, Value::Undefined))
    {
        return w
            .read(i, "closePolicy")
            .map(|v| v.to_number() as u32)
            .unwrap_or(17);
    }
    match w.items[i].parent {
        Some(p) if is(w, p, "ComboBox") => 16 | 2,
        _ => 1 | 16,
    }
}

pub fn close_popup(rt: &mut Runtime, i: usize) {
    rt.world.borrow_mut().popup_set_visible(i, false);
    rt.settle();
}

pub fn open_popup(rt: &mut Runtime, i: usize) {
    rt.world.borrow_mut().popup_set_visible(i, true);
    rt.settle();
}

/// Whether window point `pos` is inside popup `i`, and inside the item it is a popup of.
fn popup_contains(w: &mut World, i: usize, win: usize, pos: (f64, f64)) -> Option<(bool, bool)> {
    let par = w.items[i].parent?;
    let (px, py) = w.scene_pos(par, Some(win));
    let r = |w: &mut World, k: usize, n: &str| w.read(k, n).map(|v| v.to_number()).unwrap_or(0.0);
    let dy = crate::scene::popup_flip_dy(w, i, win);
    let (x, y) = (px + r(w, i, "x"), py + r(w, i, "y") + dy);
    let (pw, ph) = (r(w, i, "width"), r(w, i, "height"));
    let (qw, qh) = (r(w, par, "width"), r(w, par, "height"));
    let inn = |x0: f64, y0: f64, ww: f64, hh: f64| {
        pos.0 >= x0 && pos.1 >= y0 && pos.0 < x0 + ww && pos.1 < y0 + hh
    };
    Some((inn(x, y, pw, ph), inn(px, py, qw, qh)))
}

/// QQuickPopupPrivate::tryClose on a press at window point `pos`.
fn close_popups_outside(rt: &mut Runtime, win: usize, pos: (f64, f64)) {
    let pops = open_popups(&mut rt.world.borrow_mut(), win);
    for i in pops.into_iter().rev() {
        let (pol, inside_pop, inside_parent) = {
            let mut w = rt.world.borrow_mut();
            let pol = close_policy(&mut w, i);
            let Some((pop, parent)) = popup_contains(&mut w, i, win, pos) else {
                continue;
            };
            (pol, pop, parent)
        };
        if inside_pop {
            continue;
        }
        if pol & 1 != 0 || (pol & 2 != 0 && !inside_parent) {
            close_popup(rt, i);
        }
    }
}

/// QQuickPopup::mousePressEvent accepts a press inside the popup, also where nothing in it
/// takes the press (its padding): the items under the popup are dropped from `hs`.
fn popup_blocks_press(w: &mut World, win: usize, pos: (f64, f64), hs: &mut Vec<Hit>) {
    let pops = open_popups(w, win);
    let Some(k) = pops
        .iter()
        .rposition(|&i| popup_contains(w, i, win, pos).is_some_and(|(pop, _)| pop))
    else {
        return;
    };
    let pop = pops[k];
    hs.retain(|h| h.item == pop || is_ancestor(w, pop, h.item));
}

fn slider(w: &World, i: usize) -> bool {
    is(w, i, "Slider")
}

fn scrollbar(w: &mut World, i: usize) -> bool {
    is(w, i, "ScrollBar") && (!explicit(w, i, "interactive") || truthy(w, i, "interactive", true))
}

/// The padded track of a ScrollBar along its orientation: where it starts, how long it is,
/// and how thick the bar is.
fn scrollbar_track(w: &mut World, b: usize) -> (f64, f64, f64) {
    let (near, far, long, thick) = if num(w, b, "orientation") == 1.0 {
        ("left", "right", "width", "height")
    } else {
        ("top", "bottom", "height", "width")
    };
    let t = crate::scene::pad_side(w, b, near);
    let len = num(w, b, long) - t - crate::scene::pad_side(w, b, far);
    (t, len, num(w, b, thick))
}

/// QQuickScrollBarPrivate::positionAt: `at` is the pointer along the bar's orientation.
fn scrollbar_at(w: &mut World, b: usize, at: (f64, f64)) -> f64 {
    let at = if num(w, b, "orientation") == 1.0 {
        at.0
    } else {
        at.1
    };
    let (t, len, _) = scrollbar_track(w, b);
    if len > 0.0 { (at - t) / len } else { 0.0 }
}

/// Whether the pointer is over the padded track of the bar.
fn scrollbar_on_track(w: &mut World, b: usize, at: (f64, f64)) -> bool {
    let at = if num(w, b, "orientation") == 1.0 {
        at.0
    } else {
        at.1
    };
    let (t, len, _) = scrollbar_track(w, b);
    at >= t && at < t + len
}

/// QQuickScrollBarPrivate::handlePress: where in the handle the drag holds it.
fn scrollbar_offset(w: &mut World, b: usize, at: (f64, f64)) -> f64 {
    let (pos, size) = (num(w, b, "position"), num(w, b, "size"));
    let (long, thick) = if num(w, b, "orientation") == 1.0 {
        (num(w, b, "width"), num(w, b, "height"))
    } else {
        (num(w, b, "height"), num(w, b, "width"))
    };
    let min = if explicit(w, b, "minimumSize") {
        num(w, b, "minimumSize")
    } else if long > 0.0 {
        thick / long
    } else {
        0.0
    };
    let sz = size.max(min);
    let off = scrollbar_at(w, b, at) - pos;
    if !(0.0..=sz).contains(&off) {
        sz / 2.0
    } else {
        off
    }
}

/// QQuickScrollBarPrivate::setPositionFromDrag; the attached Flickable follows
/// (QQuickScrollBarAttachedPrivate::scrollVertical/scrollHorizontal, a plain setContentY or
/// setContentX).
fn scrollbar_drag(rt: &mut Runtime, b: usize, at: (f64, f64), offset: f64) {
    let moved = {
        let mut w = rt.world.borrow_mut();
        let size = num(&mut w, b, "size");
        let pos = (scrollbar_at(&mut w, b, at) - offset)
            .min(1.0 - size)
            .max(0.0);
        let moved = num(&mut w, b, "position") != pos;
        w.write(b, "position", Value::Num(pos));
        if let Some((f, vertical)) = crate::scene::scrollbar_flickable(&mut w, b) {
            let (at, origin, content, view) = if vertical {
                ("contentY", "originY", "contentHeight", "height")
            } else {
                ("contentX", "originX", "contentWidth", "width")
            };
            let origin = if w.items[f].prop_info(origin).is_some() {
                num(&mut w, f, origin)
            } else {
                0.0
            };
            let bounds = num(&mut w, f, content).max(num(&mut w, f, view));
            let from = num(&mut w, f, at);
            let was = std::mem::replace(&mut w.animating, true);
            w.write(f, at, Value::Num(pos * bounds + origin));
            w.animating = was;
            // The visible area of the Flickable moved (to a whole pixel when it is
            // `pixelAligned`): its bar goes where the content is. It did not: the bar stays
            // where the pointer put it.
            if num(&mut w, f, at) != from {
                crate::scene::flick_flags(&mut w, f);
            }
        }
        moved
    };
    if moved {
        rt.emit(b, "moved", &[], Vec::new());
    }
    rt.settle();
}

/// QQuickSliderPrivate::handleMove, and handleRelease when `release`: the value is the
/// stepped one under the pointer, the position (the handle) is the pointer's own unless the
/// slider snaps: always, or on release.
fn slider_drag(rt: &mut Runtime, i: usize, x: f64, release: bool) {
    {
        let mut w = rt.world.borrow_mut();
        let (f, t) = (num(&mut w, i, "from"), num(&mut w, i, "to"));
        let (lp, rp) = (
            num(&mut w, i, "leftPadding"),
            num(&mut w, i, "rightPadding"),
        );
        let hw = match w.read(i, "handle") {
            Some(Value::Item(h)) => num(&mut w, h, "width"),
            _ => 0.0,
        };
        let span = num(&mut w, i, "width") - lp - rp - hw;
        let mut p = if span > 0.0 {
            ((x - lp - hw / 2.0) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let step = num(&mut w, i, "stepSize");
        // Slider.NoSnap 0, SnapAlways 1, SnapOnRelease 2.
        let snap = num(&mut w, i, "snapMode");
        let snaps = snap == 1.0 || (release && snap == 2.0);
        if snaps && step > 0.0 && t != f {
            let s = step / (t - f);
            p = (p / s).round() * s;
        }
        let old = crate::scene::slider_position(&mut w, i);
        let mut v = f + p * (t - f);
        if step > 0.0 {
            v = f + ((v - f) / step).round() * step;
        }
        // `live` is true unless written.
        let live = !explicit(&w, i, "live") || truthy(&mut w, i, "live", true);
        let changed = (live || release) && !crate::scene::fuzzy_eq(num(&mut w, i, "value"), v);
        if changed {
            // QQuickSlider::setValue from handleMove is a C++ write: `Behavior on value` does
            // not apply. It puts the position on the value.
            w.set_internal(i, "value", Value::Num(v));
            crate::scene::set_slider_position(&mut w, i, None);
        }
        if if release {
            snaps && !changed
        } else {
            !live || !snaps
        } {
            crate::scene::set_slider_position(&mut w, i, Some(p));
        }
        if p == old {
            return;
        }
    }
    rt.emit(i, "moved", &[], Vec::new());
}

/// The pressed-state key of the SpinBox indicator under local (x, y).
fn spin_part(w: &mut World, i: usize, x: f64, y: f64) -> Option<&'static str> {
    if !is(w, i, "SpinBox") {
        return None;
    }
    let (sw, sh) = (num(w, i, "width"), num(w, i, "height"));
    let mirrored = truthy(w, i, "mirrored", false);
    for (k, p, top) in [
        ("up.indicator", "up.pressed", true),
        ("down.indicator", "down.pressed", false),
    ] {
        let (ix, iy, iw, ih) = match w.read(i, k) {
            Some(Value::Item(ind)) if ind != i => (
                num(w, ind, "x"),
                num(w, ind, "y"),
                num(w, ind, "width"),
                num(w, ind, "height"),
            ),
            // Fusion (Qt 6's default on Linux): 16 wide at the trailing edge, up over down.
            None | Some(Value::Undefined) => {
                let h = sh / 2.0 - 1.0;
                let x = if mirrored { 1.0 } else { sw - 17.0 };
                (x, if top { 1.0 } else { sh - h - 1.0 }, 16.0, h)
            }
            Some(_) => continue,
        };
        if x >= ix && x < ix + iw && y >= iy && y < iy + ih {
            return Some(p);
        }
    }
    None
}

/// QQuickSpinBoxPrivate::handleRelease: a press released over its indicator steps the value.
fn spin_release(rt: &mut Runtime, i: usize, inside: bool) {
    for (k, sign) in [("up.pressed", 1.0), ("down.pressed", -1.0)] {
        if !truthy(&mut rt.world.borrow_mut(), i, k, false) {
            continue;
        }
        put(rt, i, k, Value::Bool(false));
        if inside {
            let changed = {
                let mut w = rt.world.borrow_mut();
                let (f, t) = (num(&mut w, i, "from"), num(&mut w, i, "to"));
                let (lo, hi) = (f.min(t), f.max(t));
                let v = num(&mut w, i, "value");
                let nv = (v + sign * num(&mut w, i, "stepSize")).clamp(lo, hi);
                // QQuickSpinBox::increase/decrease are C++ writes: the `value` binding stays.
                nv != v && {
                    w.set_internal(i, "value", Value::Num(nv));
                    true
                }
            };
            if changed {
                rt.emit(i, "valueModified", &[], Vec::new());
            }
        }
    }
    rt.settle();
}

/// QQuickSwitch/CheckBox/RadioButton are checkable by default.
pub(crate) fn checkable(w: &mut World, i: usize) -> bool {
    if explicit(w, i, "checkable") {
        truthy(w, i, "checkable", false)
    } else {
        ["Switch", "CheckBox", "RadioButton", "TabButton"]
            .iter()
            .any(|t| is(w, i, t))
    }
}

/// QQuickDragAttached::hotSpot of `item`: a QPointF, read as the value it is. Where QML bound
/// its fields one by one (`Drag.hotSpot.x: 5`) the engine keeps them as an object.
fn hot_spot(w: &mut World, item: usize) -> (f64, f64) {
    let n = |v: Option<Value>| v.map_or(0.0, |v| v.to_number());
    match w.read(item, "Drag.hotSpot") {
        Some(Value::Obj(o)) => {
            let o = o.borrow();
            (n(o.props.get("x").cloned()), n(o.props.get("y").cloned()))
        }
        Some(Value::Item(g)) => (n(w.read(g, "x")), n(w.read(g, "y"))),
        _ => (0.0, 0.0),
    }
}

pub(crate) fn is_ancestor(w: &World, a: usize, mut i: usize) -> bool {
    while let Some(p) = w.items[i].parent {
        if p == a {
            return true;
        }
        i = p;
    }
    false
}

fn accepts_hover(w: &mut World, i: usize) -> bool {
    if mouse_area(w, i) {
        truthy(w, i, "hoverEnabled", false) && enabled(w, i)
    } else if button(w, i)
        || ["ComboBox", "ScrollBar", "TextArea", "TextField"]
            .iter()
            .any(|t| is(w, i, t))
    {
        // QQuickControl, QQuickTextArea and QQuickTextField: `hovered` follows hoverEnabled,
        // which defaults to true with a mouse.
        // Qt 6 delivers hover to disabled Controls (tooltips of unavailable toggles show).
        !explicit(w, i, "hoverEnabled") || truthy(w, i, "hoverEnabled", true)
    } else {
        false
    }
}

fn accepted_buttons(w: &mut World, i: usize) -> u32 {
    if explicit(w, i, "acceptedButtons") {
        num(w, i, "acceptedButtons") as u32
    } else {
        1
    }
}

fn qt_button(b: u32) -> u32 {
    match b {
        0x110 => 1,
        0x111 => 2,
        0x112 => 4,
        0x113 => 8,
        0x114 => 16,
        _ => 0,
    }
}

/// MouseArea.pressAndHoldInterval, defaulting to QStyleHints::mousePressAndHoldInterval (800 ms).
fn hold_interval(rt: &Runtime, i: usize) -> f64 {
    let v = num(&mut rt.world.borrow_mut(), i, "pressAndHoldInterval");
    if v > 0.0 { v } else { 800.0 }
}

fn accepted(ev: &Value) -> bool {
    match ev {
        Value::Obj(o) => o.borrow().props.get("accepted").is_none_or(|v| v.truthy()),
        _ => true,
    }
}

fn put(rt: &Runtime, i: usize, name: &str, v: Value) {
    rt.world.borrow_mut().write(i, name, v);
}

/// The innermost interactive Flickable under the point that flicks along some axis.
fn flick_target(w: &mut World, hs: &[Hit]) -> Option<usize> {
    hs.iter().map(|h| h.item).find(|&i| {
        crate::scene::is_flickable(w, i)
            && (!explicit(w, i, "interactive") || truthy(w, i, "interactive", true))
            && enabled(w, i)
            && (w.flick_vertical(i) || w.flick_horizontal(i))
    })
}

/// QQuickItem::keepMouseGrab of an enabled grabber: MouseArea sets it on press from
/// preventStealing.
fn prevents_stealing(w: &mut World, g: usize) -> bool {
    mouse_area(w, g) && truthy(w, g, "preventStealing", false) && enabled(w, g)
}

/// QQuickFlickable::filterPointerEvent: may the Flickable take the grab from `g`?
fn stealable(w: &mut World, g: usize) -> bool {
    if mouse_area(w, g) {
        !truthy(w, g, "preventStealing", false)
            && !matches!(w.read(g, "drag.target"), Some(Value::Item(_)))
    } else {
        !slider(w, g) && !is(w, g, "ScrollBar")
    }
}

/// QQuickDragHandler::activeTranslation (and its old name `translation`) and
/// persistentTranslation: vectors of the axes' activeValue / persistentValue.
fn set_translation(rt: &mut Runtime, h: usize) {
    let vec = |w: &mut World, field: &str| {
        Runtime::js_object(&[
            ("x", Value::Num(num(w, h, &format!("xAxis.{field}")))),
            ("y", Value::Num(num(w, h, &format!("yAxis.{field}")))),
        ])
    };
    let mut w = rt.world.borrow_mut();
    let active = vec(&mut w, "activeValue");
    let persistent = vec(&mut w, "persistentValue");
    w.write(h, "activeTranslation", active.clone());
    w.write(h, "translation", active);
    w.write(h, "persistentTranslation", persistent);
}

/// QQuickDrag::setActive: the write tells the grouped `drag.onActiveChanged` handler.
fn set_drag_active(rt: &mut Runtime, ma: usize, on: bool) {
    rt.world
        .borrow_mut()
        .write(ma, "drag.active", Value::Bool(on));
}

impl Pointer {
    /// QMutableEventPoint::setTimestamp's velocity estimate.
    fn track(&mut self) {
        let (t, (x, y)) = (self.time, self.pos);
        match self.vel_last {
            Some((t0, x0, y0)) if t > t0 => {
                let k = |d: f64, old: f64| d / (t - t0) * 1000.0 * 0.7 + old * 0.3;
                self.vel = (k(x - x0, self.vel.0), k(y - y0, self.vel.1));
                self.vel_last = Some((t, x, y));
            }
            Some(_) => {}
            None => self.vel_last = Some((t, x, y)),
        }
    }

    /// The move as the press's Flickable sees it; true when the Flickable holds the grab
    /// (taking it from the child once its drag passed the threshold), so the child gets nothing.
    fn flick_motion(&mut self, rt: &mut Runtime) -> bool {
        let Some(f) = self.flick else {
            return false;
        };
        // QQuickFlickable::filterPointerEvent leaves a receiver that keeps its grab (a
        // preventStealing MouseArea, a text editor that selects by mouse) alone and forgets
        // the press.
        if !self.flick_grab
            && let Some((g, _)) = self.grab
            && (prevents_stealing(&mut rt.world.borrow_mut(), g) || self.text_keeps(rt, g))
        {
            rt.world.borrow_mut().flick_cancel(f);
            rt.settle();
            self.flick = None;
            return false;
        }
        // QQuickScrollView::childMouseEventFilter: a mouse's moves and release never reach
        // the ScrollView's own Flickable, which only filters those of its children.
        let scroll = rt.world.borrow().scroll_view_of(f).is_some();
        if scroll && (self.flick_grab || self.grab.is_none()) {
            return self.flick_grab;
        }
        let (lx, ly, _) = self.local(rt, f);
        let steal = rt.world.borrow().flick_steal(f);
        let steals = !self.flick_grab
            && steal
            && self.grab.is_some_and(|(g, _)| {
                is_ancestor(&rt.world.borrow(), f, g) && stealable(&mut rt.world.borrow_mut(), g)
            });
        if scroll && steals {
            // The move that takes the grab is the first of the drag: the content has not
            // moved, and the Flickable sees no other.
            self.cancel(rt);
            self.flick_grab = true;
            rt.world.borrow_mut().flick_cancel(f);
            rt.settle();
            return true;
        }
        rt.world
            .borrow_mut()
            .flick_move(f, (lx, ly), self.time, self.vel);
        rt.settle();
        if steals {
            self.cancel(rt);
            self.flick_grab = true;
        }
        self.flick_grab
    }

    /// ClickFocus on a press delivered to `h`: a TextInput/TextEdit (activeFocusOnPress) or a
    /// control whose focusPolicy has Qt.ClickFocus. True when a text editor took a middle press
    /// (supportsSelection: it pastes the primary selection on release).
    fn press_focus(&mut self, rt: &mut Runtime, h: &Hit, b: u32) -> bool {
        let mut w = rt.world.borrow_mut();
        let i = h.item;
        if crate::scene::is_text_edit(&w, i) {
            if !truthy(&mut w, i, "activeFocusOnPress", true) {
                return false;
            }
            w.force_active_focus_for(i, 0);
            if b == 4 {
                drop(w);
                self.grab = Some((i, b));
                let (lx, ly, _) = self.local(rt, i);
                self.text.middle_press(rt, i, lx, ly, self.mods);
                return true;
            }
        } else if crate::keys::click_focusable(&mut w, i) && enabled(&mut w, i) {
            w.force_active_focus_for(i, 0);
        }
        false
    }

    /// keepMouseGrab of a text editor that holds the press: QQuickTextEdit::mousePressEvent sets
    /// it from selectByMouse, QQuickTextInput::mouseMoveEvent once the selecting drag passed
    /// startDragDistance.
    fn text_keeps(&self, rt: &Runtime, g: usize) -> bool {
        let mut w = rt.world.borrow_mut();
        if is(&w, g, "TextEdit") || is(&w, g, "TextArea") {
            return truthy(&mut w, g, "selectByMouse", true);
        }
        self.text_keep && crate::scene::is_text_edit(&w, g)
    }

    fn link_at(&self, rt: &Runtime, i: usize, x: f64, y: f64) -> Option<String> {
        let mut f = self.fonts.try_borrow_mut().ok()?;
        crate::scene::link_at(&mut rt.world.borrow_mut(), &mut f, i, x, y)
    }

    /// QQuickText::hoverMoveEvent: a markdown Text in the hover chain tracks `hoveredLink`
    /// and emits linkHovered on change ("" on leave). So does a TextEdit, read-only or not
    /// (QQuickTextControlPrivate::hoverEvent: it always has Qt::LinksAccessibleByMouse).
    fn update_link_hover(&mut self, rt: &mut Runtime, hs: &[Hit]) {
        let md = |w: &mut World, i: usize| {
            (is_text(w, i) || crate::scene::is_text_doc(w, i))
                && explicit(w, i, "textFormat")
                && num(w, i, "textFormat") == 3.0
        };
        let cands: Vec<&Hit> = {
            let mut w = rt.world.borrow_mut();
            let top = hs
                .iter()
                .find(|h| accepts_hover(&mut w, h.item) || md(&mut w, h.item))
                .map(|h| h.item);
            match top {
                Some(t) => hs
                    .iter()
                    .filter(|h| (h.item == t || is_ancestor(&w, h.item, t)) && md(&mut w, h.item))
                    .collect(),
                None => vec![],
            }
        };
        let now: Vec<(usize, String)> = cands
            .iter()
            .filter_map(|h| Some((h.item, self.link_at(rt, h.item, h.x, h.y)?)))
            .collect();
        if now.is_empty() && self.link_hover.is_empty() {
            return;
        }
        let old = std::mem::take(&mut self.link_hover);
        for (i, _) in old.iter().filter(|(i, _)| !now.iter().any(|n| n.0 == *i)) {
            put(rt, *i, "hoveredLink", Value::str(""));
            rt.emit(*i, "linkHovered", &["link"], vec![Value::str("")]);
        }
        for (i, l) in now.iter().filter(|n| !old.contains(n)) {
            put(rt, *i, "hoveredLink", Value::str(l));
            rt.emit(*i, "linkHovered", &["link"], vec![Value::str(l)]);
        }
        self.link_hover = now;
    }

    fn mouse_event(&self, x: f64, y: f64, button: u32) -> Value {
        Runtime::js_object(&[
            ("x", Value::Num(x)),
            ("y", Value::Num(y)),
            ("button", Value::Num(button as f64)),
            ("buttons", Value::Num(self.buttons as f64)),
            ("modifiers", Value::Num(f64::from(self.mods))),
            ("accepted", Value::Bool(true)),
            ("wasHeld", Value::Bool(false)),
            ("source", Value::Num(0.0)),
            ("flags", Value::Num(0.0)),
        ])
    }

    fn local(&self, rt: &Runtime, i: usize) -> (f64, f64, bool) {
        let mut w = rt.world.borrow_mut();
        let (sx, sy) = w.scene_pos(i, self.win);
        let pos = unscaled(&mut w, i, self.win, self.pos);
        let (x, y) = (pos.0 - sx, pos.1 - sy);
        let inside =
            x >= 0.0 && y >= 0.0 && x < num(&mut w, i, "width") && y < num(&mut w, i, "height");
        (x, y, inside)
    }

    fn update_hover(&mut self, rt: &mut Runtime, hs: &[Hit]) {
        self.under = hs.iter().map(|h| h.item).collect();
        // QQuickDeliveryAgent::deliverHoverEventRecursive: the topmost hover-enabled leaf and
        // its ancestors get hover, siblings below do not; MouseArea never blocks it.
        let new: Vec<usize> = {
            let mut w = rt.world.borrow_mut();
            let top = hs
                .iter()
                .find(|h| accepts_hover(&mut w, h.item))
                .map(|h| h.item);
            match top {
                Some(t) => hs
                    .iter()
                    .map(|h| h.item)
                    .filter(|&i| (i == t || is_ancestor(&w, i, t)) && accepts_hover(&mut w, i))
                    .collect(),
                None => Vec::new(),
            }
        };
        let grabbed = self.grab.map(|g| g.0);
        for &i in self.hovered.iter().filter(|i| !new.contains(i)) {
            let ma = mouse_area(&rt.world.borrow(), i);
            if ma {
                if grabbed != Some(i) {
                    put(rt, i, "containsMouse", Value::Bool(false));
                }
                rt.emit(i, "exited", &[], Vec::new());
            } else {
                put(rt, i, "hovered", Value::Bool(false));
            }
        }
        for &i in new.iter().filter(|i| !self.hovered.contains(i)) {
            let ma = mouse_area(&rt.world.borrow(), i);
            if ma {
                put(rt, i, "containsMouse", Value::Bool(true));
                rt.emit(i, "entered", &[], Vec::new());
            } else {
                put(rt, i, "hovered", Value::Bool(true));
            }
        }
        self.hovered = new;
        self.update_link_hover(rt, hs);
    }

    /// QQuickDeliveryAgent::event(QEvent::Enter): one hover delivery at the enter position,
    /// which also arms frame-synchronous rehover (lastMousePosition is set).
    pub fn enter(&mut self, rt: &mut Runtime, win: usize, x: f64, y: f64) {
        self.win = Some(win);
        self.pos = (x, y);
        rt.world.borrow_mut().pointer = Some((win, x, y));
        if self.grab.is_some() {
            return;
        }
        let hs = hits(&mut rt.world.borrow_mut(), win, self.pos);
        let old = self.hovered.clone();
        self.update_hover(rt, &hs);
        for h in &hs {
            if self.hovered.contains(&h.item)
                && !old.contains(&h.item)
                && mouse_area(&rt.world.borrow(), h.item)
            {
                put(rt, h.item, "mouseX", Value::Num(h.x));
                put(rt, h.item, "mouseY", Value::Num(h.y));
                // QQuickMouseArea::hoverEnterEvent ends with positionChanged.
                let ev = self.mouse_event(h.x, h.y, 0);
                rt.emit(h.item, "positionChanged", &["mouse"], vec![ev]);
            }
        }
        rt.settle();
        self.rev = rt.world.borrow().revision;
    }

    pub fn motion(&mut self, rt: &mut Runtime, win: usize, x: f64, y: f64) {
        // QGuiApplicationPrivate::processMouseEvent drops moves that keep the position.
        let moved = self.win != Some(win) || self.pos != (x, y);
        self.win = Some(win);
        self.pos = (x, y);
        if self.last_press.as_ref().is_some_and(|l| {
            (x - l.pos.0).abs() > DOUBLE_CLICK_DISTANCE
                || (y - l.pos.1).abs() > DOUBLE_CLICK_DISTANCE
        }) {
            self.last_press = None;
        }
        self.track();
        rt.world.borrow_mut().pointer = Some((win, x, y));
        let hs = hits(&mut rt.world.borrow_mut(), win, self.pos);
        self.update_hover(rt, &hs);
        // Passive grabbers (DragHandler) see the move before the exclusive grabber.
        self.handlers_move(rt);
        // A Flickable filters its children's moves before they reach the grabber.
        self.swipe_move(rt);
        // A move that keeps the position reaches no item, the Flickable's filter neither: it
        // takes no velocity sample from it.
        if (moved && self.flick_motion(rt)) || self.flick_grab {
        } else if let Some((g, b)) = self.grab {
            let (lx, _, _) = self.local(rt, g);
            if mouse_area(&rt.world.borrow(), g) {
                self.drag_move(rt, g);
                let (lx, ly, inside) = self.local(rt, g);
                put(rt, g, "mouseX", Value::Num(lx));
                put(rt, g, "mouseY", Value::Num(ly));
                put(rt, g, "containsPress", Value::Bool(inside));
                let ev = self.mouse_event(lx, ly, b);
                rt.emit(g, "positionChanged", &["mouse"], vec![ev]);
            } else if slider(&rt.world.borrow(), g) {
                slider_drag(rt, g, lx, false);
            } else if b == 1 && moved && crate::scene::is_text_edit(&rt.world.borrow(), g) {
                let (lx, ly, _) = self.local(rt, g);
                self.text.drag(rt, g, lx, ly, self.time);
                self.text_keep |=
                    self.text.selecting() && (self.pos.0 - self.press_pos.0).abs() > 10.0;
            } else if scrollbar(&mut rt.world.borrow_mut(), g)
                && truthy(&mut rt.world.borrow_mut(), g, "pressed", false)
            {
                let (lx, ly, _) = self.local(rt, g);
                scrollbar_drag(rt, g, (lx, ly), self.bar_offset);
            }
        } else if !self.handlers.iter().any(|g| g.active) {
            // No hover delivery while a handler holds the exclusive grab (handleMouseEvent).
            for h in &hs {
                if moved && self.hovered.contains(&h.item) && mouse_area(&rt.world.borrow(), h.item)
                {
                    put(rt, h.item, "mouseX", Value::Num(h.x));
                    put(rt, h.item, "mouseY", Value::Num(h.y));
                    let ev = self.mouse_event(h.x, h.y, 0);
                    rt.emit(h.item, "positionChanged", &["mouse"], vec![ev]);
                }
            }
        }
        rt.settle();
        if self.grab.is_some() || self.drop.is_some() {
            self.deliver_drop(rt);
        }
    }

    /// QQuickDragHandler on press: every DragHandler of `item` whose parent contains the point
    /// and accepts the button takes a passive grab; for a mouse that accepts the point, so items
    /// below only reach their handlers. True if any did.
    fn press_handlers(&mut self, rt: &mut Runtime, item: usize, b: u32) -> bool {
        let mut w = rt.world.borrow_mut();
        let mut took = false;
        for h in drag_handlers(&w, item) {
            let buttons = if explicit(&w, h, "acceptedButtons") {
                num(&mut w, h, "acceptedButtons") as u32
            } else {
                1
            };
            if buttons & b == 0 || !enabled(&mut w, h) {
                continue;
            }
            let press_local =
                handler_target(&mut w, h).map(|t| in_item(&mut w, t, self.win, self.pos));
            self.handlers.push(HandlerGrab {
                handler: h,
                press_local,
                active: false,
                trans: (0.0, 0.0),
            });
            took = true;
        }
        took
    }

    /// QQuickDragHandler::handlePointerEventImpl for a move: activate past the drag threshold by
    /// stealing the exclusive grab, then track activeTranslation and move the target.
    fn handlers_move(&mut self, rt: &mut Runtime) {
        if self.handlers.is_empty() || self.buttons & self.handler_button == 0 {
            return;
        }
        let d = (self.pos.0 - self.press_pos.0, self.pos.1 - self.press_pos.1);
        for k in 0..self.handlers.len() {
            let h = self.handlers[k].handler;
            let (xe, ye) = {
                let mut w = rt.world.borrow_mut();
                if w.dead.contains(&h) {
                    continue;
                }
                (
                    drag_axis(&mut w, h, "xAxis").0,
                    drag_axis(&mut w, h, "yAxis").0,
                )
            };
            if self.handlers[k].active {
                let t = (if xe { d.0 } else { 0.0 }, if ye { d.1 } else { 0.0 });
                let old = self.handlers[k].trans;
                if t != old {
                    self.handlers[k].trans = t;
                    // QQuickDragAxis::updateValue emits activeValueChanged(delta) per enabled axis.
                    for (axis, en, v, dv) in [
                        ("xAxis", xe, t.0, t.0 - old.0),
                        ("yAxis", ye, t.1, t.1 - old.1),
                    ] {
                        if en {
                            let pk = format!("{axis}.persistentValue");
                            let pv = num(&mut rt.world.borrow_mut(), h, &pk);
                            put(rt, h, &pk, Value::Num(pv + dv));
                            put(rt, h, &format!("{axis}.activeValue"), Value::Num(v));
                            rt.fire(
                                h,
                                &format!("{axis}.onActiveValueChanged"),
                                &["delta"],
                                vec![Value::Num(dv)],
                            );
                        }
                    }
                }
                set_translation(rt, h);
                self.move_target(rt, k);
                rt.settle();
                return;
            }
            if self.handlers.iter().any(|g| g.active) {
                continue;
            }
            // The travel is measured in the target (its mapFromScene of both points), in the
            // scene without one.
            let mut dd = {
                let mut w = rt.world.borrow_mut();
                match handler_target(&mut w, h) {
                    Some(t) => {
                        let c = in_item(&mut w, t, self.win, self.pos);
                        let s = in_item(&mut w, t, self.win, self.press_pos);
                        (c.0 - s.0, c.1 - s.1)
                    }
                    None => d,
                }
            };
            // Movement along a disabled axis counts only while it is not the dominant one.
            if !xe {
                if dd.0.abs() > dd.1.abs() {
                    dd.1 = 0.0;
                }
                dd.0 = 0.0;
            }
            if !ye {
                if dd.1.abs() > dd.0.abs() {
                    dd.0 = 0.0;
                }
                dd.1 = 0.0;
            }
            let th = {
                let mut w = rt.world.borrow_mut();
                if explicit(&w, h, "dragThreshold") {
                    num(&mut w, h, "dragThreshold")
                } else {
                    10.0
                }
            };
            if dd.0.abs() <= th && dd.1.abs() <= th {
                continue;
            }
            // grabPoints: the item grabber is ungrabbed (canceled) unless it keeps its grab.
            if let Some((g, _)) = self.grab {
                let dragging = self.drag.as_ref().is_some_and(|d| d.active);
                if keeps_grab(&mut rt.world.borrow_mut(), g, dragging) || self.text_keeps(rt, g) {
                    continue;
                }
                self.cancel(rt);
            }
            self.handlers[k].active = true;
            {
                // QQuickDragAxis::onActiveChanged resets activeValue without a change signal.
                let mut w = rt.world.borrow_mut();
                for axis in ["xAxis.activeValue", "yAxis.activeValue"] {
                    if w.read(h, axis).is_some_and(|v| v.to_number() != 0.0) {
                        w.set_internal(h, axis, Value::Num(0.0));
                    }
                }
            }
            set_translation(rt, h);
            put(rt, h, "active", Value::Bool(true));
            rt.settle();
            self.move_target(rt, k);
            rt.settle();
            return;
        }
    }

    /// QQuickPointerHandler::moveTarget: keep the press point under the pointer, within the axes.
    fn move_target(&mut self, rt: &mut Runtime, k: usize) {
        let h = self.handlers[k].handler;
        let mut w = rt.world.borrow_mut();
        let Some(t) = handler_target(&mut w, h) else {
            return;
        };
        if w.items[t].parent.is_none() {
            return;
        }
        let press = in_item(&mut w, t, self.win, self.press_pos);
        let pl = *self.handlers[k].press_local.get_or_insert(press);
        let cur = (num(&mut w, t, "x"), num(&mut w, t, "y"));
        // QQuickDragHandler: how far the pointer is from the pressed point, in the target, is
        // where the target's transformOrigin point goes; mapped to the parent, that gives x/y.
        let at = in_item(&mut w, t, self.win, self.pos);
        let by = (at.0 - pl.0, at.1 - pl.1);
        let mut pos = match w.item_transform(t) {
            Some(m) => {
                let o = origin_point(&mut w, t);
                let p = m.map(by.0 + o.0, by.1 + o.1);
                (p.0 + cur.0 - o.0, p.1 + cur.1 - o.1)
            }
            None => (by.0 + cur.0, by.1 + cur.1),
        };
        let (xe, xmin, xmax) = drag_axis(&mut w, h, "xAxis");
        let (ye, ymin, ymax) = drag_axis(&mut w, h, "yAxis");
        // qBound(min, v, max)
        pos.0 = if xe { xmin.max(pos.0.min(xmax)) } else { cur.0 };
        pos.1 = if ye { ymin.max(pos.1.min(ymax)) } else { cur.1 };
        for (n, v, p) in [("x", pos.0, cur.0), ("y", pos.1, cur.1)] {
            if v != p {
                // QQuickItem::setPosition drops a QProperty binding unless a Behavior intercepts.
                if !w.behaviors.contains_key(&(t, qml::runtime::Sym::new(n))) {
                    w.clear_binding(t, n);
                }
                w.set_internal(t, n, Value::Num(v));
            }
        }
    }

    /// The handlers' point was released or lost: an active DragHandler deactivates.
    fn handlers_end(&mut self, rt: &mut Runtime) {
        for g in std::mem::take(&mut self.handlers) {
            if !g.active || rt.world.borrow().dead.contains(&g.handler) {
                continue;
            }
            {
                let mut w = rt.world.borrow_mut();
                for axis in ["xAxis.activeValue", "yAxis.activeValue"] {
                    if w.read(g.handler, axis)
                        .is_some_and(|v| v.to_number() != 0.0)
                    {
                        w.set_internal(g.handler, axis, Value::Num(0.0));
                    }
                }
            }
            set_translation(rt, g.handler);
            put(rt, g.handler, "active", Value::Bool(false));
            rt.settle();
        }
    }

    /// QQuickMouseArea::mouseMoveEvent's `drag.target` branch (axis, bounds, threshold, smoothed).
    fn drag_move(&mut self, rt: &mut Runtime, ma: usize) {
        let Some(d) = self.drag.as_mut() else { return };
        let t = d.target;
        let mut w = rt.world.borrow_mut();
        let pos = (num(&mut w, t, "x"), num(&mut w, t, "y"));
        let ts = *d.target_start.get_or_insert(pos);
        let activate = d.over && !d.active;
        if activate {
            d.active = true;
        }
        let axis = if explicit(&w, ma, "drag.axis") {
            num(&mut w, ma, "drag.axis") as i32
        } else {
            3
        };
        let bound = |w: &mut World, n: &str, v: f64, max: bool| {
            if explicit(w, ma, n) {
                let b = num(w, ma, n);
                if max { v.min(b) } else { v.max(b) }
            } else {
                v
            }
        };
        // Both points and the target's start in the target's parent, as Qt gets them: the
        // start goes to the scene and back, the points come from it (mapFromScene), and the
        // sums are made in its order. Where the parent sits at a fraction of a pixel they
        // round, and a travel of exactly the threshold is past it or not as there (the
        // overview's window, dragged 10 px left, started to move one pointer step later).
        let (c, s, ts) = match w.items[t].parent {
            Some(p) => {
                let at = scene_offset(&mut w, p, self.win);
                let c = unscaled(&mut w, p, self.win, self.pos);
                let s = unscaled(&mut w, p, self.win, d.start);
                (
                    (c.0 - at.0, c.1 - at.1),
                    (s.0 - at.0, s.1 - at.1),
                    (ts.0 + at.0 - at.0, ts.1 + at.1 - at.1),
                )
            }
            None => (self.pos, d.start, ts),
        };
        let mut dp = pos;
        let mut bp = pos;
        if axis & 1 != 0 {
            dp.0 = ts.0 + c.0 - s.0;
            let v = bound(&mut w, "drag.minimumX", dp.0, false);
            bp.0 = bound(&mut w, "drag.maximumX", v, true);
        }
        if axis & 2 != 0 {
            dp.1 = ts.1 + c.1 - s.1;
            let v = bound(&mut w, "drag.minimumY", dp.1, false);
            bp.1 = bound(&mut w, "drag.maximumY", v, true);
        }
        if d.active {
            // QQuickItem::setX drops a QProperty binding; with a Behavior the binding is a
            // QQmlBinding behind the interceptor and survives (OverviewWindow `x: initX`).
            for (n, v, p) in [("x", bp.0, pos.0), ("y", bp.1, pos.1)] {
                if v != p {
                    if !w.behaviors.contains_key(&(t, qml::runtime::Sym::new(n))) {
                        w.clear_binding(t, n);
                    }
                    w.set_internal(t, n, Value::Num(v));
                    if let Some(l) = w.items[t].parent
                        && crate::scene::linear_axis(&w, l).is_some()
                    {
                        w.layout_dragged.insert(t);
                    }
                }
            }
        }
        // QQuickDeliveryAgentPrivate::dragOverThreshold: |d| > threshold (startDragDistance 10).
        let th = if explicit(&w, ma, "drag.threshold") {
            num(&mut w, ma, "drag.threshold")
        } else {
            10.0
        };
        if !d.over
            && ((pos.0 != bp.0 && (dp.0 - ts.0).abs() > th)
                || (pos.1 != bp.1 && (dp.1 - ts.1).abs() > th))
        {
            d.over = true;
            let smoothed =
                !explicit(&w, ma, "drag.smoothed") || truthy(&mut w, ma, "drag.smoothed", true);
            if smoothed {
                d.start = self.pos;
            }
        }
        drop(w);
        if activate {
            set_drag_active(rt, ma, true);
        }
    }

    /// QQuickDragAttached move delivery: the topmost enabled DropArea under the hot spot of an
    /// item with `Drag.active` gets entered/positionChanged/exited and `containsDrag`.
    fn deliver_drop(&mut self, rt: &mut Runtime) {
        let Some(win) = self.win else { return };
        let src = self
            .drag
            .as_ref()
            .map(|d| d.target)
            .or(self.drop.map(|d| d.0))
            .filter(|&s| {
                let mut w = rt.world.borrow_mut();
                !w.dead.contains(&s) && w.read(s, "Drag.active").is_some_and(|v| v.truthy())
            });
        let area = src.and_then(|s| {
            let mut w = rt.world.borrow_mut();
            let (sx, sy) = w.scene_pos(s, Some(win));
            let (hx, hy) = hot_spot(&mut w, s);
            let hs = hits(&mut w, win, (sx + hx, sy + hy));
            hs.into_iter()
                .find(|h| {
                    h.item != s
                        && !is_ancestor(&w, s, h.item)
                        && is(&w, h.item, "DropArea")
                        && enabled(&mut w, h.item)
                })
                .map(|h| (h.item, h.x, h.y))
        });
        let old = self.drop.map(|d| d.1);
        let new = area.map(|a| a.0);
        if old != new
            && let Some(o) = old
        {
            put(rt, o, "containsDrag", Value::Bool(false));
            rt.emit(o, "exited", &[], Vec::new());
        }
        if let (Some(s), Some((a, x, y))) = (src, area) {
            let ev = Runtime::js_object(&[
                ("x", Value::Num(x)),
                ("y", Value::Num(y)),
                ("source", Value::Item(s)),
                ("accepted", Value::Bool(true)),
            ]);
            if old != Some(a) {
                put(rt, a, "containsDrag", Value::Bool(true));
                put(rt, a, "drag.source", Value::Item(s));
                rt.emit(a, "entered", &["drag"], vec![ev]);
            } else {
                rt.emit(a, "positionChanged", &["drag"], vec![ev]);
            }
        }
        self.drop = src.zip(new);
        rt.settle();
    }

    /// Window `win` is drawing a frame. Each window has its own QQuickDeliveryAgent, which
    /// QQuickWindowPrivate::flushFrameSynchronousEvents runs for a frame of that window: frames
    /// of other windows deliver no hover to the one under the pointer.
    pub fn frame(&mut self, win: usize, now: std::time::Instant) {
        self.hover_timers.entry(win).or_insert(now);
        if self.win == Some(win) {
            self.flush_hover(win, now);
        }
    }

    /// QQuickDeliveryAgentPrivate::flushFrameSynchronousEvents for the pointer's window, from
    /// its frame or its delay timer. Hover is delivered again at most once in the interval:
    /// a frame that comes sooner starts the delay timer for the rest of it. After the
    /// interval it is delivered with the frame, and the interval starts again. At 0 every
    /// frame delivers it, below 0 none does.
    fn flush_hover(&mut self, win: usize, now: std::time::Instant) {
        let Ok(every) = u64::try_from(self.hover_every.unwrap_or_else(frame_hover_interval)) else {
            return;
        };
        let every = std::time::Duration::from_millis(every);
        let timer = self.hover_timers.get(&win).copied().unwrap_or(now);
        if now.saturating_duration_since(timer) < every {
            self.hover_delay.get_or_insert((win, timer + every));
        } else if self.grab.is_none() && self.pos != (0.0, 0.0) {
            // QQuickDeliveryAgent skips it while lastMousePosition isNull(): Hyprland enters
            // a surface the pointer is not over (a layer mapping above it) at (0, 0).
            self.hover_delay = None;
            self.framed = true;
            if !every.is_zero() {
                self.hover_timers.insert(win, now);
            }
        }
    }

    /// Time left until the delay timer of frame-synchronous hover fires, in ms.
    pub fn hover_due(&self, now: std::time::Instant) -> Option<f64> {
        let (_, at) = self.hover_delay?;
        Some(at.saturating_duration_since(now).as_secs_f64() * 1000.0)
    }

    /// The delay timer of frame-synchronous hover fires when it is due: `rehover` then
    /// delivers the hover no frame was allowed to.
    pub fn hover_timer(&mut self, now: std::time::Instant) {
        if let Some((win, at)) = self.hover_delay
            && at <= now
        {
            self.hover_delay = None;
            if self.win == Some(win) {
                self.flush_hover(win, now);
            }
        }
    }

    /// Windows that are gone take their frameSynchronousHoverTimer with them: `live` is how
    /// many windows there are and says whether one is among them.
    pub fn keep_windows(&mut self, live: usize, is_live: impl Fn(usize) -> bool) {
        if self.hover_timers.len() > live {
            self.hover_timers.retain(|&w, _| is_live(w));
        }
    }

    /// The delivery of QQuickDeliveryAgentPrivate::flushFrameSynchronousEvents (`flush_hover`
    /// says when): with no item holding the mouse grab (a pointer handler's grab does not
    /// count), hover is delivered again at the last mouse position. An unchanged scene has
    /// nothing to deliver and is not hit-tested. Items that came under the
    /// still pointer are entered and those that left exited; a MouseArea that stays hovered but
    /// moved under it gets a hover move at the pointer's new place in it.
    pub fn rehover(&mut self, rt: &mut Runtime) -> bool {
        if !std::mem::take(&mut self.framed) {
            return false;
        }
        let rev = rt.world.borrow().revision;
        if rev == self.rev || self.grab.is_some() || self.pos == (0.0, 0.0) {
            return false;
        }
        self.rev = rev;
        let Some(win) = self.win else { return false };
        self.hit_tests += 1;
        let hs = hits(&mut rt.world.borrow_mut(), win, self.pos);
        let was = self.hovered.clone();
        let mut changed = !hs.iter().map(|h| h.item).eq(self.under.iter().copied());
        if changed {
            self.update_hover(rt, &hs);
        }
        for h in &hs {
            if !self.hovered.contains(&h.item) || !mouse_area(&rt.world.borrow(), h.item) {
                continue;
            }
            // QQuickMouseArea::hoverEnterEvent takes the position as it is; hoverMoveEvent only
            // one that differs from the last it saw.
            let last = {
                let mut w = rt.world.borrow_mut();
                (num(&mut w, h.item, "mouseX"), num(&mut w, h.item, "mouseY"))
            };
            if was.contains(&h.item) && last == (h.x, h.y) {
                continue;
            }
            put(rt, h.item, "mouseX", Value::Num(h.x));
            put(rt, h.item, "mouseY", Value::Num(h.y));
            let ev = self.mouse_event(h.x, h.y, 0);
            rt.emit(h.item, "positionChanged", &["mouse"], vec![ev]);
            changed = true;
        }
        if !changed {
            return false;
        }
        rt.settle();
        self.rev = rt.world.borrow().revision;
        true
    }

    /// Qt::CursorShape of the topmost MouseArea under the pointer (the grabbing MouseArea while
    /// pressed; a text editor that holds the press has no cursor of its own to keep: the item
    /// under the pointer decides, as without a press).
    pub fn cursor_shape(&self, rt: &Runtime) -> i32 {
        let w = rt.world.borrow();
        let shape = |i: usize| {
            w.items[i]
                .props
                .get("cursorShape")
                .map(|p| p.value.to_number() as i32)
        };
        // A pressed control is not a cursor of its own: the MouseArea over it stays the one
        // under the pointer (StyledSlider's closed hand).
        if let Some((g, _)) = self.grab
            && mouse_area(&w, g)
        {
            return shape(g).unwrap_or(0);
        }
        drop(w);
        // A hover-enabled MouseArea on top (e.g. a dialog scrim) occludes cursors set below it.
        let mut w = rt.world.borrow_mut();
        for &i in &self.under {
            // QQuickWindowPrivate::findCursorItemAndHandler skips items that are not
            // QQuickItem::isEnabled. A MouseArea with `enabled: false` still is, and keeps
            // its cursor.
            if !item_enabled(&mut w, i) {
                continue;
            }
            // QQuickTextInput/QQuickTextEdit: IBeam, or Arrow while read-only.
            if crate::scene::is_text_edit(&w, i) {
                return if w.read(i, "readOnly").is_some_and(|v| v.truthy()) {
                    0
                } else {
                    4
                };
            }
            // AbstractButton, Slider, ComboBox, ScrollBar, SpinBox, Dial, RangeSlider call
            // setCursor(Qt::ArrowCursor) in their constructors.
            if button(&w, i)
                || slider(&w, i)
                || ["ComboBox", "ScrollBar", "SpinBox", "Dial", "RangeSlider"]
                    .iter()
                    .any(|t| is(&w, i, t))
            {
                return 0;
            }
            if !mouse_area(&w, i) {
                continue;
            }
            // QQuickMouseArea's constructor calls setCursor(Qt::ArrowCursor), so every MouseArea
            // under the pointer decides the cursor (QQuickWindowPrivate::findCursorItemAndHandler).
            return w.items[i]
                .props
                .get("cursorShape")
                .map_or(0, |p| p.value.to_number() as i32);
        }
        0
    }

    pub fn leave(&mut self, rt: &mut Runtime) {
        self.update_hover(rt, &[]);
        self.win = None;
        rt.settle();
    }

    /// A mouse button is down.
    pub fn held(&self) -> bool {
        self.buttons != 0
    }

    pub fn press(&mut self, rt: &mut Runtime, code: u32) {
        let b = qt_button(code);
        let Some(win) = self.win else { return };
        self.track();
        // A grab whose button is no longer down lost its release: drop it like Qt would.
        if self.grab.is_some_and(|(_, gb)| self.buttons & gb == 0) {
            self.cancel(rt);
        }
        if !self.handlers.is_empty() && self.buttons & self.handler_button == 0 {
            self.handlers_end(rt);
        }
        self.buttons |= b;
        let double = self.last_press.take().is_some_and(|l| {
            let dt = self.time - l.at;
            (0.0..DOUBLE_CLICK_MS).contains(&dt) && l.button == b && l.win == win
        });
        // After a double click mousePressButton is reset: a third press starts over.
        if !double {
            self.last_press = Some(LastPress {
                at: self.time,
                pos: self.pos,
                button: b,
                win,
            });
        }
        if b == 0 || self.grab.is_some() || self.handlers.iter().any(|g| g.active) {
            return;
        }
        self.handlers.clear();
        self.press_pos = self.pos;
        self.handler_button = b;
        close_popups_outside(rt, win, self.pos);
        let mut hs = hits(&mut rt.world.borrow_mut(), win, self.pos);
        popup_blocks_press(&mut rt.world.borrow_mut(), win, self.pos, &mut hs);
        if qml::log_on!("ri::ptr") {
            tracing::debug!(
                target: "ri::ptr",
                "ptr press b={b} win={win} pos={:?} hits={}",
                self.pos,
                hs.len()
            );
        }
        self.flick = None;
        self.flick_grab = false;
        // A keepMouseGrab receiver (Slider, ScrollBar) keeps the Flickable's filter out entirely.
        let target = (b == 1)
            .then(|| flick_target(&mut rt.world.borrow_mut(), &hs))
            .flatten()
            .filter(|&f| {
                let w = rt.world.borrow();
                !hs.iter()
                    .take_while(|h| h.item != f)
                    .any(|h| slider(&w, h.item) || is(&w, h.item, "ScrollBar"))
            });
        if let Some(f) = target {
            let (lx, ly, _) = self.local(rt, f);
            rt.world.borrow_mut().flick_press(f, (lx, ly), self.time);
            rt.settle();
            self.flick = Some(f);
            // A moving Flickable filters the press: the content stops, children see nothing.
            if rt.world.borrow().flick_moving(f) {
                self.flick_grab = true;
                return;
            }
        }
        self.swipe_press(rt, &hs, b);
        self.text_keep = false;
        // QQuickDeliveryAgentPrivate::deliverPressOrReleaseEvent: once the point is accepted,
        // the remaining items only reach their pointer handlers.
        let mut handlers_only = false;
        for h in hs {
            let took = self.press_handlers(rt, h.item, b);
            let skip = handlers_only;
            handlers_only |= took;
            if skip {
                continue;
            }
            // QQuickItemPrivate::setFocusIfNeeded: only an item the press reaches takes focus,
            // not one below the MouseArea (a dialog's scrim) that accepted it.
            if self.press_focus(rt, &h, b) {
                return;
            }
            if self.flick == Some(h.item) {
                // QQuickFlickable::mousePressEvent accepts a press none of its children took:
                // items below the Flickable (a dialog's background MouseArea) miss it.
                handlers_only = true;
                continue;
            }
            // QQuickText::mousePressEvent: a press on a link (with linkActivated connected) is
            // accepted; the release over the same link emits linkActivated.
            if b == 1
                && is_text(&rt.world.borrow(), h.item)
                && rt.has_handler(h.item, "onLinkActivated")
                && enabled(&mut rt.world.borrow_mut(), h.item)
                && let Some(l) = self.link_at(rt, h.item, h.x, h.y)
            {
                self.link = Some(l);
                self.grab = Some((h.item, b));
                self.drag = None;
                handlers_only = true;
                continue;
            }
            if b == 1 && {
                let mut w = rt.world.borrow_mut();
                crate::textmouse::takes_press(&mut w, h.item) && enabled(&mut w, h.item)
            } {
                // QQuickTextInput/QQuickTextEdit::mousePressEvent accept the left press: items
                // below miss it.
                self.grab = Some((h.item, b));
                self.drag = None;
                handlers_only = true;
                let (lx, ly, _) = self.local(rt, h.item);
                self.link = self.link_at(rt, h.item, lx, ly);
                self.text.fonts = self.fonts.clone();
                self.text
                    .press(rt, h.item, lx, ly, self.pos, self.mods, self.time);
                continue;
            }
            if b == 1 && {
                let mut w = rt.world.borrow_mut();
                crate::scene::is_text_doc(&w, h.item) && enabled(&mut w, h.item)
            } {
                // QQuickTextControlPrivate::mousePressEvent: a TextEdit that can be neither
                // selected nor edited still keeps the left press, for its links
                // (Qt::LinksAccessibleByMouse).
                self.link = self.link_at(rt, h.item, h.x, h.y);
                self.grab = Some((h.item, b));
                self.drag = None;
                handlers_only = true;
                continue;
            }
            let (ma, ok) = {
                let mut w = rt.world.borrow_mut();
                let ma = mouse_area(&w, h.item);
                let ok = enabled(&mut w, h.item)
                    && if ma {
                        accepted_buttons(&mut w, h.item) & b != 0
                    } else {
                        (button(&w, h.item)
                            || slider(&w, h.item)
                            || scrollbar(&mut w, h.item)
                            || is(&w, h.item, "ComboBox")
                            || spin_part(&mut w, h.item, h.x, h.y).is_some())
                            && b == 1
                    };
                (ma, ok)
            };
            if qml::log_on!("ri::ptr") {
                let w = rt.world.borrow();
                tracing::debug!(
                    target: "ri::ptr",
                    "ptr hit {} {:?} ma={ma} ok={ok}",
                    h.item,
                    w.items[h.item].types.first()
                );
            }
            if !ok {
                continue;
            }
            {
                // QQuickItemPrivate::setFocusIfNeeded: an item the press is delivered to takes
                // active focus when its focusPolicy includes Qt.ClickFocus.
                let mut w = rt.world.borrow_mut();
                if click_focus(&mut w, h.item) {
                    w.force_active_focus_for(h.item, 0);
                }
            }
            if ma {
                for (n, v) in [
                    ("pressed", Value::Bool(true)),
                    ("pressedButtons", Value::Num(b as f64)),
                    ("containsPress", Value::Bool(true)),
                    ("containsMouse", Value::Bool(true)),
                    ("mouseX", Value::Num(h.x)),
                    ("mouseY", Value::Num(h.y)),
                ] {
                    put(rt, h.item, n, v);
                }
                let ev = self.mouse_event(h.x, h.y, b);
                rt.emit(h.item, "pressed", &["mouse"], vec![ev.clone()]);
                rt.settle();
                if !accepted(&ev) {
                    put(rt, h.item, "pressed", Value::Bool(false));
                    put(rt, h.item, "pressedButtons", Value::Num(0.0));
                    put(rt, h.item, "containsPress", Value::Bool(false));
                    continue;
                }
                self.long_press = false;
                self.hold = rt
                    .has_handler(h.item, "onPressAndHold")
                    .then(std::time::Instant::now);
            } else if let Some(k) = { spin_part(&mut rt.world.borrow_mut(), h.item, h.x, h.y) } {
                // QQuickSpinBoxPrivate::handlePress: only the indicator under the press is pressed.
                put(rt, h.item, k, Value::Bool(true));
                rt.settle();
            } else if scrollbar(&mut rt.world.borrow_mut(), h.item) {
                self.bar_offset = scrollbar_offset(&mut rt.world.borrow_mut(), h.item, (h.x, h.y));
                put(rt, h.item, "pressed", Value::Bool(true));
                rt.settle();
            } else {
                // QQuickControlPrivate::handlePress: ComboBox's StrongFocus includes ClickFocus.
                if is(&rt.world.borrow(), h.item, "ComboBox") {
                    let mut w = rt.world.borrow_mut();
                    let policy = match w.items[h.item].props.get("focusPolicy") {
                        Some(p) => p.value.to_number() as u32,
                        None => 11,
                    };
                    if policy & 2 != 0 {
                        w.force_active_focus_for(h.item, 0);
                    }
                }
                put(rt, h.item, "pressed", Value::Bool(true));
                if slider(&rt.world.borrow(), h.item) {
                    // QQuickSlider::mousePressEvent: handlePress sets pressed, then
                    // handleMove at the press point, the handle still at the width it had.
                    rt.settle();
                    slider_drag(rt, h.item, h.x, false);
                } else {
                    put(rt, h.item, "down", Value::Bool(true));
                    rt.emit(h.item, "pressed", &[], Vec::new());
                }
                rt.settle();
            }
            if qml::log_on!("ri::ptr") {
                let w = rt.world.borrow();
                tracing::debug!(
                    target: "ri::ptr",
                    "ptr grab {} {:?} ma {ma}",
                    h.item,
                    w.items[h.item].types.first()
                );
            }
            self.grab = Some((h.item, b));
            self.drag = None;
            self.double_click = false;
            if ma {
                if truthy(&mut rt.world.borrow_mut(), h.item, "drag.active", false) {
                    set_drag_active(rt, h.item, false);
                }
                let t = rt.world.borrow_mut().read(h.item, "drag.target");
                if let Some(Value::Item(t)) = t {
                    self.drag = Some(DragGrab {
                        target: t,
                        start: self.pos,
                        target_start: None,
                        over: false,
                        active: false,
                    });
                }
            }
            handlers_only = true;
        }
        if let Some(f) = self.flick
            && self.grab.is_none()
        {
            if self.handlers.is_empty() {
                // Nothing inside took the press: QQuickFlickable::mousePressEvent accepts it.
                self.flick_grab = true;
            } else {
                self.flick = None;
                rt.world.borrow_mut().flick_cancel(f);
                rt.settle();
            }
        }
        if double {
            self.double_click_grabber(rt);
        }
        if self.grab.is_some() {
            self.deliver_drop(rt);
        }
    }

    /// QGuiApplicationPrivate sends MouseButtonDblClick right after the second press; the
    /// grabbing MouseArea emits doubleClicked() (QQuickMouseArea::mouseDoubleClickEvent).
    fn double_click_grabber(&mut self, rt: &mut Runtime) {
        let Some((g, b)) = self.grab else { return };
        let (ma, ok) = {
            let mut w = rt.world.borrow_mut();
            let ma = mouse_area(&w, g);
            (ma, (ma || button(&w, g)) && enabled(&mut w, g))
        };
        if !ok {
            return;
        }
        if !ma {
            // QQuickAbstractButton::mouseDoubleClickEvent: only with doubleClicked connected,
            // and then its release triggers no clicked().
            if rt.has_handler(g, "onDoubleClicked") {
                rt.emit(g, "doubleClicked", &[], Vec::new());
                rt.settle();
                self.double_click = true;
            }
            return;
        }
        let (lx, ly, _) = self.local(rt, g);
        let connected = rt.has_handler(g, "onDoubleClicked");
        let ev = self.mouse_event(lx, ly, b);
        if let Value::Obj(o) = &ev {
            o.borrow_mut()
                .props
                .insert("accepted".into(), Value::Bool(connected));
        }
        rt.emit(g, "doubleClicked", &["mouse"], vec![ev.clone()]);
        rt.settle();
        self.double_click = connected || accepted(&ev);
    }

    /// Time left until the press-and-hold timer fires, in ms.
    pub fn hold_due(&self, rt: &Runtime) -> Option<f64> {
        let hold = self.hold.zip(self.grab).map(|(t, (g, _))| {
            (hold_interval(rt, g) - t.elapsed().as_secs_f64() * 1000.0).max(0.0)
        });
        let wheel = self
            .wheel_active
            .iter()
            .map(|&(h, t)| {
                let ms = wheel_timeout(&mut rt.world.borrow_mut(), h);
                (ms - t.elapsed().as_secs_f64() * 1000.0).max(0.0)
            })
            .reduce(f64::min);
        [hold, wheel].into_iter().flatten().reduce(f64::min)
    }

    /// A released swipe is still moving its pages; it steps once per frame.
    pub fn swipe_animating(&self) -> bool {
        !self.swipe_anims.is_empty()
    }

    /// QQuickWheelHandler::timerEvent: the deactivation timer ran out.
    fn wheel_timeouts(&mut self, rt: &mut Runtime) {
        let mut k = 0;
        while k < self.wheel_active.len() {
            let (h, t) = self.wheel_active[k];
            let dead = rt.world.borrow().dead.contains(&h);
            let ms = if dead {
                0.0
            } else {
                wheel_timeout(&mut rt.world.borrow_mut(), h)
            };
            if t.elapsed().as_secs_f64() * 1000.0 < ms {
                k += 1;
                continue;
            }
            self.wheel_active.remove(k);
            if !dead {
                put(rt, h, "active", Value::Bool(false));
                rt.settle();
            }
        }
    }

    /// QQuickMouseArea::timerEvent: emit pressAndHold once the interval elapses while pressed inside.
    pub fn tick(&mut self, rt: &mut Runtime) {
        if !self.wheel_active.is_empty() {
            self.wheel_timeouts(rt);
        }
        if !self.swipe_anims.is_empty() {
            self.swipe_animate(rt);
        }
        let due = self
            .hold
            .zip(self.grab)
            .map(|(t, (g, _))| hold_interval(rt, g) - t.elapsed().as_secs_f64() * 1000.0);
        if due.is_none_or(|ms| ms > 0.0) {
            return;
        }
        self.hold = None;
        let Some((g, b)) = self.grab else { return };
        let (lx, ly, inside) = self.local(rt, g);
        if !inside {
            return;
        }
        let ev = self.mouse_event(lx, ly, b);
        rt.emit(g, "pressAndHold", &["mouse"], vec![ev.clone()]);
        self.long_press = accepted(&ev);
        rt.settle();
    }

    /// wl_touch.down: the first touch point acts as the left mouse button, the way Qt Quick
    /// synthesizes mouse events from an unaccepted touch press. Further fingers are ignored
    /// until it lifts. Returns whether this point drives the pointer.
    pub fn touch_down(&mut self, rt: &mut Runtime, win: usize, id: i32, x: f64, y: f64) -> bool {
        if self.touch.is_some() {
            return false;
        }
        self.touch = Some(id);
        if self.win != Some(win) {
            self.enter(rt, win, x, y);
        }
        self.motion(rt, win, x, y);
        self.press(rt, BTN_LEFT);
        true
    }

    /// wl_touch.motion of the driving point.
    pub fn touch_motion(&mut self, rt: &mut Runtime, id: i32, x: f64, y: f64) {
        if let (Some(t), Some(win)) = (self.touch, self.win)
            && t == id
        {
            self.motion(rt, win, x, y);
        }
    }

    /// wl_touch.up of the driving point: release, then the finger no longer hovers anything
    /// (QQuickDeliveryAgent clears hover for touch-synthesized presses).
    pub fn touch_up(&mut self, rt: &mut Runtime, id: i32) -> bool {
        if self.touch != Some(id) {
            return false;
        }
        self.touch = None;
        self.release(rt, BTN_LEFT);
        self.leave(rt);
        true
    }

    /// wl_touch.cancel (a compositor gesture took the sequence): the press is cancelled, no
    /// click.
    pub fn touch_cancel(&mut self, rt: &mut Runtime) -> bool {
        if self.touch.take().is_none() {
            return false;
        }
        self.release_lost(rt, BTN_LEFT);
        self.leave(rt);
        true
    }

    /// The seat lost its pointer capability (mouse unplugged, virtual pointer destroyed): the
    /// wl_pointer is gone and no release or leave follows, so held presses are cancelled (no
    /// click) and hover ends. A touch sequence driving the pointer is not the wl_pointer's.
    pub fn pointer_lost(&mut self, rt: &mut Runtime) {
        if self.touch.is_some() {
            return;
        }
        for code in BTN_LEFT..BTN_LEFT + 5 {
            if self.buttons & qt_button(code) != 0 {
                self.release_lost(rt, code);
            }
        }
        self.cancel(rt);
        self.leave(rt);
    }

    /// A button went up on a surface that is gone (e.g. the press hid its own window): Qt
    /// ungrabs the item (QQuickMouseArea::ungrabMouse / QQuickAbstractButton::mouseUngrabEvent),
    /// emitting canceled() instead of released()/clicked().
    pub fn release_lost(&mut self, rt: &mut Runtime, code: u32) {
        let b = qt_button(code);
        self.buttons &= !b;
        if b == self.handler_button {
            self.handlers_end(rt);
        }
        if b == 1 {
            self.swipe_release(rt);
        }
        if b == 1
            && let Some(f) = self.flick.take()
        {
            self.flick_grab = false;
            rt.world.borrow_mut().flick_cancel(f);
            rt.settle();
        }
        if self.grab.is_some_and(|(_, gb)| gb == b) {
            self.cancel(rt);
        }
    }

    fn cancel(&mut self, rt: &mut Runtime) {
        let Some((g, _)) = self.grab.take() else {
            return;
        };
        if self.link.take().is_some() {
            return;
        }
        if crate::scene::is_text_edit(&rt.world.borrow(), g) {
            self.text.ungrab();
            return;
        }
        self.hold = None;
        self.long_press = false;
        self.double_click = false;
        if self.drag.take().is_some_and(|d| d.active) {
            set_drag_active(rt, g, false);
        }
        put(rt, g, "pressed", Value::Bool(false));
        if mouse_area(&rt.world.borrow(), g) {
            put(rt, g, "pressedButtons", Value::Num(0.0));
            put(rt, g, "containsPress", Value::Bool(false));
        } else {
            put(rt, g, "down", Value::Bool(false));
        }
        rt.emit(g, "canceled", &[], Vec::new());
        rt.settle();
    }

    pub fn release(&mut self, rt: &mut Runtime, code: u32) {
        let b = qt_button(code);
        self.buttons &= !b;
        if qml::log_on!("ri::ptr") {
            tracing::debug!(target: "ri::ptr", "ptr release {b} grab {:?}", self.grab);
        }
        // Grabbers in deliverUpdatedPoints order: the handler's activeChanged precedes released.
        if b == self.handler_button {
            self.handlers_end(rt);
        }
        if b == 1 {
            self.swipe_release(rt);
        }
        self.track();
        // The Flickable filters the release before the grabber sees it.
        if b == 1
            && let Some(f) = self.flick.take()
        {
            let grabbed = std::mem::take(&mut self.flick_grab);
            let own = grabbed || self.grab.is_none();
            if own && rt.world.borrow().scroll_view_of(f).is_some() {
                // QQuickScrollView::childMouseEventFilter: no release for its own Flickable.
                rt.world.borrow_mut().flick_cancel(f);
            } else {
                let (lx, ly, _) = self.local(rt, f);
                rt.world
                    .borrow_mut()
                    .flick_release(f, (lx, ly), self.time, grabbed);
            }
            rt.settle();
            if grabbed {
                return;
            }
        }
        let Some((g, gb)) = self.grab else { return };
        if gb != b {
            return;
        }
        self.grab = None;
        self.hold = None;
        if crate::scene::is_text_doc(&rt.world.borrow(), g) {
            if b == 4 {
                let (lx, ly, _) = self.local(rt, g);
                self.text.middle_release(rt, g, lx, ly);
                return;
            }
            self.text.release(rt, g, self.time);
            let pressed = self.link.take();
            if b == 1 {
                // QQuickTextControlPrivate::mouseReleaseEvent: a release over a link with
                // nothing selected activates the link of the press, else the one under it.
                let (lx, ly, _) = self.local(rt, g);
                let selected = {
                    let mut w = rt.world.borrow_mut();
                    num(&mut w, g, "selectionStart") != num(&mut w, g, "selectionEnd")
                };
                if !selected && let Some(at) = self.link_at(rt, g, lx, ly) {
                    let link = pressed.unwrap_or(at);
                    rt.emit(g, "linkActivated", &["link"], vec![Value::str(&link)]);
                    rt.settle();
                }
                crate::keys::export_primary(rt, g);
            }
            return;
        }
        if let Some(link) = self.link.take() {
            let (lx, ly, _) = self.local(rt, g);
            if self.link_at(rt, g, lx, ly).as_deref() == Some(link.as_str()) {
                rt.emit(g, "linkActivated", &["link"], vec![Value::str(&link)]);
            }
            rt.settle();
            return;
        }
        if b == 4 && crate::scene::is_text_edit(&rt.world.borrow(), g) {
            let (lx, ly, _) = self.local(rt, g);
            self.text.middle_release(rt, g, lx, ly);
            return;
        }
        if crate::scene::is_text_edit(&rt.world.borrow(), g) {
            self.text.release(rt, g, self.time);
            if b == 1 {
                crate::keys::export_primary(rt, g);
            }
            return;
        }
        let held = std::mem::take(&mut self.long_press);
        let (lx, ly, inside) = self.local(rt, g);
        if mouse_area(&rt.world.borrow(), g) {
            put(rt, g, "pressed", Value::Bool(false));
            put(rt, g, "pressedButtons", Value::Num(0.0));
            put(rt, g, "containsPress", Value::Bool(false));
            let ev = self.mouse_event(lx, ly, b);
            rt.emit(g, "released", &["mouse"], vec![ev]);
            let dragged = self.drag.take().is_some_and(|d| d.active);
            let double = std::mem::take(&mut self.double_click);
            if inside && !held && !dragged && !double {
                let ev = self.mouse_event(lx, ly, b);
                rt.emit(g, "clicked", &["mouse"], vec![ev]);
            }
            // QQuickMouseArea::mouseReleaseEvent drops hover only after released/clicked, whose
            // bindings (e.g. Behavior.enabled on containsMouse) evaluate before that.
            if dragged {
                // QQuickMouseArea::mouseReleaseEvent: drag->setActive(false) after released().
                rt.settle();
                set_drag_active(rt, g, false);
            }
            rt.settle();
            let hover = self.hovered.contains(&g);
            put(rt, g, "containsMouse", Value::Bool(hover));
            self.deliver_drop(rt);
        } else {
            if is(&rt.world.borrow(), g, "SpinBox") {
                spin_release(rt, g, inside);
                return;
            }
            if scrollbar(&mut rt.world.borrow_mut(), g) {
                // QQuickScrollBarPrivate::handleRelease: a release on the track moves it once more.
                let (lx, ly, _) = self.local(rt, g);
                let inside = scrollbar_on_track(&mut rt.world.borrow_mut(), g, (lx, ly));
                if inside {
                    scrollbar_drag(rt, g, (lx, ly), self.bar_offset);
                }
                self.bar_offset = 0.0;
                put(rt, g, "pressed", Value::Bool(false));
                rt.settle();
                return;
            }
            if slider(&rt.world.borrow(), g) {
                // QQuickSliderPrivate::handleRelease: positionAt() before setPressed(false).
                let (lx, _, _) = self.local(rt, g);
                slider_drag(rt, g, lx, true);
                rt.settle();
                put(rt, g, "pressed", Value::Bool(false));
                rt.settle();
                return;
            }
            put(rt, g, "pressed", Value::Bool(false));
            put(rt, g, "down", Value::Bool(false));
            rt.emit(g, "released", &[], Vec::new());
            let double = std::mem::take(&mut self.double_click);
            if inside {
                // QQuickAbstractButtonPrivate::handleRelease toggles before clicked().
                if checkable(&mut rt.world.borrow_mut(), g) {
                    let c = rt
                        .world
                        .borrow_mut()
                        .read(g, "checked")
                        .is_some_and(|v| v.truthy());
                    let radio = ["RadioButton", "TabButton"]
                        .iter()
                        .any(|t| is(&rt.world.borrow(), g, t));
                    if !(radio && c) {
                        put(rt, g, "checked", Value::Bool(!c));
                        rt.emit(g, "toggled", &[], Vec::new());
                    }
                }
                if !double {
                    trigger_action(rt, g);
                    rt.emit(g, "clicked", &[], Vec::new());
                    menu_item_triggered(rt, g);
                    combo_item_clicked(rt, g);
                }
                // QQuickComboBox::handleRelease → togglePopup.
                let combo = is(&rt.world.borrow(), g, "ComboBox");
                let pop = if combo {
                    rt.world.borrow_mut().read(g, "popup")
                } else {
                    None
                };
                if let Some(Value::Item(p)) = pop {
                    if truthy(&mut rt.world.borrow_mut(), p, "visible", false) {
                        close_popup(rt, p);
                    } else {
                        open_popup(rt, p);
                    }
                }
            }
        }
        rt.settle();
    }

    /// A mouse wheel event: [`Pointer::wheel_axis`] with a wheel as the axis source.
    /// `angle` is Qt's angleDelta (120 per notch, positive = away from the user).
    /// `time` is (event timestamp, animation clock), both in ms.
    pub fn wheel(
        &mut self,
        rt: &mut Runtime,
        angle: (f64, f64),
        pixel: (f64, f64),
        time: (f64, f64),
    ) {
        self.wheel_axis(
            rt,
            &Axis {
                angle,
                pixel,
                time,
                synthesized: false,
                inverted: false,
            },
        );
    }

    /// QWheelEvent delivery for one (merged) axis frame.
    pub fn wheel_axis(&mut self, rt: &mut Runtime, a: &Axis) {
        let Axis {
            angle, pixel, time, ..
        } = *a;
        let Some(win) = self.win else { return };
        let hs = hits(&mut rt.world.borrow_mut(), win, self.pos);
        for h in &hs {
            // QQuickDeliveryAgentPrivate::deliverSinglePointEventUntilAccepted: an item's pointer
            // handlers see the event before the item itself.
            if self.wheel_handlers(rt, h, a) {
                return;
            }
            let ok = {
                let mut w = rt.world.borrow_mut();
                mouse_area(&w, h.item)
                    && w.items[h.item].has_handler("onWheel")
                    && enabled(&mut w, h.item)
            };
            if !ok {
                continue;
            }
            let pt =
                |x: f64, y: f64| Runtime::js_object(&[("x", Value::Num(x)), ("y", Value::Num(y))]);
            let ev = Runtime::js_object(&[
                ("x", Value::Num(h.x)),
                ("y", Value::Num(h.y)),
                ("angleDelta", pt(angle.0, angle.1)),
                ("pixelDelta", pt(pixel.0, pixel.1)),
                ("buttons", Value::Num(self.buttons as f64)),
                ("modifiers", Value::Num(f64::from(self.mods))),
                ("inverted", Value::Bool(a.inverted)),
                ("accepted", Value::Bool(true)),
            ]);
            rt.emit(h.item, "wheel", &["wheel"], vec![ev.clone()]);
            rt.settle();
            if accepted(&ev) {
                return;
            }
        }
        // Unhandled: the innermost interactive Flickable scrolls (QQuickFlickable::wheelEvent);
        // one that does not consume it passes it to the next.
        let targets: Vec<usize> = {
            let mut w = rt.world.borrow_mut();
            hs.iter()
                .map(|h| h.item)
                .filter(|&i| {
                    crate::scene::is_flickable(&w, i)
                        && (!explicit(&w, i, "interactive")
                            || truthy(&mut w, i, "interactive", true))
                })
                .collect()
        };
        if qml::log_on!("ri::ptr") {
            tracing::debug!(target: "ri::ptr", "wheel flickables {targets:?}");
        }
        for f in targets {
            let ok = rt.world.borrow_mut().flick_wheel(f, angle, time.0);
            rt.settle();
            if ok {
                break;
            }
        }
    }
}

impl Pointer {
    /// QQuickWheelHandler::wantsPointerEvent + handleEventPoint for the WheelHandlers of `h`;
    /// true once a blocking one accepted the point.
    fn wheel_handlers(&mut self, rt: &mut Runtime, h: &Hit, a: &Axis) -> bool {
        let (angle, pixel) = (a.angle, a.pixel);
        let list = handlers_of(&rt.world.borrow(), h.item, "WheelHandler");
        let mut accepted = false;
        for wh in list {
            let (wants, horizontal, blocking, invertible) = {
                let mut w = rt.world.borrow_mut();
                // PointerDevice.Mouse (1) | TouchPad (4); the default is Mouse only, which
                // takes only non-synthesized events: those of a wheel, by the axis source.
                let devices = if explicit(&w, wh, "acceptedDevices") {
                    num(&mut w, wh, "acceptedDevices") as u32
                } else {
                    1
                };
                let synthesized = a.synthesized;
                let horizontal =
                    explicit(&w, wh, "orientation") && num(&mut w, wh, "orientation") == 1.0;
                let active = truthy(&mut w, wh, "active", false);
                let along = if horizontal {
                    angle.0 != 0.0 || pixel.0 != 0.0
                } else {
                    angle.1 != 0.0 || pixel.1 != 0.0
                };
                let wants = devices & 1 != 0
                    && (devices & 4 != 0 || !synthesized)
                    && (active || along)
                    && enabled(&mut w, wh);
                (
                    wants,
                    horizontal,
                    truthy(&mut w, wh, "blocking", true),
                    !explicit(&w, wh, "invertible") || truthy(&mut w, wh, "invertible", true),
                )
            };
            if !wants {
                continue;
            }
            // QQuickWheelHandler::handleEventPoint: a handler that is not invertible undoes
            // natural scrolling.
            let inversion = if !invertible && a.inverted { -1.0 } else { 1.0 };
            let delta = inversion * if horizontal { angle.0 } else { angle.1 } / 8.0;
            if truthy(&mut rt.world.borrow_mut(), wh, "active", false) {
                // NoScrollPhase restarts the deactivation timer.
            } else {
                put(rt, wh, "active", Value::Bool(true));
                rt.settle();
            }
            let rot = num(&mut rt.world.borrow_mut(), wh, "rotation");
            put(rt, wh, "rotation", Value::Num(rot + delta));
            let pt =
                |x: f64, y: f64| Runtime::js_object(&[("x", Value::Num(x)), ("y", Value::Num(y))]);
            let ev = Runtime::js_object(&[
                ("x", Value::Num(h.x)),
                ("y", Value::Num(h.y)),
                ("angleDelta", pt(angle.0, angle.1)),
                ("pixelDelta", pt(pixel.0, pixel.1)),
                ("buttons", Value::Num(self.buttons as f64)),
                ("modifiers", Value::Num(f64::from(self.mods))),
                ("inverted", Value::Bool(a.inverted)),
                ("accepted", Value::Bool(true)),
            ]);
            rt.emit(wh, "wheel", &["event"], vec![ev]);
            rt.settle();
            self.wheel_active.retain(|&(x, _)| x != wh);
            self.wheel_active.push((wh, std::time::Instant::now()));
            accepted |= blocking;
        }
        accepted
    }
}

/// SwipeView's Basic-style contentItem: a ListView with SnapOneItem, StrictlyEnforceRange
/// (preferredHighlightBegin/End 0), StopAtBounds and maximumFlickVelocity 4 * its extent.
impl Pointer {
    fn swipe_press(&mut self, rt: &mut Runtime, hs: &[Hit], b: u32) {
        self.swipe = None;
        if b != 1 {
            return;
        }
        let mut w = rt.world.borrow_mut();
        let Some(v) = hs.iter().map(|h| h.item).find(|&i| {
            is(&w, i, "SwipeView")
                && (!explicit(&w, i, "interactive") || truthy(&mut w, i, "interactive", true))
                && enabled(&mut w, i)
        }) else {
            return;
        };
        // A press stops a running flick where it is (QQuickFlickablePrivate::handlePressEvent),
        // and the view stops following a highlight move (moveReason Mouse).
        let base = match self.swipe_anims.iter().position(|a| a.view == v) {
            Some(k) => {
                self.swipe_anims.remove(k);
                crate::scene::swipe_pos(v).unwrap_or(0.0)
            }
            None => {
                let (_, pitch, _, _) = crate::scene::swipe_metrics(&mut w, v);
                let now = w.now;
                let target = num(&mut w, v, "currentIndex").max(0.0);
                let moving = w
                    .swipes
                    .get_mut(&v)
                    .filter(|s| s.running(now))
                    .map(|s| s.pos(now) * pitch);
                if let Some(p) = moving {
                    w.swipes.insert(v, qml::runtime::Swipe::at(target));
                    crate::scene::set_swipe_pos(&mut w, v, Some(p));
                }
                crate::scene::swipe_pos(v).unwrap_or(target * pitch)
            }
        };
        self.swipe = Some(SwipeGrab {
            view: v,
            press: self.pos,
            base,
            stolen: false,
            start: None,
            last_move: self.time,
        });
    }

    /// QQuickFlickablePrivate::handleMoveEvent / drag for the swiping ListView.
    fn swipe_move(&mut self, rt: &mut Runtime) {
        if self.buttons & 1 == 0 {
            return;
        }
        let Some(sg) = self.swipe.as_ref() else {
            return;
        };
        let v = sg.view;
        if rt.world.borrow().dead.contains(&v) {
            self.swipe = None;
            return;
        }
        let (vertical, pitch, _, count) =
            crate::scene::swipe_metrics(&mut rt.world.borrow_mut(), v);
        let d = if vertical {
            self.pos.1 - sg.press.1
        } else {
            self.pos.0 - sg.press.0
        };
        let start = match sg.start {
            Some(s) => s,
            None if sg.stolen => {
                let Some(sg) = self.swipe.as_mut() else {
                    return;
                };
                sg.start = Some(d);
                d
            }
            None => {
                // QQuickDeliveryAgentPrivate::dragOverThreshold (startDragDistance 10).
                if d.abs() <= 10.0 || self.handlers.iter().any(|g| g.active) {
                    return;
                }
                // The Flickable steals the grab unless the grabber keeps it; it only filters its
                // own descendants (QQuickDeliveryAgentPrivate::sendFilteredPointerEvent).
                if let Some((g, _)) = self.grab {
                    if !is_ancestor(&rt.world.borrow(), v, g) {
                        self.swipe = None;
                        return;
                    }
                    let dragging = self.drag.as_ref().is_some_and(|d| d.active);
                    if keeps_grab(&mut rt.world.borrow_mut(), g, dragging) || self.text_keeps(rt, g)
                    {
                        self.swipe = None;
                        return;
                    }
                    self.cancel(rt);
                }
                // An inner Flickable keeps the gesture once it drags; otherwise the SwipeView's
                // list steals it and the inner one ungrabs (QQuickFlickable::filterPointerEvent).
                if let Some(f) = self.flick.filter(|&f| f != v) {
                    if rt.world.borrow().flick_steal(f) {
                        self.swipe = None;
                        return;
                    }
                    rt.world.borrow_mut().flick_cancel(f);
                    rt.settle();
                    self.flick = None;
                    self.flick_grab = false;
                }
                // QQuickFlickablePrivate::drag: this move only steals; the next one is where
                // the content starts from, and it moves with the one after that.
                if let Some(sg) = self.swipe.as_mut() {
                    sg.stolen = true;
                }
                return;
            }
        };
        let Some(sg) = self.swipe.as_mut() else {
            return;
        };
        let max = (count.max(1) - 1) as f64 * pitch;
        let p = (sg.base - (d - start)).clamp(0.0, max);
        sg.last_move = self.time;
        swipe_to(rt, v, p);
    }

    /// QQuickFlickablePrivate::handleReleaseEvent: flick one page on (QQuickListViewPrivate::flick,
    /// SnapOneItem) or snap back (QQuickListViewPrivate::fixup).
    fn swipe_release(&mut self, rt: &mut Runtime) {
        let Some(sg) = self.swipe.take() else { return };
        if rt.world.borrow().dead.contains(&sg.view) {
            return;
        }
        if sg.start.is_none() {
            // A press stopped a highlight move: QQuickListViewPrivate::fixup snaps to the
            // current page.
            let v = sg.view;
            let Some(p) = crate::scene::swipe_pos(v) else {
                return;
            };
            if self.swipe_anims.iter().any(|a| a.view == v) {
                return;
            }
            let (_, pitch, _, _) = crate::scene::swipe_metrics(&mut rt.world.borrow_mut(), v);
            let to = num(&mut rt.world.borrow_mut(), v, "currentIndex").max(0.0) * pitch;
            if (to - p).abs() > 0.0 {
                self.swipe_anims.push(SwipeAnim {
                    view: v,
                    from: p,
                    to,
                    start: rt.world.borrow().now,
                    dur: 200.0,
                    decel: false,
                });
            } else {
                swipe_rest(rt, v, to, pitch);
            }
            return;
        }
        let v = sg.view;
        let (vertical, pitch, size, count) =
            crate::scene::swipe_metrics(&mut rt.world.borrow_mut(), v);
        if pitch <= 0.0 {
            crate::scene::set_swipe_pos(&mut rt.world.borrow_mut(), v, None);
            return;
        }
        let max = (count.max(1) - 1) as f64 * pitch;
        let p = crate::scene::swipe_pos(v).unwrap_or(sg.base);
        // The pointer's event-time velocity (positive while the content moves towards its
        // origin, like data.move); a pause of 100 ms before release drops it.
        let velocity = if self.time - sg.last_move < 100.0 {
            if vertical { self.vel.1 } else { self.vel.0 }
        } else {
            0.0
        };
        let travel = if vertical {
            self.pos.1 - sg.press.1
        } else {
            self.pos.0 - sg.press.0
        };
        let page = |x: f64| {
            ((x / pitch) + 0.5)
                .floor()
                .clamp(0.0, (count.max(1) - 1) as f64)
        };
        let maxv = 4.0 * size;
        let flick = velocity.abs() > MINIMUM_FLICK_VELOCITY
            && travel.abs() > FLICK_START_DISTANCE
            && ((velocity > 0.0 && p > 0.0) || (velocity < 0.0 && p < max));
        if flick {
            // averageSize / 2 + 1: the next page in the flick direction.
            let bias = size / 2.0 + 1.0;
            let to = page(if velocity < 0.0 { p + bias } else { p - bias }) * pitch;
            let dist = (to - p).abs();
            if dist > 0.0 {
                let vv = maxv.max((2.0 * FLICK_DECELERATION * dist).sqrt());
                self.swipe_anims.push(SwipeAnim {
                    view: v,
                    from: p,
                    to,
                    start: rt.world.borrow().now,
                    dur: 2000.0 * dist / vv,
                    decel: true,
                });
                return;
            }
            swipe_rest(rt, v, to, pitch);
            return;
        }
        let moved = sg.base - p;
        let bias = if velocity > 0.0 && moved > SNAP_ONE_THRESHOLD && moved < size / 2.0 {
            size / 2.0
        } else if velocity < 0.0 && moved < -SNAP_ONE_THRESHOLD && moved > -size / 2.0 {
            -size / 2.0
        } else {
            0.0
        };
        let to = page(p - bias) * pitch;
        if (to - p).abs() > 0.0 {
            self.swipe_anims.push(SwipeAnim {
                view: v,
                from: p,
                to,
                start: rt.world.borrow().now,
                dur: 200.0,
                decel: false,
            });
        } else {
            swipe_rest(rt, v, to, pitch);
        }
    }

    fn swipe_animate(&mut self, rt: &mut Runtime) {
        for a in std::mem::take(&mut self.swipe_anims) {
            if rt.world.borrow().dead.contains(&a.view) {
                continue;
            }
            let t = ((rt.world.borrow().now - a.start) / a.dur).clamp(0.0, 1.0);
            let f = if a.decel {
                // Constant deceleration from the initial speed to rest at `to`.
                1.0 - (1.0 - t) * (1.0 - t)
            } else if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            };
            let p = a.from + (a.to - a.from) * f;
            if t >= 1.0 {
                let (_, pitch, _, _) =
                    crate::scene::swipe_metrics(&mut rt.world.borrow_mut(), a.view);
                swipe_rest(rt, a.view, a.to, pitch);
            } else {
                swipe_to(rt, a.view, p);
                self.swipe_anims.push(a);
            }
        }
    }
}

/// Move the pages to content position `p`; StrictlyEnforceRange makes the page under the
/// highlight (the view's start) current as it passes (QQuickListView::viewportMoved).
fn swipe_to(rt: &mut Runtime, v: usize, p: f64) {
    let (_, pitch, _, count) = crate::scene::swipe_metrics(&mut rt.world.borrow_mut(), v);
    crate::scene::set_swipe_pos(&mut rt.world.borrow_mut(), v, Some(p));
    if pitch <= 0.0 || count == 0 {
        return;
    }
    let k = ((p / pitch) + 0.5).floor().clamp(0.0, (count - 1) as f64);
    if num(&mut rt.world.borrow_mut(), v, "currentIndex") != k {
        put(rt, v, "currentIndex", Value::Num(k));
    }
    rt.settle();
}

fn swipe_rest(rt: &mut Runtime, v: usize, to: f64, pitch: f64) {
    swipe_to(rt, v, to);
    if pitch > 0.0 && (to / pitch).fract() == 0.0 {
        // The pages already rest on the page: no currentIndex move animation from the old one.
        let mut w = rt.world.borrow_mut();
        w.swipes.insert(v, qml::runtime::Swipe::at(to / pitch));
        crate::scene::set_swipe_pos(&mut w, v, None);
    }
}

/// QQuickAbstractButtonPrivate::trigger: the button's action triggers before `clicked`.
pub(crate) fn trigger_action(rt: &mut Runtime, g: usize) {
    let action = match rt.world.borrow_mut().read(g, "action") {
        Some(Value::Item(a)) => Some(a),
        _ => None,
    };
    if let Some(a) = action
        && truthy(&mut rt.world.borrow_mut(), a, "enabled", true)
    {
        rt.emit(a, "triggered", &["source"], vec![Value::Item(g)]);
    }
}

/// QQuickMenuItem emits `triggered` on click, and its QQuickMenu dismisses.
/// QQuickComboBoxPrivate::itemClicked → hidePopup(true): a click on a delegate of the box's
/// `delegateModel` sets `currentIndex` (keeping its binding), emits `activated(index)` and closes
/// the popup.
fn combo_item_clicked(rt: &mut Runtime, g: usize) {
    let hit = {
        let mut w = rt.world.borrow_mut();
        let mut path = vec![g];
        let mut p = w.items[g].parent;
        let mut hit = None;
        while let Some(x) = p {
            if let Some(k) = w
                .delegated
                .get(&x)
                .map(|d| d.1.iter().position(|c| path.contains(c)))
            {
                if let Some(k) = k
                    && let Some(Value::Item(c)) = w.read(x, "model")
                    && is(&w, c, "ComboBox")
                {
                    hit = Some((c, k));
                }
                break;
            }
            path.push(x);
            p = w.items[x].parent;
        }
        hit
    };
    let Some((combo, index)) = hit else { return };
    put(rt, combo, "currentIndex", Value::Num(index as f64));
    rt.settle();
    rt.emit(
        combo,
        "activated",
        &["index"],
        vec![Value::Num(index as f64)],
    );
    rt.settle();
    let pop = rt.world.borrow_mut().read(combo, "popup");
    if let Some(Value::Item(p)) = pop
        && truthy(&mut rt.world.borrow_mut(), p, "visible", false)
    {
        close_popup(rt, p);
    }
}

pub(crate) fn menu_item_triggered(rt: &mut Runtime, g: usize) {
    if !qml::types::inherits(&rt.world.borrow().items[g].types, "QQuickMenuItem") {
        return;
    }
    rt.emit(g, "triggered", &[], Vec::new());
    rt.settle();
    let menu = {
        let w = rt.world.borrow();
        let mut p = w.items[g].parent;
        while let Some(x) = p {
            if qml::types::inherits(&w.items[x].types, "QQuickMenu") {
                break;
            }
            p = w.items[x].parent;
        }
        p
    };
    if let Some(m) = menu {
        rt.world.borrow_mut().popup_set_visible(m, false);
        rt.settle();
    }
}

/// `focusPolicy & Qt.ClickFocus`; QQuickControl defaults input controls to Qt.StrongFocus, plain
/// items (MouseArea included) to Qt.NoFocus.
fn click_focus(w: &mut World, i: usize) -> bool {
    if explicit(w, i, "focusPolicy") {
        return num(w, i, "focusPolicy") as i32 & 2 != 0;
    }
    is_button(&w.items[i].types)
        || [
            "Slider",
            "RangeSlider",
            "ComboBox",
            "SpinBox",
            "Dial",
            "Tumbler",
        ]
        .iter()
        .any(|t| is(w, i, t))
}

/// One merged `wl_pointer` axis frame for `Pointer::wheel_axis`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub angle: (f64, f64),
    pub pixel: (f64, f64),
    pub time: (f64, f64),
    /// QWheelEvent::source() is not MouseEventNotSynthesized: the frame's axis source is not a
    /// wheel ([`wheel_source`]).
    pub synthesized: bool,
    /// QWheelEvent::inverted(): an axis of the frame has `axis_relative_direction` inverted
    /// (natural scrolling).
    pub inverted: bool,
}

/// QWaylandInputDevice::Pointer::FrameData: whether a frame with this `wl_pointer.axis_source`
/// is a mouse wheel's. A frame without the event counts as a wheel (`axisSource` starts as
/// `axis_source_wheel`); finger, continuous and any later source do not, however small the
/// motion. Only the others carry a pixel delta and are synthesized for a WheelHandler.
pub fn wheel_source(source: Option<wayland_client::protocol::wl_pointer::AxisSource>) -> bool {
    use wayland_client::protocol::wl_pointer::AxisSource;
    matches!(
        source,
        None | Some(AxisSource::Wheel | AxisSource::WheelTilt)
    )
}

/// One wl_pointer frame's wheel deltas as QWaylandInputDevice::Pointer::flushScrollEvent
/// reports them: `value120` (with v5-7 `discrete` already scaled by 120) wins over the
/// continuous `absolute` value, which becomes `qRound(-absolute * 12)` (Qt's angleDelta is a
/// QPoint). Pixel deltas are whole pixels too, carrying the rounding error to the next frame
/// (`pixelDeltaAndError`) so slow touchpad scrolls still add up. A frame whose angle rounds to
/// zero is no wheel event at all (Qt drops it; its pixel delta is not accumulated).
#[derive(Debug, Default)]
pub struct WheelDeltas {
    remainder: (f64, f64),
}

impl WheelDeltas {
    /// `(angle, pixel)` for the frame, or None if Qt sends no event for it. `wheel` frames
    /// (mouse wheel clicks) carry no pixel delta.
    pub fn frame(
        &mut self,
        value120: (i32, i32),
        absolute: (f64, f64),
        wheel: bool,
    ) -> Option<((f64, f64), (f64, f64))> {
        let angle = if value120 == (0, 0) {
            ((-absolute.0 * 12.0).round(), (-absolute.1 * 12.0).round())
        } else {
            (-value120.0 as f64, -value120.1 as f64)
        };
        if angle == (0.0, 0.0) {
            return None;
        }
        if wheel {
            return Some((angle, (0.0, 0.0)));
        }
        let px = |d: f64, r: &mut f64| {
            let p = (d + *r).round();
            *r += d - p;
            -p
        };
        let pixel = (
            px(absolute.0, &mut self.remainder.0),
            px(absolute.1, &mut self.remainder.1),
        );
        Some((angle, pixel))
    }

    /// wl_pointer.axis_stop: the scroll sequence ended (Qt resets the remainder at ScrollEnd).
    pub fn end(&mut self) {
        self.remainder = (0.0, 0.0);
    }
}

/// QWaylandEventCompressionPrivate::compressEvent: a pointer frame within 100 µs of the previous
/// one, or while a merged frame waits, was queued behind a busy main thread; its axis deltas add
/// to that frame (the last frame's timestamp wins), delivered once the queue is drained.
#[derive(Debug, Default)]
pub struct AxisCompression {
    last: Option<std::time::Instant>,
    pending: Option<Axis>,
}

impl AxisCompression {
    /// A pointer frame arrived at `at`: whether its axis events are merged instead of delivered.
    pub fn frame(&mut self, at: std::time::Instant) -> bool {
        let merge = self.pending.is_some()
            || self.last.is_some_and(|l| {
                at.saturating_duration_since(l) < std::time::Duration::from_micros(100)
            });
        self.last = Some(at);
        merge
    }

    pub fn merge(&mut self, a: Axis) {
        match &mut self.pending {
            Some(p) => {
                p.angle = (p.angle.0 + a.angle.0, p.angle.1 + a.angle.1);
                p.pixel = (p.pixel.0 + a.pixel.0, p.pixel.1 + a.pixel.1);
                p.time = a.time;
                p.synthesized = a.synthesized;
                p.inverted = a.inverted;
            }
            None => self.pending = Some(a),
        }
    }

    /// The merged frame, once the event queue is drained (Qt's 0 ms `delayTimer`).
    pub fn take(&mut self) -> Option<Axis> {
        self.pending.take()
    }
}

/// The same rule for motions: a held-back motion replaces the one that waits (Qt's
/// `setFrameEvent`), so of a burst the first reaches the scene at once and the last once the
/// queue is drained. `M` is what the motion carries.
#[derive(Debug)]
pub struct MotionCompression<M> {
    last: Option<std::time::Instant>,
    pending: Option<M>,
}

impl<M> Default for MotionCompression<M> {
    fn default() -> Self {
        Self {
            last: None,
            pending: None,
        }
    }
}

impl<M> MotionCompression<M> {
    /// A pointer frame arrived at `at`: whether its motion waits instead of being delivered.
    pub fn frame(&mut self, at: std::time::Instant) -> bool {
        let hold = self.pending.is_some()
            || self.last.is_some_and(|l| {
                at.saturating_duration_since(l) < std::time::Duration::from_micros(100)
            });
        self.last = Some(at);
        hold
    }

    pub fn hold(&mut self, m: M) {
        self.pending = Some(m);
    }

    pub fn waits(&self) -> bool {
        self.pending.is_some()
    }

    /// The motion that waits: before an event of another kind, and once the queue is drained.
    pub fn take(&mut self) -> Option<M> {
        self.pending.take()
    }
}

#[cfg(test)]
mod axis_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Touchpad (finger) frames: Qt's angleDelta is `qRound(-v * 12)` and its pixelDelta whole
    /// pixels with the rounding error carried; frames that round to no angle send nothing.
    #[test]
    fn continuous_scroll_deltas_are_whole_like_qt() {
        let mut w = WheelDeltas::default();
        assert_eq!(
            w.frame((0, 0), (0.0, 0.3), false),
            Some(((0.0, -4.0), (0.0, -0.0)))
        );
        assert_eq!(
            w.frame((0, 0), (0.0, 1.25), false),
            Some(((0.0, -15.0), (0.0, -2.0)))
        );
        // -0.375 plus the carried -0.45 rounds to -1: the pixels follow the summed motion.
        assert_eq!(
            w.frame((0, 0), (0.0, -0.375), false),
            Some(((0.0, 5.0), (0.0, 1.0)))
        );
        // -0.24° rounds to 0: Qt sends no wheel event (and does not accumulate its pixels).
        assert_eq!(w.frame((0, 0), (0.02, 0.0), false), None);
        w.end();
        assert_eq!(
            w.frame((0, 0), (0.0, 0.5), false),
            Some(((0.0, -6.0), (0.0, -1.0)))
        );
        assert_eq!(
            w.frame((0, 0), (0.0, 0.5), false),
            Some(((0.0, -6.0), (0.0, -0.0)))
        );
    }

    /// A finger moving right or down on a touchpad (positive `wl_pointer.axis` values) is a
    /// negative angleDelta and pixelDelta on that axis in Qt, and the other way round: both
    /// axes take the same sign.
    #[test]
    fn horizontal_touchpad_scroll_has_the_sign_of_vertical() {
        // A finger's frames and the WheelEvent (angleDelta, pixelDelta) Qt 6.12 makes of each:
        // the `pad` steps of tests/interact, which run them through Quickshell.
        let mut w = WheelDeltas::default();
        for (absolute, angle, pixel) in [
            ((2.5, 0.0), (-30.0, 0.0), (-3.0, 0.0)),
            ((-5.0, 0.0), (60.0, 0.0), (6.0, 0.0)),
            ((0.0, 2.5), (0.0, -30.0), (-1.0, -3.0)),
            ((-2.5, 5.0), (30.0, -60.0), (3.0, -5.0)),
        ] {
            assert_eq!(
                w.frame((0, 0), absolute, false),
                Some((angle, pixel)),
                "a finger moved by {absolute:?}"
            );
        }
    }

    #[test]
    fn wheel_clicks_use_value120_and_carry_no_pixels() {
        let mut w = WheelDeltas::default();
        assert_eq!(
            w.frame((0, 120), (0.0, 15.0), true),
            Some(((-0.0, -120.0), (0.0, 0.0)))
        );
        // High-resolution wheel: value120 wins over the continuous value on both axes.
        assert_eq!(
            w.frame((0, 30), (0.4, 3.75), true),
            Some(((-0.0, -30.0), (0.0, 0.0)))
        );
    }

    /// QWaylandInputDevice::Pointer::FrameData::wheelEventSource and hasPixelDelta.
    #[test]
    fn axis_source_tells_a_wheel_from_a_touchpad() {
        use wayland_client::protocol::wl_pointer::AxisSource;
        assert!(wheel_source(None));
        assert!(wheel_source(Some(AxisSource::Wheel)));
        assert!(wheel_source(Some(AxisSource::WheelTilt)));
        assert!(!wheel_source(Some(AxisSource::Finger)));
        assert!(!wheel_source(Some(AxisSource::Continuous)));
    }

    /// Motions of frames that arrived `us` after the test began, all in one pass of the
    /// queue: the ones that reached the scene, in order.
    fn motions_delivered(us: &[u64]) -> Vec<usize> {
        let t0 = Instant::now();
        let mut c = MotionCompression::default();
        let mut out = Vec::new();
        for (i, us) in us.iter().enumerate() {
            if c.frame(t0 + Duration::from_micros(*us)) {
                c.hold(i);
            } else {
                out.push(i);
            }
        }
        out.extend(c.take());
        out
    }

    /// QWaylandEventCompressionPrivate::compressEvent, `elapsed < 100us`: frames 99 µs apart
    /// merge into the first and the last, frames 101 µs apart all reach the scene, and after
    /// one frame was held the following ones wait however late they come (Qt's `delayTimer`
    /// is active until the queue is drained).
    #[test]
    fn motion_frames_merge_under_100_us_like_qt_wayland() {
        let every = |gap: u64| (0..20).map(|i| i * gap).collect::<Vec<_>>();
        assert_eq!(motions_delivered(&every(99)), [0, 19]);
        assert_eq!(motions_delivered(&every(100)), (0..20).collect::<Vec<_>>());
        assert_eq!(motions_delivered(&every(101)), (0..20).collect::<Vec<_>>());
        // 101, 99, 101 and 5000 µs after the frame before.
        assert_eq!(motions_delivered(&[0, 101, 200, 301, 5301]), [0, 1, 4]);
    }

    #[test]
    fn queued_axis_frames_merge_like_qt_wayland() {
        // 5 wheel notches; the 3rd and 4th were queued behind a busy main thread (50 µs apart):
        // qs -c impulse-shell delivers angleDelta -120, -120, -240, -120.
        let t0 = Instant::now();
        let at = [0, 30_000, 30_050, 30_100, 120_000].map(|us| t0 + Duration::from_micros(us));
        let mut c = AxisCompression::default();
        let mut out = Vec::new();
        for (i, at) in at.into_iter().enumerate() {
            let a = Axis {
                angle: (0.0, -120.0),
                pixel: (0.0, -15.0),
                time: (i as f64, i as f64),
                synthesized: false,
                inverted: false,
            };
            if c.frame(at) {
                c.merge(a);
            } else {
                out.push(a);
            }
            if i == 3 {
                // The queue drained: the delay timer delivers the merged frame.
                out.extend(c.take());
            }
        }
        let got: Vec<_> = out
            .iter()
            .map(|a| (a.angle.1, a.pixel.1, a.time.0))
            .collect();
        assert_eq!(
            got,
            [
                (-120.0, -15.0, 0.0),
                (-120.0, -15.0, 1.0),
                (-240.0, -30.0, 3.0),
                (-120.0, -15.0, 4.0)
            ]
        );
    }
}

#[cfg(all(test, feature = "interp"))]
mod tests {
    use super::*;

    #[test]
    fn combo_delegate_click_activates_and_closes() {
        let d = std::env::temp_dir().join(format!("shell-combo-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("main.qml"),
            r#"import QtQuick
                import QtQuick.Controls
                Item {
                    id: root
                    property int sel: 0
                    property int got: -1
                    property alias box: cb
                    ComboBox {
                        id: cb
                        model: ["Headset", "Speakers", "HDMI"]
                        currentIndex: root.sel
                        onActivated: (index) => root.got = index
                        delegate: ItemDelegate { required property int index; width: 100; height: 40 }
                        popup: Popup {
                            contentItem: ListView { model: cb.delegateModel; implicitHeight: 120 }
                        }
                    }
                }"#,
        )
        .unwrap();
        let mut rt = Runtime::new(Some(d.clone()));
        let root = rt.load_file(&d.join("main.qml")).unwrap();
        rt.settle();
        let Value::Item(cb) = rt.get(root, "box") else {
            panic!("no box")
        };
        let Value::Item(pop) = rt.get(cb, "popup") else {
            panic!("no popup")
        };
        open_popup(&mut rt, pop);
        let ds = rt.world.borrow_mut().of_type("ItemDelegate");
        assert_eq!(ds.len(), 3);
        combo_item_clicked(&mut rt, ds[2]);
        assert_eq!(rt.get(root, "got").to_number(), 2.0);
        assert_eq!(rt.get(cb, "currentIndex").to_number(), 2.0);
        assert!(!rt.get(pop, "visible").truthy());
        // QQuickComboBox::setCurrentIndex keeps the QML binding.
        rt.set(root, "sel", Value::Num(1.0));
        assert_eq!(rt.get(cb, "currentIndex").to_number(), 1.0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// QQuickPopupItem accepts a press: one on the popup's padding does not reach what is
    /// under the popup, one outside the popup does (and closes a ComboBox's popup).
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn a_press_on_a_popups_padding_stays_in_the_popup() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
                import QtQuick.Controls
                Item {
                    id: root
                    width: 300; height: 300
                    property int under: 0
                    property alias box: cb
                    MouseArea { anchors.fill: parent; onPressed: root.under++ }
                    ComboBox {
                        id: cb
                        x: 50; y: 20; width: 200; height: 40
                        model: ["Headset", "Speakers", "HDMI"]
                        delegate: ItemDelegate { required property int index; width: 184; height: 40 }
                        popup: Popup {
                            y: cb.height; width: cb.width; height: 136; padding: 8
                            contentItem: ListView { model: cb.popup.visible ? cb.delegateModel : null }
                        }
                    }
                }"#,
            )
            .unwrap();
        rt.settle();
        let Value::Item(cb) = rt.get(root, "box") else {
            panic!("no box")
        };
        let Value::Item(pop) = rt.get(cb, "popup") else {
            panic!("no popup")
        };
        open_popup(&mut rt, pop);
        let mut fonts = crate::scene::Fonts::default();
        crate::scene::layout_window(&mut rt, &mut fonts, root, [300.0, 300.0]);
        let mut ptr = Pointer::default();
        ptr.enter(&mut rt, root, 150.0, 192.0);
        let mut click = |rt: &mut Runtime, y: f64| {
            ptr.motion(rt, root, 150.0, y);
            ptr.press(rt, 0x110);
            ptr.release(rt, 0x110);
            (
                rt.get(root, "under").to_number(),
                rt.get(pop, "visible").truthy(),
            )
        };
        // The popup is at y 60..196, its rows end at 188.
        assert_eq!(click(&mut rt, 192.0), (0.0, true), "on the padding");
        assert_eq!(click(&mut rt, 250.0), (1.0, false), "outside the popup");
    }
}

#[cfg(test)]
mod scroll_view_mouse_tests {
    use super::*;

    fn drag(rt: &mut Runtime, ptr: &mut Pointer, root: usize, from: (f64, f64), by: (f64, f64)) {
        ptr.motion(rt, root, from.0 - 1.0, from.1);
        ptr.motion(rt, root, from.0, from.1);
        ptr.press(rt, 0x110);
        for k in 1..=20 {
            let k = f64::from(k);
            ptr.motion(rt, root, from.0 + by.0 * k / 20.0, from.1 + by.1 * k / 20.0);
        }
        ptr.release(rt, 0x110);
    }

    /// QQuickScrollView::childMouseEventFilter: with a mouse the moves and the release never
    /// reach the ScrollView's Flickable, so a drag does not move its content. The Flickable
    /// still filters a child's moves: it takes the grab of a MouseArea (canceled) once the drag
    /// is past the threshold, and leaves a TextArea that selects by mouse alone. Recorded with
    /// Qt 6.12 (the plain Flickable follows the drag).
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn scroll_view_content_is_not_dragged_by_the_mouse_like_qt() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 520
    property string log: ""
    property real aMax: 0
    property real bMax: 0
    property real cMax: 0
    property real dMax: 0
    property string sel: field.selectedText
    ScrollView {
        id: a
        width: 300; height: 140
        Column { Repeater { model: 20; Text { text: "row " + index; height: 20 } } }
    }
    ScrollView {
        id: b
        y: 150; width: 300; height: 140
        MouseArea {
            implicitWidth: 300; implicitHeight: 400
            onPressed: root.log += "p"
            onReleased: root.log += "r"
            onCanceled: root.log += "c"
        }
    }
    Flickable {
        id: c
        y: 300; width: 300; height: 140
        contentWidth: 300; contentHeight: 400
        Rectangle { width: 300; height: 400 }
    }
    ScrollView {
        id: d
        y: 450; width: 300; height: 40
        TextArea {
            id: field
            anchors.fill: parent
            implicitWidth: 180
            padding: 10
            font.pixelSize: 16
            wrapMode: TextArea.Wrap
            background: null
            text: "alpha beta gamma"
        }
    }
    Connections { target: a.contentItem; function onContentYChanged() { root.aMax = Math.max(root.aMax, Math.abs(a.contentItem.contentY)) } }
    Connections { target: b.contentItem; function onContentYChanged() { root.bMax = Math.max(root.bMax, Math.abs(b.contentItem.contentY)) } }
    Connections { target: c; function onContentYChanged() { root.cMax = Math.max(root.cMax, Math.abs(c.contentY)) } }
    Connections { target: d.contentItem; function onContentXChanged() { root.dMax = Math.max(root.dMax, Math.abs(d.contentItem.contentX)) } }
}"#,
            )
            .unwrap();
        rt.settle();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        let n = |rt: &Runtime, p: &str| rt.get(root, p).to_number();
        let mut ptr = Pointer::default();
        let fonts = ptr.fonts.clone();
        let lay = |rt: &mut Runtime| {
            crate::scene::layout_window(rt, &mut fonts.borrow_mut(), root, [300.0, 520.0]);
            rt.settle();
        };
        lay(&mut rt);
        lay(&mut rt);
        ptr.enter(&mut rt, root, 150.0, 100.0);
        drag(&mut rt, &mut ptr, root, (150.0, 100.0), (0.0, -60.0));
        lay(&mut rt);
        drag(&mut rt, &mut ptr, root, (150.0, 250.0), (0.0, -60.0));
        lay(&mut rt);
        drag(&mut rt, &mut ptr, root, (150.0, 400.0), (0.0, -60.0));
        lay(&mut rt);
        drag(&mut rt, &mut ptr, root, (20.0, 470.0), (80.0, 0.0));
        let sel = rt.get(root, "sel").to_js_string();
        lay(&mut rt);
        ptr.motion(&mut rt, root, 20.0, 470.0);
        ptr.press(&mut rt, 0x110);
        ptr.motion(&mut rt, root, 60.0, 470.0);
        assert_eq!(ptr.cursor_shape(&rt), 4, "no I-beam in a selecting drag");
        ptr.release(&mut rt, 0x110);
        assert_eq!(
            n(&rt, "aMax"),
            0.0,
            "a ScrollView's content followed the mouse"
        );
        assert_eq!(
            n(&rt, "bMax"),
            0.0,
            "a ScrollView's content followed the mouse"
        );
        assert_eq!(rt.get(root, "log").to_js_string(), "pc");
        assert!(n(&rt, "cMax") > 30.0, "a plain Flickable follows the mouse");
        assert_eq!(
            n(&rt, "dMax"),
            0.0,
            "the field's text slid under a selecting drag"
        );
        assert!(sel.len() >= 8, "the drag selected only {sel:?}");
    }
}

#[cfg(test)]
mod edit_link_tests {
    use super::*;

    /// QQuickTextControl: a TextEdit always has Qt::LinksAccessibleByMouse, so a read-only one
    /// that is not selectable still tracks `hoveredLink`, keeps the left press and emits
    /// linkActivated on the release over a link.
    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn read_only_text_area_hovers_and_activates_its_links_like_qt() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 100
    property string got: ""
    property string hov: "-"
    property int below: 0
    MouseArea { anchors.fill: parent; onPressed: root.below++ }
    TextArea {
        width: 300; height: 40
        padding: 0
        readOnly: true
        selectByMouse: false
        textFormat: TextEdit.MarkdownText
        font.pixelSize: 16
        text: "[link](https://example.com/a) and a long tail of plain text"
        onLinkActivated: link => root.got = link
        onHoveredLinkChanged: root.hov = hoveredLink
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.NoButton; hoverEnabled: true }
    }
}"#,
            )
            .unwrap();
        rt.settle();
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
        let s = |rt: &Runtime, n: &str| rt.get(root, n).to_js_string();
        let mut ptr = Pointer::default();
        ptr.enter(&mut rt, root, 8.0, 10.0);
        ptr.motion(&mut rt, root, 8.0, 10.0);
        assert_eq!(s(&rt, "hov"), "https://example.com/a");
        ptr.press(&mut rt, 0x110);
        ptr.release(&mut rt, 0x110);
        assert_eq!(s(&rt, "got"), "https://example.com/a");
        rt.set(root, "got", Value::str(""));
        ptr.motion(&mut rt, root, 200.0, 10.0);
        assert_eq!(s(&rt, "hov"), "");
        ptr.press(&mut rt, 0x110);
        ptr.release(&mut rt, 0x110);
        assert_eq!(s(&rt, "got"), "");
        assert_eq!(rt.get(root, "below").to_number(), 0.0);
    }
}

#[cfg(test)]
mod middle_tests {
    use super::*;

    #[test]
    #[cfg_attr(
        not(feature = "interp"),
        ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
    )]
    fn middle_click_pastes_the_primary_selection_like_qt() {
        crate::keys::FAKE_PRIMARY.with(|c| *c.borrow_mut() = Some("прим 1%".into()));
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nItem { width: 200; height: 100; property int edits: 0; \
                 property int clicks: 0\n MouseArea { anchors.fill: parent; \
                 acceptedButtons: Qt.AllButtons; onClicked: parent.clicks++ }\n \
                 TextInput { width: 200; height: 40; font.pixelSize: 16; text: \"x\"; cursorPosition: 1; \
                 activeFocusOnPress: true; \
                 onTextEdited: parent.edits++ }\n \
                 TextInput { y: 50; width: 200; height: 40; readOnly: true } }\n",
            )
            .unwrap();
        rt.settle();
        let (field, ro) = {
            let w = rt.world.borrow();
            (w.items[root].children[1], w.items[root].children[2])
        };
        let get = |rt: &Runtime, i: usize, n: &str| {
            rt.world.borrow_mut().read(i, n).map(|v| match v {
                Value::Str(s) => s.to_string(),
                v => v.to_number().to_string(),
            })
        };
        let mut ptr = Pointer::default();
        ptr.enter(&mut rt, root, 150.0, 10.0);
        ptr.motion(&mut rt, root, 150.0, 10.0);
        ptr.press(&mut rt, 0x112);
        ptr.release(&mut rt, 0x112);
        assert_eq!(get(&rt, field, "text").as_deref(), Some("xприм 1%"));
        assert_eq!(get(&rt, field, "cursorPosition").as_deref(), Some("8"));
        assert_eq!(get(&rt, root, "edits").as_deref(), Some("1"));
        assert_eq!(get(&rt, root, "clicks").as_deref(), Some("0"));

        ptr.motion(&mut rt, root, 10.0, 60.0);
        ptr.press(&mut rt, 0x112);
        ptr.release(&mut rt, 0x112);
        assert_eq!(get(&rt, ro, "text").as_deref(), Some(""));
        assert_eq!(get(&rt, root, "clicks").as_deref(), Some("0"));
        crate::keys::FAKE_PRIMARY.with(|c| *c.borrow_mut() = None);
    }
}
