//! The edit mode controller: selection, dragging, resizing, snapping with guides, moving
//! widgets between the panel, desktop and sidebar containers of one monitor, undo/redo and
//! cancel. It works in output coordinates (logical pixels from the monitor's top-left
//! corner), where the edit overlay, a fullscreen layer surface, receives the pointer: a drag
//! from one surface to another happens inside that one surface, since Wayland gives a client
//! no pointer events across its surfaces.
//!
//! The pure part ([`Scene`], [`Gesture`]) decides; `Impulse/EditController`
//! ([`call`], [`sync`]) gathers the scene from the live item tree, applies the resulting
//! [`Edit`]s through [`crate::layout_model::Live`] (one undo step each) and publishes what the
//! overlay draws. QML only forwards input and draws.

use std::collections::HashMap;

use crate::layout_model::desktop::{self, Bounds, Limits, Rect as DRect, Shows, Snap};
use crate::layout_model::{ContainerKind, Edit, Layout, Place, Width, Zone};
use crate::runtime::World;
use crate::value::Value;
use crate::widgets::{Placement, Registry};

pub(crate) const TYPE: &str = "EditController";

/// Pointer travel before a press becomes a drag (QStyleHints::startDragDistance).
pub const DRAG_DISTANCE: f64 = 5.0;
/// The same for a press on a resize handle, which is not a click on anything: enough for a
/// hand that shakes on the button.
pub const HANDLE_DRAG_DISTANCE: f64 = 2.0;
/// Distance at which a dragged edge or centre line snaps to another one.
pub const SNAP_DISTANCE: f64 = 8.0;
/// Side of the square resize handle on a selected widget's bottom-right corner.
pub const HANDLE: f64 = 16.0;
/// How far from an edge of the selected widget's frame, on either side of it, a press still
/// takes that edge for a resize.
pub const GRIP: f64 = 8.0;

// Qt::Key and Qt::KeyboardModifier values.
const KEY_ESCAPE: i64 = 0x0100_0000;
const KEY_BACKSPACE: i64 = 0x0100_0003;
const KEY_DELETE: i64 = 0x0100_0007;
const KEY_LEFT: i64 = 0x0100_0012;
const KEY_UP: i64 = 0x0100_0013;
const KEY_RIGHT: i64 = 0x0100_0014;
const KEY_DOWN: i64 = 0x0100_0015;
const KEY_Y: i64 = 0x59;
const KEY_Z: i64 = 0x5a;
bits::bit_flags! {
const SHIFT: i64 = 0x0200_0000;
const CTRL: i64 = 0x0400_0000;
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn right(&self) -> f64 {
        self.x + self.w
    }

    fn bottom(&self) -> f64 {
        self.y + self.h
    }

    fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    fn of(r: DRect) -> Rect {
        Rect {
            x: r.x as f64,
            y: r.y as f64,
            w: r.w as f64,
            h: r.h as f64,
        }
    }

    fn snapped(&self) -> DRect {
        DRect {
            x: self.x.round() as i32,
            y: self.y.round() as i32,
            w: self.w.round().max(1.0) as u32,
            h: self.h.round().max(1.0) as u32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Panel,
    Desktop,
    Sidebar,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Panel => "panel",
            Kind::Desktop => "desktop",
            Kind::Sidebar => "sidebar",
        }
    }

    fn placement(self) -> Placement {
        match self {
            Kind::Panel => Placement::Panel,
            Kind::Desktop => Placement::Desktop,
            Kind::Sidebar => Placement::Sidebar,
        }
    }

    fn of(k: &ContainerKind) -> Kind {
        match k {
            ContainerKind::Panel(_) => Kind::Panel,
            ContainerKind::Desktop => Kind::Desktop,
            ContainerKind::Sidebar(_) => Kind::Sidebar,
        }
    }
}

/// A widget as shown on the monitor.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub id: String,
    pub ty: String,
    pub container: String,
    pub kind: Kind,
    pub rect: Rect,
    /// Whether its variant on this surface can be resized there (it has a resize handle).
    pub resizable: bool,
    /// The least length along its panel at which none of what it shows is cut, as its host
    /// measured it (`contentLength`); 0 where its host names none.
    pub least: f64,
    /// The length along its panel that keeps what it shows at any length (a clock's text)
    /// clear of the handles at its ends (`clearLength`); 0 where its host names none. Shorter
    /// than that, the widget is as the layout has it and its outline is around this length.
    pub clear: f64,
}

impl Frame {
    /// How far past each end of the widget, `len` long along its panel, its outline and the
    /// handles on it are ([`Frame::clear`]): the widget itself is no longer for them.
    pub fn pad(&self, len: f64) -> f64 {
        if self.kind == Kind::Panel && self.clear > len {
            (self.clear - len) / 2.0
        } else {
            0.0
        }
    }

    /// `rect`, the widget's or its ghost's, with that past its ends ([`Frame::pad`]).
    pub fn padded(&self, rect: &Rect, vertical: bool) -> Rect {
        let pad = self.pad(if vertical { rect.h } else { rect.w });
        if vertical {
            Rect {
                y: rect.y - pad,
                h: rect.h + pad * 2.0,
                ..*rect
            }
        } else {
            Rect {
                x: rect.x - pad,
                w: rect.w + pad * 2.0,
                ..*rect
            }
        }
    }
}

/// A container's area on the monitor, where widgets can be dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub container: String,
    pub kind: Kind,
    pub rect: Rect,
    /// Panels on a left or right edge lay widgets out top to bottom.
    pub vertical: bool,
    /// Desktop grid cell (0: free placement).
    pub grid: u32,
    /// The length along a panel one more widget may take in its start, center and end zone
    /// without any widget giving up its natural size (unbounded where the panel does not say).
    pub room: [f64; 3],
    /// The length along the panel each widget of the monitor's other containers takes there
    /// in its panel form, by id, as the panel lays it out (`panelLengths`).
    pub lengths: HashMap<String, f64>,
    /// What the sidebar measured, where it says (SidebarLayoutContent).
    pub sidebar: Option<SidebarRoom>,
}

/// A sidebar's height and what each widget it shows, or may take, costs it there.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SidebarRoom {
    /// The height its sections may take.
    pub available: f64,
    /// By id, the height of each widget of the monitor in its sidebar form (`sidebarHeights`):
    /// a grid cell's, or a block's with the least a filling block keeps.
    pub heights: HashMap<String, f64>,
    pub metrics: crate::layout_model::sidebar::Metrics,
}

/// Where a widget sits, as its settings dialog shows it ([`Scene::placement`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sits {
    Panel {
        zone: Zone,
        width: Width,
    },
    Sidebar {
        size: u32,
        columns: u32,
    },
    /// The size it shows on the desktop, with the least and the most its type takes there (no
    /// more than the monitor).
    Desktop {
        size: (u32, u32),
        min: (u32, u32),
        max: (u32, u32),
    },
}

/// What a gesture needs to know about the monitor.
pub struct Scene<'a> {
    pub layout: &'a Layout,
    pub monitor: usize,
    pub registry: &'a Registry,
    pub frames: Vec<Frame>,
    pub regions: Vec<Region>,
    /// The monitor's size.
    pub bounds: (f64, f64),
    /// The natural size the desktop views measured, by widget id ([`desktop::Natural`]).
    pub natural: HashMap<String, (u32, u32)>,
    /// Where the desktop widgets that do not show as laid out show, by widget id
    /// ([`desktop::Placed`]).
    pub placed: HashMap<String, Shows>,
}

/// A straight snapping guide from (x1, y1) to (x2, y2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Move,
    Resize,
}

/// The edges of its frame a resize moves: the handle that was pressed. The opposite ones stay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl Edges {
    pub const BOTTOM_RIGHT: Edges = Edges {
        left: false,
        right: true,
        top: false,
        bottom: true,
    };

    /// The handle's name for the overlay (its pointer shape): "top-left", "right", ...; ""
    /// for none.
    pub fn name(self) -> &'static str {
        match (self.left, self.right, self.top, self.bottom) {
            (true, _, true, _) => "top-left",
            (_, true, true, _) => "top-right",
            (true, _, _, true) => "bottom-left",
            (_, true, _, true) => "bottom-right",
            (true, ..) => "left",
            (_, true, ..) => "right",
            (_, _, true, _) => "top",
            (_, _, _, true) => "bottom",
            _ => "",
        }
    }
}

/// Where a drop lands.
#[derive(Clone, Debug, PartialEq)]
pub struct Drop {
    pub container: String,
    pub place: Place,
    /// Position among the target's widgets (of the same zone in a panel); `None`: keep or append.
    pub index: Option<usize>,
}

/// What the overlay shows while dragging.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    pub ghost: Rect,
    /// A desktop widget moved within its container, where the pointer has it: the widget
    /// itself is shown there, the ghost where it would land.
    pub raw: Option<Rect>,
    pub valid: bool,
    pub target: Option<Rect>,
    pub guides: Vec<Guide>,
    /// The insertion point in a panel or sidebar.
    pub marker: Option<Rect>,
    /// Where the "not allowed" badge of a refused drop goes: beside the pointer, clear of it.
    pub refusal: Option<Rect>,
    pub drop: Option<Drop>,
    /// What else the drop changes with it: the room a panel widget pulled in leaves beside it.
    pub also: Vec<Edit>,
    /// A panel widget pulled by an end, where it is known without its panel laying it out:
    /// the ghost is where the widget will show, with its other end and every widget past
    /// that one where they are. The panel shows such a step once the pointer rests
    /// ([`State::pending`]); any other one at once, since the next is made from how its
    /// zone answered ([`Scene::measure`]).
    pub known: bool,
}

/// A press on a widget, until its release.
#[derive(Clone, Debug, PartialEq)]
pub struct Gesture {
    pub connector: String,
    pub id: String,
    pub mode: Mode,
    /// For a resize, the edges it moves.
    pub edges: Edges,
    pub press: (f64, f64),
    pub start: Rect,
    pub kind: Kind,
    /// Past [`DRAG_DISTANCE`].
    pub dragging: bool,
    pub preview: Option<Preview>,
    /// Where the widget shows now, once the drag began: its frame stays where the drag
    /// started ([`gather`]), the widget itself may follow the drag.
    pub shown: Option<Rect>,
    /// A panel widget's end under a resize: how far it went for each pixel its widget grew by
    /// (1 where the other end stays, a half in a centred group, 0 for the end its zone holds),
    /// and the frame that was measured from. None until its length changed.
    pub gain: Option<(Rect, f64)>,
}

/// Edit mode state, shared by every monitor's overlay.
#[derive(Debug, Default)]
pub struct State {
    pub active: bool,
    /// Connector and widget id.
    pub selected: Option<(String, String)>,
    pub gesture: Option<Gesture>,
    /// Where the pointer last was while no button held a widget: connector, x, y.
    pub hover: Option<(String, f64, f64)>,
    /// A panel's widget is shown as a gesture has it: where the pointer carries it, or at the
    /// length a resize would give it ([`live_hosts`]).
    pub carrying: bool,
    /// The panel a pull by an end would leave, with its monitor's connector, while the pull
    /// lasts: what the panel shows meanwhile ([`State::pulled_panel`]).
    pub pulled: Option<(String, crate::layout_model::Container)>,
    /// The panel the step of a pull the pointer is at would leave, where the panel does not
    /// show it yet ([`Preview::known`]): while the pointer moves only the outline goes with
    /// it, and the panel takes this once the pointer rests ([`rest`]) and at the release.
    pub pending: Option<(String, crate::layout_model::Container)>,
    /// Bumped by every change of the above.
    pub rev: u64,
    /// What the overlays' outputs were last written from ([`sync_key`]).
    synced: Option<u64>,
}

impl State {
    /// The panel as the pull under way would leave it, with its monitor's connector: the
    /// widget at the length the pointer has its end at, and the room that leaves beside it.
    pub fn pulled_panel(&self) -> Option<&(String, crate::layout_model::Container)> {
        let g = self.gesture.as_ref()?;
        (g.dragging && g.mode == Mode::Resize && g.kind == Kind::Panel)
            .then_some(self.pulled.as_ref())
            .flatten()
            .filter(|(connector, _)| *connector == g.connector)
    }
}

/// The frame under (x, y): widgets on the panel and sidebar surfaces lie above the desktop,
/// and the smaller of two overlapping frames is the one in front.
pub fn hit(frames: &[Frame], x: f64, y: f64) -> Option<&Frame> {
    frames
        .iter()
        .filter(|f| f.rect.contains(x, y))
        .min_by(|a, b| {
            let layer = |f: &Frame| (f.kind == Kind::Desktop) as u8;
            layer(a)
                .cmp(&layer(b))
                .then((a.rect.w * a.rect.h).total_cmp(&(b.rect.w * b.rect.h)))
        })
}

/// The resize handle of `r` at (x, y), on a surface of `kind`: a desktop widget has one on each
/// corner and each side; a panel's widget on its two ends along the panel, a sidebar's on its
/// left and right (the only axis a resize changes there). A handle reaches [`GRIP`] to either
/// side of its edge, a corner and a panel's or a sidebar's end [`HANDLE`] into the widget, and
/// never past a third of a small widget, whose middle stays where a press moves it.
pub fn handle_at(r: &Rect, kind: Kind, vertical: bool, x: f64, y: f64) -> Option<Edges> {
    if x < r.x - GRIP || x > r.right() + GRIP || y < r.y - GRIP || y > r.bottom() + GRIP {
        return None;
    }
    // Whether `v` is by the edge at `edge`, `inward` being the frame's side of it.
    let by = |v: f64, edge: f64, inward: f64, size: f64, reach: f64| {
        let d = (v - edge) * inward;
        d >= -GRIP && d <= reach.min(size / 3.0)
    };
    let sides = |reach: f64| Edges {
        left: by(x, r.x, 1.0, r.w, reach),
        right: by(x, r.right(), -1.0, r.w, reach),
        top: by(y, r.y, 1.0, r.h, reach),
        bottom: by(y, r.bottom(), -1.0, r.h, reach),
    };
    let (near, corner) = (sides(GRIP), sides(HANDLE));
    // The far edge wins where a widget is so small that both are near.
    let far = |e: Edges| Edges {
        left: e.left && !e.right,
        right: e.right,
        top: e.top && !e.bottom,
        bottom: e.bottom,
    };
    let e = match kind {
        Kind::Desktop if (corner.left || corner.right) && (corner.top || corner.bottom) => {
            far(corner)
        }
        Kind::Desktop => far(near),
        // An end of a panel's or a sidebar's widget is all there is to hold it by: it reaches as
        // far into the widget as a corner does.
        Kind::Panel if vertical => Edges {
            left: false,
            right: false,
            ..far(corner)
        },
        Kind::Panel | Kind::Sidebar => Edges {
            top: false,
            bottom: false,
            ..far(corner)
        },
    };
    (e != Edges::default()).then_some(e)
}

/// The end of a panel widget at `r` that the handle `e` holds: where it is along the panel, the
/// widget's length, and which way that end goes as the widget grows (-1 the leading end's).
fn held_end(r: &Rect, e: Edges, vertical: bool) -> (f64, f64, f64) {
    let (at, len, leading) = if vertical {
        (r.y, r.h, e.top && !e.bottom)
    } else {
        (r.x, r.w, e.left && !e.right)
    };
    if leading {
        (at, len, -1.0)
    } else {
        (at + len, len, 1.0)
    }
}

/// Whether a widget of type `ty` can be resized on a surface of `kind`: its variant there has
/// room between its minimum and maximum along an axis a resize changes there (along a panel,
/// the sidebar's columns, either on the desktop). A type the registry doesn't know can.
pub fn resizable(registry: &Registry, ty: &str, kind: Kind, vertical: bool) -> bool {
    let Some(v) = registry.get(ty).and_then(|t| t.variant(kind.placement())) else {
        return true;
    };
    let (min, max) = (v.size.min, v.size.max);
    let (w, h) = (min.w < max.w, min.h < max.h);
    match kind {
        Kind::Desktop => w || h,
        Kind::Panel if vertical => h,
        Kind::Panel => w,
        // One that needs the whole grid whatever its columns has no other size.
        Kind::Sidebar => w && min.w < crate::layout_model::MAX_COLUMNS as f64,
    }
}

/// The side of the "not allowed" badge, in logical pixels.
pub const BADGE: f64 = 32.0;
/// The badge's offset from the pointer's hotspot on each axis: past a 32 px cursor whether its
/// hotspot is a corner (an arrow) or the centre (a circle), so the cursor never covers it.
const BADGE_OFFSET: f64 = 32.0;

/// The badge of a refused drop with the pointer at (x, y): below and right of the pointer, as
/// a drag-and-drop status icon sits by the cursor, and on the other side of it on an axis
/// where the screen ends; the same size whatever the ghost's.
pub fn refusal_badge(x: f64, y: f64, bounds: (f64, f64)) -> Rect {
    let place = |at: f64, end: f64| {
        let after = at + BADGE_OFFSET;
        if after + BADGE <= end {
            after
        } else {
            (at - BADGE_OFFSET - BADGE).max(0.0)
        }
    };
    Rect {
        x: place(x, bounds.0),
        y: place(y, bounds.1),
        w: BADGE,
        h: BADGE,
    }
}

/// Snaps `r`'s edges and centre to the nearest of `lines` (x lines, y lines) within
/// [`SNAP_DISTANCE`] on each axis; returns the moved rectangle and the guides it snapped to.
pub fn snap_move(r: Rect, xs: &[f64], ys: &[f64], bounds: (f64, f64)) -> (Rect, Vec<Guide>) {
    let best = |edges: [f64; 3], lines: &[f64]| {
        let mut out: Option<(f64, f64)> = None;
        for e in edges {
            for &l in lines {
                let d = l - e;
                if d.abs() <= SNAP_DISTANCE && out.is_none_or(|(bd, _)| d.abs() < bd.abs()) {
                    out = Some((d, l));
                }
            }
        }
        out
    };
    let mut r = r;
    let mut guides = vec![];
    if let Some((d, l)) = best([r.x, r.x + r.w / 2.0, r.right()], xs) {
        r.x += d;
        guides.push(Guide {
            x1: l,
            y1: 0.0,
            x2: l,
            y2: bounds.1,
        });
    }
    if let Some((d, l)) = best([r.y, r.y + r.h / 2.0, r.bottom()], ys) {
        r.y += d;
        guides.push(Guide {
            x1: 0.0,
            y1: l,
            x2: bounds.0,
            y2: l,
        });
    }
    (r, guides)
}

/// Snaps the edges of `r` a resize moves (`edges`) to the nearest of the lines within
/// [`SNAP_DISTANCE`]; the other edges stay where they are.
pub fn snap_resize(
    r: Rect,
    edges: Edges,
    xs: &[f64],
    ys: &[f64],
    bounds: (f64, f64),
) -> (Rect, Vec<Guide>) {
    let best = |e: f64, lines: &[f64]| {
        lines
            .iter()
            .map(|&l| (l - e, l))
            .filter(|(d, _)| d.abs() <= SNAP_DISTANCE)
            .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
    };
    let mut r = r;
    let mut guides = vec![];
    let x = if edges.right {
        best(r.right(), xs).inspect(|(d, _)| r.w += d)
    } else if edges.left {
        best(r.x, xs).inspect(|(d, _)| {
            r.x += d;
            r.w -= d;
        })
    } else {
        None
    };
    if let Some((_, l)) = x {
        guides.push(Guide {
            x1: l,
            y1: 0.0,
            x2: l,
            y2: bounds.1,
        });
    }
    let y = if edges.bottom {
        best(r.bottom(), ys).inspect(|(d, _)| r.h += d)
    } else if edges.top {
        best(r.y, ys).inspect(|(d, _)| {
            r.y += d;
            r.h -= d;
        })
    } else {
        None
    };
    if let Some((_, l)) = y {
        guides.push(Guide {
            x1: 0.0,
            y1: l,
            x2: bounds.0,
            y2: l,
        });
    }
    (r, guides)
}

/// The fewest and the most columns a sidebar of `cols` columns gives a widget with limits
/// `lim` (its type's sidebar variant, in cells).
fn sidebar_span(lim: Limits, cols: u32) -> (u32, u32) {
    let lo = lim.min.0.clamp(1, cols);
    (lo, lim.max.0.clamp(lo, cols))
}

/// The fewest columns a sidebar of `cols` columns gives a widget of type `ty`: its sidebar
/// variant's least width, the whole grid for one whose least is no less than the most columns
/// a sidebar may have.
pub(crate) fn sidebar_least(registry: &Registry, ty: &str, cols: u32) -> u32 {
    registry
        .get(ty)
        .and_then(|t| t.variant(Placement::Sidebar))
        .map_or(1, |v| (v.size.min.w.max(1.0) as u32).clamp(1, cols.max(1)))
}

/// `layout` with every sidebar widget in at least the columns its type needs
/// ([`crate::layout_model::sidebar::fit`]), as the sidebar shows them; borrowed when none
/// asks for fewer.
fn fitted<'a>(layout: &'a Layout, registry: &Registry) -> std::borrow::Cow<'a, Layout> {
    let mut out = std::borrow::Cow::Borrowed(layout);
    for (mi, m) in layout.monitors.iter().enumerate() {
        for (ci, c) in m.containers.iter().enumerate() {
            let mut c = c.clone();
            if crate::layout_model::sidebar::fit(&mut c, |ty, cols| {
                sidebar_least(registry, ty, cols)
            }) {
                out.to_mut().monitors[mi].containers[ci] = c;
            }
        }
    }
    out
}

/// How far inside the thickness its panel's widgets take the outline of one of them is drawn.
const PANEL_FRAME_INSET: f64 = 3.0;

/// What the overlay says of a resize in progress: the size it would give and whether that is
/// the least or the most the widget can be there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeHint {
    pub kind: Kind,
    /// The width and height in logical pixels; in a sidebar the columns taken and the grid's.
    pub size: (f64, f64),
    pub smallest: bool,
    pub largest: bool,
}

impl Scene<'_> {
    fn frame(&self, id: &str) -> Option<&Frame> {
        self.frames.iter().find(|f| f.id == id)
    }

    fn vertical(&self, f: &Frame) -> bool {
        self.regions
            .iter()
            .any(|r| r.container == f.container && r.vertical)
    }

    /// The resize handle of `f` at (x, y), if it can be resized ([`handle_at`]).
    pub fn handle_of(&self, f: &Frame, x: f64, y: f64) -> Option<Edges> {
        if !f.resizable {
            return None;
        }
        let vertical = self.vertical(f);
        handle_at(&f.padded(&f.rect, vertical), f.kind, vertical, x, y)
    }

    /// Notes how the held end of a panel widget under a resize answers its length, from where
    /// its panel has laid it out by now (the widget takes the length of each step at once,
    /// [`live_hosts`]): the next step asks for the length that puts that end under the pointer,
    /// whichever way its zone grows.
    pub fn measure(&self, g: &mut Gesture) {
        if g.mode != Mode::Resize || g.kind != Kind::Panel {
            return;
        }
        let Some(f) = self.frame(&g.id) else {
            return;
        };
        let vertical = self.vertical(f);
        let (from, _) = g.gain.unwrap_or((g.start, 1.0));
        let (was, now) = (
            held_end(&from, g.edges, vertical),
            held_end(&f.rect, g.edges, vertical),
        );
        // Only what the widget grew by past the room beside the held end says how its zone
        // answers: up to there that end goes as far as the widget grows, whatever the zone.
        let floor = held_end(&g.start, g.edges, vertical).1 + self.room_beside(g, now.2 < 0.0);
        let past = |(at, len, out): (f64, f64, f64)| {
            (at + (floor - len).max(0.0) * out, len.max(floor), out)
        };
        let (was, now) = (past(was), past(now));
        let grown = now.1 - was.1;
        if grown.abs() >= 2.0 {
            g.gain = Some((f.rect, (now.0 - was.0) * now.2 / grown));
        }
    }

    /// The room the layout has beside the end of `g`'s panel widget that is held, the leading
    /// one or the other ([`crate::layout_model::is_gap`]).
    fn room_beside(&self, g: &Gesture, leading: bool) -> f64 {
        let Some((c, w)) = self.widget(&g.id) else {
            return 0.0;
        };
        let (before, after) = crate::layout_model::gaps_beside(c, &w.id);
        (if leading { before } else { after }).map_or(0.0, |b| match b.place {
            Place::Panel {
                width: Width::Px(n),
                ..
            } => f64::from(n),
            _ => 0.0,
        })
    }

    /// The resize handle of the selected widget `f` that the pointer at (x, y) is on
    /// ([`Scene::handle_of`]). A handle reaches out of its frame; where that is on another
    /// widget (the next one in a panel's row, a panel's widget above a desktop one), the
    /// pointer is on that widget: a press there takes it, not the selected one's handle.
    pub fn grip(&self, f: &Frame, x: f64, y: f64) -> Option<Edges> {
        let e = self.handle_of(f, x, y)?;
        // Its outline is its own, where that is wider than the widget ([`Frame::padded`]).
        let on_another = !f.padded(&f.rect, self.vertical(f)).contains(x, y)
            && self
                .frames
                .iter()
                .any(|o| o.id != f.id && o.rect.contains(x, y));
        (!on_another).then_some(e)
    }

    /// What a press at (x, y) takes while the widget `selected` is the selected one: that
    /// widget by a resize handle the pointer is on ([`Scene::grip`]), else the widget under
    /// the pointer ([`hit`]), to move it. Never a widget the pointer is not on while it is on
    /// another.
    pub fn pick(&self, selected: Option<&str>, x: f64, y: f64) -> Option<(&Frame, Mode, Edges)> {
        if let Some(f) = selected.and_then(|id| self.frame(id))
            && let Some(e) = self.grip(f, x, y)
        {
            return Some((f, Mode::Resize, e));
        }
        hit(&self.frames, x, y).map(|f| (f, Mode::Move, Edges::default()))
    }

    /// Where the overlay draws the outline of `rect`, the frame of `f` or its ghost: on whole
    /// pixels, and for a panel's widget across the thickness its panel's widgets take together
    /// less a margin, so the outlines of a row of widgets of different heights line up.
    pub fn outline(&self, f: &Frame, rect: &Rect) -> Rect {
        let mut r = *rect;
        if f.kind == Kind::Panel {
            let vertical = self.vertical(f);
            // And past the ends of a widget shorter than what it shows.
            r = f.padded(rect, vertical);
            let (from, to) = self
                .frames
                .iter()
                .filter(|o| o.container == f.container)
                .map(|o| {
                    if vertical {
                        (o.rect.x, o.rect.right())
                    } else {
                        (o.rect.y, o.rect.bottom())
                    }
                })
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (c, d)| {
                    (a.min(c), b.max(d))
                });
            if to - from > PANEL_FRAME_INSET * 2.0 {
                let (at, len) = (
                    from + PANEL_FRAME_INSET,
                    to - from - PANEL_FRAME_INSET * 2.0,
                );
                if vertical {
                    (r.x, r.w) = (at, len);
                } else {
                    (r.y, r.h) = (at, len);
                }
            }
        }
        let (x, y) = (r.x.round(), r.y.round());
        Rect {
            x,
            y,
            w: (r.right().round() - x).max(1.0),
            h: (r.bottom().round() - y).max(1.0),
        }
    }

    /// What to say of the resize `g` at its present preview.
    pub fn size_hint(&self, g: &Gesture) -> Option<SizeHint> {
        if g.mode != Mode::Resize {
            return None;
        }
        let p = g.preview.as_ref().filter(|p| p.valid)?;
        let (c, w) = self.widget(&g.id)?;
        let e = g.edges;
        let lim = self.limits(&w.r#type, g.kind.placement());
        let at = |v: f64, limit: u32| (v - limit as f64).abs() < 0.5;
        match (&p.drop.as_ref()?.place, &c.kind) {
            (Place::Desktop { .. }, _) => {
                let (gw, gh) = (p.ghost.w.round(), p.ghost.h.round());
                let (x, y) = (e.left || e.right, e.top || e.bottom);
                Some(SizeHint {
                    kind: Kind::Desktop,
                    size: (gw, gh),
                    smallest: (x && at(gw, lim.min.0)) || (y && at(gh, lim.min.1)),
                    largest: (x && at(gw, lim.max.0)) || (y && at(gh, lim.max.1)),
                })
            }
            (Place::Panel { zone, .. }, ContainerKind::Panel(_)) => {
                let region = self.regions.iter().find(|r| r.container == c.id);
                let vertical = region.is_some_and(|r| r.vertical);
                let (len, shown, min, max) = if vertical {
                    (p.ghost.h, g.start.h, lim.min.1, lim.max.1)
                } else {
                    (p.ghost.w, g.start.w, lim.min.0, lim.max.0)
                };
                let room = region.map_or(f64::INFINITY, |r| {
                    // The zone of the middle has its room on both sides, and grows to one.
                    let room = r.room[*zone as usize].max(0.0);
                    (shown
                        + if *zone == Zone::Center {
                            room / 2.0
                        } else {
                            room
                        })
                    .floor()
                });
                Some(SizeHint {
                    kind: Kind::Panel,
                    size: (len.round(), 0.0),
                    smallest: at(len, min),
                    largest: at(len, max) || (len - room).abs() < 0.5,
                })
            }
            (Place::Sidebar { size }, ContainerKind::Sidebar(s)) => {
                let cols = s.columns.max(1);
                let (lo, hi) = sidebar_span(lim, cols);
                Some(SizeHint {
                    kind: Kind::Sidebar,
                    size: (*size as f64, cols as f64),
                    smallest: *size <= lo,
                    largest: *size >= hi,
                })
            }
            _ => None,
        }
    }

    fn widget(
        &self,
        id: &str,
    ) -> Option<(
        &crate::layout_model::Container,
        &crate::layout_model::Widget,
    )> {
        let m = self.layout.monitors.get(self.monitor)?;
        m.containers
            .iter()
            .find_map(|c| c.widgets.iter().find(|w| w.id == id).map(|w| (c, w)))
    }

    fn allows(&self, ty: &str, kind: Kind) -> bool {
        self.registry
            .get(ty)
            .is_none_or(|t| t.allows(kind.placement()))
    }

    /// The size limits of `ty`'s variant on surface `on`.
    fn limits(&self, ty: &str, on: Placement) -> Limits {
        let px = |v: f64| v.max(1.0).min(u32::MAX as f64) as u32;
        self.registry
            .get(ty)
            .and_then(|t| t.variant(on))
            .map_or(Limits::ANY, |v| Limits {
                min: (px(v.size.min.w), px(v.size.min.h)),
                max: (px(v.size.max.w), px(v.size.max.h)),
            })
    }

    fn default_size(&self, ty: &str, fallback: Rect) -> (f64, f64) {
        self.registry
            .get(ty)
            .and_then(|t| t.variant(Placement::Desktop))
            .map_or((fallback.w, fallback.h), |v| {
                (v.size.default.w.max(1.0), v.size.default.h.max(1.0))
            })
    }

    /// The length a widget dragged onto a panel takes along it: a panel widget's own, a
    /// stretching one's the least its panel variant allows (what PanelWidget keeps for it), one
    /// from elsewhere what the panel measured of its panel form, or before it did its panel
    /// variant's default size.
    fn panel_length(&self, g: &Gesture, w: &crate::layout_model::Widget, region: &Region) -> f64 {
        let vertical = region.vertical;
        let along = |s: crate::widgets::Size| if vertical { s.h } else { s.w };
        let variant = self
            .registry
            .get(&w.r#type)
            .and_then(|t| t.variant(Placement::Panel));
        let own = if vertical { g.start.h } else { g.start.w };
        match w.place {
            Place::Panel {
                width: Width::Stretch,
                ..
            } => region
                .lengths
                .get(&w.id)
                .copied()
                .unwrap_or_else(|| variant.map_or(0.0, |v| along(v.size.min))),
            Place::Panel { .. } => own,
            _ => region
                .lengths
                .get(&w.id)
                .copied()
                .unwrap_or_else(|| variant.map_or(own, |v| along(v.size.default))),
        }
    }

    /// The region a drop at (x, y) goes to: a panel or the sidebar before the desktop under them.
    fn region_at(&self, x: f64, y: f64) -> Option<&Region> {
        self.regions
            .iter()
            .filter(|r| r.rect.contains(x, y))
            .min_by_key(|r| (r.kind == Kind::Desktop) as u8)
    }

    /// Whether the sidebar of `region` has room for `w` spanning `size` cells at `index` among
    /// its other widgets: the height its sections would take, from what it measured, against the
    /// height it has. A widget moving inside it may keep a layout that already overflows (its
    /// last sections hidden), never make it taller. A sidebar that measured nothing yet, or a
    /// widget it has no height for, is not judged.
    fn sidebar_fits(
        &self,
        region: &Region,
        from: &crate::layout_model::Container,
        w: &crate::layout_model::Widget,
        size: u32,
        index: Option<usize>,
    ) -> bool {
        use crate::layout_model::sidebar;
        let Some(room) = &region.sidebar else {
            return true;
        };
        let Some(target) = self
            .layout
            .monitors
            .get(self.monitor)
            .and_then(|m| m.containers.iter().find(|c| c.id == region.container))
        else {
            return true;
        };
        let height = |c: &crate::layout_model::Container| {
            sidebar::sections(c).and_then(|(_, s)| {
                sidebar::height(
                    &s,
                    |cell| room.heights.get(&cell.id).copied(),
                    &room.metrics,
                )
            })
        };
        let mut after = target.clone();
        let at = after.widgets.iter().position(|o| o.id == w.id);
        after.widgets.retain(|o| o.id != w.id);
        let mut moved = w.clone();
        moved.place = Place::Sidebar { size };
        let index = index.or(at).unwrap_or(after.widgets.len());
        after.widgets.insert(index.min(after.widgets.len()), moved);
        let Some(need) = height(&after) else {
            return true;
        };
        let limit = if from.id == target.id {
            height(target).map_or(room.available, |now| now.max(room.available))
        } else {
            room.available
        };
        need <= limit + 1e-6
    }

    /// The desktop widgets' rectangles in `container`, except `id`, as they show.
    fn desktop_others(&self, container: &str, id: &str) -> Vec<DRect> {
        self.layout
            .monitors
            .get(self.monitor)
            .and_then(|m| m.containers.iter().find(|c| c.id == container))
            .map(|c| {
                desktop::others(&c.widgets, Some(id), &|wd| self.natural_size(wd), &|wd| {
                    self.shows(wd)
                })
            })
            .unwrap_or_default()
    }

    /// A desktop widget's size without one in the layout: what its view measured, else its
    /// type's default size.
    fn shows(&self, wd: &crate::layout_model::Widget) -> Shows {
        self.placed.get(&wd.id).copied().unwrap_or(Shows::Laid)
    }

    /// Where desktop widget `wd`, which the layout puts at (`x`, `y`) with `size`, shows: there,
    /// or where it places itself (by the wallpaper), which is where a resize starts from.
    fn desktop_shown(
        &self,
        wd: &crate::layout_model::Widget,
        x: i32,
        y: i32,
        size: Option<(u32, u32)>,
    ) -> DRect {
        match self.shows(wd) {
            Shows::At(r) => r,
            _ => {
                let (w, h) = size.unwrap_or_else(|| self.natural_size(wd));
                DRect { x, y, w, h }
            }
        }
    }

    fn natural_size(&self, wd: &crate::layout_model::Widget) -> (u32, u32) {
        self.natural.get(&wd.id).copied().unwrap_or_else(|| {
            let px = |v: f64| v.round().clamp(1.0, u32::MAX as f64) as u32;
            self.registry
                .get(&wd.r#type)
                .and_then(|t| t.variant(Placement::Desktop))
                .map_or((1, 1), |v| (px(v.size.default.w), px(v.size.default.h)))
        })
    }

    fn lines(&self, others: &[DRect]) -> (Vec<f64>, Vec<f64>) {
        let (bw, bh) = self.bounds;
        let mut xs = vec![0.0, bw / 2.0, bw];
        let mut ys = vec![0.0, bh / 2.0, bh];
        for o in others {
            let r = Rect::of(*o);
            xs.extend([r.x, r.x + r.w / 2.0, r.right()]);
            ys.extend([r.y, r.y + r.h / 2.0, r.bottom()]);
        }
        (xs, ys)
    }

    fn desktop_bounds(&self) -> Bounds {
        Bounds {
            w: self.bounds.0.max(1.0) as u32,
            h: self.bounds.1.max(1.0) as u32,
        }
    }

    /// The preview of `g` with the pointer at (x, y).
    pub fn preview(&self, g: &Gesture, x: f64, y: f64) -> Preview {
        let Some((c, w)) = self.widget(&g.id) else {
            return Preview::default();
        };
        let (dx, dy) = (x - g.press.0, y - g.press.1);
        let mut p = match g.mode {
            Mode::Resize => self.resize_preview(g, c, w, dx, dy),
            Mode::Move => self.move_preview(g, c, w, x, y, dx, dy),
        };
        if !p.valid {
            p.refusal = Some(refusal_badge(x, y, self.bounds));
        }
        p
    }

    fn resize_preview(
        &self,
        g: &Gesture,
        c: &crate::layout_model::Container,
        w: &crate::layout_model::Widget,
        dx: f64,
        dy: f64,
    ) -> Preview {
        // The pressed handle's edges follow the pointer; the opposite ones stay.
        let e = g.edges;
        let grow = |near: bool, far: bool, d: f64| {
            if far {
                d
            } else if near {
                -d
            } else {
                0.0
            }
        };
        let (gw, gh) = (grow(e.left, e.right, dx), grow(e.top, e.bottom, dy));
        let want = Rect {
            x: if e.left && !e.right {
                g.start.x - gw.max(1.0 - g.start.w)
            } else {
                g.start.x
            },
            y: if e.top && !e.bottom {
                g.start.y - gh.max(1.0 - g.start.h)
            } else {
                g.start.y
            },
            w: (g.start.w + gw).max(1.0),
            h: (g.start.h + gh).max(1.0),
        };
        let region = self.regions.iter().find(|r| r.container == c.id);
        match (&w.place, &c.kind) {
            (Place::Desktop { x, y, size }, _) => {
                let current = self.desktop_shown(w, *x, *y, *size);
                let (ow, oh) = (current.w, current.h);
                let others = self.desktop_others(&c.id, &w.id);
                let grid = region.map_or(0, |r| r.grid);
                let (want, guides) = if grid > 0 {
                    (want, vec![])
                } else {
                    let (xs, ys) = self.lines(&others);
                    snap_resize(want, e, &xs, &ys, self.bounds)
                };
                let snap = if grid > 0 {
                    Snap::Grid { cell: grid }
                } else {
                    Snap::Free
                };
                // An axis the handle does not move keeps the size the widget shows.
                let got = desktop::resize_from(
                    current,
                    if e.left || e.right {
                        want.w.round().max(1.0) as u32
                    } else {
                        ow
                    },
                    if e.top || e.bottom {
                        want.h.round().max(1.0) as u32
                    } else {
                        oh
                    },
                    self.limits(&w.r#type, Placement::Desktop),
                    &others,
                    self.desktop_bounds(),
                    snap,
                    (e.left && !e.right, e.top && !e.bottom),
                );
                match got {
                    Some(r) => Preview {
                        ghost: Rect::of(r),
                        raw: None,
                        valid: true,
                        target: region.map(|r| r.rect),
                        guides,
                        marker: None,
                        refusal: None,
                        also: vec![],
                        drop: Some(Drop {
                            container: c.id.clone(),
                            place: Place::Desktop {
                                x: r.x,
                                y: r.y,
                                size: Some((r.w, r.h)),
                            },
                            index: None,
                        }),
                        known: false,
                    },
                    None => Preview {
                        ghost: want,
                        valid: false,
                        ..Preview::default()
                    },
                }
            }
            (Place::Panel { zone, .. }, ContainerKind::Panel(_)) => {
                // The shown panel's resolved orientation (an ri-bar panel follows the original's bar settings).
                let vertical = region.is_some_and(|r| r.vertical);
                let lim = self.limits(&w.r#type, Placement::Panel);
                let (min, most) = if vertical {
                    (lim.min.1 as f64, lim.max.1 as f64)
                } else {
                    (lim.min.0 as f64, lim.max.0 as f64)
                };
                // The widget shows a step's length ([`live_hosts`]), at once or once the pointer
                // rests ([`Preview::known`]), and its zone places it: the next length is the one that puts the held end under the
                // pointer, by how that end has answered so far ([`Scene::measure`]). The end
                // its zone holds in place cannot follow: the widget grows by as far as the
                // pointer went.
                let frame = self.frame(&w.id);
                let now = frame.map_or(g.start, |f| f.rect);
                let (start_at, start_len, out) = held_end(&g.start, e, vertical);
                let (now_at, now_len, _) = held_end(&now, e, vertical);
                let gone = if vertical { dy } else { dx };
                // The room already beside the held end ([`crate::layout_model::is_gap`]): pulled
                // in, the widget leaves as much more there as it gives up, and pulled out it
                // takes that first, so its other end and every widget around it stand still.
                let (before, after) = crate::layout_model::gaps_beside(c, &w.id);
                let leading = out < 0.0;
                let beside = if leading { before } else { after };
                let had = beside.map_or(0.0, |b| match b.place {
                    Place::Panel {
                        width: Width::Px(n),
                        ..
                    } => f64::from(n),
                    _ => 0.0,
                });
                // The handle is on the outline, which is past the end of a widget shorter
                // than what it shows by half of what it lacks ([`Frame::pad`]): the widget
                // grows by twice what the pointer went until it has that length, so the
                // handle stays under the pointer.
                let clear = frame.map_or(0.0, |f| f.clear);
                let to = start_len + frame.map_or(0.0, |f| f.pad(start_len)) + gone * out;
                let wanted = if to < clear { to * 2.0 - clear } else { to };
                let gain = g.gain.map_or(1.0, |(_, gain)| gain);
                // A zone in the middle of its panel grows to both sides: it is held off the
                // middle by as far as the widget grows past that room ([`Scene::shift_edits`]),
                // so the held end alone goes, as far as the pointer.
                let centred = *zone == Zone::Center;
                // Where the length does not come from how the zone answered, or that end has
                // gone as far as the widget grew, the step is known before the panel shows it.
                let known =
                    wanted <= start_len + had || centred || g.gain.is_some_and(|g| g.1 >= 0.75);
                let len = if wanted <= start_len + had || centred {
                    wanted
                } else if gain < 0.25 {
                    // The held end is against what its zone ends at: growing would move the
                    // other one.
                    start_len + had
                } else {
                    // Past that room the widget's zone places it: the next length is the one
                    // that puts the held end under the pointer, by how it has answered so far.
                    let off = (start_at + gone - now_at) * out;
                    if off.abs() < 1.0 {
                        now_len
                    } else {
                        now_len + off / if gain < 0.75 { 0.5 } else { 1.0 }
                    }
                };
                // No longer than its type allows, nor than its zone has room for: the length it
                // shows, the room beside the held end and what one more widget could take
                // there. No shorter than its type allows, nor than shows its contents whole
                // where there is room for that; one the layout already has shorter than that
                // stays as short, and is as it was when the pull begins.
                let room = region.map_or(f64::INFINITY, |r| {
                    // The zone of the middle has its room on both sides, and grows to one.
                    let room = r.room[*zone as usize].max(0.0);
                    now_len.max(start_len + had) + if centred { room / 2.0 } else { room }
                });
                let most = most.min(room.floor());
                let least = frame.map_or(0.0, |f| f.least.ceil()).min(most);
                let min = min.max(least.min(start_len.round()));
                let len = len.round().min(most).max(min).max(1.0);
                let room_left = (start_len.round() + had - len).max(0.0) as u32;
                let mut also = self.gap_edits(c, w, *zone, leading, beside, room_left);
                if centred {
                    let past = (len - start_len.round() - had).max(0.0) as u32;
                    also.extend(self.shift_edits(c, leading, past));
                }
                // The end the handle is on moves; the widget's place along the panel is its
                // zone's to give once dropped.
                let ghost = if vertical {
                    Rect {
                        y: if e.top && !e.bottom {
                            g.start.bottom() - len
                        } else {
                            g.start.y
                        },
                        w: g.start.w,
                        h: len,
                        ..g.start
                    }
                } else {
                    Rect {
                        x: if e.left && !e.right {
                            g.start.right() - len
                        } else {
                            g.start.x
                        },
                        w: len,
                        h: g.start.h,
                        ..g.start
                    }
                };
                Preview {
                    ghost,
                    raw: None,
                    valid: true,
                    target: region.map(|r| r.rect),
                    guides: vec![],
                    marker: None,
                    refusal: None,
                    also,
                    drop: Some(Drop {
                        container: c.id.clone(),
                        place: Place::Panel {
                            zone: *zone,
                            width: Width::Px(len as u32),
                        },
                        index: None,
                    }),
                    known,
                }
            }
            (Place::Sidebar { size }, ContainerKind::Sidebar(s)) => {
                let cols = s.columns.max(1);
                let cell = g.start.w / (*size).clamp(1, cols) as f64;
                let (lo, hi) = sidebar_span(self.limits(&w.r#type, Placement::Sidebar), cols);
                let n = ((want.w / cell.max(1.0)).round() as u32).clamp(lo, hi);
                let ghost = Rect {
                    x: if e.left && !e.right {
                        g.start.right() - cell * n as f64
                    } else {
                        g.start.x
                    },
                    w: cell * n as f64,
                    h: g.start.h,
                    ..g.start
                };
                // What the sidebar measured is the widget in its present form: a grid cell, or
                // a block when it spans the grid.
                let same_form = (n == cols) == ((*size).clamp(1, cols) == cols);
                if same_form
                    && let Some(r) = region
                    && !self.sidebar_fits(r, c, w, n, None)
                {
                    return Preview {
                        ghost,
                        valid: false,
                        target: Some(r.rect),
                        ..Preview::default()
                    };
                }
                Preview {
                    ghost,
                    raw: None,
                    valid: true,
                    target: region.map(|r| r.rect),
                    guides: vec![],
                    marker: None,
                    refusal: None,
                    also: vec![],
                    drop: Some(Drop {
                        container: c.id.clone(),
                        place: Place::Sidebar { size: n },
                        index: None,
                    }),
                    known: false,
                }
            }
            _ => Preview::default(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn move_preview(
        &self,
        g: &Gesture,
        c: &crate::layout_model::Container,
        w: &crate::layout_model::Widget,
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    ) -> Preview {
        let moved = Rect {
            x: g.start.x + dx,
            y: g.start.y + dy,
            ..g.start
        };
        // A desktop widget is carried itself, wherever the pointer takes it: also over a
        // place it cannot land on, where letting go puts it back.
        let carried = matches!(w.place, Place::Desktop { .. }).then_some(moved);
        // A panel's widget is carried itself along its panel while the pointer is over it (the
        // panel's window ends there): what the pointer is over is the widget it holds, not an
        // empty box on top of the others.
        let carried = carried.or_else(|| {
            let r = self.region_at(x, y)?;
            (r.kind == Kind::Panel && r.container == c.id && matches!(w.place, Place::Panel { .. }))
                .then(|| {
                    if r.vertical {
                        let top = r.rect.y.min(g.start.y);
                        let bottom = (r.rect.bottom() - g.start.h).max(g.start.y);
                        Rect {
                            y: moved.y.clamp(top, bottom),
                            ..g.start
                        }
                    } else {
                        let left = r.rect.x.min(g.start.x);
                        let right = (r.rect.right() - g.start.w).max(g.start.x);
                        Rect {
                            x: moved.x.clamp(left, right),
                            ..g.start
                        }
                    }
                })
        });
        let Some(region) = self.region_at(x, y) else {
            return Preview {
                ghost: moved,
                valid: false,
                raw: carried,
                ..Preview::default()
            };
        };
        if !self.allows(&w.r#type, region.kind) {
            return Preview {
                ghost: moved,
                valid: false,
                target: Some(region.rect),
                raw: carried,
                ..Preview::default()
            };
        }
        let same = region.container == c.id;
        match region.kind {
            Kind::Desktop => {
                // A widget from another container takes its type's default size, centred on
                // the pointer.
                let ghost = if same || matches!(w.place, Place::Desktop { .. }) {
                    moved
                } else {
                    let (dw, dh) = self.default_size(&w.r#type, g.start);
                    let lim = self.limits(&w.r#type, Placement::Desktop);
                    let (dw, dh) = (
                        dw.clamp(lim.min.0 as f64, lim.max.0 as f64),
                        dh.clamp(lim.min.1 as f64, lim.max.1 as f64),
                    );
                    Rect {
                        x: x - dw / 2.0,
                        y: y - dh / 2.0,
                        w: dw,
                        h: dh,
                    }
                };
                let others = self.desktop_others(&region.container, &w.id);
                let (snapped, guides) = if region.grid > 0 {
                    (ghost, vec![])
                } else {
                    let (xs, ys) = self.lines(&others);
                    snap_move(ghost, &xs, &ys, self.bounds)
                };
                let snap = if region.grid > 0 {
                    Snap::Grid { cell: region.grid }
                } else {
                    Snap::Free
                };
                match desktop::nearest_free(snapped.snapped(), &others, self.desktop_bounds(), snap)
                {
                    Some(r) => {
                        // Guides only for lines the landing rectangle still touches.
                        let land = Rect::of(r);
                        let guides = guides
                            .into_iter()
                            .filter(|gd| {
                                if gd.x1 == gd.x2 {
                                    [land.x, land.x + land.w / 2.0, land.right()]
                                        .iter()
                                        .any(|e| (e - gd.x1).abs() < 0.5)
                                } else {
                                    [land.y, land.y + land.h / 2.0, land.bottom()]
                                        .iter()
                                        .any(|e| (e - gd.y1).abs() < 0.5)
                                }
                            })
                            .collect();
                        Preview {
                            ghost: land,
                            raw: carried,
                            valid: true,
                            target: Some(region.rect),
                            guides,
                            marker: None,
                            refusal: None,
                            also: vec![],
                            // A move keeps the widget's size, or its natural size if it has
                            // none; one from a panel or the sidebar takes its natural size.
                            drop: Some(Drop {
                                container: region.container.clone(),
                                place: Place::Desktop {
                                    x: r.x,
                                    y: r.y,
                                    size: match w.place {
                                        Place::Desktop { size, .. } => size,
                                        _ => None,
                                    },
                                },
                                index: None,
                            }),
                            known: false,
                        }
                    }
                    None => Preview {
                        ghost: snapped,
                        valid: false,
                        target: Some(region.rect),
                        raw: carried,
                        ..Preview::default()
                    },
                }
            }
            Kind::Panel => {
                let own = (c.id == region.container).then_some(g.start);
                // A widget carried along its own panel goes by its middle, not by the pointer:
                // held near its end and carried a little, a long widget stays in its place
                // until its body is over the next one. Near a panel end the point runs ahead
                // to the end with the pointer, so the last place stays in reach.
                let (sx, sy) = match carried {
                    Some(_) if region.vertical => {
                        let (lo, hi) = (region.rect.y, region.rect.bottom());
                        let mid = moved.y + moved.h / 2.0;
                        (x, mid.max(2.0 * y - hi).min(2.0 * y - lo))
                    }
                    Some(_) => {
                        let (lo, hi) = (region.rect.x, region.rect.right());
                        let mid = moved.x + moved.w / 2.0;
                        (mid.max(2.0 * x - hi).min(2.0 * x - lo), y)
                    }
                    None => (x, y),
                };
                let (zone, index, marker) = self.panel_slot(region, &w.id, own, sx, sy);
                let width = match w.place {
                    Place::Panel { width, .. } => width,
                    _ => Width::Auto,
                };
                let same_zone = c.id == region.container
                    && matches!(w.place, Place::Panel { zone: z, .. } if z == zone);
                if !same_zone && self.panel_length(g, w, region) > region.room[zone as usize] {
                    return Preview {
                        ghost: Rect {
                            x: x - g.start.w / 2.0,
                            y: y - g.start.h / 2.0,
                            ..g.start
                        },
                        valid: false,
                        target: Some(region.rect),
                        raw: carried,
                        ..Preview::default()
                    };
                }
                let ghost = if matches!(w.place, Place::Panel { .. }) {
                    Rect {
                        x: x - g.start.w / 2.0,
                        y: y - g.start.h / 2.0,
                        ..g.start
                    }
                } else {
                    let (dw, dh) = self.default_size(&w.r#type, g.start);
                    Rect {
                        x: x - dw / 2.0,
                        y: y - dh / 2.0,
                        w: dw,
                        h: dh,
                    }
                };
                Preview {
                    ghost,
                    raw: carried,
                    valid: true,
                    target: Some(region.rect),
                    guides: vec![],
                    marker: Some(marker),
                    refusal: None,
                    also: vec![],
                    drop: Some(Drop {
                        container: region.container.clone(),
                        place: Place::Panel { zone, width },
                        index: Some(index),
                    }),
                    known: false,
                }
            }
            Kind::Sidebar => {
                let (index, marker) = self.sidebar_slot(region, &w.id, x, y);
                // Coming from another surface it takes its sidebar variant's default span,
                // at most the sidebar grid's width.
                let columns = self
                    .layout
                    .monitors
                    .get(self.monitor)
                    .and_then(|m| m.containers.iter().find(|c| c.id == region.container))
                    .and_then(|c| match &c.kind {
                        ContainerKind::Sidebar(s) => Some(s.columns.max(1)),
                        _ => None,
                    })
                    .unwrap_or(1);
                let size = sidebar_drop_size(self.registry, w, columns);
                let ghost = Rect {
                    x: x - g.start.w / 2.0,
                    y: y - g.start.h / 2.0,
                    ..g.start
                };
                if !self.sidebar_fits(region, c, w, size, Some(index)) {
                    return Preview {
                        ghost,
                        valid: false,
                        target: Some(region.rect),
                        raw: carried,
                        ..Preview::default()
                    };
                }
                Preview {
                    ghost,
                    raw: carried,
                    valid: true,
                    target: Some(region.rect),
                    guides: vec![],
                    marker: Some(marker),
                    refusal: None,
                    also: vec![],
                    drop: Some(Drop {
                        container: region.container.clone(),
                        place: Place::Sidebar { size },
                        index: Some(index),
                    }),
                    known: false,
                }
            }
        }
    }

    /// The zone and the index in it for a drop at (x, y) on a panel, and the insertion marker.
    /// The centre zone spans its widgets (the middle third of the panel when it has none);
    /// the start and end zones take the rest on either side. The index counts every widget the
    /// layout has in the zone, also one that draws nothing now (weather that is off, a battery
    /// on a desktop PC): the drop goes right before the drawn widget the marker stands before.
    /// Where a separator lies between the two drawn widgets the pointer is between, they are in
    /// two groups: the half of the gap the pointer is in says which one it joins. `own` is where
    /// the carried widget stood on this panel: let go between the drawn widgets it stood
    /// between, it stays where it is in the layout, also alone between two separators.
    pub fn panel_slot(
        &self,
        region: &Region,
        id: &str,
        own: Option<Rect>,
        x: f64,
        y: f64,
    ) -> (Zone, usize, Rect) {
        let v = region.vertical;
        let along = |r: &Rect| {
            if v {
                (r.y, r.bottom())
            } else {
                (r.x, r.right())
            }
        };
        let (lo, hi) = along(&region.rect);
        let p = if v { y } else { x };
        let zone_of = |fid: &str| {
            self.widget(fid).and_then(|(_, w)| match w.place {
                Place::Panel { zone, .. } => Some(zone),
                _ => None,
            })
        };
        let mut frames: Vec<(&Frame, Zone)> = self
            .frames
            .iter()
            .filter(|f| f.container == region.container && f.id != id)
            .filter_map(|f| zone_of(&f.id).map(|z| (f, z)))
            .collect();
        frames.sort_by(|a, b| along(&a.0.rect).0.total_cmp(&along(&b.0.rect).0));
        let center: Vec<&Frame> = frames
            .iter()
            .filter(|(_, z)| *z == Zone::Center)
            .map(|(f, _)| *f)
            .collect();
        let (c_lo, c_hi) = match (center.first(), center.last()) {
            (Some(a), Some(b)) => (along(&a.rect).0, along(&b.rect).1),
            _ => (lo + (hi - lo) / 3.0, lo + 2.0 * (hi - lo) / 3.0),
        };
        let zone = if p < c_lo {
            Zone::Start
        } else if p <= c_hi {
            Zone::Center
        } else {
            Zone::End
        };
        let peers: Vec<&Frame> = frames
            .iter()
            .filter(|(_, z)| *z == zone)
            .map(|(f, _)| *f)
            .collect();
        let index = peers
            .iter()
            .filter(|f| {
                let (a, b) = along(&f.rect);
                (a + b) / 2.0 < p
            })
            .count();
        let (prev, next) = (
            index.checked_sub(1).and_then(|i| peers.get(i)),
            peers.get(index),
        );
        let pos = match (prev, next) {
            (Some(prev), Some(next)) => (along(&prev.rect).1 + along(&next.rect).0) / 2.0,
            (Some(prev), None) => along(&prev.rect).1,
            (None, Some(next)) => along(&next.rect).0,
            (None, None) => match zone {
                Zone::Start => lo,
                Zone::Center => (c_lo + c_hi) / 2.0,
                Zone::End => hi,
            },
        };
        let zone_widgets: Vec<&crate::layout_model::Widget> = self
            .layout
            .monitors
            .get(self.monitor)
            .and_then(|m| m.containers.iter().find(|c| c.id == region.container))
            .map(|c| {
                c.widgets
                    .iter()
                    .filter(|w| zone_of(&w.id) == Some(zone))
                    .collect()
            })
            .unwrap_or_default();
        // Where it is in its zone now: as many of the others stand before it.
        let stood = zone_widgets.iter().position(|w| w.id == id);
        let order: Vec<&crate::layout_model::Widget> =
            zone_widgets.into_iter().filter(|w| w.id != id).collect();
        let at = |f: &&Frame| order.iter().position(|w| w.id == f.id);
        // The places in the layout between the two drawn widgets: more than one where widgets
        // that draw nothing stand between them.
        let first = prev.and_then(at).map_or(0, |i| i + 1);
        let last = match next {
            Some(next) => at(next).unwrap_or(index),
            None => peers.last().and_then(at).map_or(index, |i| i + 1),
        };
        let parted = order
            .get(first..last)
            .is_some_and(|between| between.iter().any(|w| w.r#type == "separator"));
        // Its own place holds for the whole gap it stands in, up to the drawn widgets on either
        // side: carried a little way, it does not leave its group for the one next to it.
        let gap = (
            prev.map_or(lo, |f| along(&f.rect).1),
            next.map_or(hi, |f| along(&f.rect).0),
        );
        let kept = stood
            .filter(|i| (first..=last).contains(i) && (gap.0..=gap.1).contains(&p))
            .zip(own.map(|r| along(&r)));
        let (index, pos) = match (kept, prev, next) {
            (Some((i, (a, _))), ..) => (i, a),
            (None, Some(prev), Some(next)) if parted => {
                if p < pos {
                    (first, along(&prev.rect).1)
                } else {
                    (last, along(&next.rect).0)
                }
            }
            _ => (last, pos),
        };
        let t = 3.0;
        let marker = if v {
            Rect {
                x: region.rect.x,
                y: (pos - t / 2.0).clamp(lo, hi - t),
                w: region.rect.w,
                h: t,
            }
        } else {
            Rect {
                x: (pos - t / 2.0).clamp(lo, hi - t),
                y: region.rect.y,
                w: t,
                h: region.rect.h,
            }
        };
        (zone, index, marker)
    }

    /// The index for a drop at (x, y) among a sidebar's widgets (row by row, left to right)
    /// and the insertion marker. Within a row of several widgets the pointer's side of a
    /// widget decides and the marker is a vertical bar between them; a widget alone in its row
    /// (a column of widgets) goes by halves top and bottom, with a horizontal bar across the row
    /// above the widget the drop goes before, as panel markers lie across their flow too.
    pub fn sidebar_slot(&self, region: &Region, id: &str, x: f64, y: f64) -> (usize, Rect) {
        let order: Vec<&str> = self
            .layout
            .monitors
            .get(self.monitor)
            .and_then(|m| m.containers.iter().find(|c| c.id == region.container))
            .map(|c| {
                c.widgets
                    .iter()
                    .map(|w| w.id.as_str())
                    .filter(|w| *w != id)
                    .collect()
            })
            .unwrap_or_default();
        let frames: Vec<&Frame> = order.iter().filter_map(|i| self.frame(i)).collect();
        let same_row = |a: &Rect, b: &Rect| a.y < b.bottom() && b.y < a.bottom();
        let shares_row = |f: &Frame| {
            frames
                .iter()
                .any(|o| o.id != f.id && same_row(&o.rect, &f.rect))
        };
        let before = |f: &Frame| {
            let (cx, cy) = f.rect.center();
            f.rect.bottom() <= y || (f.rect.y <= y && if shares_row(f) { cx < x } else { cy < y })
        };
        let index = frames.iter().take_while(|f| before(f)).count();
        let t = 3.0;
        let (top, bottom) = (region.rect.y, region.rect.bottom() - t);
        let across = |f: &Frame, at: f64| {
            let (l, r) = frames
                .iter()
                .filter(|o| same_row(&o.rect, &f.rect))
                .fold((f.rect.x, f.rect.right()), |(l, r), o| {
                    (l.min(o.rect.x), r.max(o.rect.right()))
                });
            Rect {
                x: l,
                y: (at - t / 2.0).clamp(top, bottom.max(top)),
                w: r - l,
                h: t,
            }
        };
        let prev = index.checked_sub(1).and_then(|i| frames.get(i)).copied();
        let marker = match (prev, frames.get(index).copied()) {
            (Some(p), Some(n)) if same_row(&p.rect, &n.rect) => Rect {
                x: (p.rect.right() + n.rect.x - t) / 2.0,
                y: n.rect.y,
                w: t,
                h: n.rect.h,
            },
            (Some(p), Some(n)) => across(n, (p.rect.bottom() + n.rect.y) / 2.0),
            (None, Some(n)) => across(n, n.rect.y - t),
            (Some(p), None) if shares_row(p) && p.rect.y <= y && y < p.rect.bottom() => Rect {
                x: p.rect.right().min(region.rect.right() - t),
                y: p.rect.y,
                w: t,
                h: p.rect.h,
            },
            (Some(p), None) => across(p, p.rect.bottom() + t),
            (None, None) => Rect {
                x: region.rect.x,
                y: region.rect.y,
                w: region.rect.w,
                h: t,
            },
        };
        (index, marker)
    }

    /// The edit a finished gesture makes, if any.
    pub fn commit(&self, g: &Gesture) -> Option<Vec<Edit>> {
        let p = g.preview.as_ref().filter(|p| p.valid)?;
        let d = p.drop.clone()?;
        let mut edits = vec![Edit::MoveWidget {
            monitor: self.monitor,
            id: g.id.clone(),
            container: d.container,
            place: d.place,
            index: d.index,
        }];
        edits.extend(p.also.iter().cloned());
        Some(edits)
    }

    /// The edits that leave `len` px of room beside panel widget `w` at its leading or trailing
    /// end, where `beside` is the gap the layout has there now: that one made as long or taken
    /// out, or a new one put in.
    fn gap_edits(
        &self,
        c: &crate::layout_model::Container,
        w: &crate::layout_model::Widget,
        zone: Zone,
        leading: bool,
        beside: Option<&crate::layout_model::Widget>,
        len: u32,
    ) -> Vec<Edit> {
        let place = Place::Panel {
            zone,
            width: Width::Px(len),
        };
        match (beside, len) {
            (None, 0) => vec![],
            (Some(b), 0) => vec![Edit::RemoveWidget {
                monitor: self.monitor,
                id: b.id.clone(),
            }],
            (Some(b), _) if b.place == place => vec![],
            (Some(b), _) => vec![Edit::MoveWidget {
                monitor: self.monitor,
                id: b.id.clone(),
                container: c.id.clone(),
                place,
                index: None,
            }],
            (None, _) => {
                // Among the zone's widgets, the gaps not counted.
                let at = c
                    .widgets
                    .iter()
                    .filter(|o| matches!(o.place, Place::Panel { zone: z, .. } if z == zone))
                    .filter(|o| !crate::layout_model::is_gap(o))
                    .position(|o| o.id == w.id);
                let Some(m) = self.layout.monitors.get(self.monitor) else {
                    return vec![];
                };
                vec![Edit::AddWidget {
                    monitor: self.monitor,
                    container: c.id.clone(),
                    widget: crate::layout_model::Widget {
                        id: m.fresh_id(if leading { "gap-before" } else { "gap-after" }),
                        r#type: "spacer".into(),
                        place,
                        settings: Default::default(),
                    },
                    index: at.map(|at| if leading { at } else { at + 1 }),
                }]
            }
        }
    }

    /// The edits that hold panel `c`'s center zone `past` px further off the middle of the
    /// panel, away from the end a widget of it grew at (`leading`: its first), so that end
    /// alone went: half of it is how far the zone goes, as a spacer of that length at the
    /// zone's other end puts it ([`crate::layout_model::shifts`]). What holds it off the
    /// other way is taken back first; the zone has such a gap at one end at most.
    fn shift_edits(
        &self,
        c: &crate::layout_model::Container,
        leading: bool,
        past: u32,
    ) -> Vec<Edit> {
        let zone = Zone::Center;
        let peers: Vec<_> = c
            .widgets
            .iter()
            .filter(|o| matches!(o.place, Place::Panel { zone: z, .. } if z == zone))
            .collect();
        let first = peers
            .iter()
            .position(|o| !crate::layout_model::is_gap(o))
            .unwrap_or(peers.len());
        let len = |o: &crate::layout_model::Widget| match o.place {
            Place::Panel {
                width: Width::Px(n),
                ..
            } => i64::from(n),
            _ => 0,
        };
        let shifts = |o: &&&crate::layout_model::Widget| crate::layout_model::shifts(o);
        let before = peers[..first].iter().find(shifts).copied();
        let after = peers[first..].iter().rev().find(shifts).copied();
        let net = before.map_or(0, len) - after.map_or(0, len)
            + if leading { -1 } else { 1 } * i64::from(past);
        let mut edits = vec![];
        for (gap, want, index) in [(before, net.max(0), Some(0)), (after, (-net).max(0), None)] {
            let place = Place::Panel {
                zone,
                width: Width::Px(want.min(i64::from(crate::layout_model::MAX_PX)) as u32),
            };
            match (gap, want) {
                (None, 0) => {}
                (Some(g), 0) => edits.push(Edit::RemoveWidget {
                    monitor: self.monitor,
                    id: g.id.clone(),
                }),
                (Some(g), _) if g.place == place => {}
                (Some(g), _) => edits.push(Edit::MoveWidget {
                    monitor: self.monitor,
                    id: g.id.clone(),
                    container: c.id.clone(),
                    place,
                    index: None,
                }),
                (None, _) => {
                    let Some(m) = self.layout.monitors.get(self.monitor) else {
                        continue;
                    };
                    edits.push(Edit::AddWidget {
                        monitor: self.monitor,
                        container: c.id.clone(),
                        widget: crate::layout_model::Widget {
                            id: m.fresh_id("gap-shift"),
                            r#type: "spacer".into(),
                            place,
                            settings: Default::default(),
                        },
                        index,
                    });
                }
            }
        }
        edits
    }

    /// The edit for an arrow key on the selected widget: a desktop widget moves by `step`
    /// pixels (one grid cell on a grid), a panel or sidebar widget swaps with its neighbour.
    pub fn nudge(&self, id: &str, dx: i32, dy: i32, step: i32) -> Option<Edit> {
        let (c, w) = self.widget(id)?;
        match w.place {
            Place::Desktop { x, y, .. } => {
                let grid = self
                    .regions
                    .iter()
                    .find(|r| r.container == c.id)
                    .map_or(0, |r| r.grid);
                let step = if grid > 0 { grid as i32 } else { step };
                let snap = if grid > 0 {
                    Snap::Grid { cell: grid }
                } else {
                    Snap::Free
                };
                let natural = |wd: &crate::layout_model::Widget| self.natural_size(wd);
                desktop::move_to(
                    self.layout,
                    self.monitor,
                    id,
                    x + dx * step,
                    y + dy * step,
                    self.desktop_bounds(),
                    snap,
                    &natural,
                    &|wd| self.shows(wd),
                )
                .ok()
            }
            Place::Panel { zone, width } => {
                let peers: Vec<&str> = c
                    .widgets
                    .iter()
                    .filter(|o| matches!(o.place, Place::Panel { zone: z, .. } if z == zone))
                    .map(|o| o.id.as_str())
                    .collect();
                let at = peers.iter().position(|p| *p == id)?;
                let to = at.checked_add_signed((dx + dy) as isize)?;
                (to < peers.len()).then(|| Edit::MoveWidget {
                    monitor: self.monitor,
                    id: id.to_string(),
                    container: c.id.clone(),
                    place: Place::Panel { zone, width },
                    index: Some(to),
                })
            }
            Place::Sidebar { size } => {
                let at = c.widgets.iter().position(|o| o.id == id)?;
                let to = at.checked_add_signed((dx + dy) as isize)?;
                let fits = self
                    .regions
                    .iter()
                    .find(|r| r.container == c.id)
                    .is_none_or(|r| self.sidebar_fits(r, c, w, size, Some(to)));
                (to < c.widgets.len() && fits).then(|| Edit::MoveWidget {
                    monitor: self.monitor,
                    id: id.to_string(),
                    container: c.id.clone(),
                    place: Place::Sidebar { size },
                    index: Some(to),
                })
            }
        }
    }

    /// Where widget `id` sits, for its settings dialog: a panel widget's zone and width mode, a
    /// sidebar widget's span with the sidebar's columns, a desktop widget's size and limits.
    pub fn placement(&self, id: &str) -> Option<Sits> {
        let (c, w) = self.widget(id)?;
        match (&w.place, &c.kind) {
            (&Place::Desktop { size, .. }, _) => {
                let lim = self.limits(&w.r#type, Placement::Desktop);
                let b = self.desktop_bounds();
                let min = (lim.min.0.min(b.w), lim.min.1.min(b.h));
                let shown = self.desktop_shown(w, 0, 0, size);
                Some(Sits::Desktop {
                    size: (shown.w, shown.h),
                    min,
                    max: (lim.max.0.min(b.w).max(min.0), lim.max.1.min(b.h).max(min.1)),
                })
            }
            (&Place::Panel { zone, width }, _) => Some(Sits::Panel { zone, width }),
            (&Place::Sidebar { size }, ContainerKind::Sidebar(s)) => Some(Sits::Sidebar {
                size,
                columns: s.columns.max(1),
            }),
            _ => None,
        }
    }

    /// Whether the sidebar that holds `id` needs more height than it has, by what it measured:
    /// it hides its last sections then. A widget added from the catalog is measured only once
    /// the sidebar lays it out, so this is asked after the add.
    pub fn overflows(&self, id: &str) -> bool {
        use crate::layout_model::sidebar;
        let Some((c, _)) = self.widget(id) else {
            return false;
        };
        let Some(room) = self
            .regions
            .iter()
            .find(|r| r.container == c.id)
            .and_then(|r| r.sidebar.as_ref())
        else {
            return false;
        };
        sidebar::sections(c)
            .and_then(|(_, s)| {
                sidebar::height(
                    &s,
                    |cell| room.heights.get(&cell.id).copied(),
                    &room.metrics,
                )
            })
            .is_some_and(|need| need > room.available + 1e-6)
    }

    /// The edit that moves panel widget `id` to `zone`, next to the widgets already there
    /// (before the end zone's, after the others'). `None` when it is there already, or the zone
    /// has no room for it at the length it shows.
    pub fn to_zone(&self, id: &str, zone: Zone) -> Option<Edit> {
        let (c, w) = self.widget(id)?;
        let Place::Panel { zone: from, width } = w.place.clone() else {
            return None;
        };
        if from == zone {
            return None;
        }
        let region = self.regions.iter().find(|r| r.container == c.id);
        let frame = self.frames.iter().find(|f| f.id == id);
        if let (Some(r), Some(f)) = (region, frame) {
            let length = if r.vertical { f.rect.h } else { f.rect.w };
            if length > r.room[zone as usize] {
                return None;
            }
        }
        Some(Edit::MoveWidget {
            monitor: self.monitor,
            id: id.to_string(),
            container: c.id.clone(),
            place: Place::Panel { zone, width },
            index: (zone == Zone::End).then_some(0),
        })
    }

    /// The edit that gives desktop widget `id` the size `w`×`h`, its top-left corner where it
    /// is: within its type's limits, on the monitor and off the other widgets, as a resize by
    /// the bottom-right handle. `None` when it has that size already or nothing fits.
    pub fn to_size(&self, id: &str, w: u32, h: u32) -> Option<Edit> {
        let (c, wd) = self.widget(id)?;
        let Place::Desktop { x, y, size } = wd.place else {
            return None;
        };
        let grid = self
            .regions
            .iter()
            .find(|r| r.container == c.id)
            .map_or(0, |r| r.grid);
        let to = desktop::resize(
            self.desktop_shown(wd, x, y, size),
            w,
            h,
            self.limits(&wd.r#type, Placement::Desktop),
            &self.desktop_others(&c.id, &wd.id),
            self.desktop_bounds(),
            if grid > 0 {
                Snap::Grid { cell: grid }
            } else {
                Snap::Free
            },
        )?;
        (size != Some((to.w, to.h))).then(|| Edit::MoveWidget {
            monitor: self.monitor,
            id: id.to_string(),
            container: c.id.clone(),
            place: Place::Desktop {
                x: to.x,
                y: to.y,
                size: Some((to.w, to.h)),
            },
            index: None,
        })
    }

    /// The edit that makes sidebar widget `id` span `size` cells. `None` when it does already,
    /// the sidebar has no such column count, or it would not hold the widget that wide.
    pub fn to_span(&self, id: &str, size: u32) -> Option<Edit> {
        let (c, w) = self.widget(id)?;
        let (&Place::Sidebar { size: now }, ContainerKind::Sidebar(s)) = (&w.place, &c.kind) else {
            return None;
        };
        if now == size || size == 0 || size > s.columns.max(1) {
            return None;
        }
        let fits = self
            .regions
            .iter()
            .find(|r| r.container == c.id)
            .is_none_or(|r| self.sidebar_fits(r, c, w, size, None));
        fits.then(|| Edit::MoveWidget {
            monitor: self.monitor,
            id: id.to_string(),
            container: c.id.clone(),
            place: Place::Sidebar { size },
            index: None,
        })
    }

    /// The edit that puts a new widget of type `ty` on the monitor's surface `kind`, and the
    /// id it gets: on the desktop the free spot nearest the middle at the type's default size,
    /// on a panel the first of the end, start and centre zones with room for it, in a sidebar
    /// after its last widget. `None` where the type may not go, the monitor has no such
    /// container, or there is no room.
    pub fn add(&self, ty: &str, kind: Kind) -> Option<(Edit, String)> {
        let t = self.registry.get(ty)?;
        let variant = t.variant(kind.placement())?;
        let m = self.layout.monitors.get(self.monitor)?;
        // The container shown on the monitor, else the first of the kind.
        let c = m
            .containers
            .iter()
            .filter(|c| Kind::of(&c.kind) == kind)
            .min_by_key(|c| !self.regions.iter().any(|r| r.container == c.id) as u8)?;
        let region = self.regions.iter().find(|r| r.container == c.id);
        let taken = |id: &str| {
            m.containers
                .iter()
                .any(|c| c.id == id || c.widgets.iter().any(|w| w.id == id))
        };
        let id = (1u32..)
            .map(|n| format!("{ty}-{n}"))
            .find(|id| !taken(id))?;
        let widget = |place| crate::layout_model::Widget {
            id: id.clone(),
            r#type: ty.to_string(),
            place,
            settings: toml::Table::new(),
        };
        let (place, index) = match kind {
            Kind::Desktop => {
                let lim = self.limits(ty, Placement::Desktop);
                let (w, h) = (
                    variant
                        .size
                        .default
                        .w
                        .clamp(lim.min.0 as f64, lim.max.0 as f64),
                    variant
                        .size
                        .default
                        .h
                        .clamp(lim.min.1 as f64, lim.max.1 as f64),
                );
                let want = Rect {
                    x: (self.bounds.0 - w) / 2.0,
                    y: (self.bounds.1 - h) / 2.0,
                    w,
                    h,
                };
                let snap = match region.map_or(0, |r| r.grid) {
                    0 => Snap::Free,
                    cell => Snap::Grid { cell },
                };
                let others = self.desktop_others(&c.id, &id);
                let r =
                    desktop::nearest_free(want.snapped(), &others, self.desktop_bounds(), snap)?;
                (
                    Place::Desktop {
                        x: r.x,
                        y: r.y,
                        size: None,
                    },
                    None,
                )
            }
            Kind::Panel => {
                let vertical = region.is_some_and(|r| r.vertical);
                let length = if vertical {
                    variant.size.default.h
                } else {
                    variant.size.default.w
                };
                let room = region.map_or([f64::INFINITY; 3], |r| r.room);
                let zone = [Zone::End, Zone::Start, Zone::Center]
                    .into_iter()
                    .find(|z| length <= room[*z as usize])?;
                // Next to the widgets already there: before the end zone's, after the others'.
                let index = (zone == Zone::End).then_some(0);
                (
                    Place::Panel {
                        zone,
                        width: Width::Auto,
                    },
                    index,
                )
            }
            Kind::Sidebar => {
                let columns = match &c.kind {
                    ContainerKind::Sidebar(s) => s.columns.max(1),
                    _ => 1,
                };
                let size = variant.size.default.w.round().clamp(1.0, columns as f64) as u32;
                if let Some(region) = region {
                    // From no container of the monitor: the sidebar must hold it whole.
                    let mut nowhere = c.clone();
                    nowhere.id.clear();
                    nowhere.widgets.clear();
                    if !self.sidebar_fits(
                        region,
                        &nowhere,
                        &widget(Place::Sidebar { size }),
                        size,
                        None,
                    ) {
                        return None;
                    }
                }
                (Place::Sidebar { size }, None)
            }
        };
        Some((
            Edit::AddWidget {
                monitor: self.monitor,
                container: c.id.clone(),
                widget: widget(place),
                index,
            },
            id,
        ))
    }
}

/// Where a window sits on its monitor, as the shell placed it (wlr-layer-shell anchors,
/// margins and the other surfaces' exclusive zones).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WinRect {
    pub connector: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

const WINDOW_TYPES: [&str; 5] = [
    "PanelWindow",
    "PopupWindow",
    "FloatingWindow",
    "ApplicationWindow",
    "Window",
];

fn is_controller(w: &World, item: usize) -> bool {
    w.items
        .get(item)
        .is_some_and(|i| i.types.iter().any(|t| t == TYPE))
}

fn text(w: &mut World, item: usize, k: &str) -> String {
    match w.read(item, k) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

/// The cells `w` spans dropped into a sidebar of `columns`: its own span when it is in one, else
/// its sidebar variant's default width.
pub(crate) fn sidebar_drop_size(
    registry: &crate::widgets::Registry,
    w: &crate::layout_model::Widget,
    columns: u32,
) -> u32 {
    match w.place {
        Place::Sidebar { size } => size,
        _ => registry
            .get(&w.r#type)
            .and_then(|t| t.variant(Placement::Sidebar))
            .map_or(1, |v| {
                v.size.default.w.round().clamp(1.0, columns as f64) as u32
            }),
    }
}

fn num(w: &mut World, item: usize, k: &str) -> f64 {
    w.read(item, k)
        .map(|v| v.to_number())
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
}

/// The window an item is shown in (its top-level item when no window type is above it, as
/// for a bare `impulse-shell -p` root), unless it or an ancestor is hidden.
fn shown_window(w: &mut World, item: usize) -> Option<usize> {
    let mut cur = Some(item);
    let mut top = item;
    while let Some(i) = cur {
        top = i;
        if w.dead.contains(&i) || w.read(i, "visible").is_some_and(|v| !v.truthy()) {
            return None;
        }
        if w.items[i]
            .types
            .iter()
            .any(|t| WINDOW_TYPES.iter().any(|x| t == x))
        {
            return Some(i);
        }
        cur = w.items[i].parent;
    }
    Some(top)
}

/// The monitor and area of a window: the shell's placement, else its screen at the origin.
fn window_rect(w: &mut World, win: usize) -> WinRect {
    if let Some(r) = w.window_rects.get(&win) {
        return r.clone();
    }
    let connector = match w.read(win, "screen") {
        Some(Value::Item(sc)) => text(w, sc, "name"),
        _ => String::new(),
    };
    WinRect {
        connector,
        x: 0.0,
        y: 0.0,
        w: num(w, win, "width"),
        h: num(w, win, "height"),
    }
}

/// `rect` (the item's, in output coordinates) cut to the clipping ancestors below its window: a
/// widget scrolled out of a sidebar has no frame where it isn't drawn.
fn clipped(w: &mut World, item: usize, win: usize, mut rect: Rect, origin: (f64, f64)) -> Rect {
    let mut cur = w.items[item].parent;
    while let Some(i) = cur {
        if i == win {
            break;
        }
        if w.read(i, "clip").is_some_and(|v| v.truthy()) {
            let (x, y) = w.scene_pos(i, Some(win));
            let (x0, y0) = (origin.0 + x, origin.1 + y);
            let (x1, y1) = (x0 + num(w, i, "width"), y0 + num(w, i, "height"));
            let (l, t) = (rect.x.max(x0), rect.y.max(y0));
            let (r, b) = ((rect.x + rect.w).min(x1), (rect.y + rect.h).min(y1));
            rect = Rect {
                x: l,
                y: t,
                w: (r - l).max(0.0),
                h: (b - t).max(0.0),
            };
        }
        cur = w.items[i].parent;
    }
    rect
}

/// The widgets and containers shown on `connector`, in output coordinates.
fn gather(w: &mut World, connector: &str) -> (Vec<Frame>, Vec<Region>) {
    let mut frames = vec![];
    let mut regions = vec![];
    let Some(m) = w
        .layout
        .as_ref()
        .and_then(|l| l.monitor(connector))
        .cloned()
    else {
        return (frames, regions);
    };
    let hosts: Vec<usize> = w.of_type("WidgetHost");
    for h in hosts {
        let id = text(w, h, "layoutId");
        if id.is_empty() {
            continue;
        }
        let Some(win) = shown_window(w, h) else {
            continue;
        };
        let wr = window_rect(w, win);
        if wr.connector != connector {
            continue;
        }
        let Some((ci, wd)) = m.widget(&id) else {
            continue;
        };
        let c = &m.containers[ci];
        let (sx, sy) = w.scene_pos(h, Some(win));
        let own = Rect {
            x: wr.x + sx,
            y: wr.y + sy,
            w: num(w, h, "width"),
            h: num(w, h, "height"),
        };
        let rect = clipped(w, h, win, own, (wr.x, wr.y));
        if rect.w <= 0.0 || rect.h <= 0.0 {
            continue;
        }
        frames.push(Frame {
            id,
            ty: wd.r#type.clone(),
            container: c.id.clone(),
            kind: Kind::of(&c.kind),
            rect,
            resizable: true,
            least: Some(num(w, h, "contentLength"))
                .filter(|l| w.items[h].props.contains_key("contentLength") && l.is_finite())
                .map_or(0.0, |l| l.max(0.0)),
            clear: Some(num(w, h, "clearLength"))
                .filter(|l| w.items[h].props.contains_key("clearLength") && l.is_finite())
                .map_or(0.0, |l| l.max(0.0)),
        });
    }
    for ty in [
        "PanelContainer",
        "SidebarContainer",
        crate::desktop_container::TYPE,
    ] {
        for i in w.of_type(ty) {
            // The output the container's window is on, as for frames: a sidebar's `connector`
            // is only written once its content has loaded.
            let Some(win) = shown_window(w, i) else {
                continue;
            };
            let wr = window_rect(w, win);
            if wr.connector != connector {
                continue;
            }
            let cid = text(w, i, "containerId");
            let (kind, container) = match ty {
                "PanelContainer" => (Kind::Panel, text(w, i, "panelId")),
                "SidebarContainer" => (
                    Kind::Sidebar,
                    m.containers
                        .iter()
                        .find(|c| {
                            matches!(c.kind, ContainerKind::Sidebar(_))
                                && (cid.is_empty() || c.id == cid)
                        })
                        .map_or(String::new(), |c| c.id.clone()),
                ),
                _ => (
                    Kind::Desktop,
                    m.containers
                        .iter()
                        .find(|c| {
                            matches!(c.kind, ContainerKind::Desktop)
                                && (cid.is_empty() || c.id == cid)
                        })
                        .map_or(String::new(), |c| c.id.clone()),
                ),
            };
            if container.is_empty() || regions.iter().any(|r: &Region| r.container == container) {
                continue;
            }
            let grid = if kind == Kind::Desktop {
                let g = num(w, i, "gridSize");
                if g >= 1.0 { g.round() as u32 } else { 0 }
            } else {
                0
            };
            let vertical = kind == Kind::Panel && w.read(i, "vertical").is_some_and(|v| v.truthy());
            let room = ["startRoom", "centerRoom", "endRoom"].map(|k| {
                if kind == Kind::Panel && w.explicit(i, k) {
                    num(w, i, k).max(0.0)
                } else {
                    f64::INFINITY
                }
            });
            let lengths = match (kind == Kind::Panel)
                .then(|| w.read(i, "panelLengths"))
                .flatten()
            {
                Some(Value::Obj(o)) => o
                    .borrow()
                    .props
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_number()))
                    .collect(),
                _ => HashMap::new(),
            };
            let sidebar = (kind == Kind::Sidebar && w.explicit(i, "available")).then(|| {
                let heights = match w.read(i, "sidebarHeights") {
                    Some(Value::Obj(o)) => o
                        .borrow()
                        .props
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_number()))
                        .filter(|(_, v)| v.is_finite())
                        .collect(),
                    _ => HashMap::new(),
                };
                SidebarRoom {
                    available: num(w, i, "available"),
                    heights,
                    metrics: crate::layout_model::sidebar::Metrics {
                        cell_padding: num(w, i, "cellPadding"),
                        cell_spacing: num(w, i, "cellSpacing"),
                        section_spacing: num(w, i, "sectionSpacing"),
                    },
                }
            });
            regions.push(Region {
                container,
                kind,
                rect: Rect {
                    x: wr.x,
                    y: wr.y,
                    w: wr.w,
                    h: wr.h,
                },
                vertical,
                grid,
                room,
                lengths,
                sidebar,
            });
        }
    }
    // The dragged widget shows where the drag would put it (see [`live_desktop`]); its frame
    // stays where the drag started, so `frames` is unchanged for the whole gesture.
    if let Some(g) = w
        .edit
        .gesture
        .as_mut()
        .filter(|g| g.dragging && g.connector == connector)
        && let Some(f) = frames.iter_mut().find(|f| f.id == g.id)
    {
        g.shown = Some(f.rect);
        f.rect = g.start;
    }
    for f in &mut frames {
        let vertical = regions
            .iter()
            .any(|r| r.container == f.container && r.vertical);
        f.resizable = resizable(&w.widgets, &f.ty, f.kind, vertical);
    }
    (frames, regions)
}

/// Where a desktop widget of `container` shows mid-drag: a move where the pointer has it,
/// whether or not it could land there, a resize at the size it would get, so the widget itself
/// follows the pointer (as in Plasma); nothing is written until the drop. The
/// flag: a resize, which sizes even a widget that otherwise takes its natural size.
pub(crate) fn live_desktop(
    w: &World,
    container: &crate::layout_model::Container,
) -> Option<(String, DRect, bool)> {
    let g = w.edit.gesture.as_ref().filter(|g| g.dragging)?;
    let p = g.preview.as_ref()?;
    if !container.widgets.iter().any(|wd| wd.id == g.id) {
        return None;
    }
    let px = |v: f64| v.round().clamp(1.0, u32::MAX as f64) as u32;
    let r = match g.mode {
        // A move shows the widget where it would land on its desktop: it takes the snap and
        // stops at the screen's edge itself, with no box of its landing place coming and going
        // around it. Where it cannot land it is where the pointer has it.
        Mode::Move => {
            let raw = p.raw?;
            let lands = p.drop.as_ref().is_some_and(|d| {
                p.valid && d.container == container.id && matches!(d.place, Place::Desktop { .. })
            });
            let at = if lands { p.ghost } else { raw };
            DRect {
                x: at.x.round() as i32,
                y: at.y.round() as i32,
                w: px(raw.w),
                h: px(raw.h),
            }
        }
        Mode::Resize => {
            let d = p
                .drop
                .as_ref()
                .filter(|d| p.valid && d.container == container.id)?;
            let Place::Desktop { x, y, .. } = d.place else {
                return None;
            };
            DRect {
                x,
                y,
                w: px(p.ghost.w),
                h: px(p.ghost.h),
            }
        }
    };
    Some((g.id.clone(), r, g.mode == Mode::Resize))
}

/// The columns a widget of the sidebar `container` spans while a resize pulls it: what the
/// resize would give it, so the widget itself takes the form of that size at once, as a
/// desktop widget does ([`live_desktop`]), and its text is gone by the time its outline is a
/// circle's; nothing is written until the drop.
pub(crate) fn live_sidebar(
    w: &World,
    container: &crate::layout_model::Container,
) -> Option<(String, u32)> {
    let g = w
        .edit
        .gesture
        .as_ref()
        .filter(|g| g.dragging && g.mode == Mode::Resize)?;
    let p = g.preview.as_ref().filter(|p| p.valid)?;
    let d = p.drop.as_ref().filter(|d| d.container == container.id)?;
    let Place::Sidebar { size } = d.place else {
        return None;
    };
    container
        .widgets
        .iter()
        .any(|wd| wd.id == g.id)
        .then(|| (g.id.clone(), size))
}

/// The monitor size for the controller's overlay: its window, else the desktop's.
fn bounds(w: &mut World, item: usize, regions: &[Region]) -> (f64, f64) {
    if let Some(win) = shown_window(w, item) {
        let r = window_rect(w, win);
        if r.w > 0.0 && r.h > 0.0 {
            return (r.w, r.h);
        }
    }
    regions
        .iter()
        .find(|r| r.kind == Kind::Desktop)
        .map_or((1920.0, 1080.0), |r| (r.rect.w, r.rect.h))
}

/// Runs `f` with the scene of `connector`, its monitor attached to the layout first so edits
/// to a monitor without its own layout go to its own copy.
fn with_scene<R>(w: &mut World, item: usize, f: impl FnOnce(&Scene) -> R) -> Option<R> {
    let connector = text(w, item, "connector");
    if connector.is_empty() || w.layout.is_none() {
        return None;
    }
    let gathered = gather(w, &connector);
    with_gathered(w, item, &connector, gathered, f)
}

/// [`with_scene`] over frames and regions [`gather`] has just given for `connector`.
fn with_gathered<R>(
    w: &mut World,
    item: usize,
    connector: &str,
    (frames, regions): (Vec<Frame>, Vec<Region>),
    f: impl FnOnce(&Scene) -> R,
) -> Option<R> {
    w.layout.as_ref()?;
    let bounds = bounds(w, item, &regions);
    let monitor = w.layout.as_mut()?.attach(connector);
    let registry = w.widgets.clone();
    let measured: Vec<usize> = w.desktop_natural.keys().copied().collect();
    let (mut natural, mut placed) = (HashMap::new(), HashMap::new());
    for it in measured {
        if text(w, it, "connector") == connector {
            let n = &w.desktop_natural[&it];
            natural.extend(n.sizes.iter().map(|(id, s)| (id.clone(), *s)));
            placed.extend(n.shown_elsewhere().map(|(id, s)| (id.clone(), s)));
        }
    }
    let layout = fitted(w.layout.as_ref()?.editor().layout(), &registry);
    let scene = Scene {
        layout: &layout,
        monitor,
        registry: &registry,
        frames,
        regions,
        bounds,
        natural,
        placed,
    };
    Some(f(&scene))
}

fn apply(w: &mut World, edit: Edit) -> bool {
    apply_all(w, &[edit])
}

/// Makes `edits` as one undo step.
fn apply_all(w: &mut World, edits: &[Edit]) -> bool {
    let Some(live) = w.layout.as_mut() else {
        return false;
    };
    match live.apply(edits) {
        Ok(Ok(changed)) => changed,
        Ok(Err(save)) => {
            w.errors.push(format!("EditController: {save}"));
            true
        }
        Err(e) => {
            w.errors.push(format!("EditController: {e}"));
            false
        }
    }
}

fn step(w: &mut World, redo: bool) -> bool {
    let Some(live) = w.layout.as_mut() else {
        return false;
    };
    let r = if redo { live.redo() } else { live.undo() };
    match r {
        Ok(stepped) => stepped,
        Err(save) => {
            w.errors.push(format!("EditController: {save}"));
            true
        }
    }
}

fn set_active(w: &mut World, on: bool) {
    if w.edit.active != on {
        w.edit.active = on;
        w.edit.selected = None;
        w.edit.gesture = None;
        w.edit.hover = None;
        w.edit.rev += 1;
    }
}

fn selected_on(w: &World, connector: &str) -> Option<String> {
    w.edit
        .selected
        .as_ref()
        .filter(|(c, _)| c == connector)
        .map(|(_, id)| id.clone())
}

fn press(w: &mut World, item: usize, x: f64, y: f64) -> bool {
    if !w.edit.active {
        return false;
    }
    let connector = text(w, item, "connector");
    let selected = selected_on(w, &connector);
    let picked = with_scene(w, item, |s| {
        s.pick(selected.as_deref(), x, y)
            .map(|(f, mode, edges)| (f.clone(), mode, edges))
    })
    .flatten();
    w.edit.hover = Some((connector.clone(), x, y));
    // What the panel showed of the pull before is not of this one.
    w.edit.pulled = None;
    w.edit.pending = None;
    w.edit.rev += 1;
    match picked {
        Some((f, mode, edges)) => {
            w.edit.selected = Some((connector.clone(), f.id.clone()));
            w.edit.gesture = Some(Gesture {
                connector,
                id: f.id,
                mode,
                edges,
                press: (x, y),
                start: f.rect,
                kind: f.kind,
                dragging: false,
                preview: None,
                shown: None,
                gain: None,
            });
            true
        }
        None => {
            w.edit.selected = None;
            w.edit.gesture = None;
            false
        }
    }
}

fn motion(w: &mut World, item: usize, x: f64, y: f64) -> bool {
    let connector = text(w, item, "connector");
    let Some(mut g) = w.edit.gesture.take().filter(|g| g.connector == connector) else {
        // No button holds a widget: the overlay shows what the pointer is over.
        if w.edit.active {
            w.edit.hover = Some((connector, x, y));
            w.edit.rev += 1;
        }
        return false;
    };
    w.edit.hover = Some((connector.clone(), x, y));
    if !g.dragging {
        let far = match g.mode {
            Mode::Move => DRAG_DISTANCE,
            Mode::Resize => HANDLE_DRAG_DISTANCE,
        };
        let gone = (x - g.press.0).hypot(y - g.press.1);
        if gone < far {
            w.edit.gesture = Some(g);
            return false;
        }
        // The drag starts where the pointer crossed that distance: the widget does not jump
        // the way the pointer went before it counted as a drag.
        let t = far / gone;
        g.press = (
            g.press.0 + (x - g.press.0) * t,
            g.press.1 + (y - g.press.1) * t,
        );
    }
    g.dragging = true;
    g.preview = None;
    let edits = with_scene(w, item, |s| {
        s.measure(&mut g);
        g.preview = Some(s.preview(&g, x, y));
        s.commit(&g)
    })
    .flatten();
    let next = edits
        .filter(|_| g.mode == Mode::Resize && g.kind == Kind::Panel)
        .and_then(|edits| pulled(w, &g, &edits));
    if g.preview.as_ref().is_some_and(|p| p.known) {
        w.edit.pending = next;
    } else {
        w.edit.pulled = next;
        w.edit.pending = None;
    }
    w.edit.gesture = Some(g);
    w.edit.rev += 1;
    true
}

/// The panel of `g`'s widget with `edits` made, and its monitor's connector.
fn pulled(
    w: &World,
    g: &Gesture,
    edits: &[Edit],
) -> Option<(String, crate::layout_model::Container)> {
    let layout = w.layout.as_ref()?.editor().layout();
    let (mi, container) = edits.iter().find_map(|e| match e {
        Edit::MoveWidget {
            monitor,
            id,
            container,
            ..
        } if *id == g.id => Some((*monitor, container)),
        _ => None,
    })?;
    let next = crate::layout_model::applied(layout, edits).ok()?;
    let c = next
        .monitors
        .get(mi)?
        .containers
        .iter()
        .find(|c| c.id == *container)?;
    Some((g.connector.clone(), c.clone()))
}

/// The connector of the monitor a widget host's window is on, shown or not.
pub(crate) fn host_connector(w: &mut World, host: usize) -> Option<String> {
    let mut cur = Some(host);
    let mut top = host;
    while let Some(i) = cur {
        top = i;
        if w.dead.contains(&i) {
            return None;
        }
        if w.items[i]
            .types
            .iter()
            .any(|t| WINDOW_TYPES.iter().any(|x| t == x))
        {
            break;
        }
        cur = w.items[i].parent;
    }
    Some(window_rect(w, top).connector)
}

/// The pointer rests during a pull by an end: the panel shows the step it is at.
fn rest(w: &mut World) -> bool {
    let Some(next) = w.edit.pending.take().filter(|_| w.edit.gesture.is_some()) else {
        return false;
    };
    w.edit.pulled = Some(next);
    true
}

fn release(w: &mut World, item: usize, x: f64, y: f64) -> bool {
    motion(w, item, x, y);
    let connector = text(w, item, "connector");
    let Some(g) = w.edit.gesture.take().filter(|g| g.connector == connector) else {
        return false;
    };
    w.edit.pending = None;
    w.edit.rev += 1;
    if !g.dragging {
        return false;
    }
    match with_scene(w, item, |s| s.commit(&g)).flatten() {
        Some(edits) => apply_all(w, &edits),
        None => false,
    }
}

fn key(w: &mut World, item: usize, key: i64, mods: i64) -> bool {
    if !w.edit.active {
        return false;
    }
    let ctrl = mods & CTRL != 0;
    let shift = mods & SHIFT != 0;
    if w.edit.gesture.is_some() {
        // Only Esc means something mid-drag: it drops the drag, nothing changes.
        if key == KEY_ESCAPE {
            w.edit.gesture = None;
            w.edit.rev += 1;
            return true;
        }
        return false;
    }
    let connector = text(w, item, "connector");
    let done = match key {
        KEY_ESCAPE => {
            set_active(w, false);
            true
        }
        KEY_Z if ctrl && shift => step(w, true),
        KEY_Y if ctrl => step(w, true),
        KEY_Z if ctrl => step(w, false),
        KEY_DELETE | KEY_BACKSPACE => remove_selected(w, item),
        KEY_LEFT | KEY_RIGHT | KEY_UP | KEY_DOWN => {
            let Some(id) = selected_on(w, &connector) else {
                return false;
            };
            let (dx, dy) = match key {
                KEY_LEFT => (-1, 0),
                KEY_RIGHT => (1, 0),
                KEY_UP => (0, -1),
                _ => (0, 1),
            };
            match with_scene(w, item, |s| {
                s.nudge(&id, dx, dy, if shift { 10 } else { 1 })
            })
            .flatten()
            {
                Some(e) => apply(w, e),
                None => false,
            }
        }
        _ => return false,
    };
    w.edit.rev += 1;
    done || matches!(key, KEY_Z | KEY_Y | KEY_DELETE | KEY_BACKSPACE)
}

fn remove_selected(w: &mut World, item: usize) -> bool {
    let connector = text(w, item, "connector");
    let Some(id) = selected_on(w, &connector) else {
        return false;
    };
    let Some(monitor) = w.layout.as_mut().map(|l| l.attach(&connector)) else {
        return false;
    };
    let removed = apply(w, Edit::RemoveWidget { monitor, id });
    if removed {
        w.edit.selected = None;
    }
    removed
}

/// A panel widget's width mode ("auto", "stretch"): the panel's own resize of its widgets.
fn set_width_mode(w: &mut World, item: usize, mode: &str) -> bool {
    let connector = text(w, item, "connector");
    let Some(id) = selected_on(w, &connector) else {
        return false;
    };
    let width = match mode {
        "auto" => Width::Auto,
        "stretch" => Width::Stretch,
        _ => return false,
    };
    let edit = with_scene(w, item, |s| {
        let (c, wd) = s.widget(&id)?;
        let Place::Panel { zone, .. } = wd.place else {
            return None;
        };
        Some(Edit::MoveWidget {
            monitor: s.monitor,
            id: id.clone(),
            container: c.id.clone(),
            place: Place::Panel { zone, width },
            index: None,
        })
    })
    .flatten();
    edit.is_some_and(|e| apply(w, e))
}

fn zone_name(zone: Zone) -> &'static str {
    match zone {
        Zone::Start => "start",
        Zone::Center => "center",
        Zone::End => "end",
    }
}

/// Moves the selected panel widget to `zone` ("start", "center", "end"): its settings dialog.
fn set_zone(w: &mut World, item: usize, zone: &str) -> bool {
    let connector = text(w, item, "connector");
    let (Some(id), Some(zone)) = (
        selected_on(w, &connector),
        [Zone::Start, Zone::Center, Zone::End]
            .into_iter()
            .find(|z| zone_name(*z) == zone),
    ) else {
        return false;
    };
    let edit = with_scene(w, item, |s| s.to_zone(&id, zone)).flatten();
    edit.is_some_and(|e| apply(w, e))
}

/// Makes the selected sidebar widget span `size` cells: its settings dialog.
fn set_size(w: &mut World, item: usize, width: f64, height: f64) -> bool {
    let connector = text(w, item, "connector");
    let Some(id) = selected_on(w, &connector) else {
        return false;
    };
    let px = |v: f64| (v >= 1.0 && v <= u32::MAX as f64).then(|| v.round() as u32);
    let (Some(width), Some(height)) = (px(width), px(height)) else {
        return false;
    };
    let edit = with_scene(w, item, |s| s.to_size(&id, width, height)).flatten();
    edit.is_some_and(|e| apply(w, e))
}

fn set_span(w: &mut World, item: usize, size: f64) -> bool {
    let connector = text(w, item, "connector");
    let Some(id) = selected_on(w, &connector) else {
        return false;
    };
    if !(size >= 1.0 && size.fract() == 0.0 && size <= u32::MAX as f64) {
        return false;
    }
    let edit = with_scene(w, item, |s| s.to_span(&id, size as u32)).flatten();
    edit.is_some_and(|e| apply(w, e))
}

/// Adds a widget of `ty` to the monitor's `kind` surface ("desktop", "panel", "sidebar") and
/// selects it: the widget catalog's click. Returns the new widget's id, which `selectedId`
/// shows only after the next sync.
fn add_widget(w: &mut World, item: usize, ty: &str, kind: &str) -> Option<String> {
    if !w.edit.active {
        return None;
    }
    let kind = match kind {
        "desktop" => Kind::Desktop,
        "panel" => Kind::Panel,
        "sidebar" => Kind::Sidebar,
        _ => return None,
    };
    let connector = text(w, item, "connector");
    let (edit, id) = with_scene(w, item, |s| s.add(ty, kind)).flatten()?;
    if !apply(w, edit) {
        return None;
    }
    w.edit.selected = Some((connector, id.clone()));
    w.edit.gesture = None;
    Some(id)
}

/// The types the catalog offers, by name: what each is called, its icon, the surfaces it is
/// offered for, and the config flag that shows it on the desktop and the option it places
/// itself by there.
fn catalog(registry: &Registry) -> Vec<Value> {
    let mut types: Vec<_> = registry.iter().collect();
    types.sort_by_key(|t| t.name.to_lowercase());
    types
        .into_iter()
        .map(|t| {
            let desktop = t.variant(Placement::Desktop);
            let enabled_by = desktop
                .and_then(|v| v.enabled_by.as_deref())
                .unwrap_or_default();
            let placed_by = desktop
                .and_then(|v| v.placed_by.as_deref())
                .unwrap_or_default();
            obj(vec![
                ("type", Value::str(&t.id)),
                ("name", Value::str(&t.name)),
                ("icon", Value::str(&t.icon)),
                ("desktop", Value::Bool(t.listed(Placement::Desktop))),
                ("panel", Value::Bool(t.listed(Placement::Panel))),
                ("sidebar", Value::Bool(t.listed(Placement::Sidebar))),
                ("enabledBy", Value::str(enabled_by)),
                ("placedBy", Value::str(placed_by)),
            ])
        })
        .collect()
}

/// The controller's methods; `None` for other items and names.
pub(crate) fn call(
    w: &mut World,
    item: usize,
    name: &str,
    args: Option<&[Value]>,
) -> Option<Value> {
    const NAMES: [&str; 17] = [
        "start",
        "finish",
        "toggle",
        "press",
        "move",
        "rest",
        "release",
        "cancel",
        "key",
        "undo",
        "redo",
        "removeSelected",
        "setWidthMode",
        "addWidget",
        "setZone",
        "setSpan",
        "setSize",
    ];
    if !NAMES.contains(&name) || !is_controller(w, item) {
        return None;
    }
    // `None` is Host::has_method's probe: answer it without acting.
    let Some(args) = args else {
        return Some(Value::Undefined);
    };
    let n = |i: usize| args.get(i).map_or(0.0, |v| v.to_number());
    let r = match name {
        "start" => {
            set_active(w, true);
            true
        }
        "finish" => {
            set_active(w, false);
            true
        }
        "toggle" => {
            let on = !w.edit.active;
            set_active(w, on);
            on
        }
        "press" => press(w, item, n(0), n(1)),
        "move" => motion(w, item, n(0), n(1)),
        "release" => release(w, item, n(0), n(1)),
        "rest" => rest(w),
        "cancel" => {
            w.edit.pending = None;
            let had = w.edit.gesture.take().is_some();
            w.edit.rev += 1;
            had
        }
        "key" => key(w, item, n(0) as i64, n(1) as i64),
        "undo" => w.edit.active && step(w, false),
        "redo" => w.edit.active && step(w, true),
        "removeSelected" => remove_selected(w, item),
        "addWidget" => {
            let arg = |i: usize| args.get(i).map(|v| v.to_js_string()).unwrap_or_default();
            return Some(Value::str(
                add_widget(w, item, &arg(0), &arg(1))
                    .as_deref()
                    .unwrap_or(""),
            ));
        }
        "setZone" => {
            let zone = args.first().map(|v| v.to_js_string()).unwrap_or_default();
            set_zone(w, item, &zone)
        }
        "setSpan" => set_span(w, item, n(0)),
        "setSize" => set_size(w, item, n(0), n(1)),
        _ => {
            let mode = args.first().map(|v| v.to_js_string()).unwrap_or_default();
            set_width_mode(w, item, &mode)
        }
    };
    w.edit.rev += 1;
    Some(Value::Bool(r))
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let o = crate::eval::new_obj();
    for (k, v) in pairs {
        crate::eval::set_own(&o, k, v);
    }
    Value::Obj(o)
}

fn rect_value(r: &Rect) -> Value {
    obj(vec![
        ("x", Value::Num(r.x)),
        ("y", Value::Num(r.y)),
        ("width", Value::Num(r.w)),
        ("height", Value::Num(r.h)),
    ])
}

/// Shows a panel's widget as the gesture carrying it has it, where the pointer has it along
/// its panel: how far to its host's `carryX` and `carryY`, and `carried` (PanelWidget.qml
/// declares them and moves by them; a host without them stays). The layout is not touched
/// until the drop. Nothing is written while no widget is carried and none was.
fn live_hosts(w: &mut World) -> bool {
    let held = w
        .edit
        .gesture
        .as_ref()
        .filter(|g| g.dragging && g.kind == Kind::Panel);
    // A pull by an end is shown by the panel itself ([`State::pulled_panel`]).
    let now = held.filter(|g| g.mode == Mode::Move).and_then(|g| {
        let raw = g.preview.as_ref()?.raw?;
        Some((
            g.connector.clone(),
            g.id.clone(),
            raw.x - g.start.x,
            raw.y - g.start.y,
        ))
    });
    if now.is_none() && !w.edit.carrying {
        return false;
    }
    w.edit.carrying = now.is_some();
    let mut wrote = false;
    for h in w.of_type("WidgetHost") {
        if !["carried", "carryX", "carryY"]
            .iter()
            .any(|k| w.items[h].props.contains_key(k))
        {
            continue;
        }
        let at = now.as_ref().filter(|(connector, id, ..)| {
            text(w, h, "layoutId") == *id
                && shown_window(w, h).is_some_and(|win| window_rect(w, win).connector == *connector)
        });
        let (on, x, y) = at.map_or((false, 0.0, 0.0), |&(_, _, x, y)| (true, x, y));
        if w.items[h].props.contains_key("carried")
            && w.read(h, "carried").is_some_and(|v| v.truthy()) != on
        {
            w.write(h, "carried", Value::Bool(on));
            wrote = true;
        }
        for (k, v) in [("carryX", x), ("carryY", y)] {
            if w.items[h].props.contains_key(k) && num(w, h, k) != v {
                w.write(h, k, Value::Num(v));
                wrote = true;
            }
        }
    }
    wrote
}

/// Writes every EditController's outputs: on every pass while editing, since frames follow
/// the surfaces' layout, but only the values that changed.
pub(crate) fn sync(w: &mut World) -> bool {
    // A settle ends with a pass that wrote nothing, and a frame has several settles: the
    // outputs follow only what `sync_key` covers, so with that as it was they are current.
    if w.edit.synced == Some(sync_key(w)) {
        return false;
    }
    let wrote = sync_outputs(w);
    w.edit.synced = Some(sync_key(w));
    wrote
}

/// Everything the overlays' outputs are made from, as one number: the items (any write, any
/// item made or gone), the layout, where the windows are and the editor's own state.
fn sync_key(w: &World) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (w.revision, w.tree_gen, w.geom_rev, w.layout_rev).hash(&mut h);
    w.dead.len().hash(&mut h);
    w.widgets.iter().count().hash(&mut h);
    w.layout
        .as_ref()
        .map(|l| (l.generation(), l.editor().can_undo(), l.editor().can_redo()))
        .hash(&mut h);
    let mut rects: Vec<_> = w
        .window_rects
        .iter()
        .map(|(win, r)| (*win, &r.connector, [r.x, r.y, r.w, r.h].map(f64::to_bits)))
        .collect();
    rects.sort_unstable();
    rects.hash(&mut h);
    let mut natural: Vec<_> = w
        .desktop_natural
        .iter()
        .map(|(view, n)| (*view, n.revision, n.sizes.len()))
        .collect();
    natural.sort_unstable();
    natural.hash(&mut h);
    let mut moved: Vec<_> = w
        .popup_dy
        .iter()
        .map(|(i, dy)| (*i, dy.to_bits()))
        .collect();
    moved.sort_unstable();
    moved.hash(&mut h);
    let e = &w.edit;
    (e.rev, e.active, &e.selected, e.carrying).hash(&mut h);
    e.hover
        .as_ref()
        .map(|(c, x, y)| (c, x.to_bits(), y.to_bits()))
        .hash(&mut h);
    e.gesture
        .as_ref()
        .map(|g| (&g.connector, &g.id, g.dragging))
        .hash(&mut h);
    h.finish()
}

fn sync_outputs(w: &mut World) -> bool {
    let mut wrote = live_hosts(w);
    for id in w.of_type(TYPE) {
        let connector = text(w, id, "connector");
        let active = w.edit.active;
        let (frames, regions) = if active && !connector.is_empty() {
            gather(w, &connector)
        } else {
            (vec![], vec![])
        };
        let selected = selected_on(w, &connector);
        // What the overlay draws by itself, apart from the list of frames: the outline of the
        // selected widget (of its ghost while a resize pulls it), the outline of the one the
        // pointer is over, the handle the pointer is on and what a resize would give.
        let pointer = w
            .edit
            .hover
            .as_ref()
            .filter(|(c, ..)| *c == connector && active)
            .map(|&(_, x, y)| (x, y));
        let held = w
            .edit
            .gesture
            .as_ref()
            .filter(|g| g.connector == connector)
            .cloned();
        let shown = held.as_ref().and_then(|g| g.shown);
        let (selected_frame, hovered, handle, size_hint, selected_place) =
            if active && !connector.is_empty() {
                let gathered = (frames.clone(), regions.clone());
                with_gathered(w, id, &connector, gathered, |s| {
                    let selected_place = selected
                        .as_deref()
                        .and_then(|id| s.placement(id).map(|p| (p, s.overflows(id))));
                    let sel = selected.as_deref().and_then(|id| s.frame(id));
                    let resizing = held
                        .as_ref()
                        .filter(|g| g.dragging && g.mode == Mode::Resize);
                    let selected_frame = sel.map(|f| {
                        // A sidebar's widget pulled to another size is laid out at it at once
                        // ([`live_sidebar`]), and so is a panel's where its zone places it:
                        // the outline is around the widget as it shows. A panel's widget whose
                        // step is known has it where the panel will show it, at once or once
                        // the pointer rests ([`Preview::known`]).
                        let rect = resizing
                            .and_then(|g| g.preview.as_ref())
                            .filter(|p| p.valid)
                            .map_or(f.rect, |p| match shown {
                                Some(r) if f.kind != Kind::Desktop && !p.known => r,
                                _ => p.ghost,
                            });
                        (s.outline(f, &rect), f.resizable, f.kind, s.vertical(f))
                    });
                    let handle = match (&held, sel, pointer) {
                        (Some(g), ..) if g.mode == Mode::Resize => g.edges.name(),
                        (None, Some(f), Some((x, y))) => s.grip(f, x, y).map_or("", |e| e.name()),
                        _ => "",
                    };
                    let hovered = match (&held, pointer) {
                        (None, Some((x, y))) if handle.is_empty() => hit(&s.frames, x, y)
                            .filter(|f| Some(f.id.as_str()) != selected.as_deref())
                            .map(|f| s.outline(f, &f.rect)),
                        _ => None,
                    };
                    let size_hint = resizing.and_then(|g| s.size_hint(g));
                    (selected_frame, hovered, handle, size_hint, selected_place)
                })
                .unwrap_or((None, None, "", None, None))
            } else {
                (None, None, "", None, None)
            };
        let gesture = w
            .edit
            .gesture
            .as_ref()
            .filter(|g| g.connector == connector && g.dragging);
        let preview = gesture.and_then(|g| g.preview.clone());
        let (can_undo, can_redo) = w.layout.as_ref().map_or((false, false), |l| {
            (l.editor().can_undo(), l.editor().can_redo())
        });
        let selected_kind = selected
            .as_deref()
            .and_then(|s| frames.iter().find(|f| f.id == s))
            .map_or("", |f| f.kind.name());
        let selected_vertical = selected
            .as_deref()
            .and_then(|s| frames.iter().find(|f| f.id == s))
            .and_then(|f| regions.iter().find(|r| r.container == f.container))
            .is_some_and(|r| r.vertical);
        // Each output is written only when what it shows changed: a fresh `frames` array
        // rebuilds the overlay's Repeater, and a drag moves only the ghost, guides and target.
        let keys: [String; 19] = [
            active.to_string(),
            format!("{frames:?}|{selected:?}"),
            format!("{selected:?}"),
            selected_kind.to_string(),
            selected_vertical.to_string(),
            gesture.is_some().to_string(),
            format!("{:?}", preview.as_ref().map(|p| (&p.ghost, p.valid))),
            format!("{:?}", preview.as_ref().map(|p| &p.guides)),
            format!("{:?}", preview.as_ref().map(|p| p.marker)),
            format!("{:?}", preview.as_ref().map(|p| p.target)),
            format!("{:?}", preview.as_ref().map(|p| p.refusal)),
            can_undo.to_string(),
            can_redo.to_string(),
            w.widgets.iter().count().to_string(),
            format!("{selected_place:?}"),
            format!("{selected_frame:?}"),
            format!("{hovered:?}"),
            handle.to_string(),
            format!("{size_hint:?}"),
        ];
        let last = w.edit_synced.get(&id);
        let stale: [bool; 19] = std::array::from_fn(|i| last.is_none_or(|l| l[i] != keys[i]));
        if !stale.contains(&true) {
            continue;
        }
        w.edit_synced.insert(id, keys);
        let frame_values = frames
            .iter()
            .map(|f| {
                obj(vec![
                    ("id", Value::str(&f.id)),
                    ("type", Value::str(&f.ty)),
                    ("kind", Value::str(f.kind.name())),
                    ("container", Value::str(&f.container)),
                    ("x", Value::Num(f.rect.x)),
                    ("y", Value::Num(f.rect.y)),
                    ("width", Value::Num(f.rect.w)),
                    ("height", Value::Num(f.rect.h)),
                    (
                        "selected",
                        Value::Bool(selected.as_deref() == Some(f.id.as_str())),
                    ),
                    ("resizable", Value::Bool(f.resizable)),
                ])
            })
            .collect();
        let ghost = preview.as_ref().map_or(Value::Null, |p| {
            let v = rect_value(&p.ghost);
            if let Value::Obj(o) = &v {
                crate::eval::set_own(o, "valid", Value::Bool(p.valid));
                // Whether the landing place is away from where the widget itself is shown: a
                // desktop widget that is carried over its own desktop is shown where it lands
                // ([`live_desktop`]).
                let own = gesture
                    .and_then(|g| frames.iter().find(|f| f.id == g.id))
                    .map(|f| f.container.as_str());
                let apart = p.raw.as_ref().is_none_or(|raw| {
                    p.valid
                        && p.drop.as_ref().is_some_and(|d| {
                            matches!(d.place, Place::Desktop { .. })
                                && Some(d.container.as_str()) != own
                        })
                        && (raw.x.round() != p.ghost.x.round()
                            || raw.y.round() != p.ghost.y.round())
                });
                crate::eval::set_own(o, "apart", Value::Bool(apart));
                crate::eval::set_own(o, "carried", Value::Bool(p.raw.is_some()));
            }
            v
        });
        let guides = preview.as_ref().map_or(vec![], |p| {
            p.guides
                .iter()
                .map(|g| {
                    obj(vec![
                        ("x1", Value::Num(g.x1)),
                        ("y1", Value::Num(g.y1)),
                        ("x2", Value::Num(g.x2)),
                        ("y2", Value::Num(g.y2)),
                    ])
                })
                .collect()
        });
        let marker = preview
            .as_ref()
            .and_then(|p| p.marker)
            .map_or(Value::Null, |r| rect_value(&r));
        let target = preview
            .as_ref()
            .and_then(|p| p.target)
            .map_or(Value::Null, |r| rect_value(&r));
        let refusal = preview
            .as_ref()
            .and_then(|p| p.refusal)
            .map_or(Value::Null, |r| rect_value(&r));
        let outputs: [(&str, &str, Value); 19] = [
            ("active", "bool", Value::Bool(active)),
            ("frames", "var", Value::array(frame_values)),
            (
                "selectedId",
                "string",
                Value::str(selected.as_deref().unwrap_or("")),
            ),
            ("selectedKind", "string", Value::str(selected_kind)),
            ("selectedVertical", "bool", Value::Bool(selected_vertical)),
            ("dragging", "bool", Value::Bool(gesture.is_some())),
            ("ghost", "var", ghost),
            ("guides", "var", Value::array(guides)),
            ("marker", "var", marker),
            ("target", "var", target),
            ("refusal", "var", refusal),
            ("canUndo", "bool", Value::Bool(can_undo)),
            ("canRedo", "bool", Value::Bool(can_redo)),
            (
                "catalog",
                "var",
                Value::array(if stale[13] {
                    catalog(&w.widgets)
                } else {
                    vec![]
                }),
            ),
            (
                "selectedPlace",
                "var",
                match selected_place {
                    Some((Sits::Panel { zone, width }, _)) => obj(vec![
                        ("zone", Value::str(zone_name(zone))),
                        (
                            "width",
                            Value::str(if width == Width::Stretch {
                                "stretch"
                            } else {
                                "auto"
                            }),
                        ),
                    ]),
                    Some((Sits::Desktop { size, min, max }, _)) => obj(vec![
                        ("sizeWidth", Value::Num(size.0 as f64)),
                        ("sizeHeight", Value::Num(size.1 as f64)),
                        ("minWidth", Value::Num(min.0 as f64)),
                        ("minHeight", Value::Num(min.1 as f64)),
                        ("maxWidth", Value::Num(max.0 as f64)),
                        ("maxHeight", Value::Num(max.1 as f64)),
                    ]),
                    Some((Sits::Sidebar { size, columns }, overflows)) => obj(vec![
                        ("span", Value::Num(size as f64)),
                        ("columns", Value::Num(columns as f64)),
                        ("overflows", Value::Bool(overflows)),
                    ]),
                    None => Value::Null,
                },
            ),
            (
                "selectedFrame",
                "var",
                selected_frame.map_or(Value::Null, |(r, resizable, kind, vertical)| {
                    let v = rect_value(&r);
                    if let Value::Obj(o) = &v {
                        crate::eval::set_own(o, "resizable", Value::Bool(resizable));
                        crate::eval::set_own(o, "kind", Value::str(kind.name()));
                        crate::eval::set_own(o, "vertical", Value::Bool(vertical));
                    }
                    v
                }),
            ),
            (
                "hovered",
                "var",
                hovered.map_or(Value::Null, |r| rect_value(&r)),
            ),
            ("handle", "string", Value::str(handle)),
            (
                "sizeHint",
                "var",
                size_hint.map_or(Value::Null, |h| {
                    obj(vec![
                        ("kind", Value::str(h.kind.name())),
                        ("width", Value::Num(h.size.0)),
                        ("height", Value::Num(h.size.1)),
                        ("smallest", Value::Bool(h.smallest)),
                        ("largest", Value::Bool(h.largest)),
                    ])
                }),
            ),
        ];
        for ((k, ty, v), stale) in outputs.into_iter().zip(stale) {
            if !w.items[id].props.contains_key(k) {
                w.declare(id, k, ty);
            }
            if stale {
                w.write(id, k, v);
            }
        }
        wrote = true;
    }
    wrote
}

#[cfg(test)]
mod tests;
