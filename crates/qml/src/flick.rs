//! QQuickFlickable's motion (qquickflickable.cpp): the per-axis QQuickTimeLine that flicks and
//! fixes up `contentX` / `contentY`, wheel velocity, drag and release flicks, and the moving /
//! flicking / dragging state with its signals. Positions are in Qt's `hData.move` / `vData.move`
//! space (`-contentX` / `-contentY`).

use std::collections::VecDeque;

use rustc_hash::FxHashMap as HashMap;

use crate::runtime::{Prop, SCROLL_BARS, SmoothedAnimation, Sym, World};
use crate::value::Value;

/// QQuickFlickablePrivate::fixupDuration.
/// Where the visible area of its Flickable last put an attached ScrollBar.
const BAR_AT: &str = "\x01barAt";
const FIXUP_DURATION: f64 = 400.0;
const RETAIN_GRAB_VELOCITY: f64 = 100.0;
const MINIMUM_FLICK_VELOCITY: f64 = 75.0;
/// QPlatformTheme::FlickStartDistance and StartDragDistance defaults.
const FLICK_START_DISTANCE: f64 = 15.0;
const START_DRAG_DISTANCE: f64 = 10.0;
const SAMPLE_BUFFER: usize = 3;
const MAXIMUM_WHEEL_DECELERATION: f64 = 14999.0;
/// QStyleHints::wheelScrollLines.
const WHEEL_SCROLL_LINES: f64 = 3.0;
const OVERSHOOT: f64 = 150.0;
const OVERSHOOT_FRICTION: f64 = 8.0;
/// QML_FLICK_SNAPONETHRESHOLD.
const SNAP_ONE_THRESHOLD: f64 = 30.0;
const MULTIFLICK_THRESHOLD: f64 = 1250.0;
const MULTIFLICK_RATIO: f64 = 10.0;
const MULTIFLICK_MAXBOOST: f64 = 3.0;
/// QEasingCurve::Type.
const LINEAR: i32 = 0;
const IN_QUAD: i32 = 1;
const IN_OUT_QUAD: i32 = 3;
const OUT_EXPO: i32 = 22;
bits::bit_flags! {
/// QQuickFlickable::BoundsBehavior flags.
const DRAG_OVER_BOUNDS: i32 = 1;
const OVERSHOOT_BOUNDS: i32 = 2;
}
/// QQuickFlickable::AutoFlickIfNeeded.
const AUTO_FLICK_IF_NEEDED: i32 = 12;

/// One QQuickTimeLinePrivate::Op on an axis' `move`; lengths are whole milliseconds.
#[derive(Clone, Debug)]
enum Op {
    Accel {
        v: f64,
        a: f64,
        len: f64,
    },
    Move {
        to: f64,
        ease: i32,
        len: f64,
    },
    MoveBy {
        by: f64,
        ease: i32,
        len: f64,
    },
    Set(f64),
    /// The fixupY callback.
    Fixup,
}

impl Op {
    fn len(&self) -> f64 {
        match self {
            Op::Accel { len, .. } | Op::Move { len, .. } | Op::MoveBy { len, .. } => *len,
            Op::Set(_) | Op::Fixup => 0.0,
        }
    }

    /// QQuickTimeLinePrivate::value at `t` ms into the op.
    fn value(&self, base: f64, t: f64) -> f64 {
        let ease = |ty: i32, p: f64| {
            crate::easing::Easing {
                ty,
                ..Default::default()
            }
            .value(p)
        };
        match *self {
            Op::Accel { v, a, .. } => {
                let t = t / 1000.0;
                base + v * t + 0.5 * a * t * t
            }
            Op::Move { to, ease: e, len } => {
                if t >= len {
                    to
                } else {
                    base + (to - base) * ease(e, t / len)
                }
            }
            Op::MoveBy { by, ease: e, len } => base + by * ease(e, (t / len).min(1.0)),
            Op::Set(v) => v,
            Op::Fixup => base,
        }
    }
}

/// QQuickItemViewPrivate::MovementReason: what moved the view last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveReason {
    Other,
    Mouse,
    SetIndex,
}

/// A realized ListView row along the view's axis: `top` is where it starts (its section header
/// when it has one), `pos` and `size` are the delegate's own.
#[derive(Clone, Copy, Debug)]
pub struct Row {
    top: f64,
    pos: f64,
    size: f64,
}

/// What QQuickListViewPrivate's snapping reads from the view.
struct Snap {
    rows: Vec<Row>,
    /// The model row of `rows[0]`.
    first: usize,
    /// QQuickListViewPrivate::averageSize.
    average: f64,
    spacing: f64,
    /// The highlight's size: the current item's.
    highlight: Option<f64>,
    /// highlightRangeStart / highlightRangeEnd.
    range: (f64, f64),
    one: bool,
}

impl Snap {
    /// QQuickListViewPrivate::snapItemAt.
    fn item_at(&self, pos: f64, velocity: f64) -> Option<usize> {
        let (mut snap, mut prev_size) = (None, 0.0);
        for (k, r) in self.rows.iter().enumerate() {
            let (mut top, mut size) = (r.top, r.pos + r.size - r.top);
            if self
                .highlight
                .is_some_and(|h| top >= pos && r.pos + r.size <= pos + h)
            {
                return Some(k);
            }
            if r.pos != r.top && velocity > 0.0 {
                if top + (r.pos - r.top) / 2.0 >= pos && top - prev_size / 2.0 < pos {
                    snap = k.checked_sub(1);
                }
                top = r.pos;
                size = r.size;
            }
            let next = top + (size + self.spacing) / 2.0;
            let prev = top - (prev_size + self.spacing) / 2.0;
            if next >= pos && prev < pos {
                snap = Some(k);
            }
            prev_size = r.size;
        }
        snap
    }

    /// QQuickListViewPrivate::snapPosAt.
    fn pos_at(&self, pos: f64, velocity: f64) -> f64 {
        if let Some(k) = self.item_at(pos, velocity) {
            return self.rows[k].pos;
        }
        let a = self.average;
        if let (Some(first), Some(last)) = (self.rows.first(), self.rows.last()) {
            if pos < first.top {
                return first.top - ((first.top - pos) / a).round() * a;
            } else if pos > last.top {
                return last.top + ((pos - last.top) / a).round() * a;
            }
        }
        (pos / a).round() * a
    }
}

/// A view's rows along the axis it scrolls: the realized ones and what stands in for the rest.
struct Rows {
    rows: Vec<Row>,
    /// The model row of `rows[0]` (a view holds delegates of a run of its rows).
    first: usize,
    /// QQuickListViewPrivate::averageSize.
    average: f64,
    spacing: f64,
    /// The model's count.
    count: usize,
    ax: Ax,
    /// GridView: columns and the row height.
    grid: Option<(f64, f64)>,
}

impl Rows {
    /// Where row `idx` starts (its section header's top), its delegate's position and size. A
    /// ListView row not realized yet is estimated as QQuickListViewPrivate::positionAt does.
    fn geom(&self, idx: usize) -> Option<(f64, f64, f64)> {
        if let Some((cols, cell)) = self.grid {
            let p = (idx as f64 / cols).floor() * cell;
            return Some((p, p, cell));
        }
        let Some(k) = idx.checked_sub(self.first) else {
            let first = self.rows.first()?;
            let back = (self.first - idx) as f64;
            let p = first.top - back * (self.average + self.spacing);
            return Some((p, p, self.average));
        };
        if let Some(r) = self.rows.get(k) {
            return Some((r.top, r.pos, r.size));
        }
        let last = self.rows.last()?;
        let ahead = (k - self.rows.len()) as f64;
        let p = last.pos + last.size + self.spacing + ahead * (self.average + self.spacing);
        Some((p, p, self.average))
    }

    /// Row `idx`, estimated when it is not realized yet.
    fn row(&self, idx: usize) -> Option<Row> {
        (idx < self.count)
            .then(|| self.geom(idx))
            .flatten()
            .map(|(top, pos, size)| Row { top, pos, size })
    }

    /// The first and last rows QQuickListView keeps between `from` and `to`.
    fn between(&self, from: f64, to: f64) -> (usize, usize) {
        let all: Vec<usize> = (0..self.count).collect();
        let first = all.partition_point(|&k| self.geom(k).is_some_and(|(_, p, s)| p + s < from));
        let last = all.partition_point(|&k| self.geom(k).is_some_and(|(t, ..)| t <= to));
        (first, last.saturating_sub(1))
    }
}

/// QQuickItemView's highlight item: it runs after the current item, and the view after it.
#[derive(Clone, Debug)]
pub struct Highlight {
    anim: SmoothedAnimation,
    /// Its position along the view's axis.
    pub pos: f64,
    /// The currentIndex it heads for, and that row's position.
    index: f64,
    to: f64,
    /// The rows the view holds (QQuickItemViewPrivate::visibleItems).
    held: (usize, usize),
    pub settled: bool,
}

impl Highlight {
    /// When the running move started (ms).
    pub fn started(&self) -> f64 {
        self.anim.started()
    }
}

/// A Flickable axis: QQuickFlickablePrivate::hData or vData.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ax {
    X,
    Y,
}

impl Ax {
    fn other(self) -> Ax {
        match self {
            Ax::X => Ax::Y,
            Ax::Y => Ax::X,
        }
    }
    fn pos(self) -> &'static str {
        match self {
            Ax::X => "contentX",
            Ax::Y => "contentY",
        }
    }
    fn item_pos(self) -> &'static str {
        match self {
            Ax::X => "x",
            Ax::Y => "y",
        }
    }
    fn size(self) -> &'static str {
        match self {
            Ax::X => "width",
            Ax::Y => "height",
        }
    }
    fn content(self) -> &'static str {
        match self {
            Ax::X => "contentWidth",
            Ax::Y => "contentHeight",
        }
    }
    fn start_margin(self) -> &'static str {
        match self {
            Ax::X => "leftMargin",
            Ax::Y => "topMargin",
        }
    }
    fn end_margin(self) -> &'static str {
        match self {
            Ax::X => "rightMargin",
            Ax::Y => "bottomMargin",
        }
    }
    fn origin(self) -> &'static str {
        match self {
            Ax::X => "originX",
            Ax::Y => "originY",
        }
    }
    fn suffix(self) -> &'static str {
        match self {
            Ax::X => "Horizontally",
            Ax::Y => "Vertically",
        }
    }
}

/// QQuickFlickablePrivate's AxisData plus the press/drag fields both axes keep.
#[derive(Clone, Debug)]
pub struct Flick {
    /// The timeline has operations left (QQuickTimeLine::isActive).
    pub active: bool,
    /// QQuickFlickable::isMovingVertically / isMovingHorizontally.
    pub moving: bool,
    pub flicking: bool,
    pub dragging: bool,
    /// The Flickable wants the pointer grab (QQuickFlickablePrivate::stealGrab).
    pub steal_grab: bool,
    init: bool,
    /// The view had a size along this axis before the change that is being handled.
    sized: bool,
    ops: VecDeque<Op>,
    /// Start time and value of the front op.
    t0: f64,
    base: f64,
    /// vData.move as the timeline last set it (stale between frames, like Qt's).
    mv: f64,
    /// contentY last written here: anything else that writes it resets the timeline.
    last: f64,
    v_moved: bool,
    pressed: bool,
    fixing_up: bool,
    in_overshoot: bool,
    flicking_when_drag_began: bool,
    at_beginning: bool,
    at_end: bool,
    press_y: f64,
    press_move: f64,
    /// AxisData::flickTarget of a snapping ListView.
    flick_target: f64,
    /// QQuickItemViewPrivate::moveReason.
    pub reason: MoveReason,
    /// QQuickListViewPrivate::correctFlick, and the guard of the correction it allows.
    correct: bool,
    correcting: bool,
    drag_start_offset: f64,
    previous_drag_delta: f64,
    drag_min: f64,
    drag_max: f64,
    last_pos_time: Option<f64>,
    last_press_time: f64,
    samples: VecDeque<f64>,
    velocity: f64,
    smooth: f64,
    v_time: f64,
    last_pos: f64,
    velocity_time: f64,
    continuous: f64,
    boost: f64,
    pub wheel_decel: f64,
    /// A view's size along this axis at its last layout.
    laid_size: Option<f64>,
}

impl Flick {
    /// Animation-clock time the running timeline op started at.
    pub fn started(&self) -> f64 {
        self.t0
    }
}

impl Default for Flick {
    fn default() -> Self {
        Flick {
            active: false,
            moving: false,
            flicking: false,
            dragging: false,
            steal_grab: false,
            init: false,
            sized: false,
            ops: VecDeque::new(),
            t0: 0.0,
            base: 0.0,
            mv: 0.0,
            last: 0.0,
            v_moved: false,
            pressed: false,
            fixing_up: false,
            in_overshoot: false,
            flicking_when_drag_began: false,
            at_beginning: true,
            at_end: false,
            press_y: 0.0,
            press_move: 0.0,
            flick_target: 0.0,
            reason: MoveReason::Other,
            correct: false,
            correcting: false,
            drag_start_offset: 0.0,
            previous_drag_delta: 0.0,
            drag_min: 0.0,
            drag_max: 0.0,
            last_pos_time: None,
            last_press_time: 0.0,
            samples: VecDeque::new(),
            velocity: 0.0,
            smooth: 0.0,
            v_time: 0.0,
            last_pos: 0.0,
            velocity_time: 0.0,
            continuous: 0.0,
            boost: 1.0,
            wheel_decel: wheel_deceleration(),
            laid_size: None,
        }
    }
}

/// QQuickFlickablePrivate::wheelDeceleration: 15000 unless QT_QUICK_FLICKABLE_WHEEL_DECELERATION.
fn wheel_deceleration() -> f64 {
    std::env::var("QT_QUICK_FLICKABLE_WHEEL_DECELERATION")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|v| *v > 0)
        .map_or(15000.0, |v| v as f64)
}

fn fuzzy_le(mut a: f64, mut b: f64) -> bool {
    if a == 0.0 || b == 0.0 {
        a += 1.0;
        b += 1.0;
    }
    a <= b || (a - b).abs() * 1e12 <= a.abs().min(b.abs())
}

impl World {
    fn fnum(&mut self, f: usize, k: &str, d: f64) -> f64 {
        self.read(f, k)
            .map(|v| v.to_number())
            .filter(|x| x.is_finite())
            .unwrap_or(d)
    }

    /// A property's value when set in QML, else the QQuickFlickable default.
    pub(crate) fn fprop(&mut self, f: usize, k: &str, d: f64) -> f64 {
        let set = self.is_assigned(f, k)
            || self.items[f]
                .props
                .get(k)
                .is_some_and(|p| p.binding.is_some() || p.alias.is_some());
        if set { self.fnum(f, k, d) } else { d }
    }

    fn fst(&mut self, f: usize, ax: Ax) -> &mut Flick {
        match ax {
            Ax::X => self.flicks_x.entry(f).or_default(),
            Ax::Y => self.flicks.entry(f).or_default(),
        }
    }

    fn fget(&self, f: usize, ax: Ax) -> Option<&Flick> {
        match ax {
            Ax::X => self.flicks_x.get(&f),
            Ax::Y => self.flicks.get(&f),
        }
    }

    /// The other axis has `pick` set (the combined moving / flicking / dragging state).
    fn other_has(&self, f: usize, ax: Ax, pick: fn(&Flick) -> bool) -> bool {
        self.fget(f, ax.other()).is_some_and(pick)
    }

    /// (minXExtent, maxXExtent) or (minYExtent, maxYExtent) in move space.
    fn extents(&mut self, f: usize, ax: Ax) -> (f64, f64) {
        let origin = if self.items[f].prop_info(ax.origin()).is_some() {
            self.fnum(f, ax.origin(), 0.0)
        } else {
            0.0
        };
        let start = self.fnum(f, ax.start_margin(), 0.0);
        let view = self.items[f]
            .types
            .iter()
            .any(|t| t == "ListView" || t == "GridView");
        let min = start - origin;
        let max = self.fnum(f, ax.size(), 0.0)
            - self.content_size(f, ax)
            - self.fnum(f, ax.end_margin(), 0.0)
            - origin;
        // A ListView laid out bottom to top rests at its bottom: content shorter than the
        // view stays there.
        if ax == Ax::Y
            && view
            && self.fnum(f, "verticalLayoutDirection", 0.0) == 3.0
            && self.fnum(f, "orientation", 2.0) != 1.0
        {
            return (min.max(max), max);
        }
        // QQuickItemViewPrivate::minExtentForAxis / maxExtentForAxis under StrictlyEnforceRange:
        // the first and the last row both reach the highlight range.
        if view
            && let Some((begin, end)) = self.strict_range(f)
            && let Some(v) = self.view_rows(f)
            && v.ax == ax
            && !v.rows.is_empty()
            && let Some(first) = v.row(0)
            && let Some((_, _, last)) = v.geom(v.count.saturating_sub(1))
        {
            let min = (min + begin - (first.pos - first.top)).max(-(first.pos + first.size - end));
            let reach = if end != begin {
                (begin + last).min(end)
            } else {
                begin + last
            };
            let max = max - self.fnum(f, ax.size(), 0.0) + reach;
            return (min, max.min(min));
        }
        (min, max.min(min))
    }

    /// QQuickFlickable::minXExtent/maxXExtent (minYExtent/maxYExtent) in content-move space.
    pub fn flick_extents(&mut self, f: usize, vertical: bool) -> (f64, f64) {
        self.extents(f, if vertical { Ax::Y } else { Ax::X })
    }

    /// contentWidth / contentHeight; unset (-1) sizes the contentItem to the view.
    fn content_size(&mut self, f: usize, ax: Ax) -> f64 {
        let c = self.fnum(f, ax.content(), -1.0);
        if c < 0.0 {
            self.fnum(f, ax.size(), 0.0)
        } else {
            c
        }
    }

    /// QQuickFlickable::xflick / yflick.
    fn flick_dir(&mut self, f: usize, ax: Ax) -> bool {
        if self.items[f].types.iter().any(|t| t == "ListView") {
            return (self.fnum(f, "orientation", 2.0) == 1.0) == (ax == Ax::X);
        }
        let dir = self.fprop(f, "flickableDirection", 0.0) as i32;
        // contentItem size: contentWidth / contentHeight when set in QML, else the view's.
        let c = self.fprop(f, ax.content(), -1.0);
        let c = if c < 0.0 {
            self.fnum(f, ax.size(), 0.0)
        } else {
            c
        };
        let extent =
            (c + self.fnum(f, ax.start_margin(), 0.0) + self.fnum(f, ax.end_margin(), 0.0)).trunc();
        let size = self.fnum(f, ax.size(), 0.0);
        if dir & AUTO_FLICK_IF_NEEDED != 0 && extent > size {
            return true;
        }
        if dir == 0 {
            return (extent - size).abs().floor() != 0.0;
        }
        dir & if ax == Ax::X { 1 } else { 2 } != 0
    }

    pub fn flick_vertical(&mut self, f: usize) -> bool {
        self.flick_dir(f, Ax::Y)
    }

    pub fn flick_horizontal(&mut self, f: usize) -> bool {
        self.flick_dir(f, Ax::X)
    }

    /// Picks up contentX / contentY written elsewhere (QQuickFlickable::setContentY).
    fn flick_sync(&mut self, f: usize, ax: Ax) {
        let cy = self.fnum(f, ax.pos(), 0.0);
        let now = self.now;
        let st = self.fst(f, ax);
        if !st.init {
            st.init = true;
            st.mv = -cy;
            st.last = cy;
            st.last_pos = -cy;
            self.update_beginning_end(f, ax);
            return;
        }
        if (cy - st.last).abs() > 1e-6 {
            st.ops.clear();
            st.active = false;
            st.v_time = now;
            st.mv = -cy;
            st.last = cy;
            if st.moving || st.flicking {
                self.movement_ending(f, ax);
            }
        }
    }

    /// Writes a state property; its change handler queues behind the signals already emitted.
    /// `{name}Vertically` / `{name}Horizontally`, then `name` for both axes.
    fn flag(&mut self, f: usize, ax: Ax, name: &str, v: bool, pick: fn(&Flick) -> bool) {
        let both = v || self.other_has(f, ax, pick);
        for (n, v) in [
            (format!("{name}{}", ax.suffix()), v),
            (name.to_string(), both),
        ] {
            let k = self.changed.len();
            self.write(f, &n, Value::Bool(v));
            if self.changed.len() > k {
                self.changed.truncate(k);
                self.signal(f, &format!("{n}Changed"));
            }
        }
    }

    /// Argument-less signals queue with the change handlers, keeping Qt's emission order.
    fn signal(&mut self, f: usize, sig: &str) {
        self.fire.push(crate::runtime::Fire::Handler(
            f,
            crate::runtime::signal_handler(sig),
        ));
    }

    /// QQuickFlickable::movementStarting.
    fn movement_starting(&mut self, f: usize, ax: Ax) {
        let st = self.fst(f, ax);
        if st.v_moved && !st.moving {
            st.moving = true;
            self.flag(f, ax, "moving", true, |s: &Flick| s.moving);
            if !self.other_has(f, ax, |s: &Flick| s.moving) {
                self.signal(f, "movementStarted");
            }
        }
    }

    /// QQuickFlickable::movementEnding.
    fn movement_ending(&mut self, f: usize, ax: Ax) {
        let queued = self.fire.len();
        self.flicking_ending(f, ax);
        if self.fire.len() > queued {
            // The `flickEnded` handlers still see `moving` true.
            self.fire
                .push(crate::runtime::Fire::MovingEnd(f, ax == Ax::Y));
        } else {
            self.moving_ending(f, ax);
        }
    }

    /// `Fire::MovingEnd`: the moving half of QQuickFlickable::movementEnding.
    pub fn moving_end(&mut self, f: usize, vertical: bool) {
        self.moving_ending(f, if vertical { Ax::Y } else { Ax::X });
    }

    fn flicking_ending(&mut self, f: usize, ax: Ax) {
        let st = self.fst(f, ax);
        if st.flicking {
            st.flicking = false;
            self.flag(f, ax, "flicking", false, |s: &Flick| s.flicking);
            self.signal(f, "flickEnded");
        } else if st.flicking_when_drag_began {
            st.flicking_when_drag_began = false;
            self.signal(f, "flickEnded");
        }
    }

    fn moving_ending(&mut self, f: usize, ax: Ax) {
        let st = self.fst(f, ax);
        if st.moving && !st.pressed && !st.steal_grab {
            st.moving = false;
            st.v_moved = false;
            self.flag(f, ax, "moving", false, |s: &Flick| s.moving);
            if !self.other_has(f, ax, |s: &Flick| s.moving) {
                self.signal(f, "movementEnded");
            }
        }
        let st = self.fst(f, ax);
        st.fixing_up = false;
        st.smooth = 0.0;
        st.previous_drag_delta = 0.0;
    }

    /// QQuickFlickablePrivate::flickingStarted.
    fn flicking_started(&mut self, f: usize, ax: Ax, v: bool) {
        let st = self.fst(f, ax);
        if v && !st.flicking {
            st.flicking = true;
            self.flag(f, ax, "flicking", true, |s: &Flick| s.flicking);
            if !self.other_has(f, ax, |s: &Flick| s.flicking) {
                self.signal(f, "flickStarted");
            }
        }
    }

    fn dragging_starting(&mut self, f: usize, ax: Ax) {
        let st = self.fst(f, ax);
        if st.v_moved && !st.dragging {
            st.dragging = true;
            self.flag(f, ax, "dragging", true, |s: &Flick| s.dragging);
            if !self.other_has(f, ax, |s: &Flick| s.dragging) {
                self.signal(f, "dragStarted");
            }
        }
    }

    fn dragging_ending(&mut self, f: usize, ax: Ax) {
        let st = self.fst(f, ax);
        if st.dragging {
            st.dragging = false;
            self.flag(f, ax, "dragging", false, |s: &Flick| s.dragging);
            if !self.other_has(f, ax, |s: &Flick| s.dragging) {
                self.signal(f, "dragEnded");
            }
        }
    }

    /// QQuickFlickablePrivate::updateBeginningEnd (vertical).
    fn update_beginning_end(&mut self, f: usize, ax: Ax) {
        let (min, max) = self.extents(f, ax);
        let st = self.fst(f, ax);
        let ypos = -st.mv;
        let at_beginning = fuzzy_le(ypos, (-min).ceil());
        let at_end = fuzzy_le((-max).floor(), ypos);
        if at_beginning != st.at_beginning {
            st.at_beginning = at_beginning;
            if !st.moving && at_beginning {
                st.smooth = 0.0;
            }
        }
        st.at_end = at_end;
    }

    /// The timeline of `f` on `ax` started `dt` ms later than was written down.
    pub(crate) fn flick_started_later(&mut self, f: usize, ax: Ax, dt: f64) {
        if let Some(st) = self.fget(f, ax)
            && st.active
        {
            self.fst(f, ax).t0 += dt;
        }
    }

    fn reset_timeline(&mut self, f: usize, ax: Ax) {
        let st = self.fst(f, ax);
        st.ops.clear();
        st.active = false;
    }

    fn push_op(&mut self, f: usize, ax: Ax, op: Op, at: f64) {
        let st = self.fst(f, ax);
        let starts = st.ops.is_empty();
        if starts {
            st.t0 = at;
            st.base = st.mv;
        }
        st.ops.push_back(op);
        st.active = true;
        if starts {
            self.note_started(crate::runtime::Started::Flick(f, ax));
        }
        self.flicks_live.insert((f, ax));
    }

    /// QQuickTimeLine::accel, with `max_dist` capping the distance.
    fn push_accel(&mut self, f: usize, ax: Ax, v: f64, mut a: f64, max_dist: Option<f64>, at: f64) {
        if let Some(d) = max_dist {
            if d.abs() < 1e-12 || a.abs() < 1e-12 || d.is_nan() || a.is_nan() {
                return;
            }
            a = a.max(v * v / (2.0 * d));
        }
        if (v > 0.0) == (a > 0.0) {
            a = -a;
        }
        let len = (-1000.0 * v / a).trunc();
        if len > 0.0 {
            self.push_op(f, ax, Op::Accel { v, a, len }, at);
        }
    }

    /// The timeline (or a drag) set vData.move: QQuickFlickable::viewportMoved.
    fn set_move(&mut self, f: usize, ax: Ax, v: f64, at: f64) {
        let st = self.fst(f, ax);
        st.mv = v;
        st.last = -v;
        let was = std::mem::replace(&mut self.animating, true);
        self.write(f, ax.pos(), Value::Num(-v));
        self.animating = was;
        // QQuickFlickablePrivate::setViewportX/Y: a `pixelAligned` Flickable shows the move
        // rounded to a whole pixel, and the timeline goes on from the move itself.
        self.fst(f, ax).last = self.fnum(f, ax.pos(), -v);
        let (min, max) = self.extents(f, ax);
        let decel = self.fprop(f, "flickDeceleration", 1500.0);
        let st = self.fst(f, ax);
        if st.pressed {
            let elapsed = at - st.velocity_time;
            st.velocity_time = at;
            if elapsed > 0.0 {
                let vel = (st.last_pos - st.mv) * 1000.0 / elapsed;
                if vel != 0.0 {
                    st.smooth = vel;
                }
            }
        } else if at > st.v_time {
            let dt = at - st.v_time;
            if dt > 2.0 {
                st.smooth = (st.last_pos - st.mv) * 1000.0 / dt;
            }
        }
        if !st.in_overshoot
            && !st.fixing_up
            && st.flicking
            && (st.mv > min || st.mv < max)
            && st.smooth.abs() > 10.0
        {
            let over = if st.mv > min {
                st.mv - min
            } else {
                max - st.mv
            };
            st.in_overshoot = true;
            let smooth = st.smooth;
            let dist = OVERSHOOT.min(smooth.abs() / 3.0) - over;
            self.reset_timeline(f, ax);
            if dist > 0.0 {
                self.push_accel(f, ax, -smooth, decel * OVERSHOOT_FRICTION, Some(dist), at);
            }
            self.push_op(f, ax, Op::Fixup, at);
        }
        let st = self.fst(f, ax);
        st.last_pos = st.mv;
        st.v_time = at;
        if st.flicking || st.moving {
            st.reason = MoveReason::Mouse;
        }
        self.update_beginning_end(f, ax);
        self.view_strict_moved(f, ax);
        self.snap_correct(f, ax, at);
    }

    /// The rows of a ListView or GridView.
    fn view_rows(&mut self, f: usize) -> Option<Rows> {
        let grid = self.items[f].types.iter().any(|t| t == "GridView");
        if !grid && !self.items[f].types.iter().any(|t| t == "ListView") {
            return None;
        }
        let horizontal = !grid && self.fprop(f, "orientation", 2.0) == 1.0;
        let ax = if horizontal { Ax::X } else { Ax::Y };
        let made = self.delegated.get(&f).map_or(0, |d| d.1.len());
        let count = self.view_window.get(&f).map_or(made, |v| v.1.max(made));
        if grid {
            let (cw, ch) = (
                self.fnum(f, "cellWidth", 100.0),
                self.fnum(f, "cellHeight", 100.0),
            );
            let cols = if cw > 0.0 {
                (self.fnum(f, "width", 0.0) / cw).floor().max(1.0)
            } else {
                1.0
            };
            return Some(Rows {
                rows: Vec::new(),
                first: 0,
                average: ch,
                spacing: 0.0,
                count,
                ax,
                grid: Some((cols, ch)),
            });
        }
        let rows: Vec<Row> = self.view_laid.get(&f).cloned().unwrap_or_default();
        let sum: f64 = rows.iter().map(|r| r.pos + r.size - r.top).sum();
        Some(Rows {
            average: (sum / rows.len().max(1) as f64).round(),
            rows,
            first: self.view_laid_first.get(&f).copied().unwrap_or(0),
            spacing: self.fnum(f, "spacing", 0.0),
            count,
            ax,
            grid: None,
        })
    }

    /// The view's layout placed its delegates: these are its rows until it does so again
    /// (delegates made in between have no place yet).
    pub fn view_laid_out(&mut self, f: usize) {
        let (pos, size) = if self.fprop(f, "orientation", 2.0) == 1.0 {
            ("x", "width")
        } else {
            ("y", "height")
        };
        // The run of rows the view holds delegates of; the current row's delegate kept
        // outside of it is not one of them.
        let (first, dels) = match self.view_rows.get(&f) {
            Some(v) => (v.first, v.rows.iter().map_while(|c| *c).collect()),
            None => (
                0,
                (self.delegated.get(&f))
                    .map(|d| d.1.clone())
                    .unwrap_or_default(),
            ),
        };
        self.view_laid_first.insert(f, first);
        let heads: HashMap<usize, usize> = self
            .view_sections
            .get(&f)
            .map(|v| v.iter().copied().collect())
            .unwrap_or_default();
        let mut rows = Vec::with_capacity(dels.len());
        for &c in &dels {
            let p = self.fnum(c, pos, 0.0);
            let top = match heads.get(&c) {
                Some(&h) => self.fnum(h, pos, p),
                None => p,
            };
            rows.push(Row {
                top,
                pos: p,
                size: self.fnum(c, size, 0.0),
            });
        }
        self.view_laid.insert(f, rows);
    }

    /// QQuickListViewPrivate::updateCurrentSection: `currentSection` is the section of the
    /// first row that ends below the top of the view.
    pub fn view_current_section(&mut self, f: usize) {
        let pos = if self.fprop(f, "orientation", 2.0) == 1.0 {
            Ax::X
        } else {
            Ax::Y
        };
        let at = self.fnum(f, pos.pos(), 0.0);
        let section = match (self.view_section_names.get(&f), self.view_laid.get(&f)) {
            (Some(names), Some(rows)) => (rows.iter())
                .position(|r| r.pos + r.size > at)
                .and_then(|k| names.get(k))
                .or(names.first())
                .cloned()
                .unwrap_or_default(),
            _ => String::new(),
        };
        let was = self.read(f, "currentSection");
        if !matches!(&was, Some(Value::Str(s)) if **s == *section) {
            self.write(f, "currentSection", Value::str(&section));
        }
    }

    /// highlightRangeStart / highlightRangeEnd while the view has a highlight range.
    fn highlight_range(&mut self, f: usize) -> Option<(f64, f64)> {
        let (b, e) = (
            self.fnum(f, "preferredHighlightBegin", 0.0),
            self.fnum(f, "preferredHighlightEnd", 0.0),
        );
        (self.fprop(f, "highlightRangeMode", 0.0) != 0.0 && e >= b).then_some((b, e))
    }

    /// The highlight range of a view in ApplyRange mode.
    pub fn view_apply_range(&mut self, f: usize) -> Option<(f64, f64)> {
        if self.fprop(f, "highlightRangeMode", 0.0) != 1.0 {
            return None;
        }
        self.highlight_range(f)
    }

    /// The highlight range of a ListView that keeps its highlight inside it
    /// (highlightRangeMode: StrictlyEnforceRange).
    fn strict_range(&mut self, f: usize) -> Option<(f64, f64)> {
        if self.fprop(f, "highlightRangeMode", 0.0) != 2.0
            || !self.items[f].types.iter().any(|t| t == "ListView")
        {
            return None;
        }
        self.highlight_range(f)
    }

    /// QQuickListViewPrivate::updateHighlight: the highlight heads for the current item.
    fn view_hl_restart(&mut self, f: usize) {
        let cur = self.fnum(f, self.view_cur(f), -1.0);
        let Some(to) = (cur >= 0.0)
            .then(|| self.view_rows(f)?.row(cur as usize).map(|r| r.pos))
            .flatten()
        else {
            return;
        };
        let velocity = self.fprop(f, "highlightMoveVelocity", 400.0);
        let duration = self.fprop(f, "highlightMoveDuration", -1.0);
        let now = self.now;
        if let Some(h) = self.view_hl.get_mut(&f) {
            h.index = cur;
            h.to = to;
            h.anim.velocity = velocity;
            h.anim.duration = duration;
            let _ = h.anim.target(h.pos, to, now);
            h.settled = !h.anim.running();
            if h.settled {
                h.pos = to;
            }
        }
    }

    /// QQuickListView::viewportMoved under StrictlyEnforceRange: a view moved by anything but
    /// `currentIndex` keeps its highlight inside the range, and the row under the highlight
    /// becomes the current one.
    pub fn view_strict_moved(&mut self, f: usize, ax: Ax) {
        if !self.view_hl.contains_key(&f) {
            return;
        }
        let Some((begin, end)) = self.strict_range(f) else {
            return;
        };
        if self.fst(f, ax).reason == MoveReason::SetIndex {
            return;
        }
        let Some(snap) = self.snap_rows(f, ax) else {
            return;
        };
        let Some(size) = snap.highlight else {
            return;
        };
        let cur = self.fnum(f, self.view_cur(f), -1.0);
        let at = self.fnum(f, ax.pos(), 0.0);
        let velocity = self.fst(f, ax).velocity;
        let now = self.now;
        let Some(h) = self.view_hl.get_mut(&f) else {
            return;
        };
        let pos = h.pos.min(at + end - size).max(at + begin);
        if pos != h.pos {
            let _ = h.anim.target(pos, pos, now);
            h.pos = pos;
            h.to = pos;
            h.settled = true;
        } else {
            self.view_hl_restart(f);
        }
        let at_hl = self.view_hl.get(&f).map_or(pos, |h| h.pos);
        if let Some(k) = snap.item_at(at_hl, velocity).map(|k| k + snap.first)
            && k as f64 != cur
        {
            // QQuickItemViewPrivate::updateCurrent: the move's reason stays what it was.
            self.write(f, self.view_cur(f), Value::Num(k as f64));
            self.view_current_item(f);
            self.view_hl_restart(f);
        }
    }

    /// QQuickItemView::setContentX / setContentY: a view positioned by hand forgets what
    /// moved it before.
    pub fn view_set_by_hand(&mut self, f: usize, ax: Ax) {
        if self.view_rows(f).map(|v| v.ax) != Some(ax) {
            return;
        }
        self.fst(f, ax).reason = MoveReason::Other;
        if self.strict_range(f).is_some() {
            self.flick_sync(f, ax);
            self.view_strict_moved(f, ax);
        }
    }

    /// QQuickItemViewPrivate::layout under StrictlyEnforceRange: a view at rest whose
    /// highlight is at rest puts its current item into the range (fixupPosition).
    pub fn view_strict_layout(&mut self, f: usize) {
        if self.strict_range(f).is_none() {
            return;
        }
        let Some(ax) = self.view_rows(f).map(|v| v.ax) else {
            return;
        };
        let st = self.fst(f, ax);
        if st.moving || st.flicking || self.view_hl.get(&f).is_some_and(|h| !h.settled) {
            return;
        }
        self.flick_sync(f, ax);
        let now = self.now;
        self.flick_fixup(f, ax, now);
    }

    /// QQuickFlickable::geometryChange: a view at rest that got another size along its flow
    /// comes back inside its bounds at once (fixupMode Immediate), still following its
    /// current row.
    pub fn view_resized_layout(&mut self, f: usize) {
        let Some(ax) = self.view_rows(f).map(|v| v.ax) else {
            return;
        };
        let size = self.fnum(f, ax.size(), 0.0);
        let st = self.fst(f, ax);
        let was = st.laid_size.replace(size);
        if was.is_none_or(|was| was == size) || st.pressed || st.moving || st.flicking {
            return;
        }
        if self.strict_range(f).is_some() || self.snap_of(f, ax).is_some() {
            return;
        }
        self.flick_sync(f, ax);
        let (min, max) = self.extents(f, ax);
        let mv = self.fst(f, ax).mv;
        let to = if mv >= min {
            min
        } else if mv <= max {
            max
        } else {
            return;
        };
        if to == mv {
            return;
        }
        self.reset_timeline(f, ax);
        let following = self.view_track.get(&f).is_some_and(|t| t.following);
        let was = std::mem::replace(&mut self.animating, true);
        self.write(f, ax.pos(), Value::Num(-to));
        self.animating = was;
        if let Some(t) = self.view_track.get_mut(&f) {
            t.following = following;
        }
    }

    /// The StrictlyEnforceRange branch of QQuickListViewPrivate::fixup for a view that does
    /// not snap: the current item goes into the highlight range, at once unless a flick or a
    /// drag moved the view. False when the view has no such range or no current item.
    fn strict_fixup(&mut self, f: usize, ax: Ax, at: f64) -> bool {
        let Some((begin, end)) = self.strict_range(f) else {
            return false;
        };
        let cur = self.fnum(f, self.view_cur(f), -1.0);
        let row = match self.view_rows(f) {
            Some(v) if v.ax == ax && cur >= 0.0 => v.row(cur as usize),
            _ => None,
        };
        let Some(row) = row else {
            return false;
        };
        let st = self.fst(f, ax);
        if st.reason == MoveReason::SetIndex {
            return false;
        }
        st.correct = false;
        let (mv, was_active, mouse) = (st.mv, st.active, st.reason == MoveReason::Mouse);
        self.view_hl_restart(f);
        let from = -mv;
        let pos = from.max(row.pos + row.size - end).min(row.pos - begin);
        if pos != from {
            self.reset_timeline(f, ax);
            let to = -pos;
            if mouse {
                self.push_op(
                    f,
                    ax,
                    Op::Move {
                        to,
                        ease: IN_OUT_QUAD,
                        len: FIXUP_DURATION / 2.0,
                    },
                    at,
                );
                self.fst(f, ax).fixing_up = true;
            } else if was_active {
                self.push_op(f, ax, Op::Set(to), at);
            } else {
                self.set_move(f, ax, to, at);
            }
        }
        let st = self.fst(f, ax);
        st.in_overshoot = false;
        st.v_time = at;
        true
    }

    /// The rows of a ListView with a `snapMode`, when `ax` is the axis it scrolls along.
    fn snap_of(&mut self, f: usize, ax: Ax) -> Option<Snap> {
        if self.fprop(f, "snapMode", 0.0) == 0.0 {
            return None;
        }
        self.snap_rows(f, ax)
    }

    /// The rows of a ListView or GridView as its snapping reads them, when `ax` is the axis
    /// it scrolls along.
    fn snap_rows(&mut self, f: usize, ax: Ax) -> Option<Snap> {
        let mode = self.fprop(f, "snapMode", 0.0);
        let v = self.view_rows(f)?;
        if v.ax != ax {
            return None;
        }
        if let Some((cols, cell)) = v.grid {
            // QQuickGridViewPrivate::snapItemAt: rows of cells, no highlight.
            let n = (v.count as f64 / cols).ceil() as usize;
            let rows: Vec<Row> = (0..n)
                .map(|r| r as f64 * cell)
                .map(|p| Row {
                    top: p,
                    pos: p,
                    size: cell,
                })
                .collect();
            return (!rows.is_empty()).then_some(Snap {
                average: cell.max(1.0),
                rows,
                first: 0,
                spacing: 0.0,
                highlight: None,
                range: self.highlight_range(f).unwrap_or((0.0, 0.0)),
                one: mode == 2.0,
            });
        }
        if v.rows.is_empty() {
            return None;
        }
        let cur = self.fnum(f, self.view_cur(f), -1.0);
        let strict = self.strict_range(f).is_some();
        let mut rows = v.rows.clone();
        let mut first = v.first;
        if strict {
            // The highlight reaches rows the view has not realized yet: Qt realizes the rows
            // it moves over, each of them where positionAt estimates it.
            rows = (0..v.count).filter_map(|k| v.row(k)).collect();
            first = 0;
        }
        let highlight = (cur >= 0.0)
            .then(|| rows.get((cur as usize).checked_sub(first)?).map(|r| r.size))
            .flatten();
        Some(Snap {
            average: v.average.max(1.0),
            rows,
            first,
            spacing: v.spacing,
            highlight,
            range: self.highlight_range(f).unwrap_or((0.0, 0.0)),
            one: mode == 2.0,
        })
    }

    /// Where QQuickItemViewPrivate::positionViewAtIndex puts the view for `index` and `mode`
    /// when it stands at `from`: (the row, whether it is realized, contentX / contentY).
    fn view_pos_at_index(
        &mut self,
        f: usize,
        index: f64,
        mode: i32,
        from: f64,
    ) -> Option<(usize, bool, f64)> {
        let v = self.view_rows(f)?;
        if v.count == 0 || !(0..=5).contains(&mode) {
            return None;
        }
        let idx = index.clamp(0.0, v.count as f64 - 1.0) as usize;
        let (top, pos, size) = v.geom(idx)?;
        let (full, end) = (pos + size - top, pos + size);
        let view = self.fnum(f, v.ax.size(), 0.0);
        let at = from;
        let to = match mode {
            0 => top,
            1 => top - (view - full) / 2.0,
            2 => top - view + full,
            3 if top > at + view => end - view,
            3 if end <= at => top,
            3 => at,
            4 => {
                let p = if end >= at + view {
                    top - view + full
                } else {
                    at
                };
                p.min(top)
            }
            _ => top - self.highlight_range(f).map_or(0.0, |r| r.0),
        };
        let (min, max) = self.extents(f, v.ax);
        let realized =
            v.grid.is_some() || idx.checked_sub(v.first).is_some_and(|k| k < v.rows.len());
        Some((idx, realized, to.min(-max).max(-min)))
    }

    /// QQuickItemView::positionViewAtIndex. A row that is not realized yet is aimed at by
    /// estimate, and the shell's layout of the view settles on it once it is.
    ///
    /// `from` is where the view stood before it jumped to a row without a delegate
    /// (`view_jump_at_index`).
    pub fn view_position_at_index(&mut self, f: usize, index: f64, mode: i32, from: Option<f64>) {
        let Some(ax) = self.view_rows(f).map(|v| v.ax) else {
            return;
        };
        if !(0..=5).contains(&mode) {
            return;
        }
        self.flick_stop(f);
        self.flick_stop_moving(f);
        if self.strict_range(f).is_some() {
            self.view_strict_position(f, ax, index, mode);
            return;
        }
        let from = from.unwrap_or_else(|| self.fnum(f, ax.pos(), 0.0));
        let Some((idx, realized, to)) = self.view_pos_at_index(f, index, mode, from) else {
            return;
        };
        self.view_at_end.remove(&f);
        self.write(f, ax.pos(), Value::Num(to));
        if realized {
            self.view_at_index.remove(&f);
        } else {
            let at = self.fnum(f, ax.pos(), 0.0);
            self.view_at_index.insert(f, (idx, mode, from, at));
        }
        self.view_moved_by_call(f, ax);
    }

    /// QQuickItemViewPrivate::positionViewAtIndex on a row the view holds no delegate of: it
    /// first goes where `positionAt` estimates the row, lets its rows go and makes the ones in
    /// sight there. The row, where the view stands and where it jumps to.
    pub fn view_jump_at_index(
        &mut self,
        f: usize,
        index: f64,
        mode: i32,
    ) -> Option<(usize, f64, f64)> {
        if !(0..=5).contains(&mode)
            || !self.view_rows.contains_key(&f)
            || self.strict_range(f).is_some()
        {
            return None;
        }
        let v = self.view_rows(f)?;
        if v.count == 0 || v.grid.is_some() {
            return None;
        }
        let idx = index.clamp(0.0, v.count as f64 - 1.0) as usize;
        if idx.checked_sub(v.first).is_some_and(|k| k < v.rows.len()) {
            return None;
        }
        let (top, ..) = v.geom(idx)?;
        let (_, max) = self.extents(f, v.ax);
        Some((idx, self.fnum(f, v.ax.pos(), 0.0), top.min(-max)))
    }

    /// True for a ListView with highlightRangeMode: StrictlyEnforceRange and a valid range.
    pub fn view_is_strict(&mut self, f: usize) -> bool {
        self.strict_range(f).is_some()
    }

    /// A StrictlyEnforceRange view goes to `to`, dragging its highlight along.
    fn view_strict_put(&mut self, f: usize, ax: Ax, to: f64) {
        let was = std::mem::replace(&mut self.animating, true);
        self.write(f, ax.pos(), Value::Num(to));
        self.animating = was;
        self.flick_sync(f, ax);
        self.view_strict_moved(f, ax);
    }

    /// The first half of QQuickItemViewPrivate::positionViewAtIndex under
    /// StrictlyEnforceRange: a row outside the buffered rows is jumped to, which drags the
    /// highlight and with it `currentIndex`, before the view takes the position the mode asks
    /// for (`view_position_at_index`).
    pub fn view_strict_jump(&mut self, f: usize, index: f64, mode: i32) {
        if self.strict_range(f).is_none() || !(0..=5).contains(&mode) {
            return;
        }
        let Some(v) = self.view_rows(f) else {
            return;
        };
        if v.count == 0 {
            return;
        }
        let ax = v.ax;
        let idx = index.clamp(0.0, v.count as f64 - 1.0);
        let Some((top, pos, size)) = v.geom(idx as usize) else {
            return;
        };
        self.flick_stop(f);
        self.flick_stop_moving(f);
        self.view_at_end.remove(&f);
        self.view_at_index.remove(&f);
        let view = self.fnum(f, ax.size(), 0.0);
        let buffer = self.fprop(f, "cacheBuffer", 320.0);
        let at = self.fnum(f, ax.pos(), 0.0);
        if pos + size <= at - buffer || pos >= at + view + buffer {
            let (_, max) = self.extents(f, ax);
            let first = if mode == 0 && idx == 0.0 { top } else { pos };
            self.view_strict_put(f, ax, first.min(-max));
        }
    }

    /// The second half of QQuickItemViewPrivate::positionViewAtIndex under
    /// StrictlyEnforceRange: the view takes the position `mode` asks for, the highlight lands
    /// on the current item and the view is fixed up.
    fn view_strict_position(&mut self, f: usize, ax: Ax, index: f64, mode: i32) {
        self.view_strict_jump(f, index, mode);
        let from = self.fnum(f, ax.pos(), 0.0);
        if let Some((_, _, to)) = self.view_pos_at_index(f, index, mode, from) {
            self.fst(f, ax).reason = MoveReason::Other;
            self.view_strict_put(f, ax, to);
        }
        let cur = self.fnum(f, self.view_cur(f), -1.0);
        let to = (cur >= 0.0)
            .then(|| self.view_rows(f)?.row(cur as usize).map(|r| r.pos))
            .flatten();
        let now = self.now;
        if let (Some(to), Some(h)) = (to, self.view_hl.get_mut(&f)) {
            // resetHighlightPosition.
            let _ = h.anim.target(to, to, now);
            h.pos = to;
            h.to = to;
            h.index = cur;
            h.settled = true;
        }
        self.fst(f, ax).reason = MoveReason::Other;
        self.flick_fixup(f, ax, now);
    }

    /// The end of QQuickItemView's positionViewAt*: moveReason Other, then fixupPosition().
    pub fn view_moved_by_call(&mut self, f: usize, ax: Ax) {
        self.flick_sync(f, ax);
        self.fst(f, ax).reason = MoveReason::Other;
        self.view_strict_moved(f, ax);
        let now = self.now;
        self.flick_fixup(f, ax, now);
    }

    /// The view's layout placed its rows: a pending `positionViewAtIndex` settles on its row
    /// once that is realized, unless something else moved the view meanwhile.
    pub fn view_settle_position(&mut self, f: usize) {
        let Some(&(idx, mode, from, at)) = self.view_at_index.get(&f) else {
            return;
        };
        let moved = match self.view_rows(f) {
            Some(v) => self.fnum(f, v.ax.pos(), 0.0) != at,
            None => true,
        };
        let to = match moved {
            true => None,
            false => self.view_pos_at_index(f, idx as f64, mode, from),
        };
        let Some((_, realized, to)) = to else {
            self.view_at_index.remove(&f);
            return;
        };
        if to != at {
            let key = if self.fprop(f, "orientation", 2.0) == 1.0 {
                Ax::X.pos()
            } else {
                Ax::Y.pos()
            };
            let was = std::mem::replace(&mut self.animating, true);
            self.write(f, key, Value::Num(to));
            self.animating = was;
        }
        if realized {
            self.view_at_index.remove(&f);
        } else {
            self.view_at_index.insert(f, (idx, mode, from, to));
        }
    }

    /// The view's layout placed its rows: QQuickListViewPrivate::updateHighlight. The highlight
    /// starts on the current item, and runs to it (QSmoothedAnimation at highlightMoveVelocity
    /// within highlightMoveDuration) when `currentIndex` or the item's position changes.
    pub fn view_follow(&mut self, f: usize) {
        let cur = self.fnum(f, self.view_cur(f), 0.0);
        let follows = self.fprop(f, "highlightFollowsCurrentItem", 1.0) != 0.0;
        let row = match self.view_rows(f) {
            Some(v) if follows && cur >= 0.0 && (cur as usize) < v.count => {
                v.geom(cur as usize).map(|g| (g.1, v))
            }
            _ => None,
        };
        let Some((to, v)) = row else {
            self.view_hl.remove(&f);
            return;
        };
        let now = self.now;
        let grid = v.grid.is_some();
        let velocity = self.fprop(f, "highlightMoveVelocity", if grid { -1.0 } else { 400.0 });
        let duration = self.fprop(f, "highlightMoveDuration", if grid { 150.0 } else { -1.0 });
        let held = self.view_held(f, &v);
        let Some(h) = self.view_hl.get_mut(&f) else {
            self.view_hl.insert(
                f,
                Highlight {
                    anim: SmoothedAnimation::default(),
                    pos: to,
                    index: cur,
                    to,
                    held,
                    settled: true,
                },
            );
            return;
        };
        if h.index == cur && h.to == to {
            return;
        }
        let set_index = h.index != cur;
        h.index = cur;
        h.to = to;
        h.anim.velocity = velocity;
        h.anim.duration = duration;
        let _ = h.anim.target(h.pos, to, now);
        h.settled = !h.anim.running();
        if h.settled {
            h.pos = to;
        }
        if set_index {
            // QQuickItemView::setCurrentIndex.
            self.fst(f, v.ax).reason = MoveReason::SetIndex;
        }
    }

    /// The rows QQuickListView holds: those within cacheBuffer of the view.
    fn view_held(&mut self, f: usize, v: &Rows) -> (usize, usize) {
        let buffer = self.fprop(f, "cacheBuffer", 320.0);
        let at = self.fnum(f, v.ax.pos(), 0.0);
        v.between(at - buffer, at + self.fnum(f, v.ax.size(), 0.0) + buffer)
    }

    /// Highlights still on their way to the current item.
    pub fn highlights_active(&self) -> bool {
        self.view_hl.values().any(|h| !h.settled)
    }

    /// The highlight of view `f` is on its way to the current item.
    pub fn highlight_running(&self, f: usize) -> bool {
        self.view_hl.get(&f).is_some_and(|h| !h.settled)
    }

    /// Move every highlight that is on its way, and its view after it.
    pub(crate) fn step_highlights(&mut self) -> bool {
        let moving: Vec<usize> = (self.view_hl.iter())
            .filter(|(_, h)| !h.settled)
            .map(|(&f, _)| f)
            .collect();
        let now = self.now;
        for &f in &moving {
            if self.dead.contains(&f) {
                self.view_hl.remove(&f);
                continue;
            }
            if let Some(h) = self.view_hl.get_mut(&f) {
                h.pos = h.anim.value(now);
                h.settled = !h.anim.running();
            }
            self.view_track(f);
        }
        !moving.is_empty()
    }

    /// QQuickItemView::trackedPositionChanged: a view moved by `currentIndex` keeps its
    /// highlight in sight.
    fn view_track(&mut self, f: usize) {
        // The view's layout moves it after its highlight (`ViewTrack`).
        if self.view_track.contains_key(&f) && self.strict_range(f).is_none() {
            return;
        }
        let Some(v) = self.view_rows(f) else {
            return;
        };
        let Some(h) = self.view_hl.get(&f) else {
            return;
        };
        let (hl, index) = (h.pos, h.index as usize);
        if self.fst(f, v.ax).reason != MoveReason::SetIndex {
            return;
        }
        let Some((top, item, size)) = v.geom(index) else {
            return;
        };
        let view = self.fnum(f, v.ax.size(), 0.0);
        let at = self.fnum(f, v.ax.pos(), 0.0);
        let mut pos = at;
        if let Some((begin, end)) = self.highlight_range(f) {
            if hl > pos + end - size {
                pos = hl - end + size;
            }
            if hl < pos + begin {
                pos = hl - begin;
            }
            if self.fprop(f, "highlightRangeMode", 0.0) != 2.0 {
                let (min, max) = self.extents(f, v.ax);
                pos = pos.min(-max).max(-min);
            }
        } else {
            // The current item's section header stays in sight too.
            let section = item - top;
            let (tracked, tracked_end) = (hl - section, hl + size);
            let (to, to_end) = (top, item + size);
            if tracked_end >= at + view && to_end >= at + view {
                if tracked_end <= to_end {
                    pos = tracked_end - view;
                    if size + section > view {
                        pos = tracked;
                    }
                } else {
                    pos = to_end - view;
                    if to_end - to > view {
                        pos = to;
                    }
                }
            }
            if tracked < pos && to < pos {
                pos = tracked.max(to);
            }
        }
        if pos != at {
            let was = std::mem::replace(&mut self.animating, true);
            self.write(f, v.ax.pos(), Value::Num(pos));
            self.animating = was;
        }
        if pos == at {
            // No move to refill after: bufferPause runs out.
            self.view_refill(f);
        }
    }

    /// QQuickItemViewPrivate::refill, for the rows the view holds. A call that finds rows in
    /// sight missing adds only those and leaves cacheBuffer to the call one tick later
    /// (bufferPause); any other fills cacheBuffer. QQuickListViewPrivate::visibleItemsChanged:
    /// while the current item is outside the rows held, each change of those restarts the
    /// highlight's move.
    pub fn view_refill(&mut self, f: usize) {
        let Some(v) = self.view_rows(f) else {
            return;
        };
        if v.grid.is_some() {
            return;
        }
        let buffer = self.fprop(f, "cacheBuffer", 320.0);
        let at = self.fnum(f, v.ax.pos(), 0.0);
        let end = at + self.fnum(f, v.ax.size(), 0.0);
        let seen = v.between(at, end);
        let all = v.between(at - buffer, end + buffer);
        let now = self.now;
        let next = |was: (usize, usize)| {
            if was.0 <= seen.0 && seen.1 <= was.1 {
                all
            } else if seen.0 > was.1 || seen.1 < was.0 {
                seen
            } else {
                (was.0.max(all.0).min(seen.0), was.1.min(all.1).max(seen.1))
            }
        };
        if let Some(h) = self.view_hl.get_mut(&f) {
            let was = h.held;
            h.held = next(was);
            let index = h.index as usize;
            if h.held != was && !(h.held.0..=h.held.1).contains(&index) && h.anim.running() {
                let _ = h.anim.target(h.pos, h.to, now);
                h.settled = !h.anim.running();
            }
        }
        if let Some(t) = self.view_track.get_mut(&f) {
            let Some(was) = t.held else {
                t.held = Some(all);
                return;
            };
            let held = next(was);
            t.held = Some(held);
            if held != was && t.index >= 0.0 && !(held.0..=held.1).contains(&(t.index as usize)) {
                t.restart(now);
            }
        }
    }

    /// The snapping branch of QQuickListViewPrivate::fixup: rest on the item nearest the top of
    /// the view, at once unless a flick or a drag moved it. False when no item is there to snap
    /// to (QQuickItemViewPrivate::fixup then takes over).
    fn snap_fixup(&mut self, f: usize, ax: Ax, at: f64, snap: &Snap) -> bool {
        let (min, max) = self.extents(f, ax);
        let st = self.fst(f, ax);
        st.correct = false;
        if st.reason == MoveReason::SetIndex {
            return false;
        }
        let (mv, was_active, mouse) = (st.mv, st.active, st.reason == MoveReason::Mouse);
        let velocity = st.velocity;
        let mut temp = -mv;
        if snap.one && mouse {
            // Dragged less than half an item: lean towards the next one.
            let dist = mv - st.press_move;
            let half = snap.average / 2.0;
            if velocity > 0.0 && dist > SNAP_ONE_THRESHOLD && dist < half {
                temp -= half;
            } else if velocity < 0.0 && dist < -SNAP_ONE_THRESHOLD && dist > -half {
                temp += half;
            }
        }
        let in_bounds = mv > max && mv <= min;
        let strict = self.strict_range(f).is_some();
        if strict {
            self.view_hl_restart(f);
        }
        let (mut top, mut bottom) = (
            snap.item_at(temp + snap.range.0, velocity),
            snap.item_at(temp + snap.range.1, velocity),
        );
        let cur = self.fnum(f, self.view_cur(f), -1.0);
        if strict
            && cur >= 0.0
            && let Some(cur) = (cur as usize).checked_sub(snap.first)
            && cur < snap.rows.len()
        {
            // StrictlyEnforceRange always keeps an item in range.
            let cur = Some(cur);
            if top.is_none() || (top != cur && !mouse) {
                top = cur;
            }
            if bottom.is_none() || (bottom != cur && !mouse) {
                bottom = cur;
            }
        }
        let pos = match (top, bottom) {
            (Some(t), _) if in_bounds || strict => snap.rows[t].pos - snap.range.0,
            (None, Some(b)) if in_bounds => snap.rows[b].pos - snap.range.1,
            _ => return false,
        };
        let to = min.min(max.max(-pos));
        self.reset_timeline(f, ax);
        if mouse {
            self.push_op(
                f,
                ax,
                Op::Move {
                    to,
                    ease: IN_OUT_QUAD,
                    len: FIXUP_DURATION / 2.0,
                },
                at,
            );
            self.fst(f, ax).fixing_up = true;
        } else if was_active {
            self.push_op(f, ax, Op::Set(to), at);
        } else if to != mv {
            self.set_move(f, ax, to, at);
        }
        let st = self.fst(f, ax);
        st.in_overshoot = false;
        st.v_time = at;
        true
    }

    /// QQuickListViewPrivate::flick of a snapping view: the flick ends on an item boundary,
    /// estimated when it starts and looked up again as it nears it. True when it starts one.
    fn snap_flick(&mut self, f: usize, ax: Ax, v: (f64, bool), at: f64, snap: &Snap) -> bool {
        let (mut velocity, wheel) = v;
        let (min, max) = self.extents(f, ax);
        let bounds = self.fprop(f, "boundsBehavior", 3.0) as i32;
        let max_v = self.fprop(f, "maximumFlickVelocity", 2500.0);
        let view = self.fnum(f, ax.size(), 0.0);
        let flicking = self.fget(f, Ax::X).is_some_and(|s| s.flicking)
            || self.fget(f, Ax::Y).is_some_and(|s| s.flicking);
        let mut accel = if wheel {
            self.fst(f, ax).wheel_decel
        } else {
            self.fprop(f, "flickDeceleration", 1500.0)
        };
        let st = self.fst(f, ax);
        st.fixing_up = false;
        st.reason = MoveReason::Mouse;
        let (mv, in_overshoot) = (st.mv, st.in_overshoot);
        let begin = snap.range.0;
        // -snapPosAt(-(move - highlightRangeStart) + by) + highlightRangeStart, in our space.
        let target_by = |by: f64, v: f64| -snap.pos_at(-(mv - begin) + by, v) + begin;
        let press = if st.pressed { st.press_move } else { 0.0 };
        let bias = snap.average / 2.0 + 1.0 - press;
        let mut max_dist = 0.0;
        let mut target = st.flick_target;
        if velocity > 0.0 && mv < min {
            if snap.one && !flicking {
                target = target_by(-bias, velocity);
                max_dist = (target - mv).abs();
                velocity = max_v;
            } else {
                max_dist = (min - mv).abs();
            }
        } else if velocity <= 0.0 && mv > max {
            if snap.one && !flicking {
                target = target_by(bias, velocity);
                max_dist = (target - mv).abs();
                velocity = -max_v;
            } else {
                max_dist = (max - mv).abs();
            }
        }
        let overshoot = bounds & OVERSHOOT_BOUNDS != 0;
        if max_dist <= 0.0 && !overshoot {
            self.fst(f, ax).correct = false;
            self.reset_timeline(f, ax);
            self.flick_fixup(f, ax, at);
            return false;
        }
        let mut v = velocity;
        if max_v != -1.0 && max_v < v.abs() {
            v = max_v.copysign(v);
        }
        // QQuickFlickablePrivate::overShootDistance(vSize).
        let over_by = if max_v <= 0.0 {
            0.0
        } else {
            OVERSHOOT.min(view / 3.0)
        };
        if !flicking {
            let mut v2 = v * v;
            let mut over = 0.0;
            // A quarter of an item more, to move at least one item the way of the flick.
            let mut dist = v2 / (accel * 2.0) + snap.average / 4.0;
            if max_dist > 0.0 {
                dist = dist.min(max_dist);
            }
            if v > 0.0 {
                dist = -dist;
            }
            let mut widen = |target: &mut f64| {
                if *target > min {
                    over = over_by;
                    *target += over;
                } else if *target < max {
                    over = over_by;
                    *target -= over;
                }
            };
            if (max_dist > 0.0 && v2 / (2.0 * max_dist) < accel) || snap.one {
                if !snap.one {
                    target = target_by(dist, velocity);
                }
                if overshoot {
                    widen(&mut target);
                }
                let adjusted = -target + mv;
                if adjusted.abs() > dist.abs() {
                    // Not painfully slow: the velocity fits flickDeceleration.
                    let adjusted_v2 = accel * 2.0 * adjusted.abs();
                    if adjusted_v2 > v2 {
                        v2 = adjusted_v2;
                        v = v2.sqrt();
                        if dist > 0.0 {
                            v = -v;
                        }
                    }
                }
                dist = adjusted;
                accel = v2 / (2.0 * dist.abs());
            } else if overshoot {
                target = mv - dist;
                widen(&mut target);
            }
            self.reset_timeline(f, ax);
            if !in_overshoot {
                self.push_accel(f, ax, v, accel, Some(max_dist + over), at);
            }
            self.push_op(f, ax, Op::Fixup, at);
            let st = self.fst(f, ax);
            st.flick_target = target;
            st.correct = true;
            return self.flick_dir(f, ax);
        }
        // Already flicking: look the boundary up again.
        let old = self.fst(f, ax).flick_target;
        let mut new = -snap.pos_at(-(old - begin), velocity) + begin;
        if velocity < 0.0 && new <= max {
            new = max - over_by;
        } else if velocity > 0.0 && new >= min {
            new = min + over_by;
        }
        if new == old {
            if velocity.abs() < MINIMUM_FLICK_VELOCITY {
                self.fst(f, ax).correct = false;
            }
            return false;
        }
        self.fst(f, ax).flick_target = new;
        let dist = -new + mv;
        self.reset_timeline(f, ax);
        if (v < 0.0 && dist < 0.0) || (v > 0.0 && dist > 0.0) {
            self.flick_fixup(f, ax, at);
            return false;
        }
        // QQuickTimeLine::accelDistance(move, v, -dist).
        let len = (1000.0 * (2.0 * -dist) / v).trunc();
        if dist.abs() > 1e-12 && v.abs() > 1e-12 && len > 0.0 {
            let a = -1000.0 * v / len;
            self.push_op(f, ax, Op::Accel { v, a, len }, at);
        }
        self.push_op(f, ax, Op::Fixup, at);
        false
    }

    /// QQuickListView::viewportMoved near the end of a snapping flick: the rows realized since
    /// it started may have moved the boundary it aims at.
    fn snap_correct(&mut self, f: usize, ax: Ax, at: f64) {
        let st = self.fst(f, ax);
        if !st.correct || st.correcting || st.in_overshoot || !st.flicking {
            return;
        }
        let (mv, target, velocity, smooth) = (st.mv, st.flick_target, st.velocity, st.smooth);
        let (min, max) = self.extents(f, ax);
        let half = self.fnum(f, ax.size(), 0.0) / 2.0;
        let near = if velocity > 0.0 {
            (min - mv < half || target - mv < half) && min != target
        } else if velocity < 0.0 {
            (mv - max < half || mv - target < half) && max != target
        } else {
            false
        };
        if near {
            self.fst(f, ax).correcting = true;
            self.flick_start(f, ax, -smooth, false, at);
            self.fst(f, ax).correcting = false;
        }
    }

    /// QQuickFlickablePrivate::fixup: animate back into bounds or onto a whole pixel.
    fn flick_fixup(&mut self, f: usize, ax: Ax, at: f64) {
        match self.snap_of(f, ax) {
            Some(snap) => {
                if self.snap_fixup(f, ax, at, &snap) {
                    return;
                }
            }
            None => {
                if self.strict_fixup(f, ax, at) {
                    return;
                }
            }
        }
        let (min, max) = self.extents(f, ax);
        let st = self.fst(f, ax);
        let (mv, was_active) = (st.mv, st.active);
        if mv >= min || max > min {
            self.reset_timeline(f, ax);
            if mv != min {
                self.adjust_content_pos(f, ax, min, at);
            } else if was_active {
                self.push_op(
                    f,
                    ax,
                    Op::Move {
                        to: mv,
                        ease: LINEAR,
                        len: 1.0,
                    },
                    at,
                );
            }
        } else if mv <= max {
            self.reset_timeline(f, ax);
            self.adjust_content_pos(f, ax, max, at);
        } else if -(-mv).round() != mv {
            self.reset_timeline(f, ax);
            let smooth = self.fst(f, ax).smooth;
            let val = if (mv.round() - mv).abs() < 0.25 {
                mv.round()
            } else if smooth > 0.0 {
                mv.ceil()
            } else if smooth < 0.0 {
                mv.floor()
            } else {
                mv.round()
            };
            self.push_op(f, ax, Op::Set(val), at);
        }
        let st = self.fst(f, ax);
        st.in_overshoot = false;
        st.v_time = at;
    }

    fn adjust_content_pos(&mut self, f: usize, ax: Ax, to: f64, at: f64) {
        let dist = to - self.fst(f, ax).mv;
        let q = FIXUP_DURATION / 4.0;
        self.push_op(
            f,
            ax,
            Op::Move {
                to: to - dist / 2.0,
                ease: IN_QUAD,
                len: q,
            },
            at,
        );
        self.push_op(
            f,
            ax,
            Op::Move {
                to,
                ease: OUT_EXPO,
                len: 3.0 * q,
            },
            at,
        );
        self.fst(f, ax).fixing_up = true;
    }

    /// QQuickFlickablePrivate::flick: decelerate from `velocity` (move space) to a whole pixel.
    /// True when this starts a new flick.
    fn flick_start(&mut self, f: usize, ax: Ax, velocity: f64, wheel: bool, at: f64) -> bool {
        let snap = match self.strict_range(f) {
            Some(_) => self.snap_rows(f, ax),
            None => self.snap_of(f, ax),
        };
        if let Some(snap) = snap {
            return self.snap_flick(f, ax, (velocity, wheel), at, &snap);
        }
        let (min, max) = self.extents(f, ax);
        let bounds = self.fprop(f, "boundsBehavior", 3.0) as i32;
        let max_v = self.fprop(f, "maximumFlickVelocity", 2500.0);
        let decel = if wheel {
            self.fst(f, ax).wheel_decel
        } else {
            self.fprop(f, "flickDeceleration", 1500.0)
        };
        let st = self.fst(f, ax);
        st.fixing_up = false;
        let mv = st.mv;
        let max_dist = if velocity > 0.0 {
            (min - mv).abs()
        } else {
            (max - mv).abs()
        };
        if max_dist > 0.0 || bounds & OVERSHOOT_BOUNDS != 0 {
            let mut v = velocity;
            if max_v != -1.0 && max_v < v.abs() {
                v = max_v.copysign(v);
            }
            let v2 = v * v;
            let mut dist = v2 / (decel * 2.0);
            if v > 0.0 {
                dist = -dist;
            }
            let target = (mv - dist).round();
            dist = -target + mv;
            let accel = v2 / (2.0 * dist.abs());
            self.reset_timeline(f, ax);
            if !self.fst(f, ax).in_overshoot {
                let cap = (bounds & OVERSHOOT_BOUNDS == 0).then_some(max_dist);
                self.push_accel(f, ax, v, accel, cap, at);
            }
            self.push_op(f, ax, Op::Fixup, at);
            !self.fst(f, ax).flicking && self.flick_dir(f, ax)
        } else {
            self.reset_timeline(f, ax);
            self.flick_fixup(f, ax, at);
            false
        }
    }

    /// Advance running timelines to the animation clock (QQuickTimeLine::advance).
    pub(crate) fn step_flicks(&mut self) {
        if self.flicks_live.is_empty() {
            return;
        }
        let keys: Vec<(usize, Ax)> = self.flicks_live.iter().copied().collect();
        for (f, ax) in keys {
            if self.dead.contains(&f) {
                self.flicks.remove(&f);
                self.flicks_x.remove(&f);
                self.flicks_live.remove(&(f, ax));
                continue;
            }
            if !self.fget(f, ax).is_some_and(|s| s.active) {
                self.flicks_live.remove(&(f, ax));
                continue;
            }
            self.flick_sync(f, ax);
            if !self.fst(f, ax).active {
                self.flicks_live.remove(&(f, ax));
                continue;
            }
            let now = self.now;
            loop {
                let st = self.fst(f, ax);
                let Some(op) = st.ops.front().cloned() else {
                    break;
                };
                let (t0, base, len) = (st.t0, st.base, op.len());
                let el = (now - t0).floor();
                if el >= len {
                    let v = op.value(base, len);
                    st.ops.pop_front();
                    st.t0 = t0 + len;
                    st.base = v;
                    let end = t0 + len;
                    if v != self.fst(f, ax).mv {
                        self.set_move(f, ax, v, end);
                    }
                    if matches!(op, Op::Fixup) {
                        self.flick_fixup(f, ax, end);
                    }
                } else {
                    let v = op.value(base, el);
                    if v != st.mv {
                        self.set_move(f, ax, v, now);
                    }
                    break;
                }
            }
            let st = self.fst(f, ax);
            if st.ops.is_empty() && st.active {
                st.active = false;
                // QQuickFlickable::timelineCompleted.
                self.movement_ending(f, ax);
                self.update_beginning_end(f, ax);
            }
            if !self.fget(f, ax).is_some_and(|s| s.active) {
                self.flicks_live.remove(&(f, ax));
            }
        }
    }

    /// QQuickFlickable::wheelEvent for angleDelta `angle` (x, y) at event time `t`. False when
    /// the event is not consumed (it propagates to the parent). Like any QAbstractAnimationJob,
    /// the timeline starts at the last animation tick and first advances a frame later.
    pub fn flick_wheel(&mut self, f: usize, angle: (f64, f64), t: f64) -> bool {
        for ax in [Ax::X, Ax::Y] {
            self.flick_sync(f, ax);
        }
        // lastPosTime is one value for both axes.
        let last = self.fst(f, Ax::Y).last_pos_time.unwrap_or(-1.0);
        let elapsed = (t - last) / 1000.0;
        let moving = self.fst(f, Ax::X).moving || self.fst(f, Ax::Y).moving;
        let mut accepted = false;
        if elapsed > 0.0 {
            accepted |= self.wheel_axis(f, Ax::Y, angle.1, elapsed, moving);
            accepted |= self.wheel_axis(f, Ax::X, angle.0, elapsed, moving);
        }
        for ax in [Ax::X, Ax::Y] {
            self.fst(f, ax).last_pos_time = Some(t);
        }
        accepted
    }

    /// One axis of `flick_wheel`: `elapsed` seconds since the last event, `moving` is isMoving().
    fn wheel_axis(&mut self, f: usize, ax: Ax, angle: f64, elapsed: f64, moving: bool) -> bool {
        let now = self.now;
        let st = self.fst(f, ax);
        let decel = st.wheel_decel;
        let along = self.flick_dir(f, ax) && angle != 0.0;
        let (min, max) = self.extents(f, ax);
        let mut accepted = false;
        if decel > MAXIMUM_WHEEL_DECELERATION {
            if along {
                let bounds = self.fprop(f, "boundsBehavior", 3.0) as i32;
                let mut px = angle / 120.0 * WHEEL_SCROLL_LINES * 24.0;
                let st = self.fst(f, ax);
                let mv = st.mv;
                let moved = if px > 0.0 { mv < min } else { mv > max };
                accepted = moved;
                if moved {
                    if bounds == 0 {
                        let est = px + mv;
                        if px > 0.0 && est > min {
                            px = min - mv;
                            accepted = false;
                        } else if px < 0.0 && est < max {
                            px = max - mv;
                            accepted = false;
                        }
                    }
                    self.reset_timeline(f, ax);
                    self.fst(f, ax).v_moved = true;
                    self.movement_starting(f, ax);
                    let len = 3.0 * FIXUP_DURATION / 4.0;
                    self.push_op(
                        f,
                        ax,
                        Op::MoveBy {
                            by: px,
                            ease: OUT_EXPO,
                            len,
                        },
                        now,
                    );
                    self.fst(f, ax).fixing_up = true;
                    self.push_op(f, ax, Op::Fixup, now);
                }
            }
        } else if along {
            let max_v = self.fprop(f, "maximumFlickVelocity", 2500.0);
            let st = self.fst(f, ax);
            let elapsed = if moving {
                elapsed
            } else {
                120.0 / (decel * 2.0 * WHEEL_SCROLL_LINES * 24.0).sqrt()
            };
            let inst = angle / elapsed;
            if (inst < 0.0 && st.velocity > 0.0) || (inst > 0.0 && st.velocity < 0.0) {
                st.samples.clear();
            }
            add_sample(st, inst, max_v);
            update_velocity(st);
            let cy = -st.mv;
            if (angle > 0.0 && cy > -min) || (angle < 0.0 && cy < -max) {
                let v = st.velocity;
                let new_flick = self.flick_start(f, ax, v, true, now);
                let st = self.fst(f, ax);
                if new_flick && (st.at_beginning != (angle > 0.0) || st.at_end != (angle < 0.0)) {
                    self.flicking_started(f, ax, true);
                    self.fst(f, ax).v_moved = true;
                    self.movement_starting(f, ax);
                }
                accepted = true;
            }
        }
        accepted
    }

    /// QQuickFlickablePrivate::handlePressEvent at local `(x, y)`, event time `t`.
    pub fn flick_press(&mut self, f: usize, (x, y): (f64, f64), t: f64) {
        self.press_axis(f, Ax::X, x, t);
        self.press_axis(f, Ax::Y, y, t);
        self.share_steal(f);
    }

    /// stealGrab is one flag for both axes.
    fn share_steal(&mut self, f: usize) -> bool {
        let steal = self.fst(f, Ax::X).steal_grab || self.fst(f, Ax::Y).steal_grab;
        self.fst(f, Ax::X).steal_grab = steal;
        self.fst(f, Ax::Y).steal_grab = steal;
        steal
    }

    /// The Flickable wants the pointer grab (QQuickFlickablePrivate::stealGrab).
    pub fn flick_steal(&self, f: usize) -> bool {
        [Ax::X, Ax::Y]
            .into_iter()
            .any(|ax| self.fget(f, ax).is_some_and(|s| s.steal_grab))
    }

    fn press_axis(&mut self, f: usize, ax: Ax, y: f64, t: f64) {
        self.flick_sync(f, ax);
        let st = self.fst(f, ax);
        if st.active && st.smooth.abs() > RETAIN_GRAB_VELOCITY && !st.fixing_up && !st.in_overshoot
        {
            st.steal_grab = true;
            st.continuous = -st.smooth;
        } else {
            st.steal_grab = false;
            st.continuous = 0.0;
            st.boost = 1.0;
        }
        // maybeBeginDrag.
        st.pressed = true;
        if !st.fixing_up {
            self.reset_timeline(f, ax);
        }
        let (min, max) = self.extents(f, ax);
        let st = self.fst(f, ax);
        st.samples.clear();
        st.drag_start_offset = 0.0;
        st.fixing_up = false;
        st.in_overshoot = false;
        st.drag_min = min;
        st.drag_max = max;
        st.press_y = y;
        st.press_move = st.mv;
        st.flicking_when_drag_began = st.flicking;
        if st.flicking {
            st.flicking = false;
            self.flag(f, ax, "flicking", false, |s: &Flick| s.flicking);
        }
        let st = self.fst(f, ax);
        st.last_pos_time = Some(t);
        st.last_press_time = t;
        st.velocity_time = t;
    }

    /// QQuickFlickablePrivate::handleMoveEvent + drag at local `(x, y)`; `vel` is the point's
    /// velocity. Returns stealGrab.
    pub fn flick_move(&mut self, f: usize, (x, y): (f64, f64), t: f64, vel: (f64, f64)) -> bool {
        self.flick_sync(f, Ax::X);
        self.flick_sync(f, Ax::Y);
        // overThreshold: the view is moving or the point passed the drag threshold on an axis
        // it flicks along.
        let mut over = self.flick_moving(f);
        for (ax, p) in [(Ax::Y, y), (Ax::X, x)] {
            if self.flick_dir(f, ax) {
                let st = self.fst(f, ax);
                over |= st.last_pos_time.is_some() && (p - st.press_y).abs() > START_DRAG_DISTANCE;
            }
        }
        self.move_axis(f, Ax::Y, y, t, vel.1, over);
        self.move_axis(f, Ax::X, x, t, vel.0, over);
        self.share_steal(f)
    }

    fn move_axis(&mut self, f: usize, ax: Ax, y: f64, t: f64, vel: f64, over: bool) {
        let bounds = self.fprop(f, "boundsBehavior", 3.0) as i32;
        let max_v = self.fprop(f, "maximumFlickVelocity", 2500.0);
        let vertical = self.flick_dir(f, ax);
        let st = self.fst(f, ax);
        if st.last_pos_time.is_none() {
            return;
        }
        let dy = y - st.press_y;
        let prev_moved = st.v_moved;
        let mut steal_y = st.steal_grab;
        let mut reject = false;
        if vertical {
            if over || t - st.last_press_time > 200.0 {
                if !st.v_moved && !st.dragging {
                    st.drag_start_offset = dy;
                }
                let mut new_y = dy + st.press_move - st.drag_start_offset;
                let (min_y, max_y) = (st.drag_min, st.drag_max);
                if bounds & DRAG_OVER_BOUNDS == 0 {
                    if fuzzy_le(new_y, max_y) {
                        new_y = max_y;
                        reject = st.press_move == max_y && st.mv == max_y && dy < 0.0;
                    }
                    if fuzzy_le(min_y, new_y) {
                        new_y = min_y;
                        reject |= st.press_move == min_y && st.mv == min_y && dy > 0.0;
                    }
                } else if new_y > min_y {
                    new_y = min_y + (new_y - min_y) / 2.0;
                } else if new_y < max_y && max_y - min_y <= 0.0 {
                    new_y = max_y + (new_y - max_y) / 2.0;
                }
                if !reject && st.steal_grab && dy != st.previous_drag_delta {
                    self.reset_timeline(f, ax);
                    self.set_move(f, ax, new_y, t);
                    self.fst(f, ax).v_moved = true;
                }
                if !reject && over {
                    steal_y = true;
                }
            }
            self.fst(f, ax).previous_drag_delta = dy;
        }
        let st = self.fst(f, ax);
        st.steal_grab = steal_y;
        if reject {
            st.samples.clear();
            st.velocity = 0.0;
        }
        self.dragging_starting(f, ax);
        if self.fst(f, ax).v_moved && !prev_moved {
            self.movement_starting(f, ax);
        }
        let st = self.fst(f, ax);
        st.last_pos_time = Some(t);
        if vertical && !reject {
            add_sample(st, vel, max_v);
        }
    }

    /// QQuickFlickablePrivate::handleReleaseEvent; `grabbed`: the Flickable held the grab.
    pub fn flick_release(&mut self, f: usize, (x, y): (f64, f64), t: f64, grabbed: bool) {
        self.release_axis(f, Ax::Y, y, t, grabbed);
        self.release_axis(f, Ax::X, x, t, grabbed);
    }

    fn release_axis(&mut self, f: usize, ax: Ax, y: f64, t: f64, grabbed: bool) {
        self.flick_sync(f, ax);
        let now = self.now;
        let vertical = self.flick_dir(f, ax);
        let ratio = self.fnum(f, ax.content(), 0.0) / self.fnum(f, ax.size(), 1.0);
        let st = self.fst(f, ax);
        st.steal_grab = false;
        st.pressed = false;
        let elapsed = t - st.last_pos_time.unwrap_or(-1.0);
        update_velocity(st);
        self.dragging_ending(f, ax);
        let st = self.fst(f, ax);
        if st.last_pos_time.is_none() {
            return;
        }
        st.v_time = now;
        let mut vv = 0.0;
        if elapsed < 100.0 && st.velocity != 0.0 {
            vv = st.velocity;
        }
        let mut can_boost = false;
        if (st.at_beginning && vv > 0.0) || (st.at_end && vv < 0.0) {
            vv /= 2.0;
        } else if st.continuous != 0.0
            && ratio > MULTIFLICK_RATIO
            && (vv > 0.0) == (st.continuous > 0.0)
            && vv.abs() > MULTIFLICK_THRESHOLD
        {
            can_boost = true;
        }
        st.boost = if can_boost {
            (st.boost + 0.25).clamp(1.0, MULTIFLICK_MAXBOOST)
        } else {
            1.0
        };
        vv *= st.boost;
        let allowed = grabbed
            && vertical
            && vv.abs() > MINIMUM_FLICK_VELOCITY
            && (y - st.press_y).abs() > FLICK_START_DISTANCE;
        let mut flicked = false;
        if allowed {
            st.smooth = -vv;
            flicked = self.flick_start(f, ax, vv, false, now);
        } else {
            self.flick_fixup(f, ax, now);
        }
        self.flicking_started(f, ax, flicked);
        if !self.fst(f, ax).active {
            self.movement_ending(f, ax);
        } else {
            if flicked {
                self.fst(f, ax).v_moved = true;
            }
            self.movement_starting(f, ax);
        }
    }

    /// QQuickFlickablePrivate::cancelInteraction (the grab went elsewhere).
    pub fn flick_cancel(&mut self, f: usize) {
        self.cancel_axis(f, Ax::Y);
        self.cancel_axis(f, Ax::X);
    }

    fn cancel_axis(&mut self, f: usize, ax: Ax) {
        self.flick_sync(f, ax);
        let now = self.now;
        let st = self.fst(f, ax);
        if !st.pressed {
            return;
        }
        st.pressed = false;
        self.dragging_ending(f, ax);
        self.fst(f, ax).steal_grab = false;
        self.flick_fixup(f, ax, now);
        if !self.fst(f, ax).active {
            self.movement_ending(f, ax);
        }
    }

    /// QQuickFlickable::flick(xVelocity, yVelocity), in pixels/sec, up to `movementStarting`;
    /// [`World::flick_by_started`] takes the returned axes and finishes it. Qt emits inside the
    /// call, so the caller delivers the signals queued by each half before the next.
    pub fn flick_by(&mut self, f: usize, (xv, yv): (f64, f64)) -> [bool; 2] {
        let now = self.now;
        for (ax, v) in [(Ax::X, xv), (Ax::Y, yv)] {
            self.flick_sync(f, ax);
            let st = self.fst(f, ax);
            // AxisData::reset.
            st.samples.clear();
            st.drag_start_offset = 0.0;
            st.fixing_up = false;
            st.in_overshoot = false;
            st.velocity = v;
            st.v_time = now;
        }
        let mut flicked = [false; 2];
        for (k, (ax, v)) in [(Ax::X, xv), (Ax::Y, yv)].into_iter().enumerate() {
            flicked[k] =
                self.flick_dir(f, ax) && v.abs() > 1e-12 && self.flick_start(f, ax, v, false, now);
            if flicked[k] {
                self.fst(f, ax).v_moved = true;
            }
        }
        for ax in [Ax::X, Ax::Y] {
            self.movement_starting(f, ax);
        }
        flicked
    }

    /// The `flickingStarted` half of QQuickFlickable::flick.
    pub fn flick_by_started(&mut self, f: usize, flicked: [bool; 2]) {
        for (k, ax) in [Ax::X, Ax::Y].into_iter().enumerate() {
            self.flicking_started(f, ax, flicked[k]);
        }
    }

    /// QQuickFlickable::cancelFlick up to the flicking signals; [`World::flick_stop_moving`]
    /// emits the moving ones.
    pub fn flick_stop(&mut self, f: usize) {
        for ax in [Ax::X, Ax::Y] {
            self.flick_sync(f, ax);
            self.reset_timeline(f, ax);
        }
        for ax in [Ax::X, Ax::Y] {
            self.flicking_ending(f, ax);
        }
    }

    /// QQuickFlickable::setTopMargin and its three siblings: a Flickable at rest (and laid out
    /// once, `scene`'s `\x01vstart`) is brought back inside its new bounds at once, then
    /// updateBeginningEnd.
    pub fn flick_margin_set(&mut self, f: usize, name: &str) {
        if !self.items[f].props.contains_key("\x01vstart") {
            return;
        }
        let ax = match name {
            "leftMargin" | "rightMargin" => Ax::X,
            _ => Ax::Y,
        };
        let pressed = self.fget(f, ax).is_some_and(|s| s.pressed);
        if !pressed && !self.flick_moving(f) {
            let (min, max) = self.extents(f, ax);
            let mv = -self.fnum(f, ax.pos(), 0.0);
            let to = if mv >= min {
                min
            } else if mv <= max {
                max
            } else {
                mv
            };
            if to != mv {
                let was = std::mem::replace(&mut self.animating, true);
                self.write(f, ax.pos(), Value::Num(-to));
                self.animating = was;
            }
        }
        self.flick_edges(f);
    }

    /// The moving half of QQuickFlickable::movementEnding, for both axes.
    pub fn flick_stop_moving(&mut self, f: usize) {
        for ax in [Ax::X, Ax::Y] {
            self.moving_ending(f, ax);
        }
    }

    /// QQuickFlickable::returnToBounds.
    pub fn flick_return(&mut self, f: usize) {
        let now = self.now;
        for ax in [Ax::X, Ax::Y] {
            self.flick_sync(f, ax);
            self.flick_fixup(f, ax, now);
        }
    }

    /// QQuickFlickablePrivate::setViewportX / setViewportY, once contentX / contentY (`pos`) is
    /// stored: the contentItem moves, QQuickFlickable::viewportMoved runs, then contentXChanged /
    /// contentYChanged is emitted, and the contentItem notifies its own `x` / `y` last.
    pub(crate) fn viewport_moved(&mut self, f: usize, pos: Sym) {
        let Some(&c) = self.content_item_of.get(&f) else {
            return;
        };
        let reading = self.reading.take();
        let ax = if &*pos == "contentY" { Ax::Y } else { Ax::X };
        if self.view_rows.contains_key(&f) {
            // QQuickListView::viewportMoved at velocity 0: the cache is filled after the
            // visible rows only from now on.
            self.view_after.insert(f);
        }
        // QQuickItemView::setContentY, QQuickListView::viewportMoved while flicking: the view
        // is moved by hand and stops following the current row (`moveReason`).
        if let Some(t) = self.view_track.get_mut(&f) {
            t.following = false;
        }
        let key = (c, Sym::new(ax.item_pos()));
        let hold = self.notify_hold.replace(key);
        let at = self.content_pos(f, ax);
        self.place(c, ax.item_pos(), at);
        self.notify_hold = hold;
        self.flick_edges(f);
        self.release_notify((f, pos));
        self.release_notify(key);
        self.reading = reading;
    }

    /// Where the contentItem sits along `ax`: at minus contentX / contentY.
    fn content_pos(&mut self, f: usize, ax: Ax) -> f64 {
        -self.fnum(f, ax.pos(), 0.0)
    }

    /// QQuickFlickable::componentComplete: a Flickable with a left or top margin starts at
    /// minus that margin, unless its contentX / contentY was set.
    pub(crate) fn flick_complete(&mut self, f: usize) {
        // A view is put at its start by its first layout (`scene`'s `\x01vstart`).
        let view = self.items[f]
            .types
            .iter()
            .any(|t| t == "ListView" || t == "GridView");
        if view {
            return;
        }
        for ax in [Ax::X, Ax::Y] {
            let m = self.fnum(f, ax.start_margin(), 0.0);
            if m != 0.0 && self.fprop(f, ax.pos(), f64::NAN).is_nan() {
                self.write(f, ax.pos(), Value::Num(-m));
            }
        }
    }

    /// QQuickFlickable::geometryChange, setContentWidth / setContentHeight and the views'
    /// setOrientation / setFlow, once `name` is stored: the contentItem is as large as the
    /// content, or as the view less its margins while the content has no size. Only the axis
    /// that changed is written.
    pub(crate) fn content_resized(&mut self, f: usize, name: &str) {
        let Some(&c) = self.content_item_of.get(&f) else {
            return;
        };
        // An item view sets the content size along its flow itself (0 while it is empty).
        let view = self.items[f]
            .types
            .iter()
            .any(|t| t == "ListView" || t == "GridView");
        let axes: &[Ax] = match name {
            "width" | "contentWidth" => &[Ax::X],
            "height" | "contentHeight" => &[Ax::Y],
            "leftMargin" | "topMargin" | "rightMargin" | "bottomMargin" => {
                let ax = match name {
                    "topMargin" | "bottomMargin" => Ax::Y,
                    _ => Ax::X,
                };
                // A margin written with the object is stored before its size (QmlIR
                // prepends constant bindings), so the size is set with the margin known.
                if self.creating == 0 || self.fnum(f, ax.size(), 0.0) == 0.0 {
                    return;
                }
                if ax == Ax::X { &[Ax::X] } else { &[Ax::Y] }
            }
            _ => &[Ax::X, Ax::Y],
        };
        let reading = self.reading.take();
        for &ax in axes {
            let content = if view && crate::runtime::view_vertical(self, f) == (ax == Ax::Y) {
                self.fnum(f, ax.content(), 0.0).max(0.0)
            } else {
                self.fprop(f, ax.content(), -1.0)
            };
            let v = if content < 0.0 {
                self.fnum(f, ax.size(), 0.0)
                    - self.fnum(f, ax.start_margin(), 0.0)
                    - self.fnum(f, ax.end_margin(), 0.0)
            } else {
                content
            };
            self.set_num(c, ax.size(), v);
            if !view && matches!(name, "width" | "height" | "contentWidth" | "contentHeight") {
                self.keep_in_view(f, ax, name.starts_with("content"));
            }
        }
        self.reading = reading;
    }

    /// "Make sure that we're entirely in view" of QQuickFlickable::geometryChange and
    /// setContentWidth / setContentHeight: a Flickable that is neither pressed nor moving is put
    /// back inside its extents at once (fixupMode Immediate), and one returning to bounds heads
    /// for the new extent of its content (ExtentChanged). Objects being created are left alone,
    /// and so is a view that gets its first size: the scene sizes it after its
    /// Component.onCompleted, Qt before, with nothing to bring back yet.
    fn keep_in_view(&mut self, f: usize, ax: Ax, content: bool) {
        let has = self.fnum(f, ax.size(), 0.0) > 0.0;
        let sized = std::mem::replace(&mut self.fst(f, ax).sized, has);
        if self.creating != 0 || !sized {
            return;
        }
        let any = |w: &Self, pick: fn(&Flick) -> bool| {
            [Ax::X, Ax::Y]
                .into_iter()
                .any(|a| w.fget(f, a).is_some_and(pick))
        };
        if any(self, |s| s.pressed) {
            return;
        }
        let mut immediate = !any(self, |s| s.moving);
        if !immediate && !(content && self.fget(f, ax).is_some_and(|s| s.fixing_up)) {
            return;
        }
        let now = self.now;
        self.flick_sync(f, ax);
        let (min, max) = self.extents(f, ax);
        let st = self.fst(f, ax);
        let (mv, smooth, was_active) = (st.mv, st.smooth, st.active);
        // QQuickFlickablePrivate::fixup, with adjustContentPos in the fixupMode of the caller.
        let to = if mv >= min || max > min {
            self.reset_timeline(f, ax);
            (mv != min).then_some(min)
        } else if mv <= max {
            self.reset_timeline(f, ax);
            Some(max)
        } else if -(-mv).round() != mv {
            self.reset_timeline(f, ax);
            immediate = true;
            Some(if (mv.round() - mv).abs() < 0.25 || smooth == 0.0 {
                mv.round()
            } else if smooth > 0.0 {
                mv.ceil()
            } else {
                mv.floor()
            })
        } else {
            None
        };
        match to {
            // QQuickTimeLine::set: the value is there as the timeline starts.
            Some(to) if immediate => self.set_move(f, ax, to, now),
            Some(to) => {
                let len = 3.0 * FIXUP_DURATION / 4.0;
                let ease = OUT_EXPO;
                self.push_op(f, ax, Op::Move { to, ease, len }, now);
                self.fst(f, ax).fixing_up = true;
            }
            None => {}
        }
        if was_active && !self.fst(f, ax).active {
            // The timeline that was reset still completes: the movement ends.
            let (to, ease) = (self.fst(f, ax).mv, LINEAR);
            self.push_op(f, ax, Op::Move { to, ease, len: 1.0 }, now);
        }
        let st = self.fst(f, ax);
        st.in_overshoot = false;
        st.v_time = now;
    }

    /// QQuickScrollBarPrivate::visualArea and resizeContent of ScrollBar `b`, once its
    /// `position`, `size` or `minimumSize` is stored; Basic `minimumSize` is the bar's thickness
    /// over its length.
    pub fn scrollbar_content(&mut self, b: usize) {
        let reading = self.reading.take();
        let (bw, bh) = (self.fnum(b, "width", 0.0), self.fnum(b, "height", 0.0));
        let (pos, size) = (self.fnum(b, "position", 0.0), self.fnum(b, "size", 0.0));
        let horizontal = self.fnum(b, "orientation", 2.0) == 1.0;
        let (thick, long) = if horizontal { (bh, bw) } else { (bw, bh) };
        let min = self.fprop(
            b,
            "minimumSize",
            if long > 0.0 { thick / long } else { 0.0 },
        );
        let mut vpos = pos;
        if min > size && size != 1.0 {
            vpos = pos / (1.0 - size) * (1.0 - min);
        }
        let max_size = (1.0 - vpos).max(0.0);
        let vsize = min.max((size.max(min) + vpos.min(0.0)).min(max_size));
        let vpos = vpos.min((1.0 - vsize).max(0.0)).max(0.0);
        self.set_num(b, "visualPosition", vpos);
        self.set_num(b, "visualSize", vsize);
        if let Some(Value::Item(ci)) = self.read(b, "contentItem")
            && ci != b
            && self.items[ci].parent == Some(b)
        {
            let [l, r, t, bt] = ["leftPadding", "rightPadding", "topPadding", "bottomPadding"]
                .map(|k| self.fnum(b, k, 0.0));
            let (aw, ah) = ((bw - l - r).max(0.0), (bh - t - bt).max(0.0));
            if horizontal {
                self.place(ci, "x", l + vpos * aw);
                self.place(ci, "y", t);
                self.set_num(ci, "width", vsize * aw);
                self.set_num(ci, "height", ah);
            } else {
                self.place(ci, "x", l);
                self.place(ci, "y", t + vpos * ah);
                self.set_num(ci, "width", aw);
                self.set_num(ci, "height", vsize * ah);
            }
        }
        self.reading = reading;
    }

    fn set_num(&mut self, item: usize, name: &str, v: f64) {
        let cur = self.items[item]
            .props
            .get(name)
            .map(|p| p.value.to_number());
        if cur != Some(v) {
            self.write(item, name, Value::Num(v));
        }
    }

    /// QQuickFlickablePrivate::updateBeginningEnd: atXBeginning / atXEnd / atYBeginning / atYEnd
    /// (fuzzy compare), then QQuickFlickableVisibleArea::updateVisible, which the attached
    /// ScrollBars follow with their `position` and `size`
    /// (QQuickScrollBarAttachedPrivate::scrollVertical / scrollHorizontal).
    pub fn flick_edges(&mut self, f: usize) {
        let reading = self.reading.take();
        let (cx, cy) = (self.fnum(f, "contentX", 0.0), self.fnum(f, "contentY", 0.0));
        // The edges come from the extents (margins, origin), not from contentWidth/Height alone.
        let (x0, x1) = self.flick_extents(f, false);
        let (y0, y1) = self.flick_extents(f, true);
        let flags = [
            ("atXBeginning", cx <= -x0 + 0.5),
            ("atXEnd", cx >= -x1 - 0.5),
            ("atYBeginning", cy <= -y0 + 0.5),
            ("atYEnd", cy >= -y1 - 0.5),
        ];
        for (k, v) in flags {
            if self.read(f, k).map(|x| x.truthy()) != Some(v) {
                self.write(f, k, Value::Bool(v));
            }
        }
        // The bars of a ScrollView are attached to the ScrollView.
        let owner = self.scroll_view_of(f).unwrap_or(f);
        for (key, vertical) in SCROLL_BARS {
            let Some(Value::Item(b)) = self.read(owner, key) else {
                continue;
            };
            if self.dead.contains(&b) || !self.items[b].types.iter().any(|t| t == "ScrollBar") {
                continue;
            }
            // QQuickFlickableVisibleArea::updateVisible: maxYBounds = max(contentHeight, height).
            let (at, origin, content, size) = if vertical {
                (cy, "originY", "contentHeight", "height")
            } else {
                (cx, "originX", "contentWidth", "width")
            };
            let origin = match self.items[f].prop_info(origin) {
                Some(_) => self.fnum(f, origin, 0.0),
                None => 0.0,
            };
            let view = self.fnum(f, size, 0.0);
            let bounds = self.fnum(f, content, 0.0).max(view);
            let (pos, size) = if bounds > 0.0 {
                ((at - origin) / bounds, view / bounds)
            } else {
                (0.0, 0.0)
            };
            // The bar follows the visible area of the Flickable when that moves; where its
            // handle was let go it stays until then.
            let was = self.items[b].props.get(BAR_AT).map(|p| p.value.to_number());
            if was != Some(pos) {
                let at = Prop {
                    value: Value::Num(pos),
                    ..Default::default()
                };
                self.items[b].props.insert(BAR_AT, at);
                self.set_num(b, "position", pos);
            }
            self.set_num(b, "size", size);
        }
        self.reading = reading;
    }

    /// The ScrollView whose Flickable `f` is: its `contentItem`.
    pub fn scroll_view_of(&self, f: usize) -> Option<usize> {
        let p = self.items.get(f)?.parent?;
        let it = &self.items[p];
        let is = it.types.iter().any(|t| t == "ScrollView")
            && matches!(it.props.get("contentItem").map(|p| &p.value), Some(&Value::Item(c)) if c == f);
        is.then_some(p)
    }

    /// QQuickFlickable::isMoving.
    pub fn flick_moving(&self, f: usize) -> bool {
        [Ax::X, Ax::Y]
            .into_iter()
            .any(|ax| self.fget(f, ax).is_some_and(|s| s.moving))
    }

    /// A flick or fixup is running on either axis.
    pub fn flicks_active(&self) -> bool {
        self.flicks_live
            .iter()
            .any(|&(f, ax)| self.fget(f, ax).is_some_and(|s| s.active))
    }
}

fn add_sample(st: &mut Flick, v: f64, max_v: f64) {
    st.samples.push_back(v.clamp(-max_v, max_v));
    if st.samples.len() > SAMPLE_BUFFER {
        st.samples.pop_front();
    }
}

/// AxisData::updateVelocity (QML_FLICK_DISCARDSAMPLES = 0).
fn update_velocity(st: &mut Flick) {
    st.velocity = if st.samples.is_empty() {
        0.0
    } else {
        st.samples.iter().sum::<f64>() / st.samples.len() as f64
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;

    /// A 200x200 Flickable over 1000 px of content: minYExtent 0, maxYExtent -800.
    fn view(extra: &str) -> (Runtime, usize) {
        let mut rt = Runtime::new(None);
        // A property is set once (Qt: "Property value set multiple times").
        let content = match extra.contains("contentHeight:") {
            true => "",
            false => "contentHeight: 1000;",
        };
        let root = rt
            .load_str(&format!(
                "import QtQuick\nFlickable {{ width: 200; height: 200; {content} {extra} }}\n"
            ))
            .unwrap();
        rt.settle();
        rt.tick(1000.0);
        rt.world.borrow_mut().flick_sync(root, Ax::Y);
        (rt, root)
    }

    fn cy(rt: &Runtime, f: usize) -> f64 {
        rt.get(f, "contentY").to_number()
    }

    fn st(rt: &Runtime, f: usize) -> Flick {
        rt.world.borrow().flicks[&f].clone()
    }

    /// Ticks 16 ms frames from `t` until the timelines finish; returns the end time and the
    /// largest contentY seen.
    fn run(rt: &mut Runtime, f: usize, mut t: f64) -> (f64, f64) {
        let mut peak = cy(rt, f);
        for _ in 0..1000 {
            t += 16.0;
            let busy = rt.tick(t);
            peak = peak.max(cy(rt, f));
            if !busy && !rt.world.borrow().flicks_active() {
                return (t, peak);
            }
        }
        panic!("flick never settled");
    }

    fn first_accel(rt: &Runtime, f: usize) -> (f64, f64, f64) {
        match st(rt, f).ops.front() {
            Some(Op::Accel { v, a, len }) => (*v, *a, *len),
            op => panic!("{op:?}"),
        }
    }

    fn log(rt: &Runtime, f: usize) -> String {
        match rt.get(f, "log") {
            Value::Str(s) => s.to_string(),
            v => panic!("{v:?}"),
        }
    }

    // QQuickTimeLinePrivate::value: Accel is base + v·t + a·t²/2 (t in s), Move eases from
    // the base to its target, MoveBy eases by a delta, Set jumps.
    #[test]
    fn timeline_op_values() {
        let accel = Op::Accel {
            v: 1000.0,
            a: -2000.0,
            len: 500.0,
        };
        assert_eq!(accel.value(10.0, 0.0), 10.0);
        assert_eq!(accel.value(10.0, 250.0), 10.0 + 250.0 - 62.5);
        // Decelerating to rest covers v²/2a.
        assert_eq!(accel.value(10.0, 500.0), 10.0 + 250.0);
        let linear = Op::Move {
            to: 100.0,
            ease: LINEAR,
            len: 200.0,
        };
        assert_eq!(linear.value(0.0, 50.0), 25.0);
        assert_eq!(linear.value(0.0, 200.0), 100.0);
        // easeOutExpo (qeasingcurve easing.cpp): 1.001 · (1 - 2^(-10t)) before t == 1.
        let expo = Op::Move {
            to: 0.0,
            ease: OUT_EXPO,
            len: 300.0,
        };
        let half = 30.0 - 30.0 * 1.001 * (1.0 - 2f64.powi(-5));
        assert!((expo.value(30.0, 150.0) - half).abs() < 1e-9);
        assert_eq!(expo.value(30.0, 300.0), 0.0);
        let by = Op::MoveBy {
            by: 72.0,
            ease: OUT_EXPO,
            len: 300.0,
        };
        assert_eq!(by.value(10.0, 0.0), 10.0);
        assert_eq!(by.value(10.0, 300.0), 82.0);
        assert_eq!(Op::Set(5.0).value(10.0, 0.0), 5.0);
        assert_eq!(Op::Set(5.0).len(), 0.0);
    }

    // QQuickTimeLine::accel: the acceleration opposes the velocity, the length is
    // int(-1000·v/a) ms and no op is added when that is 0; with maxDistance the acceleration
    // rises to v²/2d and a zero distance adds nothing.
    #[test]
    fn accel_op_length_and_max_distance() {
        let (rt, root) = view("");
        let ops = |f: &dyn Fn(&mut World)| {
            let mut w = rt.world.borrow_mut();
            w.reset_timeline(root, Ax::Y);
            f(&mut w);
            w.flicks[&root].ops.iter().cloned().collect::<Vec<_>>()
        };
        let o = ops(&|w| w.push_accel(root, Ax::Y, 1000.0, 3000.0, None, 0.0));
        assert!(
            matches!(
                o[..],
                [Op::Accel {
                    v: 1000.0,
                    a: -3000.0,
                    len: 333.0
                }]
            ),
            "{o:?}"
        );
        let o = ops(&|w| w.push_accel(root, Ax::Y, -1500.0, 1500.0, None, 0.0));
        assert!(
            matches!(
                o[..],
                [Op::Accel {
                    v: -1500.0,
                    a: 1500.0,
                    len: 1000.0
                }]
            ),
            "{o:?}"
        );
        assert!(ops(&|w| w.push_accel(root, Ax::Y, 0.1, 1500.0, None, 0.0)).is_empty());
        let o = ops(&|w| w.push_accel(root, Ax::Y, 2500.0, 1500.0, Some(800.0), 0.0));
        assert!(
            matches!(
                o[..],
                [Op::Accel {
                    v: 2500.0,
                    a: -3906.25,
                    len: 640.0
                }]
            ),
            "{o:?}"
        );
        // A smaller v²/2d keeps the given deceleration.
        let o = ops(&|w| w.push_accel(root, Ax::Y, 1000.0, 1500.0, Some(800.0), 0.0));
        assert!(
            matches!(
                o[..],
                [Op::Accel {
                    v: 1000.0,
                    a: -1500.0,
                    len: 666.0
                }]
            ),
            "{o:?}"
        );
        assert!(ops(&|w| w.push_accel(root, Ax::Y, 2500.0, 1500.0, Some(0.0), 0.0)).is_empty());
    }

    // QQuickFlickablePrivate::flick: distance v²/2·deceleration, adjusted so the flick ends on
    // a whole pixel, followed by the fixup callback.
    #[test]
    fn flick_travels_v2_over_2a_to_a_whole_pixel() {
        let (mut rt, root) = view("");
        assert!(
            rt.world
                .borrow_mut()
                .flick_start(root, Ax::Y, -1500.0, false, 1000.0)
        );
        assert_eq!(first_accel(&rt, root), (-1500.0, 1500.0, 1000.0));
        assert!(matches!(st(&rt, root).ops.back(), Some(Op::Fixup)));
        run(&mut rt, root, 1000.0);
        assert_eq!(cy(&rt, root), 750.0);
        // 1000²/3000 = 333.3 px rounds to 333, with the deceleration adjusted to match.
        let (mut rt, root) = view("");
        rt.world
            .borrow_mut()
            .flick_start(root, Ax::Y, -1000.0, false, 1000.0);
        let (_, a, _) = first_accel(&rt, root);
        assert!((a - 1e6 / 666.0).abs() < 1e-9, "{a}");
        run(&mut rt, root, 1000.0);
        assert_eq!(cy(&rt, root), 333.0);
        // flickDeceleration from QML.
        let (mut rt, root) = view("flickDeceleration: 3000");
        rt.world
            .borrow_mut()
            .flick_start(root, Ax::Y, -1500.0, false, 1000.0);
        run(&mut rt, root, 1000.0);
        assert_eq!(cy(&rt, root), 375.0);
        // Positive velocity moves towards the beginning.
        let (mut rt, root) = view("contentY: 500");
        rt.world
            .borrow_mut()
            .flick_start(root, Ax::Y, 1000.0, false, 1000.0);
        run(&mut rt, root, 1000.0);
        assert_eq!(cy(&rt, root), 500.0 - 333.0);
    }

    // QQuickFlickablePrivate::flick clamps |v| to maximumFlickVelocity unless it is -1.
    #[test]
    fn flick_velocity_clamps_to_maximum_flick_velocity() {
        let (rt, root) = view("");
        rt.world
            .borrow_mut()
            .flick_start(root, Ax::Y, -10000.0, false, 1000.0);
        assert_eq!(first_accel(&rt, root).0, -2500.0);
        let (rt, root) = view("maximumFlickVelocity: 1000");
        rt.world
            .borrow_mut()
            .flick_start(root, Ax::Y, 10000.0, false, 1000.0);
        assert_eq!(first_accel(&rt, root).0, 1000.0);
        let (rt, root) = view("maximumFlickVelocity: -1");
        rt.world
            .borrow_mut()
            .flick_start(root, Ax::Y, -10000.0, false, 1000.0);
        assert_eq!(first_accel(&rt, root).0, -10000.0);
    }

    // Without OvershootBounds QQuickFlickablePrivate::flick uses accel(…, maxDistance), so the
    // flick stops exactly at the bound; at the bound it only fixes up.
    #[test]
    fn stop_at_bounds_flick_stops_at_the_bound() {
        let (mut rt, root) = view("boundsBehavior: Flickable.StopAtBounds");
        rt.world
            .borrow_mut()
            .flick_start(root, Ax::Y, -2500.0, false, 1000.0);
        assert_eq!(first_accel(&rt, root), (-2500.0, 3906.25, 640.0));
        let (_, peak) = run(&mut rt, root, 1000.0);
        assert_eq!(cy(&rt, root), 800.0);
        assert!(peak <= 800.0 + 1e-9, "{peak}");
        let (mut rt, root) = view("boundsBehavior: Flickable.StopAtBounds; contentY: 800");
        assert!(
            !rt.world
                .borrow_mut()
                .flick_start(root, Ax::Y, -1000.0, false, 1000.0)
        );
        assert!(
            !st(&rt, root)
                .ops
                .iter()
                .any(|o| matches!(o, Op::Accel { .. }))
        );
        let (_, peak) = run(&mut rt, root, 1000.0);
        assert_eq!((cy(&rt, root), peak), (800.0, 800.0));
    }

    // QQuickFlickablePrivate::viewportAxisMoved: a flick passing a bound decelerates with
    // deceleration·QML_FLICK_OVERSHOOTFRICTION for at most overShootDistance
    // (min(QML_FLICK_OVERSHOOT = 150, |v|/3)) past it, then fixes up back to the bound.
    #[test]
    fn overshoot_is_capped_then_fixes_up() {
        let (mut rt, root) = view("contentY: 700");
        {
            let mut w = rt.world.borrow_mut();
            assert!(w.flick_start(root, Ax::Y, -2500.0, false, 1000.0));
            w.flicking_started(root, Ax::Y, true);
        }
        let (_, peak) = run(&mut rt, root, 1000.0);
        assert!(peak > 800.0 && peak <= 950.0, "{peak}");
        assert_eq!(cy(&rt, root), 800.0);
        let s = st(&rt, root);
        assert!(!s.flicking && !s.in_overshoot && !s.active);
        assert!(matches!(rt.get(root, "flicking"), Value::Bool(false)));
    }

    // QQuickFlickablePrivate::fixup + adjustContentPos: back to the bound in fixupDuration
    // (400 ms): an InQuad move halfway in 100 ms, then OutExpo the rest in 300 ms.
    #[test]
    fn fixup_returns_to_bounds_in_fixup_duration() {
        let (mut rt, root) = view("contentY: -60");
        {
            let mut w = rt.world.borrow_mut();
            w.flick_sync(root, Ax::Y);
            w.flick_fixup(root, Ax::Y, 1000.0);
        }
        let s = st(&rt, root);
        assert!(s.fixing_up);
        assert!(
            matches!(
                s.ops.iter().collect::<Vec<_>>()[..],
                [
                    Op::Move {
                        to: 30.0,
                        ease: IN_QUAD,
                        len: 100.0
                    },
                    Op::Move {
                        to: 0.0,
                        ease: OUT_EXPO,
                        len: 300.0
                    }
                ]
            ),
            "{:?}",
            s.ops
        );
        rt.tick(1050.0);
        assert_eq!(cy(&rt, root), -(60.0 - 30.0 * 0.25));
        rt.tick(1100.0);
        assert_eq!(cy(&rt, root), -30.0);
        rt.tick(1250.0);
        let expo = 30.0 - 30.0 * 1.001 * (1.0 - 2f64.powi(-5));
        assert!((cy(&rt, root) + expo).abs() < 1e-9, "{}", cy(&rt, root));
        rt.tick(1399.0);
        assert!(cy(&rt, root) < 0.0);
        rt.tick(1400.0);
        assert_eq!(cy(&rt, root), 0.0);
        assert!(!st(&rt, root).active);
        // Past the end it returns to maxYExtent.
        let (mut rt, root) = view("contentY: 900");
        rt.world.borrow_mut().flick_fixup(root, Ax::Y, 1000.0);
        rt.tick(1100.0);
        assert_eq!(cy(&rt, root), 850.0);
        rt.tick(1400.0);
        assert_eq!(cy(&rt, root), 800.0);
        // Content smaller than the view: maxYExtent is capped at minYExtent, so it goes to top.
        let (mut rt, root) = view("contentHeight: 100; contentY: 30");
        rt.world.borrow_mut().flick_fixup(root, Ax::Y, 1000.0);
        rt.tick(1400.0);
        assert_eq!(cy(&rt, root), 0.0);
    }

    // QQuickFlickable::setContentHeight and geometryChange, "Make sure that we're entirely in
    // view": content that gets shorter, or a view that gets taller, leaves a resting Flickable
    // inside its extents at once, a Behavior on contentY not in the way.
    #[test]
    fn a_shorter_content_keeps_the_view_inside_its_extents() {
        let (mut rt, root) =
            view("contentY: 800; Behavior on contentY { NumberAnimation { duration: 200 } }");
        rt.set(root, "contentHeight", Value::Num(700.0));
        assert_eq!(cy(&rt, root), 500.0);
        assert!(!st(&rt, root).active);
        // Shorter than the view: maxYExtent is capped at minYExtent.
        rt.set(root, "contentHeight", Value::Num(150.0));
        assert_eq!(cy(&rt, root), 0.0);
        let (mut rt, root) = view("contentY: 800");
        rt.set(root, "height", Value::Num(300.0));
        assert_eq!(cy(&rt, root), 700.0);
        // Longer content moves nothing.
        rt.set(root, "contentHeight", Value::Num(2000.0));
        assert_eq!(cy(&rt, root), 700.0);
        let (mut rt, root) = view("contentWidth: 600; contentX: 400");
        rt.set(root, "contentWidth", Value::Num(300.0));
        assert_eq!(rt.get(root, "contentX").to_number(), 100.0);
        // A drag holds the view where it is.
        let (mut rt, root) = view("contentY: 800");
        rt.world
            .borrow_mut()
            .flick_press(root, (100.0, 100.0), 1000.0);
        rt.set(root, "contentHeight", Value::Num(700.0));
        assert_eq!(cy(&rt, root), 800.0);
        // The first size of a view moves nothing.
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str("import QtQuick\nFlickable { width: 200 }\n")
            .unwrap();
        rt.settle();
        rt.set(root, "contentY", Value::Num(60.0));
        rt.set(root, "height", Value::Num(120.0));
        assert_eq!(cy(&rt, root), 60.0);
        rt.set(root, "height", Value::Num(100.0));
        assert_eq!(cy(&rt, root), 0.0);
    }

    // QQuickFlickablePrivate::fixup inside the bounds: a fractional position is set to a whole
    // pixel, rounding under 0.25 px and otherwise continuing the direction of smoothVelocity.
    #[test]
    fn fixup_snaps_subpixel_positions() {
        for (y, smooth, want) in [
            (100.3, 0.0, 100.0),
            (100.3, 50.0, 100.0),
            (100.3, -50.0, 101.0),
            (100.7, 50.0, 100.0),
            (100.2, -50.0, 100.0),
            (100.8, 50.0, 101.0),
        ] {
            let (mut rt, root) = view(&format!("contentY: {y}"));
            {
                let mut w = rt.world.borrow_mut();
                w.flick_sync(root, Ax::Y);
                w.fst(root, Ax::Y).smooth = smooth;
                w.flick_fixup(root, Ax::Y, 1000.0);
            }
            assert!(matches!(st(&rt, root).ops.front(), Some(Op::Set(_))));
            rt.tick(1001.0);
            assert_eq!(cy(&rt, root), want, "{y} {smooth}");
        }
        // Already whole: nothing to do.
        let (rt, root) = view("contentY: 100");
        rt.world.borrow_mut().flick_fixup(root, Ax::Y, 1000.0);
        assert!(!st(&rt, root).active);
    }

    fn wheel_view(extra: &str, decel: f64) -> (Runtime, usize) {
        let (rt, root) = view(extra);
        rt.world.borrow_mut().fst(root, Ax::Y).wheel_decel = decel;
        (rt, root)
    }

    // QQuickFlickable::wheelEvent with wheelDeceleration > _q_MaximumWheelDeceleration: an OutExpo
    // moveBy of wheelScrollLines·24 px per 120 angle units over 3·fixupDuration/4; StopAtBounds
    // clamps it to the bound and lets the event propagate.
    #[test]
    fn wheel_line_scroll_clamps_at_bounds() {
        let (mut rt, root) = wheel_view("contentY: 780", 15000.0);
        assert!(
            rt.world
                .borrow_mut()
                .flick_wheel(root, (0.0, -120.0), 5000.0)
        );
        assert!(matches!(
            st(&rt, root).ops.front(),
            Some(Op::MoveBy {
                by: -72.0,
                ease: OUT_EXPO,
                len: 300.0
            })
        ));
        rt.tick(1300.0);
        assert_eq!(cy(&rt, root), 852.0);
        run(&mut rt, root, 1300.0);
        assert_eq!(cy(&rt, root), 800.0);
        let (mut rt, root) = wheel_view(
            "contentY: 780; boundsBehavior: Flickable.StopAtBounds",
            15000.0,
        );
        assert!(
            !rt.world
                .borrow_mut()
                .flick_wheel(root, (0.0, -120.0), 5000.0)
        );
        let (_, peak) = run(&mut rt, root, 1000.0);
        assert_eq!((peak, cy(&rt, root)), (800.0, 800.0));
        // Two notches scroll twice as far.
        let (mut rt, root) = wheel_view("contentY: 400", 15000.0);
        assert!(
            rt.world
                .borrow_mut()
                .flick_wheel(root, (0.0, -240.0), 5000.0)
        );
        run(&mut rt, root, 1000.0);
        assert_eq!(cy(&rt, root), 544.0);
        // At the bound the event is not consumed and nothing moves.
        let (rt, root) = wheel_view("contentY: 800", 15000.0);
        assert!(
            !rt.world
                .borrow_mut()
                .flick_wheel(root, (0.0, -120.0), 5000.0)
        );
        let s = st(&rt, root);
        assert!(!s.active && !s.moving);
    }

    // QQuickFlickable::wheelEvent with a finite wheelDeceleration: from rest elapsed is
    // 120/sqrt(2·decel·initialWheelFlickDistance), so one notch flicks exactly
    // wheelScrollLines·24 = 72 px whatever the deceleration.
    #[test]
    fn wheel_notch_from_rest_flicks_72px() {
        for decel in [1000.0, 5000.0, 10000.0, 14999.0] {
            let (mut rt, root) = wheel_view("", decel);
            assert!(
                rt.world
                    .borrow_mut()
                    .flick_wheel(root, (0.0, -120.0), 5000.0)
            );
            let s = st(&rt, root);
            assert!(s.flicking && s.moving, "{decel}");
            assert!(
                (s.velocity + (144.0 * decel).sqrt()).abs() < 1e-6,
                "{decel}"
            );
            run(&mut rt, root, 1000.0);
            assert_eq!(cy(&rt, root), 72.0, "{decel}");
            assert!(!st(&rt, root).flicking);
        }
    }

    // QQuickFlickable::wheelEvent: a reversed wheel direction clears the velocity buffer;
    // samples are clamped to maximumFlickVelocity (AxisData::addVelocitySample); a zero elapsed
    // time is not consumed.
    #[test]
    fn wheel_velocity_buffer() {
        let (mut rt, root) = wheel_view("", 5000.0);
        assert!(
            rt.world
                .borrow_mut()
                .flick_wheel(root, (0.0, -120.0), 5000.0)
        );
        assert!(
            !rt.world
                .borrow_mut()
                .flick_wheel(root, (0.0, -120.0), 5000.0)
        );
        rt.tick(1016.0);
        rt.tick(1032.0);
        assert!(
            rt.world
                .borrow_mut()
                .flick_wheel(root, (0.0, -120.0), 5010.0)
        );
        // (-sqrt(144·5000) + -2500) / 2: 120 / 0.010 s is clamped to 2500.
        let v = st(&rt, root).velocity;
        assert!(
            (v - (-(720000f64).sqrt() - 2500.0) / 2.0).abs() < 1e-9,
            "{v}"
        );
        rt.tick(1048.0);
        assert!(
            rt.world
                .borrow_mut()
                .flick_wheel(root, (0.0, 120.0), 5020.0)
        );
        assert_eq!(st(&rt, root).velocity, 2500.0);
        assert!(first_accel(&rt, root).0 > 0.0);
    }

    /// Presses at y 100 on a Flickable at `extra`, moves to `y` at 1020 ms with point velocity
    /// `vel` and releases there at `release`; returns the runtime after the release.
    fn drag_release(
        extra: &str,
        y: f64,
        vel: f64,
        release: f64,
        grabbed: bool,
    ) -> (Runtime, usize) {
        let (mut rt, root) = view(extra);
        rt.world
            .borrow_mut()
            .flick_press(root, (0.0, 100.0), 1000.0);
        rt.tick(1020.0);
        rt.world
            .borrow_mut()
            .flick_move(root, (0.0, y), 1020.0, (0.0, vel));
        rt.tick(release);
        rt.world
            .borrow_mut()
            .flick_release(root, (0.0, y), release, grabbed);
        (rt, root)
    }

    fn flicked(r: (Runtime, usize)) -> bool {
        st(&r.0, r.1).flicking
    }

    // QQuickFlickablePrivate::handleReleaseEvent: a flick needs the grab, |v| >
    // _q_MinimumFlickVelocity (75), a move within the last 100 ms and more than
    // FlickStartDistance (15 px) from the press.
    #[test]
    fn release_flick_thresholds() {
        let mid = "contentY: 400";
        assert!(flicked(drag_release(mid, 130.0, 80.0, 1040.0, true)));
        assert!(!flicked(drag_release(mid, 130.0, 70.0, 1040.0, true)));
        assert!(flicked(drag_release(mid, 130.0, 1000.0, 1119.0, true)));
        assert!(!flicked(drag_release(mid, 130.0, 1000.0, 1120.0, true)));
        assert!(flicked(drag_release(mid, 116.0, 1000.0, 1040.0, true)));
        assert!(!flicked(drag_release(mid, 112.0, 1000.0, 1040.0, true)));
        assert!(!flicked(drag_release(mid, 130.0, 1000.0, 1040.0, false)));
        // The slow flick: 80²/3000 px rounds to 2.
        let (mut rt, root) = drag_release(mid, 130.0, 80.0, 1040.0, true);
        assert_eq!(cy(&rt, root), 400.0);
        run(&mut rt, root, 1040.0);
        assert_eq!(cy(&rt, root), 398.0);
        // Too slow: no flick and nothing moved.
        let (rt, root) = drag_release(mid, 130.0, 70.0, 1040.0, true);
        let s = st(&rt, root);
        assert!(!s.active && !s.moving && cy(&rt, root) == 400.0);
    }

    // QQuickFlickablePrivate::handleReleaseEvent halves the velocity of a flick away from
    // the content at the beginning or end.
    #[test]
    fn release_at_bound_halves_velocity() {
        let (rt, root) = drag_release("", 130.0, 1000.0, 1040.0, true);
        assert_eq!(first_accel(&rt, root).0, 500.0);
        let (rt, root) = drag_release("contentY: 800", 70.0, -1000.0, 1040.0, true);
        assert_eq!(first_accel(&rt, root).0, -500.0);
        let (rt, root) = drag_release("contentY: 400", 130.0, 1000.0, 1040.0, true);
        assert_eq!(first_accel(&rt, root).0, 1000.0);
    }

    fn drag(extra: &str, moves: &[f64]) -> (Runtime, usize, f64) {
        let (mut rt, root) = view(extra);
        rt.world.borrow_mut().flick_press(root, (0.0, 0.0), 1000.0);
        let mut t = 1000.0;
        for y in moves {
            t += 20.0;
            rt.tick(t);
            rt.world
                .borrow_mut()
                .flick_move(root, (0.0, *y), t, (0.0, 0.0));
        }
        rt.settle();
        (rt, root, t)
    }

    // QQuickFlickablePrivate::drag: dragStartOffset is retaken until the content has moved
    // (the threshold crossing only takes the grab), then the content follows the point 1:1;
    // DragOverBounds pulls past a bound at half the distance, StopAtBounds
    // clamps and rejects the drag.
    #[test]
    fn drag_follows_the_point_and_honours_bounds_behavior() {
        let (rt, root, _) = drag("contentY: 400", &[-20.0, -30.0, -90.0]);
        assert_eq!(cy(&rt, root), 460.0);
        assert!(st(&rt, root).dragging && rt.world.borrow().flick_moving(root));
        let (rt, root, _) = drag(
            "boundsBehavior: Flickable.DragOverBounds",
            &[20.0, 30.0, 130.0],
        );
        assert_eq!(cy(&rt, root), -50.0);
        let (rt, root, _) = drag("contentY: 800", &[-20.0, -30.0, -130.0]);
        assert_eq!(cy(&rt, root), 850.0);
        let (rt, root, _) = drag(
            "boundsBehavior: Flickable.StopAtBounds",
            &[20.0, 30.0, 130.0],
        );
        assert_eq!(cy(&rt, root), 0.0);
        let s = st(&rt, root);
        assert!(!s.moving && !s.dragging && s.samples.is_empty());
        // StopAtBounds still drags inside the bounds, clamping at the bound.
        let (rt, root, _) = drag(
            "boundsBehavior: Flickable.StopAtBounds; contentY: 50",
            &[20.0, 30.0, 130.0],
        );
        assert_eq!(cy(&rt, root), 0.0);
        assert!(st(&rt, root).moving);
    }

    // handleReleaseEvent without a flick calls fixupY: the overshoot drag returns to the bound
    // in fixupDuration and the movement ends after the timeline completes.
    #[test]
    fn release_after_overdrag_fixes_up() {
        let (mut rt, root, t) = drag(
            "property string log: ''\n onMovingChanged: log += 'moving ' + moving + ';'\n onDraggingChanged: log += 'dragging ' + dragging + ';'\n onFlickStarted: log += 'flickStarted;'",
            &[20.0, 30.0, 130.0],
        );
        // The timeline starts at the last animation tick (`t`), not at the event time.
        rt.world
            .borrow_mut()
            .flick_release(root, (0.0, 130.0), t + 10.0, true);
        rt.tick(t + 16.0);
        assert!(st(&rt, root).fixing_up && rt.world.borrow().flick_moving(root));
        rt.tick(t + 399.0);
        assert!(cy(&rt, root) < 0.0);
        rt.tick(t + 400.0);
        assert_eq!(cy(&rt, root), 0.0);
        rt.tick(t + 416.0);
        assert!(!rt.world.borrow().flick_moving(root));
        assert_eq!(
            log(&rt, root),
            "dragging true;moving true;dragging false;moving false;"
        );
    }

    // handlePressEvent keeps the grab while a flick runs faster than RetainGrabVelocity (100)
    // and is neither fixing up nor overshooting; release emits flickingStarted before
    // movementStarting; maybeBeginDrag clears flicking at the press and
    // movementEnding still emits flickEnded through flickingWhenDragBegan.
    #[test]
    fn press_during_flick_steals_and_ends_the_flick() {
        let qml = "property string log: ''\n onMovementStarted: log += 'movementStarted;'\n onMovementEnded: log += 'movementEnded;'\n onFlickStarted: log += 'flickStarted;'\n onFlickEnded: log += 'flickEnded;'\n onFlickingChanged: log += 'flicking ' + flicking + ';'";
        let (mut rt, root) = drag_release(qml, 70.0, -2000.0, 1040.0, true);
        rt.settle();
        assert!(st(&rt, root).flicking);
        for t in [1056.0, 1072.0, 1088.0] {
            rt.tick(t);
        }
        assert!(st(&rt, root).smooth.abs() > RETAIN_GRAB_VELOCITY);
        rt.world.borrow_mut().flick_press(root, (0.0, 70.0), 1090.0);
        rt.settle();
        assert!(rt.world.borrow().flick_steal(root));
        assert!(!st(&rt, root).flicking);
        assert!(matches!(rt.get(root, "flicking"), Value::Bool(false)));
        rt.world
            .borrow_mut()
            .flick_release(root, (0.0, 70.0), 1095.0, true);
        run(&mut rt, root, 1095.0);
        assert!(!rt.world.borrow().flick_moving(root));
        assert_eq!(
            log(&rt, root),
            "flicking true;flickStarted;movementStarted;flicking false;flickEnded;movementEnded;"
        );
        // A wheel line scroll is a fixup: pressing during it does not steal.
        let (mut rt, root) = wheel_view("contentY: 400", 15000.0);
        rt.world
            .borrow_mut()
            .flick_wheel(root, (0.0, -120.0), 5000.0);
        rt.tick(1050.0);
        rt.world.borrow_mut().flick_press(root, (0.0, 0.0), 1050.0);
        assert!(!rt.world.borrow().flick_steal(root));
    }
}
