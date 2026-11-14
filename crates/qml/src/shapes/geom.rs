//! `RoundedPolygon` building, written from the original's former scripts
//! (`shapes/{point,cubic,utils,rounded-corner,rounded-polygon}.js`, `graphics/matrix.js`)
//! operation for operation, so every coordinate is bit-identical to what they gave.

use std::f64::consts::PI;
use std::rc::Rc;

use super::{Cubic, DISTANCE_EPSILON, Feature, interpolate, js_max, js_min, point_on_curve, split};

pub const MAX_SAFE: f64 = 9007199254740991.0;
pub const MIN_SAFE: f64 = -9007199254740991.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct P {
    pub x: f64,
    pub y: f64,
}

pub const fn p(x: f64, y: f64) -> P {
    P { x, y }
}

impl P {
    fn minus(self, o: P) -> P {
        p(self.x - o.x, self.y - o.y)
    }
    fn plus(self, o: P) -> P {
        p(self.x + o.x, self.y + o.y)
    }
    fn times(self, k: f64) -> P {
        p(self.x * k, self.y * k)
    }
    fn div(self, k: f64) -> P {
        p(self.x / k, self.y / k)
    }
    pub fn distance(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    fn direction(self) -> P {
        let d = self.distance();
        self.div(d)
    }
    fn dot(self, o: P) -> f64 {
        self.x * o.x + self.y * o.y
    }
    fn rotate90(self) -> P {
        p(-self.y, self.x)
    }
    fn clockwise(self, o: P) -> bool {
        self.x * o.y - self.y * o.x > 0.0
    }
    /// `Offset.angleDegrees`.
    pub fn angle_degrees(self) -> f64 {
        self.y.atan2(self.x) * 180.0 / PI
    }
    /// `Offset.rotateDegrees`.
    pub fn rotate_degrees(self, angle: f64, center: P) -> P {
        let a = angle * PI / 180.0;
        let off = self.minus(center);
        let (cos_a, sin_a) = (a.cos(), a.sin());
        p(off.x * cos_a - off.y * sin_a, off.x * sin_a + off.y * cos_a).plus(center)
    }
}

/// `Math.pow` as the builtin computes it: libm `pow` at run time (LLVM would turn a constant
/// `powf(x, 2.0)` into `x * x`, which can differ by an ulp).
fn js_pow(x: f64, y: f64) -> f64 {
    x.powf(std::hint::black_box(y))
}

/// `Point.interpolate`.
fn lerp(start: P, stop: P, f: f64) -> P {
    p(
        start.x + (stop.x - start.x) * f,
        start.y + (stop.y - start.y) * f,
    )
}

fn direction_vector(x: f64, y: f64) -> P {
    let d = (x * x + y * y).sqrt();
    p(x / d, y / d)
}

/// `radialToCartesian(radius, angle)` with its default `center` of (0, 0).
fn radial(radius: f64, angle: f64) -> P {
    p(angle.cos(), angle.sin()).times(radius).plus(p(0.0, 0.0))
}

fn convex(prev: P, cur: P, next: P) -> bool {
    cur.minus(prev).clockwise(next.minus(cur))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rounding {
    pub radius: f64,
    pub smoothing: f64,
}

pub const UNROUNDED: Rounding = Rounding {
    radius: 0.0,
    smoothing: 0.0,
};

pub const fn r(radius: f64, smoothing: f64) -> Rounding {
    Rounding { radius, smoothing }
}

pub fn straight_line(x0: f64, y0: f64, x1: f64, y1: f64) -> Cubic {
    [
        x0,
        y0,
        interpolate(x0, x1, 1.0 / 3.0),
        interpolate(y0, y1, 1.0 / 3.0),
        interpolate(x0, x1, 2.0 / 3.0),
        interpolate(y0, y1, 2.0 / 3.0),
        x1,
        y1,
    ]
}

fn circular_arc(cx: f64, cy: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> Cubic {
    let p0d = direction_vector(x0 - cx, y0 - cy);
    let p1d = direction_vector(x1 - cx, y1 - cy);
    let rp0 = p0d.rotate90();
    let rp1 = p1d.rotate90();
    let clockwise = rp0.x * (x1 - cx) + rp0.y * (y1 - cy) >= 0.0;
    let cosa = p0d.dot(p1d);
    if cosa > 0.999 {
        return straight_line(x0, y0, x1, y1);
    }
    let dist = ((x0 - cx) * (x0 - cx) + (y0 - cy) * (y0 - cy)).sqrt();
    let k = dist * 4.0 / 3.0 * ((2.0 * (1.0 - cosa)).sqrt() - (1.0 - cosa * cosa).sqrt())
        / (1.0 - cosa)
        * if clockwise { 1.0 } else { -1.0 };
    [
        x0,
        y0,
        x0 + rp0.x * k,
        y0 + rp0.y * k,
        x1 - rp1.x * k,
        y1 - rp1.y * k,
        x1,
        y1,
    ]
}

fn zero_length(c: &Cubic) -> bool {
    (c[0] - c[6]).abs() < DISTANCE_EPSILON && (c[1] - c[7]).abs() < DISTANCE_EPSILON
}

fn reverse(c: Cubic) -> Cubic {
    [c[6], c[7], c[4], c[5], c[2], c[3], c[0], c[1]]
}

fn zero_ish(v: f64) -> bool {
    v.abs() < DISTANCE_EPSILON
}

/// Extremum of one coordinate (`k` 0 for x, 1 for y) along the curve.
fn extend(c: &Cubic, k: usize, lo: &mut f64, hi: &mut f64) {
    let at = |t: f64| {
        let q = point_on_curve(c, t);
        if k == 0 { q.0 } else { q.1 }
    };
    let mut probe = |t: f64| {
        if (0.0..=1.0).contains(&t) {
            let v = at(t);
            if v < *lo {
                *lo = v;
            }
            if v > *hi {
                *hi = v;
            }
        }
    };
    let (a0, c0, c1, a1) = (c[k], c[2 + k], c[4 + k], c[6 + k]);
    let a = -a0 + 3.0 * c0 - 3.0 * c1 + a1;
    let b = 2.0 * a0 - 4.0 * c0 + 2.0 * c1;
    let cc = -a0 + c0;
    if zero_ish(a) {
        if b != 0.0 {
            probe(2.0 * cc / (-2.0 * b));
        }
    } else {
        let s = b * b - 4.0 * a * cc;
        if s >= 0.0 {
            probe((-b + s.sqrt()) / (2.0 * a));
            probe((-b - s.sqrt()) / (2.0 * a));
        }
    }
}

/// `Cubic.calculateBounds`.
pub fn cubic_bounds(c: &Cubic, approximate: bool) -> [f64; 4] {
    if zero_length(c) {
        return [c[0], c[1], c[0], c[1]];
    }
    let mut min_x = js_min(c[0], c[6]);
    let mut min_y = js_min(c[1], c[7]);
    let mut max_x = js_max(c[0], c[6]);
    let mut max_y = js_max(c[1], c[7]);
    if approximate {
        return [
            js_min(min_x, js_min(c[2], c[4])),
            js_min(min_y, js_min(c[3], c[5])),
            js_max(max_x, js_max(c[2], c[4])),
            js_max(max_y, js_max(c[3], c[5])),
        ];
    }
    extend(c, 0, &mut min_x, &mut max_x);
    extend(c, 1, &mut min_y, &mut max_y);
    [min_x, min_y, max_x, max_y]
}

/// `RoundedCorner`.
struct Corner {
    p0: P,
    p1: P,
    p2: P,
    d1: P,
    d2: P,
    corner_radius: f64,
    smoothing: f64,
    expected_round_cut: f64,
}

impl Corner {
    fn new(p0: P, p1: P, p2: P, rounding: Rounding) -> Self {
        let v01 = p0.minus(p1);
        let v21 = p2.minus(p1);
        let d01 = v01.distance();
        let d21 = v21.distance();
        if d01 > 0.0 && d21 > 0.0 {
            let d1 = v01.div(d01);
            let d2 = v21.div(d21);
            let cos = d1.dot(d2);
            let sin = (1.0 - js_pow(cos, 2.0)).sqrt();
            let erc = if sin > 1e-3 {
                rounding.radius * (cos + 1.0) / sin
            } else {
                0.0
            };
            Corner {
                p0,
                p1,
                p2,
                d1,
                d2,
                corner_radius: rounding.radius,
                smoothing: rounding.smoothing,
                expected_round_cut: erc,
            }
        } else {
            Corner {
                p0,
                p1,
                p2,
                d1: p(0.0, 0.0),
                d2: p(0.0, 0.0),
                corner_radius: 0.0,
                smoothing: 0.0,
                expected_round_cut: 0.0,
            }
        }
    }

    fn expected_cut(&self) -> f64 {
        (1.0 + self.smoothing) * self.expected_round_cut
    }

    fn cubics(&self, cut0: f64, cut1: f64) -> Vec<Cubic> {
        let erc = self.expected_round_cut;
        let allowed = js_min(cut0, cut1);
        if erc < DISTANCE_EPSILON
            || allowed < DISTANCE_EPSILON
            || self.corner_radius < DISTANCE_EPSILON
        {
            return vec![straight_line(self.p1.x, self.p1.y, self.p1.x, self.p1.y)];
        }
        let round_cut = js_min(allowed, erc);
        let s0 = self.smoothing_for(cut0);
        let s1 = self.smoothing_for(cut1);
        let actual_r = self.corner_radius * round_cut / erc;
        let center_distance = (js_pow(actual_r, 2.0) + js_pow(round_cut, 2.0)).sqrt();
        let center = self.p1.plus(
            self.d1
                .plus(self.d2)
                .div(2.0)
                .direction()
                .times(center_distance),
        );
        let ci0 = self.p1.plus(self.d1.times(round_cut));
        let ci2 = self.p1.plus(self.d2.times(round_cut));
        let f0 = flanking(round_cut, s0, self.p1, self.p0, ci0, ci2, center, actual_r);
        let f2 = reverse(flanking(
            round_cut, s1, self.p1, self.p2, ci2, ci0, center, actual_r,
        ));
        vec![
            f0,
            circular_arc(center.x, center.y, f0[6], f0[7], f2[0], f2[1]),
            f2,
        ]
    }

    fn smoothing_for(&self, cut: f64) -> f64 {
        let erc = self.expected_round_cut;
        if cut > self.expected_cut() {
            self.smoothing
        } else if cut > erc {
            self.smoothing * (cut - erc) / (self.expected_cut() - erc)
        } else {
            0.0
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn flanking(
    round_cut: f64,
    smoothing: f64,
    corner: P,
    side_start: P,
    circle_intersection: P,
    other_intersection: P,
    circle_center: P,
    actual_r: f64,
) -> Cubic {
    let side_dir = side_start.minus(corner).direction();
    let curve_start = corner.plus(side_dir.times(round_cut * (1.0 + smoothing)));
    let q = lerp(
        circle_intersection,
        circle_intersection.plus(other_intersection).div(2.0),
        smoothing,
    );
    let curve_end = circle_center
        .plus(direction_vector(q.x - circle_center.x, q.y - circle_center.y).times(actual_r));
    let tangent = curve_end.minus(circle_center).rotate90();
    let anchor_end =
        line_intersection(side_start, side_dir, curve_end, tangent).unwrap_or(circle_intersection);
    let anchor_start = curve_start.plus(anchor_end.times(2.0)).div(3.0);
    [
        curve_start.x,
        curve_start.y,
        anchor_start.x,
        anchor_start.y,
        anchor_end.x,
        anchor_end.y,
        curve_end.x,
        curve_end.y,
    ]
}

fn line_intersection(p0: P, d0: P, p1: P, d1: P) -> Option<P> {
    let rd1 = d1.rotate90();
    let den = d0.dot(rd1);
    if den.abs() < DISTANCE_EPSILON {
        return None;
    }
    let num = p1.minus(p0).dot(rd1);
    if den.abs() < DISTANCE_EPSILON * num.abs() {
        return None;
    }
    Some(p0.plus(d0.times(num / den)))
}

/// `RoundedPolygon`.
#[derive(Debug)]
pub struct Polygon {
    pub features: Rc<Vec<Feature>>,
    pub center: P,
    pub cubics: Vec<Cubic>,
}

impl Polygon {
    pub fn new(features: Vec<Feature>, center: P) -> Self {
        let cubics = build_cubic_list(&features, center);
        Polygon {
            features: Rc::new(features),
            center,
            cubics,
        }
    }

    /// `transformed(f)`: `f` is called on the center, then on each cubic's four points.
    pub fn transformed<E>(&self, mut f: impl FnMut(f64, f64) -> Result<P, E>) -> Result<Self, E> {
        let center = f(self.center.x, self.center.y)?;
        let mut features = Vec::with_capacity(self.features.len());
        for ft in self.features.iter() {
            let mut cubics = Vec::with_capacity(ft.cubics.len());
            for c in &ft.cubics {
                let mut c = *c;
                for i in [0, 2, 4, 6] {
                    let q = f(c[i], c[i + 1])?;
                    c[i] = q.x;
                    c[i + 1] = q.y;
                }
                cubics.push(c);
            }
            features.push(Feature {
                cubics,
                corner: ft.corner,
            });
        }
        Ok(Polygon::new(features, center))
    }

    pub fn map(&self, m: &Matrix) -> Self {
        self.transformed::<()>(|x, y| Ok(m.map(x, y))).unwrap()
    }

    pub fn normalized(&self) -> Self {
        let b = self.bounds(true);
        let width = b[2] - b[0];
        let height = b[3] - b[1];
        let side = js_max(width, height);
        let ox = (side - width) / 2.0 - b[0];
        let oy = (side - height) / 2.0 - b[1];
        self.transformed::<()>(|x, y| Ok(p((x + ox) / side, (y + oy) / side)))
            .unwrap()
    }

    /// `calculateBounds([], approximate)`.
    pub fn bounds(&self, approximate: bool) -> [f64; 4] {
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (MAX_SAFE, MAX_SAFE, MIN_SAFE, MIN_SAFE);
        for c in &self.cubics {
            let b = cubic_bounds(c, approximate);
            min_x = js_min(min_x, b[0]);
            min_y = js_min(min_y, b[1]);
            max_x = js_max(max_x, b[2]);
            max_y = js_max(max_y, b[3]);
        }
        [min_x, min_y, max_x, max_y]
    }

    /// `calculateMaxBounds()`.
    pub fn max_bounds(&self) -> [f64; 4] {
        let (cx, cy) = (self.center.x, self.center.y);
        let mut max_sq = 0.0;
        for c in &self.cubics {
            let anchor = (c[0] - cx) * (c[0] - cx) + (c[1] - cy) * (c[1] - cy);
            let (mx, my) = point_on_curve(c, 0.5);
            let middle = (mx - cx) * (mx - cx) + (my - cy) * (my - cy);
            max_sq = js_max(max_sq, js_max(anchor, middle));
        }
        let d = f64::sqrt(max_sq);
        [cx - d, cy - d, cx + d, cy + d]
    }
}

fn build_cubic_list(features: &[Feature], center: P) -> Vec<Cubic> {
    let mut result = Vec::new();
    let mut first: Option<Cubic> = None;
    let mut last: Option<Cubic> = None;
    let split_first = match features.first() {
        Some(f) if f.cubics.len() == 3 => {
            let (a, b) = split(&f.cubics[1], 0.5);
            Some(([f.cubics[0], a], [b, f.cubics[2]]))
        }
        _ => None,
    };
    for i in 0..=features.len() {
        let cubics: &[Cubic] = match &split_first {
            Some((_, end)) if i == 0 => end,
            Some((start, _)) if i == features.len() => start,
            None if i == features.len() => break,
            _ => &features[i].cubics,
        };
        for c in cubics {
            if !zero_length(c) {
                if let Some(l) = last {
                    result.push(l);
                }
                last = Some(*c);
                if first.is_none() {
                    first = Some(*c);
                }
            } else if let Some(l) = &mut last {
                l[6] = c[6];
                l[7] = c[7];
            }
        }
    }
    match (last, first) {
        (Some(l), Some(f)) => result.push([l[0], l[1], l[2], l[3], l[4], l[5], f[0], f[1]]),
        _ => {
            let (x, y) = (center.x, center.y);
            result.push([x, y, x, y, x, y, x, y]);
        }
    }
    result
}

/// `RoundedPolygon.calculateCenter`.
pub fn calculate_center(v: &[f64]) -> P {
    let (mut cx, mut cy) = (0.0, 0.0);
    for xy in v.chunks(2) {
        cx += xy[0];
        cy += xy.get(1).copied().unwrap_or(f64::NAN);
    }
    let n = v.len() as f64 / 2.0;
    p(cx / n, cy / n)
}

pub fn vertices_from_num_verts(n: f64, radius: f64, cx: f64, cy: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut i = 0.0;
    while i < n {
        let v = radial(radius, PI / n * 2.0 * i).plus(p(cx, cy));
        out.push(v.x);
        out.push(v.y);
        i += 1.0;
    }
    out
}

pub fn star_vertices(n: f64, radius: f64, inner: f64, cx: f64, cy: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut i = 0.0;
    while i < n {
        let v = radial(radius, PI / n * 2.0 * i);
        out.push(v.x + cx);
        out.push(v.y + cy);
        let v = radial(inner, PI / n * (2.0 * i + 1.0));
        out.push(v.x + cx);
        out.push(v.y + cy);
        i += 1.0;
    }
    out
}

/// `RoundedPolygon.fromVertices`; `per_vertex[i] == None` falls back to `rounding`.
pub fn from_vertices(
    vertices: &[f64],
    rounding: Rounding,
    per_vertex: Option<&[Option<Rounding>]>,
    cx: f64,
    cy: f64,
) -> Polygon {
    let n = vertices.len() / 2;
    let vtx = |i: usize| p(vertices[i * 2], vertices[i * 2 + 1]);
    let corners: Vec<Corner> = (0..n)
        .map(|i| {
            let vr = per_vertex
                .and_then(|pv| pv.get(i).copied().flatten())
                .unwrap_or(rounding);
            Corner::new(vtx((i + n - 1) % n), vtx(i), vtx((i + 1) % n), vr)
        })
        .collect();
    let cut_adjusts: Vec<(f64, f64)> = (0..n)
        .map(|ix| {
            let next = (ix + 1) % n;
            let erc = corners[ix].expected_round_cut + corners[next].expected_round_cut;
            let ec = corners[ix].expected_cut() + corners[next].expected_cut();
            let (a, b) = (vtx(ix), vtx(next));
            let side = p(a.x - b.x, a.y - b.y).distance();
            if erc > side {
                (side / erc, 0.0)
            } else if ec > side {
                (1.0, (side - erc) / (ec - erc))
            } else {
                (1.0, 1.0)
            }
        })
        .collect();
    let cubics: Vec<Vec<Cubic>> = (0..n)
        .map(|i| {
            let c = &corners[i];
            let cut = |delta: usize| {
                let (rr, cr) = cut_adjusts[(i + n - 1 + delta) % n];
                c.expected_round_cut * rr + (c.expected_cut() - c.expected_round_cut) * cr
            };
            c.cubics(cut(0), cut(1))
        })
        .collect();
    let mut features = Vec::with_capacity(2 * n);
    for i in 0..n {
        let next = (i + 1) % n;
        let cv = convex(vtx((i + n - 1) % n), vtx(i), vtx(next));
        features.push(Feature {
            cubics: cubics[i].clone(),
            corner: Some(cv),
        });
        let last = cubics[i][cubics[i].len() - 1];
        let first = cubics[next][0];
        features.push(Feature {
            cubics: vec![straight_line(last[6], last[7], first[0], first[1])],
            corner: None,
        });
    }
    let center = if cx == MIN_SAFE || cy == MIN_SAFE {
        calculate_center(vertices)
    } else {
        p(cx, cy)
    };
    from_features(features, center.x, center.y)
}

/// `RoundedPolygon.fromFeatures`.
pub fn from_features(features: Vec<Feature>, mut cx: f64, mut cy: f64) -> Polygon {
    if cx.is_nan() || cy.is_nan() {
        let mut v = Vec::new();
        for f in &features {
            for c in &f.cubics {
                v.push(c[0]);
                v.push(c[1]);
            }
        }
        let c = calculate_center(&v);
        if cx.is_nan() {
            cx = c.x;
        }
        if cy.is_nan() {
            cy = c.y;
        }
    }
    Polygon::new(features, p(cx, cy))
}

pub fn from_num_vertices(
    n: f64,
    radius: f64,
    cx: f64,
    cy: f64,
    rounding: Rounding,
    per_vertex: Option<&[Option<Rounding>]>,
) -> Polygon {
    from_vertices(
        &vertices_from_num_verts(n, radius, cx, cy),
        rounding,
        per_vertex,
        cx,
        cy,
    )
}

pub fn circle(n: f64, radius: f64, cx: f64, cy: f64) -> Polygon {
    let theta = PI / n;
    let polygon_radius = radius / theta.cos();
    from_num_vertices(n, polygon_radius, cx, cy, r(radius, 0.0), None)
}

pub fn rectangle(
    w: f64,
    h: f64,
    rounding: Rounding,
    per_vertex: Option<&[Option<Rounding>]>,
    cx: f64,
    cy: f64,
) -> Polygon {
    let left = cx - w / 2.0;
    let top = cy - h / 2.0;
    let right = cx + w / 2.0;
    let bottom = cy + h / 2.0;
    from_vertices(
        &[right, bottom, left, bottom, left, top, right, top],
        rounding,
        per_vertex,
        cx,
        cy,
    )
}

/// `RoundedPolygon.star`; like the JS, `innerRounding` never reaches `fromVertices`.
pub fn star(
    n: f64,
    radius: f64,
    inner: f64,
    rounding: Rounding,
    per_vertex: Option<&[Option<Rounding>]>,
    cx: f64,
    cy: f64,
) -> Polygon {
    from_vertices(
        &star_vertices(n, radius, inner, cx, cy),
        rounding,
        per_vertex,
        cx,
        cy,
    )
}

/// `graphics/matrix.js` (row-major 4x4).
#[derive(Clone, Copy, Debug)]
pub struct Matrix(pub [f64; 16]);

impl Matrix {
    pub const IDENTITY: Matrix = Matrix([
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]);

    fn get(&self, row: usize, col: usize) -> f64 {
        self.0[row * 4 + col]
    }

    pub fn map(&self, x: f64, y: f64) -> P {
        let z = self.get(0, 3) * x + self.get(1, 3) * y + self.get(3, 3);
        let inv = 1.0 / z;
        let pz = if inv.is_finite() { inv } else { 0.0 };
        p(
            pz * (self.get(0, 0) * x + self.get(1, 0) * y + self.get(3, 0)),
            pz * (self.get(0, 1) * x + self.get(1, 1) * y + self.get(3, 1)),
        )
    }

    pub fn rotate_z(mut self, degrees: f64) -> Self {
        let rad = degrees * (PI / 180.0);
        let (s, c) = (rad.sin(), rad.cos());
        for col in 0..4 {
            let a0 = self.get(0, col);
            let a1 = self.get(1, col);
            self.0[col] = c * a0 + s * a1;
            self.0[4 + col] = -s * a0 + c * a1;
        }
        self
    }

    pub fn scale(mut self, x: f64, y: f64, z: f64) -> Self {
        for (row, k) in [x, y, z].into_iter().enumerate() {
            for col in 0..4 {
                self.0[row * 4 + col] *= k;
            }
        }
        self
    }
}
