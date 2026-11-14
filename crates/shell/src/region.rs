//! Damage regions: a few rectangles `[x0, y0, x1, y1]` (pixels, x1/y1 exclusive), like the
//! `QRegion` Qt hands to `eglSwapBuffersWithDamageKHR`. Two changes at opposite ends of a
//! window stay two rectangles instead of their bounding box.

/// Rectangles a frame repaints and reports as damaged; empty when nothing changed.
pub type Region = Vec<[i64; 4]>;

/// At most this many rectangles: each costs a scissored pass over the draw list.
pub const MAX_RECTS: usize = 4;

pub fn area(r: [i64; 4]) -> i64 {
    (r[2] - r[0]).max(0) * (r[3] - r[1]).max(0)
}

fn hull(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn meet(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

/// Pixels the bounding box of `a` and `b` covers that neither does.
fn waste(a: [i64; 4], b: [i64; 4]) -> i64 {
    area(hull(a, b)) - area(a) - area(b) + area(meet(a, b))
}

/// Adds `r` to `reg`. Rectangles whose bounding box wastes at most a quarter of what they
/// cover merge (touching or overlapping ones, mostly); past `MAX_RECTS` the pair that wastes
/// least merges.
pub fn add(reg: &mut Region, r: [i64; 4]) {
    if area(r) == 0 {
        return;
    }
    let mut r = r;
    while let Some(k) = reg
        .iter()
        .position(|&a| waste(a, r) * 4 <= area(a) + area(r) - area(meet(a, r)))
    {
        r = hull(reg.swap_remove(k), r);
    }
    reg.push(r);
    while reg.len() > MAX_RECTS {
        let mut best = (i64::MAX, 0, 1);
        for i in 0..reg.len() {
            for j in i + 1..reg.len() {
                let w = waste(reg[i], reg[j]);
                if w < best.0 {
                    best = (w, i, j);
                }
            }
        }
        let b = reg.swap_remove(best.2);
        let a = reg.swap_remove(best.1);
        add(reg, hull(a, b));
    }
}

/// `reg` clipped to `to`, empty rectangles dropped.
pub fn clip(reg: &[[i64; 4]], to: [i64; 4]) -> Region {
    reg.iter()
        .map(|&r| meet(r, to))
        .filter(|&r| area(r) > 0)
        .collect()
}

/// `reg` as rectangles that do not overlap: overlapping ones merge into their bounding box.
pub fn disjoint(reg: &[[i64; 4]]) -> Region {
    let mut out: Region = Vec::new();
    for &r in reg {
        let mut r = r;
        while let Some(i) = out.iter().position(|&o| area(meet(o, r)) > 0) {
            r = hull(out.swap_remove(i), r);
        }
        out.push(r);
    }
    out
}

/// Every rectangle of `reg` lies inside one rectangle of `of`.
pub fn inside(reg: &[[i64; 4]], of: &[[i64; 4]]) -> bool {
    reg.iter().all(|&r| of.iter().any(|&o| meet(o, r) == r))
}

/// The bounding box of `reg`.
pub fn bounds(reg: &[[i64; 4]]) -> Option<[i64; 4]> {
    reg.iter().copied().reduce(hull)
}

/// Where two w x h images (rows top first) differ: per band of consecutive changed rows, the
/// columns between the first and last change.
pub fn diff_pixels<T: PartialEq>(a: &[T], b: &[T], w: usize, h: usize) -> Region {
    let mut reg = Region::new();
    if w == 0 || a.len() < w * h || b.len() < w * h {
        return reg;
    }
    let mut band: Option<[i64; 4]> = None;
    for y in 0..h {
        let (ra, rb) = (&a[y * w..(y + 1) * w], &b[y * w..(y + 1) * w]);
        let x0 = ra.iter().zip(rb).position(|(p, q)| p != q);
        let row = x0.map(|x0| {
            let x1 = w - ra
                .iter()
                .zip(rb)
                .rev()
                .position(|(p, q)| p != q)
                .unwrap_or(0);
            [x0 as i64, y as i64, x1 as i64, y as i64 + 1]
        });
        band = match (band, row) {
            (Some(b), Some(r)) => Some(hull(b, r)),
            (None, r) => r,
            (Some(b), None) => {
                add(&mut reg, b);
                None
            }
        };
    }
    if let Some(b) = band {
        add(&mut reg, b);
    }
    reg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distant_changes_stay_apart() {
        let mut r = Region::new();
        add(&mut r, [0, 0, 10, 10]);
        add(&mut r, [1900, 0, 1910, 10]);
        r.sort();
        assert_eq!(r, vec![[0, 0, 10, 10], [1900, 0, 1910, 10]]);
        let px: i64 = r.iter().map(|&q| area(q)).sum();
        assert_eq!(
            px,
            200,
            "the 1910x10 bounding box would repaint {}",
            1910 * 10
        );
    }

    #[test]
    fn touching_and_overlapping_rects_merge() {
        let mut r = Region::new();
        add(&mut r, [0, 0, 10, 10]);
        add(&mut r, [10, 0, 20, 10]);
        assert_eq!(r, vec![[0, 0, 20, 10]]);
        add(&mut r, [5, 5, 25, 10]);
        assert_eq!(r, vec![[0, 0, 25, 10]]);
        add(&mut r, [0, 0, 0, 50]);
        assert_eq!(r, vec![[0, 0, 25, 10]], "an empty rect adds nothing");
    }

    #[test]
    fn rect_count_is_capped_by_least_waste() {
        let mut r = Region::new();
        for k in 0..6 {
            add(&mut r, [k * 100, 0, k * 100 + 10, 10]);
        }
        assert_eq!(r.len(), MAX_RECTS);
        let covered = |x: i64| r.iter().any(|q| q[0] <= x && x < q[2]);
        assert!((0..6).all(|k| covered(k * 100 + 5)), "a change lost: {r:?}");
    }

    #[test]
    fn pixel_diff_finds_each_changed_band() {
        let (w, h) = (100, 50);
        let a = vec![0u32; w * h];
        let mut b = a.clone();
        b[2 * w + 3] = 1;
        b[3 * w + 7] = 1;
        b[40 * w + 98] = 1;
        let mut r = diff_pixels(&a, &b, w, h);
        r.sort();
        assert_eq!(r, vec![[3, 2, 8, 4], [98, 40, 99, 41]]);
        assert!(diff_pixels(&a, &a, w, h).is_empty());
        assert_eq!(clip(&r, [0, 0, 50, 50]), vec![[3, 2, 8, 4]]);
        assert_eq!(bounds(&r), Some([3, 2, 99, 41]));
    }
}
