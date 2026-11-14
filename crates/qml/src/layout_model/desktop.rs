//! Desktop container geometry: widgets sit anywhere on the monitor, never overlap each other
//! and never leave the monitor. A widget that does not fit where the layout puts it (another
//! monitor size or scale) shows at the nearest place that does.

use super::{Place, Widget};

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
}

/// The size a desktop widget without one in the layout has: its natural size (QML's
/// `implicitWidth` / `implicitHeight`), as its view measured it.
pub type Natural<'a> = &'a dyn Fn(&Widget) -> (u32, u32);

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
    nearest_free(r, others, bounds).unwrap_or(r)
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
/// something); the pixels next to each candidate are tried as well.
pub fn nearest_free(want: Rect, others: &[Rect], bounds: Bounds) -> Option<Rect> {
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
            let mut out = Vec::with_capacity(v.len() * 4 + 1);
            for c in v {
                for d in [-1, 0, 1] {
                    if (0..=max).contains(&(c + d)) {
                        out.push(c + d);
                    }
                }
                out.push(c.clamp(0, max));
            }
            if max >= 0 {
                out.push(max);
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
    let (tx, ty) = (want.x as i64, want.y as i64);
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
            let r = nearest_free(clamp_into(g, bounds), &taken, bounds)?;
            taken.push(r);
            Some(r)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Bounds = Bounds { w: 1920, h: 1080 };

    /// The natural size of the widgets in these tests that have none in the layout.
    fn natural_size(_: &Widget) -> (u32, u32) {
        (230, 255)
    }

    fn r(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    fn widget(id: &str, place: Place) -> Widget {
        Widget {
            id: id.into(),
            r#type: "clock".into(),
            place,
            settings: Default::default(),
        }
    }

    fn sized(id: &str, rect: Rect) -> Widget {
        let size = Some((rect.w, rect.h));
        widget(
            id,
            Place::Desktop {
                x: rect.x,
                y: rect.y,
                size,
            },
        )
    }

    #[test]
    fn a_free_place_stays_put() {
        let got = nearest_free(r(333, 217, 100, 50), &[], SCREEN);
        assert_eq!(got, Some(r(333, 217, 100, 50)));
        assert_eq!(fit(r(333, 217, 100, 50), &[], SCREEN), r(333, 217, 100, 50));
    }

    #[test]
    fn a_place_off_the_monitor_is_pulled_back_inside() {
        assert_eq!(
            fit(r(1900, -40, 200, 100), &[], SCREEN),
            r(1720, 0, 200, 100)
        );
    }

    #[test]
    fn a_place_over_another_widget_moves_next_to_it() {
        let b = r(500, 500, 300, 200);
        // Over b's top-left part: the nearest free spot is against b's top edge (140 px up)
        // rather than its left edge (220 px left).
        let got = fit(r(520, 540, 200, 100), &[b], SCREEN);
        assert_eq!(got, r(520, 400, 200, 100));
        assert!(!got.overlaps(&b));
    }

    #[test]
    fn touching_edges_are_not_an_overlap() {
        assert!(!r(0, 0, 100, 100).overlaps(&r(100, 0, 100, 100)));
        assert!(r(0, 0, 100, 100).overlaps(&r(99, 99, 100, 100)));
    }

    #[test]
    fn full_screen_has_no_room() {
        let others = [r(0, 0, 1920, 1080)];
        assert_eq!(nearest_free(r(10, 10, 50, 50), &others, SCREEN), None);
        // Nowhere free: shown just inside the monitor.
        assert_eq!(
            fit(r(1900, 10, 50, 50), &others, SCREEN),
            r(1870, 10, 50, 50)
        );
    }

    #[test]
    fn a_widget_without_a_size_shows_at_its_natural_size() {
        // Kept on the monitor by its natural 230x255 box, as the original keeps the clock on screen
        // (`x = min(x, screenWidth - width)`), never resized.
        let ws = [
            widget(
                "a",
                Place::Desktop {
                    x: 1800,
                    y: 900,
                    size: None,
                },
            ),
            sized("b", r(0, 0, 100, 100)),
        ];
        assert_eq!(
            fit_all(&ws, SCREEN, &natural_size, &|_| 0),
            vec![Some(r(1690, 825, 230, 255)), Some(r(0, 0, 100, 100))]
        );
    }

    fn natural(id: &str, x: i32, y: i32) -> Widget {
        widget(id, Place::Desktop { x, y, size: None })
    }

    #[test]
    fn a_widget_with_a_size_in_the_layout_keeps_its_place_over_a_natural_one() {
        // The natural-size widget comes first in the file but grew onto the sized one.
        let ws = [natural("a", 100, 100), sized("b", r(200, 200, 300, 150))];
        let got = fit_all(&ws, SCREEN, &natural_size, &|_| 0);
        assert_eq!(got[1], Some(r(200, 200, 300, 150)));
        let a = got[0].unwrap();
        assert!(!a.overlaps(&got[1].unwrap()), "{a:?}");
    }

    #[test]
    fn the_natural_widget_whose_size_changed_last_moves_aside() {
        let ws = [natural("a", 100, 100), natural("b", 200, 200)];
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
            natural("a", 0, 0),
            sized("b", r(1000, 0, 100, 100)),
            natural("c", 0, 500),
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
    fn guests_take_free_places_off_the_own_widgets_and_each_other() {
        let own = [r(0, 0, 400, 300)];
        let got = guest_places(&own, &[r(0, 0, 200, 100), r(0, 0, 200, 100)], SCREEN);
        let (a, b) = (got[0].unwrap(), got[1].unwrap());
        assert!(!a.overlaps(&own[0]) && !b.overlaps(&own[0]) && !a.overlaps(&b));
        assert_eq!(
            guest_places(&own, &[r(0, 0, 10, 10)], Bounds { w: 0, h: 0 }),
            vec![None]
        );
    }
}
