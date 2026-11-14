//! Desktop container geometry: widgets sit anywhere on the monitor (free placement) or on a
//! grid, never overlap each other and never leave the monitor. A drop or a resize asks for a
//! rectangle; these functions answer with the nearest one that fits.

use super::{ContainerKind, Edit, Layout, Place, Widget};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn right(&self) -> i64 {
        self.x as i64 + self.w as i64
    }

    pub fn bottom(&self) -> i64 {
        self.y as i64 + self.h as i64
    }

    /// Whether the two share any area; touching edges do not count.
    pub fn overlaps(&self, o: &Rect) -> bool {
        (self.x as i64) < o.right()
            && (o.x as i64) < self.right()
            && (self.y as i64) < o.bottom()
            && (o.y as i64) < self.bottom()
    }

    /// Where desktop widget `wd` shows in its layout; a widget without a size in the layout
    /// has its natural one.
    pub fn of(wd: &Widget, natural: Natural) -> Option<Rect> {
        match wd.place {
            Place::Desktop { x, y, size } => {
                let (w, h) = size.unwrap_or_else(|| natural(wd));
                Some(Rect { x, y, w, h })
            }
            _ => None,
        }
    }

    /// Where desktop widget `wd` shows: where its view reported it when it places itself (by
    /// the wallpaper), nowhere when it is hidden, otherwise where the layout puts it
    /// ([`Rect::of`]).
    pub fn shown(wd: &Widget, natural: Natural, placed: Placed) -> Option<Rect> {
        let laid = Rect::of(wd, natural)?;
        match placed(wd) {
            Shows::Laid => Some(laid),
            Shows::At(r) => Some(r),
            Shows::Nowhere => None,
        }
    }

    fn place(&self) -> Place {
        Place::Desktop {
            x: self.x,
            y: self.y,
            size: Some((self.w, self.h)),
        }
    }
}

/// The size a desktop widget without one in the layout has: its natural size (QML's
/// `implicitWidth` / `implicitHeight`), as its view measured it.
pub type Natural<'a> = &'a dyn Fn(&Widget) -> (u32, u32);

/// Where a desktop widget shows, for an edit that must keep off it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shows {
    /// Where the layout puts it.
    Laid,
    /// It places itself (the original's clock or weather by the wallpaper's least or most busy
    /// region) and its view reported it there.
    At(Rect),
    /// It is hidden (the original's settings turned it off) and takes no place.
    Nowhere,
}

/// Where each desktop widget shows ([`Shows`]), as its view reported.
pub type Placed<'a> = &'a dyn Fn(&Widget) -> Shows;

/// How positions and sizes are quantized: every pixel (free placement) or a grid of square
/// cells of `cell` logical pixels, anchored at the monitor's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Snap {
    Free,
    Grid { cell: u32 },
}

impl Snap {
    fn pos(self, v: i64) -> i64 {
        match self {
            Snap::Free => v,
            Snap::Grid { cell } => {
                let c = cell.max(1) as i64;
                (v as f64 / c as f64).round() as i64 * c
            }
        }
    }

    /// A size rounded to whole cells, at least one cell.
    fn size(self, v: u32) -> u32 {
        match self {
            Snap::Free => v,
            Snap::Grid { cell } => {
                let c = cell.max(1);
                ((v as f64 / c as f64).round() as u32).max(1) * c
            }
        }
    }

    fn step(self) -> i64 {
        match self {
            Snap::Free => 1,
            Snap::Grid { cell } => cell.max(1) as i64,
        }
    }
}

/// The monitor area widgets live in, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub w: u32,
    pub h: u32,
}

fn clamp_into(r: Rect, b: Bounds) -> Rect {
    let w = r.w.min(b.w).max(1);
    let h = r.h.min(b.h).max(1);
    Rect {
        x: r.x.clamp(0, (b.w - w) as i32),
        y: r.y.clamp(0, (b.h - h) as i32),
        w,
        h,
    }
}

fn free(r: &Rect, others: &[Rect]) -> bool {
    !others.iter().any(|o| r.overlaps(o))
}

/// Where a widget that wants `want` shows on `bounds`: there when that is on the monitor and
/// off `others`, otherwise the nearest place that is ([`nearest_free`]), or just inside the
/// monitor when there is none.
pub fn fit(want: Rect, others: &[Rect], bounds: Bounds) -> Rect {
    if bounds.w == 0 || bounds.h == 0 {
        return want;
    }
    let r = clamp_into(want, bounds);
    if free(&r, others) {
        return r;
    }
    nearest_free(r, others, bounds, Snap::Free).unwrap_or(r)
}

/// When a desktop widget without a size in the layout last changed its natural size, as a
/// sequence number (later changes are higher; 0 for never).
pub type Changed<'a> = &'a dyn Fn(&Widget) -> u64;

/// A desktop's widgets as they show on `bounds` (another monitor size or scale than the one
/// they were placed on, or a natural size that grew): each [`fit`] off the ones placed before
/// it. Widgets with a size in the layout go first, in layout order; widgets at their natural
/// size follow, the least recently `changed` first, so the one whose size just changed is the
/// one that moves aside. A layout that fits shows unchanged and nothing is written back, so
/// the monitor the layout was made on shows it as it was again. `None` for an entry that is
/// not a desktop widget.
pub fn fit_all(
    widgets: &[Widget],
    bounds: Bounds,
    natural: Natural,
    changed: Changed,
) -> Vec<Option<Rect>> {
    let mut order: Vec<usize> = (0..widgets.len()).collect();
    order.sort_by_key(|&i| match widgets[i].place {
        Place::Desktop { size: None, .. } => (true, changed(&widgets[i])),
        _ => (false, 0),
    });
    let mut out = vec![None; widgets.len()];
    let mut shown: Vec<Rect> = vec![];
    for i in order {
        if let Some(want) = Rect::of(&widgets[i], natural) {
            let r = fit(want, &shown, bounds);
            shown.push(r);
            out[i] = Some(r);
        }
    }
    out
}

/// The free position for a `w`×`h` widget nearest to (`x`, `y`) (Euclidean distance of the
/// top-left corners), inside `bounds` and overlapping none of `others`; `None` when the
/// widget fits nowhere. Ties go to the upper, then the left position.
///
/// The best free position always lies on a candidate line: the wanted coordinate, a monitor
/// edge, or an edge of another widget (a widget pushed aside comes to rest against
/// something). With a grid, candidates are rounded to cells and the cells next to them are
/// tried as well.
pub fn nearest_free(want: Rect, others: &[Rect], bounds: Bounds, snap: Snap) -> Option<Rect> {
    if want.w > bounds.w || want.h > bounds.h || want.w == 0 || want.h == 0 {
        return None;
    }
    let (w, h) = (want.w as i64, want.h as i64);
    let max_x = bounds.w as i64 - w;
    let max_y = bounds.h as i64 - h;
    let axis =
        |wanted: i64, max: i64, lo: &dyn Fn(&Rect) -> i64, hi: &dyn Fn(&Rect) -> i64, size: i64| {
            let mut v = vec![wanted, 0, max];
            for o in others {
                v.push(hi(o));
                v.push(lo(o) - size);
            }
            let step = snap.step();
            let mut out = Vec::with_capacity(v.len() * 3);
            for c in v {
                let s = snap.pos(c);
                for d in [-step, 0, step] {
                    let c = s + d;
                    if (0..=max).contains(&c) && snap.pos(c) == c {
                        out.push(c);
                    }
                }
                if snap == Snap::Free {
                    out.push(c.clamp(0, max));
                }
            }
            // The last grid line that fits, when the monitor is not a whole number of cells.
            let last = match snap {
                Snap::Free => max,
                Snap::Grid { cell } => max / cell.max(1) as i64 * cell.max(1) as i64,
            };
            if last >= 0 {
                out.push(last);
            }
            out.sort_unstable();
            out.dedup();
            out
        };
    let xs = axis(
        want.x as i64,
        max_x,
        &|o: &Rect| o.x as i64,
        &|o: &Rect| o.right(),
        w,
    );
    let ys = axis(
        want.y as i64,
        max_y,
        &|o: &Rect| o.y as i64,
        &|o: &Rect| o.bottom(),
        h,
    );
    let (tx, ty) = (snap.pos(want.x as i64), snap.pos(want.y as i64));
    let mut candidates: Vec<(i64, i64, i64)> = ys
        .iter()
        .flat_map(|&y| {
            xs.iter()
                .map(move |&x| ((x - tx).pow(2) + (y - ty).pow(2), y, x))
        })
        .collect();
    candidates.sort_unstable();
    candidates.into_iter().find_map(|(_, y, x)| {
        let r = Rect {
            x: x as i32,
            y: y as i32,
            w: want.w,
            h: want.h,
        };
        free(&r, others).then_some(r)
    })
}

/// Size limits of the widget's type, in logical pixels (the registry's `SizeLimits`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub min: (u32, u32),
    pub max: (u32, u32),
}

impl Limits {
    pub const ANY: Limits = Limits {
        min: (1, 1),
        max: (u32::MAX, u32::MAX),
    };

    fn clamp(&self, w: u32, h: u32) -> (u32, u32) {
        (
            w.clamp(self.min.0, self.max.0.max(self.min.0)),
            h.clamp(self.min.1, self.max.1.max(self.min.1)),
        )
    }
}

/// A resize that keeps the top-left corner where it is: the wanted size is snapped and
/// clamped to the type's limits, then shrunk until it stays on the monitor and off the other
/// widgets (a resize handle stops at a neighbour instead of pushing it). `None` when not even
/// the minimum size fits there.
pub fn resize(
    current: Rect,
    want_w: u32,
    want_h: u32,
    limits: Limits,
    others: &[Rect],
    bounds: Bounds,
    snap: Snap,
) -> Option<Rect> {
    let (w, h) = limits.clamp(snap.size(want_w), snap.size(want_h));
    let max_w = (bounds.w as i64 - current.x as i64).max(0) as u32;
    let max_h = (bounds.h as i64 - current.y as i64).max(0) as u32;
    let mut w = w.min(max_w);
    let mut h = h.min(max_h);
    // The widgets that would be in the way stop the right edge or the bottom edge, whichever
    // gives up less area.
    for o in others {
        let r = Rect {
            x: current.x,
            y: current.y,
            w,
            h,
        };
        if !r.overlaps(o) {
            continue;
        }
        let cut_w = (o.x as i64 - current.x as i64).max(0) as u32;
        let cut_h = (o.y as i64 - current.y as i64).max(0) as u32;
        let keep_w = cut_w as u64 * h as u64;
        let keep_h = w as u64 * cut_h as u64;
        if keep_w >= keep_h && cut_w >= limits.min.0 {
            w = cut_w;
        } else if cut_h >= limits.min.1 {
            h = cut_h;
        } else if cut_w >= limits.min.0 {
            w = cut_w;
        } else {
            return None;
        }
    }
    if w < limits.min.0 || h < limits.min.1 || w == 0 || h == 0 {
        return None;
    }
    let r = Rect {
        x: current.x,
        y: current.y,
        w,
        h,
    };
    free(&r, others).then_some(r)
}

/// [`resize`] from any of the widget's handles: `moves` says whether the left edge (not the
/// right one) and the top edge (not the bottom one) follow the handle; the opposite edge stays
/// where it is. The monitor is looked at in a mirror on each such axis, where the edge that
/// stays is the left or top one again.
#[allow(clippy::too_many_arguments)]
pub fn resize_from(
    current: Rect,
    want_w: u32,
    want_h: u32,
    limits: Limits,
    others: &[Rect],
    bounds: Bounds,
    snap: Snap,
    moves: (bool, bool),
) -> Option<Rect> {
    let mirror = |r: &Rect| Rect {
        x: if moves.0 {
            (bounds.w as i64 - r.right()).clamp(i32::MIN as i64, i32::MAX as i64) as i32
        } else {
            r.x
        },
        y: if moves.1 {
            (bounds.h as i64 - r.bottom()).clamp(i32::MIN as i64, i32::MAX as i64) as i32
        } else {
            r.y
        },
        ..*r
    };
    let others: Vec<Rect> = others.iter().map(mirror).collect();
    resize(
        mirror(&current),
        want_w,
        want_h,
        limits,
        &others,
        bounds,
        snap,
    )
    .map(|r| mirror(&r))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopError {
    NoMonitor,
    NoWidget,
    /// The widget is not in a desktop container.
    NotOnDesktop,
    /// No place on the monitor fits the widget.
    NoRoom,
}

impl std::fmt::Display for DesktopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DesktopError::NoMonitor => "no such monitor",
            DesktopError::NoWidget => "no such widget",
            DesktopError::NotOnDesktop => "the widget is not on a desktop container",
            DesktopError::NoRoom => "no free place fits the widget",
        })
    }
}

impl std::error::Error for DesktopError {}

/// The container id, the widget, its rectangle and the other widgets' rectangles for
/// desktop widget `id`.
fn context<'a>(
    layout: &'a Layout,
    monitor: usize,
    id: &str,
    natural: Natural,
    placed: Placed,
) -> Result<(String, &'a Widget, Rect, Vec<Rect>), DesktopError> {
    let m = layout
        .monitors
        .get(monitor)
        .ok_or(DesktopError::NoMonitor)?;
    let (c, w) = m
        .containers
        .iter()
        .find_map(|c| c.widgets.iter().find(|w| w.id == id).map(|w| (c, w)))
        .ok_or(DesktopError::NoWidget)?;
    if !matches!(c.kind, ContainerKind::Desktop) {
        return Err(DesktopError::NotOnDesktop);
    }
    let me = Rect::of(w, natural).ok_or(DesktopError::NotOnDesktop)?;
    Ok((
        c.id.clone(),
        w,
        me,
        others(&c.widgets, Some(id), natural, placed),
    ))
}

/// The rectangles of a desktop container's widgets as they show ([`Rect::shown`]), except
/// `except`.
pub fn others(
    widgets: &[Widget],
    except: Option<&str>,
    natural: Natural,
    placed: Placed,
) -> Vec<Rect> {
    widgets
        .iter()
        .filter(|w| Some(w.id.as_str()) != except)
        .filter_map(|w| Rect::shown(w, natural, placed))
        .collect()
}

/// The edit for dropping desktop widget `id` at (`x`, `y`): the nearest free position. A
/// widget at its natural size stays at its natural size.
#[allow(clippy::too_many_arguments)]
pub fn move_to(
    layout: &Layout,
    monitor: usize,
    id: &str,
    x: i32,
    y: i32,
    bounds: Bounds,
    snap: Snap,
    natural: Natural,
    placed: Placed,
) -> Result<Edit, DesktopError> {
    let (container, wd, me, others) = context(layout, monitor, id, natural, placed)?;
    let want = clamp_into(Rect { x, y, ..me }, bounds);
    let to = nearest_free(want, &others, bounds, snap).ok_or(DesktopError::NoRoom)?;
    let place = match wd.place {
        Place::Desktop { size: None, .. } => Place::Desktop {
            x: to.x,
            y: to.y,
            size: None,
        },
        _ => to.place(),
    };
    Ok(Edit::MoveWidget {
        monitor,
        id: id.to_string(),
        container,
        place,
        index: None,
    })
}

/// The edit for resizing desktop widget `id` to `w`×`h` (see [`resize`]).
#[allow(clippy::too_many_arguments)]
pub fn resize_to(
    layout: &Layout,
    monitor: usize,
    id: &str,
    w: u32,
    h: u32,
    limits: Limits,
    bounds: Bounds,
    snap: Snap,
    natural: Natural,
    placed: Placed,
) -> Result<Edit, DesktopError> {
    let (container, _, me, others) = context(layout, monitor, id, natural, placed)?;
    let to = resize(me, w, h, limits, &others, bounds, snap).ok_or(DesktopError::NoRoom)?;
    Ok(Edit::MoveWidget {
        monitor,
        id: id.to_string(),
        container,
        place: to.place(),
        index: None,
    })
}

/// Where a new `w`×`h` widget goes on a desktop container: the free place nearest to
/// (`x`, `y`) (the drop point, or the monitor's top-left for "add").
#[allow(clippy::too_many_arguments)]
pub fn place_new(
    widgets: &[Widget],
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    bounds: Bounds,
    snap: Snap,
    natural: Natural,
    placed: Placed,
) -> Option<Place> {
    let want = clamp_into(
        Rect {
            x,
            y,
            w: snap.size(w),
            h: snap.size(h),
        },
        bounds,
    );
    nearest_free(want, &others(widgets, None, natural, placed), bounds, snap).map(|r| r.place())
}

/// Where the widgets of unplugged monitors (`guests`, at their own places) show meanwhile on
/// a remaining monitor (`bounds`, holding `own`): each as near its own place as it fits, off
/// every other widget; `None` for one that fits nowhere. The layout is not edited for this.
pub fn guest_places(own: &[Rect], guests: &[Rect], bounds: Bounds) -> Vec<Option<Rect>> {
    let mut taken = own.to_vec();
    guests
        .iter()
        .map(|&g| {
            if bounds.w == 0 || bounds.h == 0 {
                return None;
            }
            let r = nearest_free(clamp_into(g, bounds), &taken, bounds, Snap::Free)?;
            taken.push(r);
            Some(r)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout_model::{Container, MonitorLayout};

    const SCREEN: Bounds = Bounds { w: 1920, h: 1080 };

    /// The natural size of the widgets in these tests that have none in the layout.
    fn unplaced(_: &Widget) -> Shows {
        Shows::Laid
    }

    fn natural_size(_: &Widget) -> (u32, u32) {
        (230, 255)
    }

    fn sized(p: &Place) -> Rect {
        match *p {
            Place::Desktop {
                x,
                y,
                size: Some((w, h)),
            } => Rect { x, y, w, h },
            _ => panic!("{p:?}"),
        }
    }

    fn r(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    fn widget(id: &str, rect: Rect) -> Widget {
        Widget {
            id: id.into(),
            r#type: "clock".into(),
            place: rect.place(),
            settings: Default::default(),
        }
    }

    fn layout(widgets: Vec<Widget>) -> Layout {
        Layout {
            monitors: vec![MonitorLayout {
                connector: "DP-1".into(),
                edid: None,
                primary: true,
                containers: vec![Container {
                    id: "desktop".into(),
                    kind: ContainerKind::Desktop,
                    widgets,
                }],
            }],
        }
    }

    fn moved(e: Edit) -> Rect {
        match e {
            Edit::MoveWidget { place, .. } => sized(&place),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn a_resize_from_the_left_or_top_handle_keeps_the_opposite_edge() {
        let lim = Limits {
            min: (50, 40),
            max: (400, 300),
        };
        let me = r(500, 300, 200, 100);
        // The left handle pulled 60 px out: the right edge stays at 700.
        let got = resize_from(me, 260, 100, lim, &[], SCREEN, Snap::Free, (true, false));
        assert_eq!(got, Some(r(440, 300, 260, 100)));
        // The top-left corner, past the type's largest size: it stops there.
        let got = resize_from(me, 900, 900, lim, &[], SCREEN, Snap::Free, (true, true));
        assert_eq!(got, Some(r(300, 100, 400, 300)));
        // Pushed in past the smallest size: it stops there, the far edges where they were.
        let got = resize_from(me, 1, 1, lim, &[], SCREEN, Snap::Free, (true, true));
        assert_eq!(got, Some(r(650, 360, 50, 40)));
        // A neighbour on the left stops the left edge at its right edge.
        let other = [r(300, 300, 100, 100)];
        let got = resize_from(me, 400, 100, lim, &other, SCREEN, Snap::Free, (true, false));
        assert_eq!(got, Some(r(400, 300, 300, 100)));
        // The bottom-right handle is `resize` itself.
        let got = resize_from(me, 260, 120, lim, &[], SCREEN, Snap::Free, (false, false));
        assert_eq!(got, resize(me, 260, 120, lim, &[], SCREEN, Snap::Free));
    }

    #[test]
    fn free_drop_on_empty_space_stays_put() {
        let got = nearest_free(r(333, 217, 100, 50), &[], SCREEN, Snap::Free);
        assert_eq!(got, Some(r(333, 217, 100, 50)));
    }

    #[test]
    fn drop_off_screen_is_pulled_back_inside() {
        let l = layout(vec![widget("a", r(0, 0, 200, 100))]);
        let e = move_to(
            &l,
            0,
            "a",
            1900,
            -40,
            SCREEN,
            Snap::Free,
            &natural_size,
            &unplaced,
        )
        .unwrap();
        assert_eq!(moved(e), r(1720, 0, 200, 100));
    }

    #[test]
    fn drop_onto_another_widget_lands_next_to_it() {
        let l = layout(vec![
            widget("a", r(0, 0, 200, 100)),
            widget("b", r(500, 500, 300, 200)),
        ]);
        // Dropped over b's top-left part: the nearest free spot is against b's top edge (140 px
        // up) rather than its left edge (220 px left).
        let got = moved(
            move_to(
                &l,
                0,
                "a",
                520,
                540,
                SCREEN,
                Snap::Free,
                &natural_size,
                &unplaced,
            )
            .unwrap(),
        );
        assert_eq!(got, r(520, 400, 200, 100));
        assert!(!got.overlaps(&r(500, 500, 300, 200)));
    }

    #[test]
    fn grid_snaps_position_to_cells() {
        let got = nearest_free(r(130, 75, 64, 64), &[], SCREEN, Snap::Grid { cell: 32 });
        assert_eq!(got, Some(r(128, 64, 64, 64)));
    }

    #[test]
    fn grid_drop_onto_widget_takes_the_nearest_free_cell() {
        let others = [r(320, 320, 160, 160)];
        let got = nearest_free(
            r(352, 352, 64, 64),
            &others,
            SCREEN,
            Snap::Grid { cell: 32 },
        )
        .unwrap();
        assert_eq!(got.x % 32, 0);
        assert_eq!(got.y % 32, 0);
        assert!(!got.overlaps(&others[0]));
        // The closest free cells touch the obstacle: 320 - 64 = 256 (above or left), 96 px away.
        let d = (got.x - 352).pow(2) + (got.y - 352).pow(2);
        assert_eq!(d, 96 * 96);
    }

    #[test]
    fn touching_edges_are_not_an_overlap() {
        assert!(!r(0, 0, 100, 100).overlaps(&r(100, 0, 100, 100)));
        assert!(r(0, 0, 100, 100).overlaps(&r(99, 99, 100, 100)));
    }

    #[test]
    fn full_screen_has_no_room() {
        let others = [r(0, 0, 1920, 1080)];
        assert_eq!(
            nearest_free(r(10, 10, 50, 50), &others, SCREEN, Snap::Free),
            None
        );
        let l = layout(vec![
            widget("big", r(0, 0, 1920, 1000)),
            widget("a", r(0, 1000, 100, 80)),
        ]);
        // There is no free spot of 100x80 except where `a` already is.
        let got = moved(
            move_to(
                &l,
                0,
                "a",
                800,
                300,
                SCREEN,
                Snap::Free,
                &natural_size,
                &unplaced,
            )
            .unwrap(),
        );
        assert_eq!(got.y, 1000);
    }

    #[test]
    fn resize_clamps_to_type_limits_and_screen() {
        let lim = Limits {
            min: (80, 24),
            max: (400, 200),
        };
        assert_eq!(
            resize(r(10, 10, 100, 40), 1000, 5, lim, &[], SCREEN, Snap::Free),
            Some(r(10, 10, 400, 24))
        );
        assert_eq!(
            resize(
                r(1800, 1000, 100, 40),
                400,
                200,
                lim,
                &[],
                SCREEN,
                Snap::Free
            ),
            Some(r(1800, 1000, 120, 80))
        );
    }

    #[test]
    fn resize_stops_at_a_neighbour() {
        let others = [r(300, 0, 100, 100)];
        let got = resize(
            r(0, 0, 100, 100),
            500,
            100,
            Limits::ANY,
            &others,
            SCREEN,
            Snap::Free,
        );
        assert_eq!(got, Some(r(0, 0, 300, 100)));
        // Growing down-right past a neighbour below: the bottom edge stops there.
        let others = [r(0, 300, 100, 100)];
        let got = resize(
            r(0, 0, 400, 100),
            400,
            600,
            Limits::ANY,
            &others,
            SCREEN,
            Snap::Free,
        );
        assert_eq!(got, Some(r(0, 0, 400, 300)));
    }

    #[test]
    fn resize_below_minimum_is_refused() {
        let lim = Limits {
            min: (200, 50),
            max: (800, 800),
        };
        let others = [r(150, 0, 50, 50)];
        let l = layout(vec![widget("a", r(0, 0, 100, 100)), widget("b", others[0])]);
        assert_eq!(
            resize_to(
                &l,
                0,
                "a",
                300,
                40,
                lim,
                SCREEN,
                Snap::Free,
                &natural_size,
                &unplaced
            ),
            Err(DesktopError::NoRoom)
        );
    }

    #[test]
    fn grid_resize_rounds_to_whole_cells() {
        let got = resize(
            r(0, 0, 64, 64),
            100,
            50,
            Limits::ANY,
            &[],
            SCREEN,
            Snap::Grid { cell: 32 },
        );
        assert_eq!(got, Some(r(0, 0, 96, 64)));
    }

    #[test]
    fn place_new_avoids_existing_widgets() {
        let ws = vec![widget("a", r(0, 0, 400, 300))];
        let p = place_new(
            &ws,
            0,
            0,
            200,
            100,
            SCREEN,
            Snap::Free,
            &natural_size,
            &unplaced,
        )
        .unwrap();
        let got = sized(&p);
        assert!(!got.overlaps(&r(0, 0, 400, 300)));
        assert_eq!(got, r(0, 300, 200, 100));
    }

    #[test]
    fn edits_apply_and_keep_the_layout_collision_free() {
        let l = layout(vec![
            widget("a", r(0, 0, 200, 100)),
            widget("b", r(400, 400, 200, 100)),
        ]);
        let mut ed = crate::layout_model::Editor::new(l);
        let e = move_to(
            ed.layout(),
            0,
            "a",
            450,
            420,
            SCREEN,
            Snap::Free,
            &natural_size,
            &unplaced,
        )
        .unwrap();
        assert!(ed.apply(e).unwrap());
        let ws = &ed.layout().monitors[0].containers[0].widgets;
        let (a, b) = (sized(&ws[0].place), sized(&ws[1].place));
        assert!(!a.overlaps(&b));
        assert!(ed.undo());
        assert_eq!(
            Rect::of(
                &ed.layout().monitors[0].containers[0].widgets[0],
                &natural_size
            ),
            Some(r(0, 0, 200, 100))
        );
    }

    #[test]
    fn non_desktop_widget_is_rejected() {
        let mut l = layout(vec![widget("a", r(0, 0, 10, 10))]);
        l.monitors[0].containers[0].kind =
            ContainerKind::Sidebar(crate::layout_model::SidebarProps {
                side: crate::layout_model::Side::Right,
                columns: 1,
            });
        assert_eq!(
            move_to(
                &l,
                0,
                "a",
                5,
                5,
                SCREEN,
                Snap::Free,
                &natural_size,
                &unplaced
            ),
            Err(DesktopError::NotOnDesktop)
        );
        assert_eq!(
            move_to(
                &l,
                0,
                "zz",
                5,
                5,
                SCREEN,
                Snap::Free,
                &natural_size,
                &unplaced
            ),
            Err(DesktopError::NoWidget)
        );
    }

    fn natural_widget(id: &str, x: i32, y: i32) -> Widget {
        Widget {
            place: Place::Desktop { x, y, size: None },
            ..widget(id, r(0, 0, 1, 1))
        }
    }

    #[test]
    fn a_widget_without_a_size_shows_at_its_natural_size() {
        // Kept on the monitor by its natural 230x255 box, as the original keeps the clock on screen
        // (`x = min(x, screenWidth - width)`), never resized.
        let ws = [
            natural_widget("a", 1800, 900),
            widget("b", r(0, 0, 100, 100)),
        ];
        assert_eq!(
            fit_all(&ws, SCREEN, &natural_size, &|_| 0),
            vec![Some(r(1690, 825, 230, 255)), Some(r(0, 0, 100, 100))]
        );
    }

    #[test]
    fn a_widget_with_a_size_in_the_layout_keeps_its_place_over_a_natural_one() {
        // The natural-size widget comes first in the file but grew onto the sized one.
        let ws = [
            natural_widget("a", 100, 100),
            widget("b", r(200, 200, 300, 150)),
        ];
        let got = fit_all(&ws, SCREEN, &natural_size, &|_| 0);
        assert_eq!(got[1], Some(r(200, 200, 300, 150)));
        let a = got[0].unwrap();
        assert!(!a.overlaps(&got[1].unwrap()), "{a:?}");
    }

    #[test]
    fn the_natural_widget_whose_size_changed_last_moves_aside() {
        let ws = [natural_widget("a", 100, 100), natural_widget("b", 200, 200)];
        let changed_at = |a: u64, b: u64| move |wd: &Widget| if wd.id == "a" { a } else { b };
        // `a` changed last: `b` keeps its place although it comes later in the file.
        let got = fit_all(&ws, SCREEN, &natural_size, &changed_at(5, 3));
        assert_eq!(got[1], Some(r(200, 200, 230, 255)));
        assert!(!got[0].unwrap().overlaps(&got[1].unwrap()));
        // `b` changed last: `a` keeps its place.
        let got = fit_all(&ws, SCREEN, &natural_size, &changed_at(3, 5));
        assert_eq!(got[0], Some(r(100, 100, 230, 255)));
        assert!(!got[0].unwrap().overlaps(&got[1].unwrap()));
        // Neither measured yet: layout order.
        let got = fit_all(&ws, SCREEN, &natural_size, &|_| 0);
        assert_eq!(got[0], Some(r(100, 100, 230, 255)));
    }

    #[test]
    fn a_layout_that_fits_shows_unchanged_whatever_changed_last() {
        let ws = [
            natural_widget("a", 0, 0),
            widget("b", r(1000, 0, 100, 100)),
            natural_widget("c", 0, 500),
        ];
        let want = vec![
            Some(r(0, 0, 230, 255)),
            Some(r(1000, 0, 100, 100)),
            Some(r(0, 500, 230, 255)),
        ];
        for (a, c) in [(0, 0), (1, 2), (2, 1)] {
            let changed = move |wd: &Widget| if wd.id == "a" { a } else { c };
            assert_eq!(fit_all(&ws, SCREEN, &natural_size, &changed), want);
        }
    }

    #[test]
    fn moving_keeps_the_natural_size_and_resizing_sets_one() {
        let l = layout(vec![natural_widget("a", 100, 100)]);
        match move_to(
            &l,
            0,
            "a",
            1800,
            50,
            SCREEN,
            Snap::Free,
            &natural_size,
            &unplaced,
        )
        .unwrap()
        {
            Edit::MoveWidget { place, .. } => assert_eq!(
                place,
                Place::Desktop {
                    x: 1690,
                    y: 50,
                    size: None
                }
            ),
            e => panic!("{e:?}"),
        }
        let e = resize_to(
            &l,
            0,
            "a",
            300,
            200,
            Limits::ANY,
            SCREEN,
            Snap::Free,
            &natural_size,
            &unplaced,
        );
        assert_eq!(moved(e.unwrap()), r(100, 100, 300, 200));
    }

    /// The original's clock placed by the wallpaper (leastBusy) shows where its view reported it, not at
    /// its place in the layout, and a hidden widget shows nowhere: a drop keeps off the first
    /// where it shows and does not keep off the second.
    #[test]
    fn a_drop_keeps_off_widgets_where_they_show() {
        let clock = widget("clock", r(100, 100, 230, 250));
        let calendar = Widget {
            id: "calendar".into(),
            ..widget("calendar", r(60, 120, 300, 200))
        };
        let placed = |wd: &Widget| {
            if wd.id == "clock" {
                Shows::At(r(905, 305, 230, 250))
            } else {
                Shows::Laid
            }
        };
        let hidden = |wd: &Widget| {
            if wd.id == "clock" {
                Shows::Nowhere
            } else {
                Shows::Laid
            }
        };
        // A drop collides with the clock where it shows, not where the layout has it.
        let l = layout(vec![clock, calendar]);
        let got = moved(
            move_to(
                &l,
                0,
                "calendar",
                110,
                110,
                SCREEN,
                Snap::Free,
                &natural_size,
                &placed,
            )
            .unwrap(),
        );
        assert_eq!(got, r(110, 110, 300, 200));
        let got = moved(
            move_to(
                &l,
                0,
                "calendar",
                905,
                305,
                SCREEN,
                Snap::Free,
                &natural_size,
                &placed,
            )
            .unwrap(),
        );
        assert!(!got.overlaps(&r(905, 305, 230, 250)), "{got:?}");
        // Hidden, the clock is in nobody's way, where the layout has it either.
        let got = moved(
            move_to(
                &l,
                0,
                "calendar",
                100,
                100,
                SCREEN,
                Snap::Free,
                &natural_size,
                &hidden,
            )
            .unwrap(),
        );
        assert_eq!(got, r(100, 100, 300, 200));
    }
}
