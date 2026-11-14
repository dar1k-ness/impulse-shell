//! Qt Quick Shapes, CurveRenderer stroke path (Qt 6.12, legacy non-expanding stroker); the fill
//! is in `shape_fill.rs`.
//!
//! Pipeline, each step ported from Qt:
//! QQuickPath elements → QPainterPath (arcTo/qt_curves_for_arc) → QQuadPath::fromPainterPath
//! (qt_toQuadratics) → subdivide(3) + flattened → customTriangulator2 → shapestroke.{vert,frag}.

use crate::raster::Canvas;

#[path = "shape_fill.rs"]
mod fill;
pub use fill::{FillFrame, FillTri, draw_fill, fill_tris, fill_vertices};

type V = [f32; 2];
type D = [f64; 2];

const KAPPA: f64 = 0.5522847498;

fn fnull_d(x: f64) -> bool {
    x.abs() <= 1e-12
}
pub(crate) fn fcmp_d(a: f64, b: f64) -> bool {
    (a - b).abs() * 1e12 <= a.abs().min(b.abs())
}
fn fnull_f(x: f32) -> bool {
    x.abs() <= 1e-5
}
fn fcmp_f(a: f32, b: f32) -> bool {
    (a - b).abs() * 1e5 <= a.abs().min(b.abs())
}
pub(crate) fn fcmp_pt(a: D, b: D) -> bool {
    let c = |p: f64, q: f64| {
        if p == 0.0 || q == 0.0 {
            fnull_d(p - q)
        } else {
            fcmp_d(p, q)
        }
    };
    c(a[0], b[0]) && c(a[1], b[1])
}
fn fcmp_v(a: V, b: V) -> bool {
    fcmp_f(a[0], b[0]) && fcmp_f(a[1], b[1])
}

fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: V, k: f32) -> V {
    [a[0] * k, a[1] * k]
}
fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn len(a: V) -> f32 {
    a[0].hypot(a[1])
}
/// QVector2D::normalized()
fn norm(a: V) -> V {
    let l = len(a);
    if fnull_f(l - 1.0) {
        a
    } else if fnull_f(l) {
        [0.0, 0.0]
    } else {
        [a[0] / l, a[1] / l]
    }
}
fn neg(a: V) -> V {
    [-a[0], -a[1]]
}

// ---------------------------------------------------------------- QPainterPath

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cap {
    Flat,
    Square,
    Round,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Join {
    Miter,
    Bevel,
    Round,
}

#[derive(Clone, Debug)]
pub enum PathCmd {
    Move(f64, f64),
    Line(f64, f64),
    /// PathAngleArc (QML angles: clockwise degrees).
    AngleArc {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        start: f64,
        sweep: f64,
        move_to_start: bool,
    },
}

#[derive(Clone, Copy)]
enum PE {
    Move(D),
    Line(D),
    Cubic(D, D, D),
}

#[derive(Default)]
struct PPath(Vec<PE>);

impl PPath {
    fn last(&self) -> D {
        match self.0.last() {
            Some(PE::Move(p)) | Some(PE::Line(p)) | Some(PE::Cubic(_, _, p)) => *p,
            None => [0.0, 0.0],
        }
    }
    fn move_to(&mut self, p: D) {
        if let Some(PE::Move(_)) = self.0.last() {
            self.0.pop();
        }
        self.0.push(PE::Move(p));
    }
    fn ensure_start(&mut self) {
        if self.0.is_empty() {
            self.0.push(PE::Move([0.0, 0.0]));
        }
    }
    fn line_to(&mut self, p: D) {
        self.ensure_start();
        if fcmp_pt(p, self.last()) {
            return;
        }
        self.0.push(PE::Line(p));
    }
    fn cubic_to(&mut self, c1: D, c2: D, e: D) {
        self.ensure_start();
        let l = self.last();
        if fcmp_pt(l, c1) && fcmp_pt(c1, c2) && fcmp_pt(c2, e) {
            return;
        }
        self.0.push(PE::Cubic(c1, c2, e));
    }
}

fn t_for_arc_angle(angle: f64) -> f64 {
    if fnull_d(angle) {
        return 0.0;
    }
    if fcmp_d(angle, 90.0) {
        return 1.0;
    }
    let rad = angle.to_radians();
    let (ca, sa) = (rad.cos(), rad.sin());
    let k = KAPPA;
    let mut tc = angle / 90.0;
    for _ in 0..2 {
        tc -= ((((2.0 - 3.0 * k) * tc + 3.0 * (k - 1.0)) * tc) * tc + 1.0 - ca)
            / (((6.0 - 9.0 * k) * tc + 6.0 * (k - 1.0)) * tc);
    }
    let mut ts = tc;
    for _ in 0..2 {
        ts -= ((((3.0 * k - 2.0) * ts - 6.0 * k + 3.0) * ts + 3.0 * k) * ts - sa)
            / (((9.0 * k - 6.0) * ts + 12.0 * k - 6.0) * ts + 3.0 * k);
    }
    0.5 * (tc + ts)
}

fn bez_coeff(t: f64) -> (f64, f64, f64, f64) {
    let m = 1.0 - t;
    let b = m * m;
    let c = t * t;
    let d = c * t;
    let a = b * m;
    // QBezier::coefficients finishes with b *= 3t, c *= 3m
    (a, b * 3.0 * t, c * 3.0 * m, d)
}

/// qt_find_ellipse_coords
pub(crate) fn ellipse_coords(r: [f64; 4], angle: f64, length: f64) -> (D, D) {
    let (w2, h2) = (r[2] / 2.0, r[3] / 2.0);
    let (cx, cy) = (r[0] + w2, r[1] + h2);
    let mut out = [[0.0; 2]; 2];
    for (i, a) in [angle, angle + length].into_iter().enumerate() {
        let theta = a - 360.0 * (a / 360.0).floor();
        let mut t = theta / 90.0;
        let quadrant = t as i32;
        t -= quadrant as f64;
        t = t_for_arc_angle(90.0 * t);
        if quadrant & 1 != 0 {
            t = 1.0 - t;
        }
        let (a, b, c, d) = bez_coeff(t);
        let mut p = [a + b + c * KAPPA, d + c + b * KAPPA];
        if quadrant == 1 || quadrant == 2 {
            p[0] = -p[0];
        }
        if quadrant == 0 || quadrant == 1 {
            p[1] = -p[1];
        }
        out[i] = [cx + w2 * p[0], cy + h2 * p[1]];
    }
    (out[0], out[1])
}

#[derive(Clone, Copy, Debug)]
struct Bez([D; 4]);

impl Bez {
    fn split_left(&mut self, t: f64) -> Bez {
        let [p1, p2, p3, p4] = self.0;
        let l = |a: f64, b: f64| a + t * (b - a);
        let mut left = [[0.0; 2]; 4];
        let mut r = self.0;
        for k in 0..2 {
            left[0][k] = p1[k];
            left[1][k] = l(p1[k], p2[k]);
            let tmp = l(p2[k], p3[k]);
            r[2][k] = l(p3[k], p4[k]);
            r[1][k] = l(tmp, r[2][k]);
            left[2][k] = l(left[1][k], tmp);
            let m = l(left[2][k], r[1][k]);
            left[3][k] = m;
            r[0][k] = m;
        }
        self.0 = r;
        Bez(left)
    }
    fn on_interval(&self, t0: f64, t1: f64) -> Bez {
        if t0 == 0.0 && t1 == 1.0 {
            return *self;
        }
        let mut b = *self;
        b.split_left(t0);
        let tt = (t1 - t0) / (1.0 - t0);
        b.split_left(tt)
    }
    fn mid(&self) -> D {
        let p = self.0;
        std::array::from_fn(|k| (p[0][k] + p[3][k] + 3.0 * (p[1][k] + p[2][k])) / 8.0)
    }
}

/// qt_curves_for_arc → (start point, cubic control triples)
pub(crate) fn curves_for_arc(rect: [f64; 4], start_angle: f64, mut sweep: f64) -> (D, Vec<D>) {
    let [x, y, w, h] = rect;
    let (w2, h2) = (w / 2.0, h / 2.0);
    let (w2k, h2k) = (w2 * KAPPA, h2 * KAPPA);
    let pts: [D; 13] = [
        [x + w, y + h2],
        [x + w, y + h2 + h2k],
        [x + w2 + w2k, y + h],
        [x + w2, y + h],
        [x + w2 - w2k, y + h],
        [x, y + h2 + h2k],
        [x, y + h2],
        [x, y + h2 - h2k],
        [x + w2 - w2k, y],
        [x + w2, y],
        [x + w2 + w2k, y],
        [x + w, y + h2 - h2k],
        [x + w, y + h2],
    ];
    sweep = sweep.clamp(-360.0, 360.0);
    let mut curves = Vec::new();
    if start_angle == 0.0 {
        if sweep == 360.0 {
            for i in (0..=11).rev() {
                curves.push(pts[i]);
            }
            return (pts[12], curves);
        } else if sweep == -360.0 {
            for p in pts.iter().skip(1) {
                curves.push(*p);
            }
            return (pts[0], curves);
        }
    }
    let mut start_seg = (start_angle / 90.0).floor() as i32;
    let mut end_seg = ((start_angle + sweep) / 90.0).floor() as i32;
    let mut start_t = (start_angle - start_seg as f64 * 90.0) / 90.0;
    let mut end_t = (start_angle + sweep - end_seg as f64 * 90.0) / 90.0;
    let delta = if sweep > 0.0 { 1 } else { -1 };
    if delta < 0 {
        start_t = 1.0 - start_t;
        end_t = 1.0 - end_t;
    }
    if fnull_d(start_t - 1.0) {
        start_t = 0.0;
        start_seg += delta;
    }
    if fnull_d(end_t) {
        end_t = 1.0;
        end_seg -= delta;
    }
    start_t = t_for_arc_angle(start_t * 90.0);
    end_t = t_for_arc_angle(end_t * 90.0);
    let split_start = !fnull_d(start_t);
    let split_end = !fnull_d(end_t - 1.0);
    let end = end_seg + delta;
    if start_seg == end {
        let q = 3 - (start_seg % 4 + 4) % 4;
        let j = 3 * q as usize;
        return (if delta > 0 { pts[j + 3] } else { pts[j] }, curves);
    }
    let (sp, ep) = ellipse_coords(rect, start_angle, sweep);
    let mut i = start_seg;
    while i != end {
        let q = 3 - (i % 4 + 4) % 4;
        let j = 3 * q as usize;
        let mut b = if delta > 0 {
            Bez([pts[j + 3], pts[j + 2], pts[j + 1], pts[j]])
        } else {
            Bez([pts[j], pts[j + 1], pts[j + 2], pts[j + 3]])
        };
        if start_seg == end_seg && fcmp_d(start_t, end_t) {
            return (sp, Vec::new());
        }
        if i == start_seg {
            if i == end_seg && split_end {
                b = b.on_interval(start_t, end_t);
            } else if split_start {
                b = b.on_interval(start_t, 1.0);
            }
        } else if i == end_seg && split_end {
            b = b.on_interval(0.0, end_t);
        }
        curves.extend_from_slice(&b.0[1..]);
        i += delta;
    }
    let n = curves.len();
    curves[n - 1] = ep;
    (sp, curves)
}

/// qpainterpath.cpp isValidCoord.
fn valid_coord(c: f64) -> bool {
    c.is_finite() && c.abs() < 1e128
}

fn build_painter_path(cmds: &[PathCmd], start: D) -> PPath {
    let mut p = PPath::default();
    p.move_to(start);
    for c in cmds {
        match *c {
            PathCmd::Move(x, y) => p.move_to([x, y]),
            PathCmd::Line(x, y) => p.line_to([x, y]),
            PathCmd::AngleArc {
                cx,
                cy,
                rx,
                ry,
                start,
                sweep,
                move_to_start,
            } => {
                let r = [cx - rx, cy - ry, rx * 2.0, ry * 2.0];
                if r[2] == 0.0 && r[3] == 0.0 {
                    continue;
                }
                if move_to_start {
                    let m = ellipse_coords(r, -start, 0.0).0;
                    if m.iter().all(|&v| valid_coord(v)) {
                        p.move_to(m);
                    }
                }
                // QPainterPath::arcTo ignores the call for invalid coordinates (NaN sweep).
                if !r.iter().chain([&start, &sweep]).all(|&v| valid_coord(v)) {
                    continue;
                }
                let (s, cv) = curves_for_arc(r, -start, -sweep);
                p.line_to(s);
                for k in cv.chunks(3) {
                    p.cubic_to(k[0], k[1], k[2]);
                }
            }
        }
    }
    p
}

// ---------------------------------------------------------------- cubic → quadratics (qquadpath.cpp)

fn score_quadratic(b: &Bez, qcp: D) -> f64 {
    const N: usize = 21;
    let mut t2s = [0.0; N];
    let mut tmts = [0.0; N];
    let mut t = 0.20;
    let step = (1.0 - 2.0 * t) / (N - 1) as f64;
    for i in 0..N {
        t2s[i] = t * t;
        tmts[i] = 2.0 * t * (1.0 - t);
        t += step;
    }
    let m = b.mid();
    let dist = |i: usize| {
        let qp: D = std::array::from_fn(|k| {
            t2s[N - 1 - i] * b.0[0][k] + tmts[i] * qcp[k] + t2s[i] * b.0[3][k]
        });
        let d = [m[0] - qp[0], m[1] - qp[1]];
        d[0] * d[0] + d[1] * d[1]
    };
    let half = (N - 1) / 2;
    let mut found = false;
    let center = dist(half);
    let mut min = center;
    for i in 0..half {
        let td = dist(half + 1 + i);
        if td < min {
            min = td;
        } else {
            found = i > 0;
            break;
        }
    }
    if !found {
        min = center;
        for i in 0..half {
            let td = dist(half - 1 - i);
            if td < min {
                min = td;
            } else {
                found = i > 0;
                break;
            }
        }
    }
    if found { min } else { center }
}

fn line_is_null(a: D, b: D) -> bool {
    fcmp_pt(a, b)
}

fn quadratic_for_cubic(b: &Bez) -> D {
    let [p1, p2, p3, p4] = b.0;
    let st = if !line_is_null(p1, p2) {
        (p1, p2)
    } else if !line_is_null(p1, p3) {
        (p1, p3)
    } else {
        (p1, p4)
    };
    let et = if !line_is_null(p4, p3) {
        (p4, p3)
    } else if !line_is_null(p4, p2) {
        (p4, p2)
    } else {
        (p4, p1)
    };
    let mid = b.mid();
    // QLineF::intersects
    let a = [st.1[0] - st.0[0], st.1[1] - st.0[1]];
    let bb = [et.0[0] - et.1[0], et.0[1] - et.1[1]];
    let c = [st.0[0] - et.0[0], st.0[1] - et.0[1]];
    let den = a[1] * bb[0] - a[0] * bb[1];
    if den == 0.0 || !den.is_finite() {
        return mid;
    }
    let na = (bb[1] * c[0] - bb[0] * c[1]) * (1.0 / den);
    let q = [st.0[0] + a[0] * na, st.0[1] + a[1] * na];
    let bl = [p4[0] - p1[0], p4[1] - p1[1]];
    let ml = [mid[0] - p1[0], mid[1] - p1[1]];
    let ql = [q[0] - p1[0], q[1] - p1[1]];
    let cx1 = ml[0] * bl[1] - ml[1] * bl[0];
    let cx2 = ql[0] * bl[1] - ql[1] * bl[0];
    if cx1.is_sign_negative() == cx2.is_sign_negative() {
        q
    } else {
        mid
    }
}

fn inflection_points(o: &Bez) -> Vec<f64> {
    let valid = |r: f64| {
        r.is_finite() && r > 0.0 && !fnull_f(r as f32) && r < 1.0 && !fnull_f((r - 1.0) as f32)
    };
    let [p1, _, _, p4] = o.0;
    let (dx, dy) = (p4[0] - p1[0], p4[1] - p1[1]);
    let mut ang = (-dy).atan2(dx).to_degrees();
    if ang < 0.0 {
        ang += 360.0;
    }
    let (sa, ca) = if ang == 90.0 {
        (1.0, 0.0)
    } else if ang == 270.0 {
        (-1.0, 0.0)
    } else if ang == 180.0 {
        (0.0, -1.0)
    } else if ang == 0.0 {
        (0.0, 1.0)
    } else {
        let r = ang.to_radians();
        (r.sin(), r.cos())
    };
    let map = |p: D| {
        let (x, y) = (p[0] - p1[0], p[1] - p1[1]);
        [x * ca - y * sa, x * sa + y * ca]
    };
    let n: Vec<D> = o.0.iter().map(|p| map(*p)).collect();
    let (x2, x3, x4, y2, y3) = (n[1][0], n[2][0], n[3][0], n[1][1], n[2][1]);
    let (p, q, r, s) = (x3 * y2, x4 * y2, x2 * y3, x4 * y3);
    let a = 18.0 * ((-3.0 * p) + (2.0 * q) + (3.0 * r) - s);
    if fnull_f(a as f32) {
        if y2.is_sign_negative() != y3.is_sign_negative() && fcmp_f((x4 - x3) as f32, x2 as f32) {
            return vec![0.5];
        } else if a == 0.0 {
            return vec![];
        }
    }
    let b = 18.0 * (((3.0 * p) - q) - (3.0 * r));
    let c = 18.0 * (r - p);
    let rad = b * b - 4.0 * a * c;
    if rad < 0.0 {
        return vec![];
    }
    let sq = rad.sqrt();
    let (r1, r2) = ((-b + sq) / (2.0 * a), (-b - sq) / (2.0 * a));
    let mut res = vec![];
    if valid(r1) {
        res.push(r1);
    }
    if r2 != r1 && valid(r2) {
        res.push(r2);
    }
    if res.len() == 2 && res[0] > res[1] {
        res.swap(0, 1);
    }
    res
}

fn add_to_quadratics(b: &Bez, out: &mut Vec<D>, max_splits: i32, max_diff: f64) {
    let qcp = quadratic_for_cubic(b);
    if max_splits <= 0 || score_quadratic(b, qcp) < max_diff {
        out.push(qcp);
        out.push(b.0[3]);
    } else {
        let mut rhs = *b;
        let lhs = rhs.split_left(0.5);
        add_to_quadratics(&lhs, out, max_splits - 1, max_diff);
        add_to_quadratics(&rhs, out, max_splits - 1, max_diff);
    }
}

fn to_quadratics(b: &Bez) -> Vec<D> {
    let [p1, p2, p3, p4] = b.0;
    let mut out = vec![p1];
    let f = 1.5;
    let c1 = [p1[0] + f * (p2[0] - p1[0]), p1[1] + f * (p2[1] - p1[1])];
    let c2 = [p4[0] + f * (p3[0] - p4[0]), p4[1] + f * (p3[1] - p4[1])];
    if c1 == c2 {
        out.push(c1);
        out.push(p4);
        return out;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (p1[0], p1[0], p1[1], p1[1]);
    for p in &b.0[1..] {
        if p[0] < x0 {
            x0 = p[0]
        } else if p[0] > x1 {
            x1 = p[0]
        }
        if p[1] < y0 {
            y0 = p[1]
        } else if p[1] > y1 {
            y1 = p[1]
        }
    }
    let (dw, dh) = (x1 - x0, y1 - y0);
    let max_diff = (dw * dw + dh * dh) * 0.01 * 0.01;
    let inf = inflection_points(b);
    let max_sub = if inf.is_empty() { 3 } else { 2 };
    let mut t0 = 0.0;
    for i in 0..=inf.len() {
        let t1 = if i < inf.len() { inf[i] } else { 1.0 };
        add_to_quadratics(&b.on_interval(t0, t1), &mut out, max_sub, max_diff);
        t0 = t1;
    }
    out
}

// ---------------------------------------------------------------- QQuadPath

#[derive(Clone, Copy, Debug)]
pub struct Elem {
    pub sp: V,
    pub cp: V,
    pub ep: V,
    pub line: bool,
    pub sub_start: bool,
}

impl Elem {
    fn mid(&self) -> V {
        if self.line {
            mul(add(self.sp, self.ep), 0.5)
        } else {
            add(
                add(mul(self.sp, 0.25), mul(self.cp, 0.5)),
                mul(self.ep, 0.25),
            )
        }
    }
}

#[derive(Default)]
struct QPath {
    el: Vec<Elem>,
    cur: V,
    to_start: bool,
}

fn near_line(p: V, sp: V, ep: V) -> bool {
    let bv = sub(ep, sp);
    let bl2 = dot(bv, bv);
    let t = dot(sub(p, sp), bv) / bl2;
    let pv = sub(p, add(sp, mul(bv, t)));
    dot(pv, pv) / bl2 < 0.01 * 0.01
}

impl QPath {
    fn add(&mut self, control: V, to: V, mut line: bool) {
        if fcmp_v(self.cur, to) {
            return;
        }
        line = line || near_line(control, self.cur, to);
        let cp = if line {
            mul(add(self.cur, to), 0.5)
        } else {
            control
        };
        self.el.push(Elem {
            sp: self.cur,
            cp,
            ep: to,
            line,
            sub_start: self.to_start,
        });
        self.to_start = false;
        self.cur = to;
    }
}

fn d2v(p: D) -> V {
    [p[0] as f32, p[1] as f32]
}

fn quad_path(pp: &PPath) -> QPath {
    let mut q = QPath {
        to_start: true,
        ..Default::default()
    };
    let mut sp = [0.0; 2];
    for e in &pp.0 {
        match *e {
            PE::Move(p) => {
                q.to_start = true;
                q.cur = d2v(p);
                sp = p;
            }
            PE::Line(p) => {
                q.add([0.0, 0.0], d2v(p), true);
                sp = p;
            }
            PE::Cubic(c1, c2, ep) => {
                let quads = to_quadratics(&Bez([sp, c1, c2, ep]));
                let mut i = 1;
                while i + 1 < quads.len() {
                    q.add(d2v(quads[i]), d2v(quads[i + 1]), false);
                    i += 2;
                }
                sp = ep;
            }
        }
    }
    q
}

fn needs_split(e: &Elem) -> bool {
    let v1 = sub(e.cp, e.sp);
    let v2 = sub(e.ep, e.cp);
    dot(v1, v2) / (len(v1) * len(v2)) < 0.9
}

fn split_flatten(e: Elem, level: i32, out: &mut Vec<Elem>) {
    if level > 0 && needs_split(&e) {
        let mp = e.mid();
        let q1 = Elem {
            sp: e.sp,
            cp: mul(add(e.sp, e.cp), 0.5),
            ep: mp,
            line: e.line,
            sub_start: e.sub_start,
        };
        let q2 = Elem {
            sp: mp,
            cp: mul(add(e.ep, e.cp), 0.5),
            ep: e.ep,
            line: e.line,
            sub_start: false,
        };
        split_flatten(q1, level - 1, out);
        split_flatten(q2, level - 1, out);
    } else {
        out.push(e);
    }
}

/// Stroke path as the CurveRenderer sees it: subdivide(path, 3).flattened().
pub fn stroke_elements(cmds: &[PathCmd], start: (f64, f64)) -> Vec<Elem> {
    let q = quad_path(&build_painter_path(cmds, [start.0, start.1]));
    let mut out = Vec::new();
    for e in q.el {
        if e.line {
            out.push(e);
        } else {
            split_flatten(e, 3, &mut out);
        }
    }
    out
}

// ---------------------------------------------------------------- customTriangulator2 (qsgcurveprocessor.cpp)

fn det3(p1: V, p2: V, p3: V) -> f32 {
    p1[0] * (p2[1] - p3[1]) + p2[0] * (p3[1] - p1[1]) + p3[0] * (p1[1] - p2[1])
}
fn calc_normal(a: V, b: V) -> V {
    let v = sub(b, a);
    [v[1], -v[0]]
}
fn normal_of(v: V) -> V {
    norm([-v[1], v[0]])
}
fn normal_vector(e: &Elem, end: bool) -> V {
    if e.line {
        normal_of(sub(e.ep, e.sp))
    } else if !end {
        normal_of(sub(e.cp, e.sp))
    } else {
        normal_of(sub(e.ep, e.cp))
    }
}
fn tangent_vector(e: &Elem, end: bool) -> V {
    match (e.line, end) {
        (true, false) => sub(e.ep, e.sp),
        (true, true) => sub(e.sp, e.ep),
        (false, false) => sub(e.cp, e.sp),
        (false, true) => sub(e.cp, e.ep),
    }
}

fn fix_winding(p: &mut [V; 3]) {
    if (det3(p[0], p[1], p[2]) as f64) < 0.0 {
        p.swap(0, 1);
    }
}
fn check_edge(p1: V, p2: V, p: V, eps: f32) -> bool {
    det3(p1, p2, p) <= eps
}
fn triangles_overlap(mut t1: [V; 3], mut t2: [V; 3]) -> bool {
    let eps = 1.0 / 32.0;
    fix_winding(&mut t1);
    for i in 0..3 {
        let ni = (i + 1) % 3;
        if (0..3).all(|k| check_edge(t1[i], t1[ni], t2[k], eps)) {
            return false;
        }
    }
    fix_winding(&mut t2);
    for i in 0..3 {
        let ni = (i + 1) % 3;
        if (0..3).all(|k| check_edge(t2[i], t2[ni], t1[k], eps)) {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy, Debug)]
struct Tri {
    pts: [V; 3],
    nrm: [V; 3],
    idx: i32,
}

const Z: V = [0.0, 0.0];

fn simple_point_triangulator(pts: &[V], normals: &[V], idx: i32) -> Vec<Tri> {
    let mut hull: Vec<usize> = if det3(pts[0], pts[1], pts[2]) > 0.0 {
        vec![0, 1, 2]
    } else {
        vec![2, 1, 0]
    };
    for i in 3..pts.len() {
        let n = hull.len();
        let visible: Vec<usize> = (0..n)
            .filter(|&j| det3(pts[hull[j]], pts[hull[(j + 1) % n]], pts[i]) < 0.0)
            .collect();
        if visible.is_empty() {
            continue;
        }
        let (vc, hc) = (visible.len(), hull.len());
        let mut bs = visible[0];
        for j in 0..vc - 1 {
            if (visible[j] + 1) % hc != visible[j + 1] {
                bs = visible[j + 1];
                break;
            }
        }
        let keep = hc - vc + 1;
        let mut nh = vec![i];
        for j in 0..keep {
            nh.push(hull[(j + bs + vc) % hc]);
        }
        hull = nh;
    }
    (1..hull.len() - 1)
        .map(|i| {
            let (a, b, c) = (hull[0], hull[i], hull[i + 1]);
            Tri {
                pts: [pts[a], pts[b], pts[c]],
                nrm: [normals[a], normals[b], normals[c]],
                idx,
            }
        })
        .collect()
}

/// calculateJoin → ([inner1, inner2, outer1, outer2, outerMiter], withinMiterLimit, innerIsRight, giveUp)
fn calculate_join(
    e1: Option<&Elem>,
    e2: Option<&Elem>,
    pf: f32,
    inv_ml: f32,
    simple_miter: bool,
) -> ([V; 5], bool, bool, bool) {
    let (e1, e2) = match (e1, e2) {
        (None, Some(e2)) => {
            let n = normal_vector(e2, false);
            return ([n, n, neg(n), neg(n), neg(n)], true, true, false);
        }
        (Some(e1), None) => {
            let n = normal_vector(e1, true);
            return ([n, n, neg(n), neg(n), neg(n)], true, true, false);
        }
        (Some(a), Some(b)) => (a, b),
        _ => unreachable!(),
    };
    let p1 = if e1.line { e1.sp } else { e1.cp };
    let p2 = e1.ep;
    let p3 = if e2.line { e2.ep } else { e2.cp };
    let v1 = norm(sub(p1, p2));
    let v2 = norm(sub(p3, p2));
    let b = add(v1, v2);
    let eps = 1.0 / 32.0;
    if b[0].abs() < eps && b[1].abs() < eps {
        let n1 = [-v1[1], v1[0]];
        let n2 = [-v2[1], v2[0]];
        let n = norm(sub(n2, n1));
        return ([n, n, neg(n), neg(n), neg(n)], true, true, false);
    }
    let bis = norm(b);
    let cos2x = dot(v1, v2).min(1.0);
    let sine = ((1.0 - cos2x) / 2.0).sqrt().max(0.01);
    let length = pf / sine;
    let inner_right = det3(p1, p2, p3) > 0.0;
    let too_long = |a: V, b: V, n: V| {
        let v = sub(b, a);
        let pl = dot(v, n);
        pl * 0.9 < length && len(sub(v, mul(n, pl))) * 0.9 < pf
    };
    let give_up = !e1.line || !e2.line || too_long(p1, p2, bis) || too_long(p3, p2, bis);
    let within = sine >= inv_ml / 2.0;
    let bn = mul(bis, 1.0 / sine);
    if simple_miter && within && !give_up {
        return (
            [bn, bn, neg(bn), neg(bn), neg(bn)],
            within,
            inner_right,
            give_up,
        );
    }
    let n1 = normal_vector(e1, true);
    let n2 = normal_vector(e2, false);
    let r = if give_up {
        if inner_right {
            [n1, n2, neg(n1), neg(n2), neg(bn)]
        } else {
            [neg(n1), neg(n2), n1, n2, neg(bn)]
        }
    } else if inner_right {
        [bn, bn, neg(n1), neg(n2), neg(bn)]
    } else {
        [bn, bn, n1, n2, neg(bn)]
    };
    (r, within, inner_right, give_up)
}

fn triangulate(path: &[Elem], pen_width: f32, join: Join, cap: Cap, miter_limit: f32) -> Vec<Tri> {
    let bevel = join == Join::Bevel;
    let round_join = join == Join::Round;
    let miter = !bevel && !round_join;
    let simple_miter = join == Join::Round;
    let inv_ml = if miter { 1.0 / miter_limit } else { 1.0 };
    let pf = pen_width / 2.0;
    let mut ret = Vec::new();

    let tri_curve = |ret: &mut Vec<Tri>, idx: usize, p: [V; 4], n: [V; 4]| {
        let e = &path[idx];
        let right = det3(e.sp, e.cp, e.ep) > 0.0;
        let sn = normal_vector(e, false);
        let en = normal_vector(e, true);
        let mut cn = norm(add(sn, en));
        if right {
            cn = neg(cn);
        }
        let p5 = add(e.cp, mul(cn, pf));
        let i = idx as i32;
        if !triangles_overlap([p[0], p[1], p5], [p[2], p[3], p5]) {
            ret.push(Tri {
                pts: [p[0], p[1], p5],
                nrm: [n[0], n[1], cn],
                idx: i,
            });
            ret.push(Tri {
                pts: [p[2], p[3], p5],
                nrm: [n[2], n[3], cn],
                idx: i,
            });
            if right {
                ret.push(Tri {
                    pts: [p[0], p[2], p5],
                    nrm: [n[0], n[2], cn],
                    idx: i,
                });
            } else {
                ret.push(Tri {
                    pts: [p[1], p[3], p5],
                    nrm: [n[1], n[3], cn],
                    idx: i,
                });
            }
        } else {
            ret.extend(simple_point_triangulator(
                &[p[0], p[1], p5, p[2], p[3]],
                &[n[0], n[1], cn, n[2], n[3]],
                i,
            ));
        }
    };

    let count = path.len();
    let mut sub_start = 0;
    while sub_start < count {
        let sub_end = (sub_start + 1..count)
            .find(|&i| path[i].sub_start)
            .map_or(count - 1, |i| i - 1);
        let closed = path[sub_start].sp == path[sub_end].ep;
        let sub_count = (sub_end - sub_start + 1) as i32;
        let rel = |idx: usize, d: i32| -> Option<usize> {
            let mut s = idx as i32 - sub_start as i32;
            if closed {
                s = (s + sub_count + d) % sub_count;
                return Some(sub_start + s as usize);
            }
            s += d;
            if s >= 0 && s < sub_count {
                Some(sub_start + s as usize)
            } else {
                None
            }
        };
        for i in sub_start..=sub_end {
            let e = &path[i];
            let next = rel(i, 1).map(|k| &path[k]);
            let prev = rel(i, -1).map(|k| &path[k]);
            let (sj, _, s_in_right, _) = calculate_join(prev, Some(e), pf, inv_ml, simple_miter);
            let (start_inner, start_outer) = (sj[1], sj[3]);
            let (ej, e_within, e_in_right, e_give_up) =
                calculate_join(Some(e), next, pf, inv_ml, simple_miter);
            let (end_inner, end_outer, next_outer, outer_b) = (ej[0], ej[2], ej[3], ej[4]);
            let (n1, n2) = if s_in_right {
                (start_inner, start_outer)
            } else {
                (start_outer, start_inner)
            };
            let mut p1 = add(e.sp, mul(n1, pf));
            let mut p2 = add(e.sp, mul(n2, pf));
            let (n3, n4) = if e_in_right {
                (end_inner, end_outer)
            } else {
                (end_outer, end_inner)
            };
            let mut p3 = add(e.ep, mul(n3, pf));
            let mut p4 = add(e.ep, mul(n4, pf));
            if prev.is_none() {
                let cs = mul(norm(tangent_vector(e, false)), -pf);
                if cap == Cap::Round {
                    p1 = add(p1, cs);
                    p2 = add(p2, cs);
                } else if cap == Cap::Square {
                    let (c1, c2) = (add(p1, cs), add(p2, cs));
                    for t in [[p1, e.sp, c1], [c1, e.sp, c2], [p2, e.sp, c2]] {
                        ret.push(Tri {
                            pts: t,
                            nrm: [Z; 3],
                            idx: -1,
                        });
                    }
                }
            }
            if next.is_none() {
                let cs = mul(norm(tangent_vector(e, true)), -pf);
                if cap == Cap::Round {
                    p3 = add(p3, cs);
                    p4 = add(p4, cs);
                } else if cap == Cap::Square {
                    let (c3, c4) = (add(p3, cs), add(p4, cs));
                    for t in [[p3, e.ep, c3], [c3, e.ep, c4], [p4, e.ep, c4]] {
                        ret.push(Tri {
                            pts: t,
                            nrm: [Z; 3],
                            idx: -1,
                        });
                    }
                }
            }
            if e.line {
                ret.push(Tri {
                    pts: [p1, p2, p3],
                    nrm: [n1, n2, n3],
                    idx: i as i32,
                });
                ret.push(Tri {
                    pts: [p2, p3, p4],
                    nrm: [n2, n3, n4],
                    idx: i as i32,
                });
            } else {
                tri_curve(&mut ret, i, [p1, p2, p3, p4], [n1, n2, n3, n4]);
            }
            let trivial = simple_miter && e_within && !e_give_up;
            if !trivial && next.is_some() {
                let o1 = add(e.ep, mul(end_outer, pf));
                let o2 = add(e.ep, mul(next_outer, pf));
                let ii = i as i32;
                if bevel || (miter && !e_within) {
                    ret.push(Tri {
                        pts: [o1, e.ep, o2],
                        nrm: [Z; 3],
                        idx: -1,
                    });
                } else if round_join {
                    ret.push(Tri {
                        pts: [o1, e.ep, o2],
                        nrm: [Z; 3],
                        idx: ii,
                    });
                    let mut nn = mul(norm(calc_normal(o1, o2)), pf);
                    if !e_in_right {
                        nn = neg(nn);
                    }
                    ret.push(Tri {
                        pts: [o1, add(o1, nn), o2],
                        nrm: [Z; 3],
                        idx: ii,
                    });
                    ret.push(Tri {
                        pts: [add(o1, nn), o2, add(o2, nn)],
                        nrm: [Z; 3],
                        idx: ii,
                    });
                } else if miter {
                    let o = add(e.ep, mul(outer_b, pf));
                    ret.push(Tri {
                        pts: [o1, e.ep, o],
                        nrm: [Z; 3],
                        idx: -2,
                    });
                    ret.push(Tri {
                        pts: [o, e.ep, o2],
                        nrm: [Z; 3],
                        idx: -2,
                    });
                }
                if !e_give_up {
                    let inner = add(e.ep, mul(end_inner, pf));
                    ret.push(Tri {
                        pts: [inner, e.ep, o1],
                        nrm: [end_inner, Z, end_outer],
                        idx: ii,
                    });
                    let nidx = rel(i, 1).unwrap() as i32;
                    ret.push(Tri {
                        pts: [inner, e.ep, o2],
                        nrm: [end_inner, Z, next_outer],
                        idx: nidx,
                    });
                }
            }
        }
        sub_start = sub_end + 1;
    }
    ret
}

// ---------------------------------------------------------------- QSGCurveStrokeNode + shapestroke shaders

/// One GPU triangle: corners (logical), curve Q(s) = A s² + B s + C, per-vertex normals.
#[derive(Clone, Copy, Debug)]
pub struct StrokeTri {
    pub vtx: [V; 3],
    pub abc: [V; 3],
    pub nrm: [V; 3],
}

#[derive(Clone, Debug)]
pub struct Stroke {
    pub width: f32,
    pub color: [u8; 4],
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f32,
}

/// QSGCurveProcessor::processStroke (non-expanding) + QQuickShapeCurveRenderer::addCurveStrokeNodes.
pub fn stroke_tris(path: &[Elem], s: &Stroke) -> Vec<StrokeTri> {
    let pw = s.width;
    let mut out = Vec::new();
    for t in triangulate(path, pw, s.join, s.cap, s.miter_limit) {
        if t.idx < 0 {
            let p = t.pts;
            let mut nn = calc_normal(p[0], p[2]);
            if (det3(p[0], p[1], p[2]) as f64) < 0.0 {
                nn = neg(nn);
            }
            let off = mul(norm(nn), pw / 2.0);
            let (fp1, fp2) = (add(p[0], off), add(p[2], off));
            let n = [norm(sub(p[0], p[1])), Z, norm(sub(p[2], p[1]))];
            out.push(StrokeTri {
                vtx: p,
                abc: [sub(fp2, fp1), Z, fp1],
                nrm: n,
            });
        } else {
            let e = &path[t.idx as usize];
            let abc = if e.line {
                [sub(e.ep, e.sp), Z, e.sp]
            } else {
                let a = add(sub(e.sp, mul(e.cp, 2.0)), e.ep);
                let b = sub(mul(e.cp, 2.0), mul(e.sp, 2.0));
                [a, b, e.sp]
            };
            out.push(StrokeTri {
                vtx: t.pts,
                abc,
                nrm: t.nrm,
            });
        }
    }
    out
}

fn cuberoot(x: f32) -> f32 {
    x.signum() * x.abs().powf(1.0 / 3.0)
}

fn solve_depressed_cubic(h: f32, g: f32) -> [f32; 3] {
    let d = g * g / 4.0 + h * h * h / 27.0;
    if d > 0.0 {
        let u1 = cuberoot(-g / 2.0 - d.sqrt());
        let u2 = cuberoot(-g / 2.0 + d.sqrt());
        [u1 - h / (3.0 * u1), u2 - h / (3.0 * u2), 0.0]
    } else if h == 0.0 {
        // x³ = 0, where the formula below divides by zero.
        [0.0; 3]
    } else {
        let v = 2.0 * (-h / 3.0).sqrt();
        let t = (3.0 * g / h / v).acos() / 3.0;
        let k = 2.0 * std::f32::consts::PI / 3.0;
        [v * t.cos(), v * (t - k).cos(), v * (t - 2.0 * k).cos()]
    }
}

/// shapestroke.vert + shapestroke.frag (solid color, matrixScale 1, no derivatives) at logical point p.
fn coverage(t: &StrokeTri, p: V, w: f32, scale: f32) -> f32 {
    let [a, b, c] = t.abc;
    let qa = -2.0 * dot(a, a);
    let qb = -3.0 * dot(a, b);
    let qc = 2.0 * dot(a, p) - dot(b, b) - 2.0 * dot(a, c);
    let qd = dot(b, p) - dot(b, c);
    let h = (3.0 * qa * qc - qb * qb) / (3.0 * qa * qa);
    let g = (2.0 * qb * qb * qb - 9.0 * qa * qb * qc + 27.0 * qa * qa * qd) / (27.0 * qa * qa * qa);
    let off = qb / (3.0 * qa);
    let s = solve_depressed_cubic(h, g).map(|v| v - off);
    let mut qmin = [1e10f32, 1e10];
    let mut dmin = 1e4f32;
    for si in s {
        let tt = si.clamp(0.0, 1.0);
        let q = add(add(mul(a, tt * tt), mul(b, tt)), c);
        let d = len(sub(q, p));
        let found = if dmin >= d { 1.0 } else { 0.0 };
        dmin = d.min(dmin);
        qmin = add(mul(q, found), mul(qmin, 1.0 - found));
    }
    let n = mul(sub(p, qmin), 1.0 / dmin);
    let q1 = add(qmin, mul(n, w / 2.0));
    let q2 = sub(qmin, mul(n, w / 2.0));
    let d1 = dot(sub(mul(p, scale), mul(q1, scale)), n);
    let d2 = dot(sub(mul(p, scale), mul(q2, scale)), n);
    let f = |d: f32| {
        let step = if -d >= 0.0 { 1.0 } else { 0.0 };
        let m = (std::f32::consts::SQRT_2 / 2.0 - d.abs()).max(0.0);
        (step + d.signum() * m * m).clamp(0.0, 1.0)
    };
    // On the centre line itself there is no direction away from it (0 / 0).
    if dmin >= w * 0.01 {
        f(d1) - f(d2)
    } else {
        (w * scale).min(1.0)
    }
}

/// Rasterize stroke triangles at scene offset (tx, ty), premultiplied source-over per triangle like the GPU.
pub fn draw_stroke(
    cv: &mut Canvas,
    tris: &[StrokeTri],
    s: &Stroke,
    tx: f32,
    ty: f32,
    opacity: f32,
) {
    let ca = s.color[3] as f32 / 255.0;
    let col = [
        s.color[0] as f32 / 255.0 * ca,
        s.color[1] as f32 / 255.0 * ca,
        s.color[2] as f32 / 255.0 * ca,
        ca,
    ];
    let snap = |v: f32| (v * 256.0).round() / 256.0;
    for t in tris {
        let sv: [[f32; 2]; 3] = std::array::from_fn(|i| {
            let p = add(t.vtx[i], mul(t.nrm[i], std::f32::consts::SQRT_2));
            [snap(p[0] + tx), snap(p[1] + ty)]
        });
        let area = det3(sv[0], sv[1], sv[2]);
        if area == 0.0 || !area.is_finite() {
            continue;
        }
        let v = if area > 0.0 {
            sv
        } else {
            [sv[0], sv[2], sv[1]]
        };
        let x0 = (v.iter().map(|p| p[0]).fold(f32::MAX, f32::min) - 0.5)
            .ceil()
            .max(0.0) as usize;
        let x1 = ((v.iter().map(|p| p[0]).fold(f32::MIN, f32::max) - 0.5).floor() as i64)
            .min(cv.w as i64 - 1);
        let y0 = (v.iter().map(|p| p[1]).fold(f32::MAX, f32::min) - 0.5)
            .ceil()
            .max(0.0) as usize;
        let y1 = ((v.iter().map(|p| p[1]).fold(f32::MIN, f32::max) - 0.5).floor() as i64)
            .min(cv.h as i64 - 1);
        if x1 < 0 || y1 < 0 {
            continue;
        }
        let edge = |a: V, b: V, p: V| {
            let e = det3(a, b, p);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            e > 0.0 || (e == 0.0 && (dy < 0.0 || (dy == 0.0 && dx > 0.0)))
        };
        for py in y0..=y1 as usize {
            for px in x0..=x1 as usize {
                let p = [px as f32 + 0.5, py as f32 + 0.5];
                if !(edge(v[0], v[1], p) && edge(v[1], v[2], p) && edge(v[2], v[0], p)) {
                    continue;
                }
                let k = coverage(t, [p[0] - tx, p[1] - ty], s.width, 1.0) * opacity;
                if k.is_nan() {
                    continue;
                }
                cv.blend_f(px, py, col.map(|c| c * k));
            }
        }
    }
}

#[cfg(test)]
mod arc_tests {
    use super::*;

    #[test]
    fn nan_sweep_angle_arc_adds_nothing() {
        let arc = |sweep| PathCmd::AngleArc {
            cx: 10.0,
            cy: 10.0,
            rx: 9.0,
            ry: 9.0,
            start: -90.0,
            sweep,
            move_to_start: false,
        };
        let cmds = [arc(f64::NAN), PathCmd::Line(10.0, 10.0)];
        let p = build_painter_path(&cmds, [10.0, 10.0]);
        assert!(matches!(p.0[..], [PE::Move(_)]), "{:?}", p.0.len());
        let p = build_painter_path(&[arc(90.0), PathCmd::Line(10.0, 10.0)], [10.0, 10.0]);
        assert!(p.0.len() > 2);
    }
}
